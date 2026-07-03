// Account utilities.
// Resolves account names to source folder paths and other non-search helpers.

/** Resolve an account name to its source folder path.
 *  Looks up accountFoldersMap first, walking up the hierarchy.
 *  Falls back to stripping "assets:" and prepending the account set. */
export function resolveAccountFolder(
  accountFoldersMap: Record<string, string>,
  selectedAccountSet: string | undefined,
  account: string,
): string {
  const found = resolveAccountFolderFromMap(accountFoldersMap, account);
  if (found) return found;
  // Final fallback: strip first prefix and prepend account set
  const suffix = account.split(":").slice(1).join("/");
  return selectedAccountSet ? `${selectedAccountSet}/${suffix}` : suffix;
}

/** Try to resolve an account to a folder using only the map (no fallback).
 *  Returns undefined if no map entry matches. */
export function resolveAccountFolderFromMap(
  accountFoldersMap: Record<string, string>,
  account: string,
): string | undefined {
  if (accountFoldersMap[account]) return accountFoldersMap[account];
  const parts = account.split(":");
  for (let i = parts.length - 1; i >= 2; i--) {
    const parent = parts.slice(0, i).join(":");
    if (accountFoldersMap[parent]) return accountFoldersMap[parent];
  }
  return undefined;
}

/** Convert a folder-relative path to its derived account name.
 *  Mirrors the Rust `folder_to_account_name()` logic.
 *  e.g. "richard/crypto/exchange/binance/personal" → "assets:crypto:exchange:binance:personal" */
export function folderToAccountName(folder: string): string {
  const parts = folder.split("/").filter(Boolean);
  if (parts.length <= 1) return "assets:" + (parts[0] ?? "unknown");
  // Skip the first segment (owner/account set), rest becomes account hierarchy
  return "assets:" + parts.slice(1).join(":");
}

// ── Manual transaction entry (Add New modal) ──
//
// The modal collects one structured "top" leg for the account you opened it
// from — either a plain Value (an amount in the account's cash commodity) or a
// Trade (commodity / quantity / per-unit price) — plus one or more "other
// account" contra rows in the cash commodity. `buildManualPostings` turns that
// into ledger postings, mirroring the generated CommSec 3-leg trade format:
//
//   assets:…:commsec:personal 10000 BQT @@ 3650.00 AUD   ← top (trade)
//   assets:transfer:cash      -3679.95 AUD               ← contra (auto-filled)
//   expenses:fees:brokerage    29.95 AUD                 ← contra
//
// The ledger parser does not enforce transaction balance, so the cash-side
// balance check here is a form-level guardrail: the top leg's cash value
// (Value: amount; Trade: quantity × price) plus the contra amounts must net to
// zero. Exactly one blank contra row auto-fills the remainder.

/** One leg of a manual transaction, shape-compatible with the backend
 *  `ManualPostingInput`. A trade's price annotation rides in `remainder`,
 *  which the backend serializes verbatim onto the posting line. */
export type BuiltPosting = {
  account: string;
  amount: string;
  commodity: string;
  remainder: string | null;
};

/** Structured fields the Add New modal collects, free of AppState/DOM. */
export type ManualDraftCore = {
  mode: "value" | "trade";
  account: string;
  cashCommodity: string;
  amount: string; // value mode
  tradeCommodity: string; // trade mode
  quantity: string;
  price: string;
  contras: { account: string; amount: string }[];
};

/** Live balance snapshot driving the indicator + the blank row's placeholder. */
export type ManualBalance = {
  /** Signed cash value of the top (account) leg, or NaN when not yet a number. */
  topCash: number;
  /** Sum of contra rows that already have a numeric amount. */
  contraSum: number;
  /** Contra rows naming an account but leaving the amount blank (auto-fill candidates). */
  blanks: number;
  /** What a single blank row takes to balance: -(topCash + contraSum). */
  remainder: number;
  /** Top leg valid AND (exactly one blank, or zero blanks already netting to zero). */
  balanceable: boolean;
};

export type BuildManualResult =
  | { ok: true; postings: BuiltPosting[] }
  | { ok: false; error: string };

const MANUAL_BALANCE_EPS = 1e-9;

function parseManualNum(s: string): number {
  const t = s.trim();
  if (t === "") return NaN;
  const n = Number(t);
  return Number.isFinite(n) ? n : NaN;
}

/** Format a *computed* cash amount (auto-fill remainder, trade total) as money:
 *  round to 8dp, trim trailing zeros, keep at least 2 decimals. User-typed
 *  amounts are never reformatted — only values we derive. */
export function formatCash(n: number): string {
  if (!Number.isFinite(n)) return "0.00";
  const v = n === 0 ? 0 : n; // normalize -0
  let s = v.toFixed(8).replace(/0+$/, "").replace(/\.$/, "");
  const dot = s.indexOf(".");
  if (dot === -1) s += ".00";
  else if (s.length - dot - 1 < 2) s += "0".repeat(2 - (s.length - dot - 1));
  return s;
}

/** Signed cash value of the top (account) leg, or NaN if its inputs aren't valid yet. */
function manualTopCash(d: ManualDraftCore): number {
  if (d.mode === "value") return parseManualNum(d.amount);
  const q = parseManualNum(d.quantity);
  const p = parseManualNum(d.price);
  if (Number.isNaN(q) || Number.isNaN(p)) return NaN;
  return q * p;
}

export function computeManualBalance(d: ManualDraftCore): ManualBalance {
  const topCash = manualTopCash(d);
  let contraSum = 0;
  let blanks = 0;
  for (const c of d.contras) {
    const hasAcct = c.account.trim() !== "";
    const amt = c.amount.trim();
    if (amt === "") {
      if (hasAcct) blanks += 1;
      continue; // fully-empty row is ignored
    }
    const n = parseManualNum(amt);
    if (!Number.isNaN(n)) contraSum += n;
  }
  const base = Number.isNaN(topCash) ? 0 : topCash;
  const remainder = -(base + contraSum);
  const balanceable =
    !Number.isNaN(topCash) &&
    (blanks === 1 || (blanks === 0 && Math.abs(base + contraSum) < MANUAL_BALANCE_EPS));
  return { topCash, contraSum, blanks, remainder, balanceable };
}

/** Build the postings for a manual entry, auto-filling the single blank contra
 *  row, or return a human-readable error. The top leg goes first. */
export function buildManualPostings(d: ManualDraftCore): BuildManualResult {
  const top: BuiltPosting[] = [];
  let topCash: number;
  if (d.mode === "value") {
    const amt = d.amount.trim();
    const n = parseManualNum(amt);
    if (Number.isNaN(n)) return { ok: false, error: "Enter a valid amount." };
    top.push({ account: d.account, amount: amt, commodity: d.cashCommodity, remainder: null });
    topCash = n;
  } else {
    const ticker = d.tradeCommodity.trim();
    const qtyStr = d.quantity.trim();
    const q = parseManualNum(qtyStr);
    const p = parseManualNum(d.price);
    if (ticker === "") return { ok: false, error: "Enter the commodity (ticker) for the trade." };
    if (Number.isNaN(q)) return { ok: false, error: "Enter a valid quantity." };
    if (Number.isNaN(p) || p < 0) return { ok: false, error: "Enter a valid (non-negative) price." };
    topCash = q * p;
    top.push({
      account: d.account,
      amount: qtyStr,
      commodity: ticker,
      // Total price (@@), matching the generated CommSec format. Sign rides on quantity.
      remainder: `@@ ${formatCash(Math.abs(topCash))} ${d.cashCommodity}`,
    });
  }

  const rows = d.contras
    .map((c) => ({ account: c.account.trim(), amount: c.amount.trim() }))
    .filter((c) => c.account !== "" || c.amount !== "");
  if (rows.length === 0) return { ok: false, error: "Add at least one account for the other side." };

  let filled = 0;
  const blankIdx: number[] = [];
  for (let i = 0; i < rows.length; i++) {
    const c = rows[i];
    if (c.account === "") return { ok: false, error: "Every posting needs an account." };
    if (c.amount === "") {
      blankIdx.push(i);
      continue;
    }
    const n = parseManualNum(c.amount);
    if (Number.isNaN(n)) return { ok: false, error: `Invalid amount: ${c.amount}` };
    filled += n;
  }
  if (blankIdx.length > 1) {
    return { ok: false, error: "Enter an amount for all but one of the other accounts." };
  }

  const contraPostings: BuiltPosting[] = rows.map((c) => ({
    account: c.account,
    amount: c.amount, // verbatim; the single blank (if any) is filled below
    commodity: d.cashCommodity,
    remainder: null,
  }));

  if (blankIdx.length === 1) {
    contraPostings[blankIdx[0]].amount = formatCash(-(topCash + filled));
  } else if (Math.abs(topCash + filled) > MANUAL_BALANCE_EPS) {
    return {
      ok: false,
      error: `Out of balance by ${formatCash(topCash + filled)} ${d.cashCommodity}.`,
    };
  }

  return { ok: true, postings: [...top, ...contraPostings] };
}

/** Abbreviate a long address/name for display.
 *  Shows enough characters to differentiate addresses that share
 *  a common prefix+suffix (e.g. address-poisoning attacks). */
export function shortAddress(addr: string): string {
  if (addr.length > 20) {
    return addr.slice(0, 10) + "\u2026" + addr.slice(-6);
  }
  return addr;
}

/** Format a colon-separated account path for a tight column.
 *
 *  Priority: show the leaf (rightmost segment) in full, then fill any remaining
 *  budget with parent context taken from the right of the parent path with a
 *  leading ellipsis. If the leaf itself doesn't fit, truncate it from the right
 *  with a trailing ellipsis. Long addresses in the leaf are first abbreviated
 *  via `shortAddress`; if even the abbreviated form is too big, it gets a
 *  second trailing ellipsis (the `XXXX\u2026XX\u2026` pattern). */
export function shortAccountPath(account: string, maxLen = 36): string {
  if (account.length <= maxLen) return account;
  const parts = account.split(":");
  const rawLeaf = parts[parts.length - 1] ?? "";
  // Address-style leaves get the first10\u2026last6 collapse before fitting.
  const leaf = rawLeaf.length > 20 ? shortAddress(rawLeaf) : rawLeaf;

  // Case A: leaf alone exceeds budget (after addition of trailing ellipsis).
  if (leaf.length + 1 > maxLen) {
    return leaf.slice(0, Math.max(0, maxLen - 1)) + "\u2026";
  }

  // Case B: leaf fits \u2014 backfill parent context from the right.
  const parentWithColon = parts.slice(0, -1).join(":") + (parts.length > 1 ? ":" : "");
  // Try to fit the full parent first \u2014 if so, no leading ellipsis is needed.
  if (parentWithColon.length + leaf.length <= maxLen) {
    return parentWithColon + leaf;
  }
  const remaining = maxLen - leaf.length - 1; // 1 for leading ellipsis
  if (remaining <= 0) return "\u2026" + leaf;
  return "\u2026" + parentWithColon.slice(-remaining) + leaf;
}

/** Minimal transaction shape for pair detection. */
export type TxnForPairing = {
  datetime: string;
  amount: number;
  amount_commodity: string;
  [key: string]: unknown;
};

/** Detect indices where consecutive transactions form a potential trade pair.
 *  Returns a Set of indices where a chain connector should appear AFTER that row
 *  (i.e. between row[i] and row[i+1]). */
export function detectTradePairs(txns: TxnForPairing[]): Set<number> {
  const pairs = new Set<number>();
  for (let i = 0; i < txns.length - 1; i++) {
    const a = txns[i];
    const b = txns[i + 1];
    // Same datetime, different commodities, opposite signs, both non-zero
    if (
      a.datetime === b.datetime &&
      a.amount_commodity !== b.amount_commodity &&
      a.amount * b.amount < 0 &&
      Math.abs(a.amount) > 1e-9 &&
      Math.abs(b.amount) > 1e-9
    ) {
      pairs.add(i);
    }
  }
  return pairs;
}

/** Detect indices where consecutive group-header items form a potential trade pair.
 *  Same logic as detectTradePairs but operates on TxRowItem[] and matches adjacent
 *  group-headers with same date+venue, different commodities, opposite net amounts. */
export function detectGroupTradePairs(items: { kind: string; group?: { date: string; venueName: string; commodity: string; netAmount: number } }[]): Set<number> {
  const pairs = new Set<number>();
  for (let i = 0; i < items.length - 1; i++) {
    const a = items[i];
    const b = items[i + 1];
    if (a.kind !== "group-header" || b.kind !== "group-header") continue;
    const ga = a.group!;
    const gb = b.group!;
    if (
      ga.date === gb.date &&
      ga.venueName === gb.venueName &&
      ga.commodity !== gb.commodity &&
      ga.netAmount * gb.netAmount < 0 &&
      Math.abs(ga.netAmount) > 1e-9 &&
      Math.abs(gb.netAmount) > 1e-9
    ) {
      pairs.add(i);
    }
  }
  return pairs;
}

/** Extract txn IDs from transactions, sorted by absolute posting amount.
 *  Used for deterministic pairing of group trade pair constituents. */
export function sortTxnIdsByAbsAmount(
  txns: { meta?: string | null; amount: number; postings: { account: string; amount: number }[] }[],
  selectedAccount: string | undefined,
): string[] {
  return txns
    .map((t) => ({
      id: (t.meta ?? "").split(",").map((p) => p.trim()).find((p) => p.startsWith("txn:")) ?? "",
      absAmount: Math.abs(
        selectedAccount
          ? t.postings
              .filter((p) => p.account === selectedAccount || p.account.startsWith(selectedAccount + ":"))
              .reduce((s, p) => s + p.amount, 0)
          : t.amount,
      ),
    }))
    .filter((x) => x.id)
    .sort((a, b) => a.absAmount - b.absAmount)
    .map((x) => x.id);
}

/** Reset state fields for account navigation.
 *  Preserves tradeLinkSelection so users can link transactions across accounts. */
export function resetStateForAccountNavigation(state: Record<string, any>, account: string): void {
  state.selectedAccount = account;
  state.prefixQuery = undefined;
  state.transactionValues = undefined;
  state.accountTotalValue = undefined;
  // Preserve tradeLinkSelection — user needs to navigate accounts to find the partner transaction
  state.txWindowStart = 0;
  state.txExpandedGroups = new Set();
}

/** Categories-pane sibling of {@link resetStateForAccountNavigation}: writes the
 *  category selection slice while resetting the same shared transaction window. */
export function resetStateForCategoryNavigation(state: Record<string, any>, category: string): void {
  state.selectedCategory = category;
  state.prefixQuery = undefined;
  state.transactionValues = undefined;
  state.accountTotalValue = undefined;
  state.txWindowStart = 0;
  state.txExpandedGroups = new Set();
}
