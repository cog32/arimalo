// Helpers for optimistic UI state — txns the user has clicked but the
// pipeline hasn't yet processed.

import type { Transaction, Posting, CommodityAmount } from "./types";
import type { BuiltPosting } from "./account-utils";

/** Reset the paged search window state. Use this anywhere we want the
 *  next runSearchFilter call to start fetching from offset 0 — switching
 *  date filter, drilling between accounts, search input change, etc.
 *
 *  Forgetting to reset txWindowStart is the bug class behind "rows are
 *  missing": if the user had scrolled (txWindowStart > 0) and the search
 *  parameters change, the next fetch returns offset N..N+TX_WINDOW of
 *  the NEW query and silently drops the first N rows. */
export function resetSearchPaging(state: {
  txWindowStart?: number;
  searchFilteredTransactions?: Transaction[];
  searchFilteredCount?: number;
  searchFilteredOffset?: number;
}): void {
  state.txWindowStart = 0;
  state.searchFilteredTransactions = undefined;
  state.searchFilteredCount = undefined;
  state.searchFilteredOffset = undefined;
}

function metaTxnId(meta: string | null | undefined): string {
  return (meta ?? "").split(",").map((p) => p.trim()).find((p) => p.startsWith("txn:")) ?? "";
}

/** Filter out txns whose id is in pendingDeletes. The renderer calls this
 *  before pagination so optimistic deletes never accumulate as faded rows
 *  while the queued pipeline rebuild catches up. */
export function filterPendingDeletes<T extends { meta?: string | null }>(
  txns: T[],
  pendingDeletes: Set<string>,
): T[] {
  if (pendingDeletes.size === 0) return txns;
  return txns.filter((t) => {
    const id = metaTxnId(t.meta);
    return !id || !pendingDeletes.has(id);
  });
}

/** Drop any expandKey whose underlying transaction is being hidden.
 *
 *  expandKey format: `${txnId}|${datetime}|${amount}|${narration}`.
 *  Without this prune, txExpandedRows accumulates orphan entries: the
 *  user expands a row, deletes it, but state.txExpandedRows still
 *  carries that key. If the row ever re-appears (e.g. show-ignored
 *  toggled back on, the rule didn't catch it, or a multi-leg txn
 *  partial-hide leaves a sibling), the renderer reads the stale
 *  entry and emits its detail row — a "hidden row" surfacing without
 *  the user clicking the row that owns it. */
export function pruneExpandedKeysForTxn(
  expandedKeys: Set<string> | undefined,
  txnId: string,
): void {
  if (!expandedKeys || !txnId) return;
  const prefix = `${txnId}|`;
  for (const key of [...expandedKeys]) {
    if (key === txnId || key.startsWith(prefix)) expandedKeys.delete(key);
  }
}

// ── Optimistic ADD (the row appears before the backend rebuild lands) ──

let optimisticAddSeq = 0;
/** Monotonic temp id for an optimistic add (distinguishable from real `man-`
 *  / `txn:` ids by its `tmp-` prefix). */
export function nextOptimisticTempId(): string {
  optimisticAddSeq += 1;
  return `tmp-${optimisticAddSeq}`;
}

/** Synthesize a Transaction for an optimistic add — the row shown instantly,
 *  before the backend rebuild lands. Shaped like a real parsed txn so the
 *  existing row renderer and balance math treat it identically. The selected-
 *  account leg drives the amount column; `tempId` becomes the meta txn id. */
export function buildOptimisticTransaction(
  fields: { datetime: string; payee: string; narration: string },
  postings: BuiltPosting[],
  selectedAccount: string,
  tempId: string,
): Transaction {
  const legs: Posting[] = postings.map((p) => ({
    account: p.account,
    amount: Number(p.amount) || 0,
    amount_text: p.amount,
    commodity: p.commodity,
    remainder: p.remainder,
  }));
  const sel = legs.find((p) => p.account === selectedAccount) ?? legs[0];
  return {
    date: fields.datetime.slice(0, 10),
    datetime: fields.datetime,
    status: "*",
    payee: fields.payee,
    narration: fields.narration,
    meta: `txn:${tempId}`,
    postings: legs,
    amount: sel?.amount ?? 0,
    amount_commodity: sel?.commodity ?? "",
  };
}

/** Content signature over the fields that identify a manual txn (datetime,
 *  payee, narration, postings). Numeric amounts to 8dp + sorted legs so a temp
 *  row and its eventual real row compare equal regardless of posting order or
 *  amount-text formatting. Used to suppress the temp once its real row lands. */
function txnContentSignature(t: Transaction): string {
  const legs = t.postings
    .map((p) => `${p.account}|${(Number(p.amount) || 0).toFixed(8)}|${p.commodity}`)
    .sort()
    .join(";");
  return [
    (t.datetime ?? "").trim(),
    (t.payee ?? "").trim(),
    (t.narration ?? "").trim(),
    legs,
  ].join("¦");
}

/** Temp adds to splice into the visible table for `selectedAccount`: those
 *  whose postings touch the account AND that don't yet have a content-matching
 *  real row in `realTxns`. The content-match is the dedup guard that prevents a
 *  double-row flash if the real row arrives (via this add's refresh OR an
 *  unrelated pipeline-rebuilt) while the temp is still in pendingAdds. */
export function pendingAddsForView(
  pendingAdds: Transaction[] | undefined,
  selectedAccount: string | undefined,
  realTxns: Transaction[],
): Transaction[] {
  if (!pendingAdds || pendingAdds.length === 0 || !selectedAccount) return [];
  const realSigs = new Set(realTxns.map(txnContentSignature));
  return pendingAdds.filter(
    (t) =>
      t.postings.some((p) => p.account === selectedAccount) &&
      !realSigs.has(txnContentSignature(t)),
  );
}

/** Fold pending adds' selected-account leg amounts into a copy of `totals`, per
 *  commodity. Derived at render time so rollback/reconcile is automatic: when a
 *  temp leaves pendingAdds the delta simply disappears — no separate balance
 *  bookkeeping that could drift out of sync with the displayed rows. */
export function applyPendingBalanceDelta(
  totals: CommodityAmount[],
  pendingAdds: Transaction[] | undefined,
  selectedAccount: string | undefined,
): CommodityAmount[] {
  if (!pendingAdds || pendingAdds.length === 0 || !selectedAccount) return totals;
  const out = totals.map((c) => ({ ...c }));
  for (const txn of pendingAdds) {
    for (const p of txn.postings) {
      if (p.account !== selectedAccount) continue;
      const existing = out.find((c) => c.commodity === p.commodity);
      if (existing) existing.amount += p.amount;
      else out.push({ commodity: p.commodity, amount: p.amount });
    }
  }
  return out;
}
