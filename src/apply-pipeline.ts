import type { PipelineResponse, ParseResponse, AccountProperties, Transaction } from "./types";
import { refreshIssuesCache } from "./issues";
import { matchesPrefix, txnIdFromMeta } from "./meta";

/** Subset of AppState consumed by applyParse/applyPipelineResponse. */
export interface ApplyTarget {
  parse?: ParseResponse;
  selectedAccount?: string;
  displayConfig?: { default_account?: string } | undefined;
  accountPropertiesMap: Record<string, AccountProperties>;
  accountSetMap: Record<string, string[]>;
  accountFoldersMap: Record<string, string>;
  pipelineWarnings: string[];
  reportsBuilding?: boolean;
  refreshView?: () => Promise<void>;
  txWindowStart?: number;
  searchFilteredTransactions?: Transaction[];
  searchFilteredCount?: number;
  searchFilteredOffset?: number;
  /** Set of txn ids that appeared in this pipeline rebuild but were NOT
   *  in the previous parse. The renderer reads this once to add a
   *  fade-in class, then it's cleared. */
  justAddedTxnIds?: Set<string>;
  /** Lighter-weight refresh used by mutations: only re-fetches the
   *  visible search window (small, paged), not the prefix query or
   *  issues cache. Falls back to refreshView if not provided. */
  refreshSearch?: () => Promise<void>;
}

function collectTxnIds(transactions: Transaction[] | undefined): Set<string> {
  const ids = new Set<string>();
  if (!transactions) return ids;
  for (const t of transactions) {
    const id = txnIdFromMeta(t.meta);
    if (id) ids.add(id);
  }
  return ids;
}

export function applyParse(state: ApplyTarget, response: ParseResponse): void {
  state.parse = response;

  if (response.account_properties) {
    state.accountPropertiesMap = { ...state.accountPropertiesMap, ...response.account_properties };
  }

  const nextBalances = response.balances ?? [];

  if (!state.selectedAccount) {
    const defaultAccount = state.displayConfig?.default_account;
    state.selectedAccount = (defaultAccount && nextBalances.find((b) => matchesPrefix(b.account, defaultAccount))?.account)
      ?? nextBalances[0]?.account;
  }
}

// Any mutation that affects which transactions are visible (pipeline rebuild,
// hide, manual delete) invalidates every previously-loaded page of the search
// window. Callers MUST use this after applyParse so refreshView re-fetches
// from offset 0 and replaces the list rather than appending to stale pages.
export async function invalidateSearchWindowAndRefresh(state: ApplyTarget): Promise<void> {
  state.txWindowStart = 0;
  state.searchFilteredTransactions = undefined;
  state.searchFilteredCount = undefined;
  state.searchFilteredOffset = undefined;
  try { await refreshIssuesCache(); } catch { /* ignore */ }
  await state.refreshView?.();
}

/** Slim mutation response — backend confirmed the write succeeded but
 *  didn't re-send the full ledger. Used by hide_transaction (and any
 *  future mutation that doesn't need a full refresh) to avoid the
 *  multi-second main-thread JSON deserialize stall on large vaults.
 *  state.parse is left as-is; the visible search window is refreshed
 *  via the paged query API. */
export interface MutationResponse {
  ok: boolean;
  warnings: string[];
  output_files_written: number;
}

export async function applyMutationResponse(
  state: ApplyTarget,
  response: MutationResponse,
): Promise<void> {
  state.pipelineWarnings = response.warnings ?? [];
  if (response.output_files_written > 0) {
    state.reportsBuilding = true;
  }
  // Reset paged scroll so the next refresh re-fetches from offset 0.
  state.txWindowStart = 0;
  state.searchFilteredTransactions = undefined;
  state.searchFilteredCount = undefined;
  state.searchFilteredOffset = undefined;
  // Refresh the visible window and the account header balance. The prefix
  // (balance) query is now balances-only and cheap, so this no longer ships the
  // full transaction list. The issues cache is refreshed in the background.
  await (state.refreshView ?? state.refreshSearch)?.();
  // Background, fire-and-forget refresh of the issues cache.
  void refreshIssuesCache().catch(() => { /* ignore */ });
}

export async function applyPipelineResponse(state: ApplyTarget, response: PipelineResponse): Promise<void> {
  // Snapshot the prev set BEFORE applyParse mutates state.parse so we can
  // diff against the new transactions and surface a fade-in for the rows
  // that genuinely arrived in this rebuild.
  const prevTxnIds = collectTxnIds(state.parse?.transactions);
  applyParse(state, response.parse);
  const nextTxnIds = collectTxnIds(state.parse?.transactions);
  const justAdded = new Set<string>();
  for (const id of nextTxnIds) {
    if (!prevTxnIds.has(id)) justAdded.add(id);
  }
  state.justAddedTxnIds = justAdded.size > 0 ? justAdded : undefined;
  state.pipelineWarnings = response.warnings ?? [];
  if (response.result.owner_accounts) {
    state.accountSetMap = response.result.owner_accounts;
  }
  if (response.result.account_folders) {
    state.accountFoldersMap = response.result.account_folders;
  }
  if (response.result.account_properties) {
    state.accountPropertiesMap = response.result.account_properties;
  }
  if (response.result.output_files_written > 0) {
    state.reportsBuilding = true;
  }
  await invalidateSearchWindowAndRefresh(state);
}
