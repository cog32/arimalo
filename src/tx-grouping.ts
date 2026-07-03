/** Transaction aggregation — groups same-date+venue+commodity transactions for display. */

import type { Posting, Transaction, TradeLink } from "./types";

export type TxGroup = {
  key: string;
  date: string;
  venueName: string;
  commodity: string;
  narration: string;
  transactions: Transaction[];
  netAmount: number;
  totalIn: number;
  totalOut: number;
};

export type TxRowItem =
  | { kind: "single"; transaction: Transaction }
  | { kind: "group-header"; group: TxGroup }
  | { kind: "group-detail"; transaction: Transaction; group: TxGroup };

function matchesPrefix(acct: string, prefix: string): boolean {
  return acct === prefix || acct.startsWith(prefix + ":");
}

export function getTxnId(t: Transaction): string {
  return (t.meta ?? "").split(",").map((p) => p.trim()).find((p) => p.startsWith("txn:")) ?? "";
}

/** Build a map from txn ID → Transaction[] for O(1) partner lookups. */
export function buildTxnIdMap(transactions: Transaction[]): Map<string, Transaction[]> {
  const map = new Map<string, Transaction[]>();
  for (const t of transactions) {
    const id = getTxnId(t);
    if (!id) continue;
    const arr = map.get(id);
    if (arr) arr.push(t);
    else map.set(id, [t]);
  }
  return map;
}

// Pick the first in-scope posting's amount as the row's representative
// magnitude. Summing across in-scope postings nets to zero for any
// transaction whose legs all fall within the scope (e.g. internal transfers
// at a broader prefix), which hides the row from the user — exactly the
// rows they need to see to detect half-recorded transfers.
function postingAmount(t: Transaction, selectedAccount: string): number {
  return t.postings.find((p) => matchesPrefix(p.account, selectedAccount))?.amount ?? 0;
}

function postingCommodity(t: Transaction, selectedAccount: string): string {
  const p = t.postings.find((p) => matchesPrefix(p.account, selectedAccount));
  return p?.commodity ?? "USD";
}

const MIN_GROUP_SIZE = 3;

/**
 * Walk sorted transactions. Collect all transactions with the same date+venue
 * into a block, then sub-group by commodity+narration within that block.
 * Groups with 3+ transactions become a group header; others remain singles.
 * Stops emitting after `pageSize` top-level items but counts the full total.
 */
// eslint-disable-next-line complexity
export function paginateWithGroups(
  sorted: Transaction[],
  selectedAccount: string | undefined,
  tradeLinkMap: Map<string, { link: { id: string }; partnerId: string }>,
  expandedGroups: Set<string>,
  pageSize: number,
): { items: TxRowItem[]; totalTopLevel: number } {
  if (!Number.isFinite(pageSize)) throw new Error("pageSize must be finite");
  const items: TxRowItem[] = [];
  let topLevelCount = 0;
  let totalTopLevel = 0;
  let i = 0;

  while (i < sorted.length) {
    const t = sorted[i];
    const txnId = getTxnId(t);

    // Trade-linked transactions are always singles
    if (txnId && tradeLinkMap.has(txnId)) {
      totalTopLevel++;
      if (topLevelCount < pageSize) {
        items.push({ kind: "single", transaction: t });
        topLevelCount++;
      }
      i++;
      continue;
    }

    // Collect a block: all transactions with the same date+venue
    const venue = t.display_payee ?? t.payee ?? "";
    const blockKey = `${t.date}|${venue}`;
    let blockEnd = i + 1;
    while (blockEnd < sorted.length) {
      const nt = sorted[blockEnd];
      const ntId = getTxnId(nt);
      if (ntId && tradeLinkMap.has(ntId)) break;
      const nVenue = nt.display_payee ?? nt.payee ?? "";
      if (`${nt.date}|${nVenue}` !== blockKey) break;
      blockEnd++;
    }

    // Sub-group the block by commodity
    if (!selectedAccount) {
      // No account selected — everything is singles
      for (let j = i; j < blockEnd; j++) {
        totalTopLevel++;
        if (topLevelCount < pageSize) {
          items.push({ kind: "single", transaction: sorted[j] });
          topLevelCount++;
        }
      }
      i = blockEnd;
      continue;
    }

    // Sub-group by commodity + narration
    const byKey = new Map<string, Transaction[]>();
    for (let j = i; j < blockEnd; j++) {
      const commodity = postingCommodity(sorted[j], selectedAccount);
      const narration = sorted[j].narration ?? "";
      const subKey = `${commodity}|${narration}`;
      const arr = byKey.get(subKey);
      if (arr) arr.push(sorted[j]);
      else byKey.set(subKey, [sorted[j]]);
    }

    for (const [subKey, txns] of byKey) {
      const [commodity, ...narrationParts] = subKey.split("|");
      const narration = narrationParts.join("|");
      if (txns.length < MIN_GROUP_SIZE) {
        for (const txn of txns) {
          totalTopLevel++;
          if (topLevelCount < pageSize) {
            items.push({ kind: "single", transaction: txn });
            topLevelCount++;
          }
        }
      } else {
        totalTopLevel++;
        if (topLevelCount < pageSize) {
          const first = txns[0];
          let totalIn = 0;
          let totalOut = 0;
          for (const gt of txns) {
            const amt = postingAmount(gt, selectedAccount);
            if (amt >= 0) totalIn += amt;
            else totalOut += Math.abs(amt);
          }
          const key = `${first.date}|${venue}|${commodity}|${narration}`;
          const group: TxGroup = {
            key,
            date: first.date,
            venueName: venue,
            commodity,
            narration,
            transactions: txns,
            netAmount: totalIn - totalOut,
            totalIn,
            totalOut,
          };
          items.push({ kind: "group-header", group });
          topLevelCount++;
          if (expandedGroups.has(key)) {
            for (const gt of txns) {
              items.push({ kind: "group-detail", transaction: gt, group });
            }
          }
        }
      }
    }

    i = blockEnd;
  }

  return { items, totalTopLevel };
}
