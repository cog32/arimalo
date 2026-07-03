// Per-transaction base-currency value computation for the ledger view (the
// "Value" column). Extracted from main.ts so this path is unit-testable: the
// regression it guards against is sourcing values from the wrong transaction
// set, so the Value column renders "—" even though prices resolve fine.
//
// `computeTransactionValues` is a faithful extraction of the former in-main.ts
// `loadTransactionValues` body — same guard, same source — with the Tauri
// `convert_to_base_currency` IPC injected so tests can stub it.

import type { Transaction, TradeLink } from "./types";
import { filterByAccountPrefix } from "./account-search";
import { matchesPrefix, swapPartnerRefFromMeta, txnIdFromMeta, txnValueKey } from "./meta";
import { postingPriceValue } from "./posting-value";
import { valuationLegs } from "./transaction-legs";

export type ConversionRequest = { commodity: string; amount: number; datetime: string };

/** Converts a batch of (commodity, amount, datetime) requests to the base
 *  currency. Injected so main.ts wires it to the Tauri `convert_to_base_currency`
 *  command while tests stub it. */
export type ConvertToBase = (
  baseCurrency: string,
  requests: ConversionRequest[],
) => Promise<(number | null)[]>;

export type LegValuePlan = {
  requests: ConversionRequest[];
  legReqs: { txnIdx: number; sign: number; priceFallback: number | null }[];
  totals: Map<string, { sum: number; has: boolean }>;
};

/** The subset of AppState the value-load reads. Structural so main.ts's AppState
 *  satisfies it without a circular import. */
export interface ValueLoadState {
  displayConfig?: { base_currency?: string } | undefined;
  parse?: { transactions: Transaction[] } | undefined;
  searchFilteredTransactions?: Transaction[] | undefined;
  sidebarView?: string | undefined;
  selectedCategory?: string | undefined;
  selectedAccount?: string | undefined;
  tradeLinks?: TradeLink[] | undefined;
}

/** Plan each transaction's base-currency value from its in-scope legs. Legs
 *  already priced in the base currency (e.g. a property's `@@ … AUD` land /
 *  building legs) are summed immediately; the rest become conversion requests
 *  the caller folds back in. A co-held multi-commodity holding sums to the full
 *  value; in-scope transfers and swaps (e.g. a broker trade's shares + cash
 *  sub-account) value a single side — see `valuationLegs`. `has` stays false
 *  until a leg yields a value, so a fully-unpriced transaction records `null`,
 *  not a misleading 0. */
export function buildLegValueRequests(
  txns: Transaction[],
  account: string,
  baseCurrency: string,
): LegValuePlan {
  const requests: LegValuePlan["requests"] = [];
  const legReqs: LegValuePlan["legReqs"] = [];
  const totals: LegValuePlan["totals"] = new Map();
  txns.forEach((t, i) => {
    const legs = valuationLegs(t, account);
    if (legs.length === 0) return;
    const key = txnValueKey(t);
    const acc = totals.get(key) ?? { sum: 0, has: false };
    totals.set(key, acc);
    for (const posting of legs) {
      const sign = posting.amount < 0 ? -1 : 1;
      const priceVal = postingPriceValue(posting);
      if (priceVal !== null && posting.price?.commodity === baseCurrency) {
        acc.sum += sign * priceVal;
        acc.has = true;
      } else {
        requests.push({ commodity: posting.commodity, amount: Math.abs(posting.amount), datetime: t.datetime });
        legReqs.push({ txnIdx: i, sign, priceFallback: priceVal });
      }
    }
  });
  return { requests, legReqs, totals };
}

/** Fold the converted leg values back into per-transaction totals and produce
 *  the value map (txn key → summed base-currency value, or null if none). */
export function applyLegValues(
  plan: LegValuePlan,
  txns: Transaction[],
  values: (number | null)[],
): Map<string, number | null> {
  const { legReqs, totals } = plan;
  values.forEach((v, j) => {
    const { txnIdx, sign, priceFallback } = legReqs[j];
    const key = txnValueKey(txns[txnIdx]);
    const acc = totals.get(key) ?? { sum: 0, has: false };
    const contribution = v !== null ? v : priceFallback;
    if (contribution !== null) {
      acc.sum += sign * contribution;
      acc.has = true;
    }
    totals.set(key, acc);
  });
  const valMap = new Map<string, number | null>();
  for (const [key, { sum, has }] of totals) {
    valMap.set(key, has ? sum : null);
  }
  return valMap;
}

/** For trade-linked rows, overwrite each leg's value with the partner's
 *  counterparty value so a swap shows the same value whether displayed linked or
 *  individually. Mutates `valMap` in place. */
async function applyTradeLinkPartnerValues(
  valMap: Map<string, number | null>,
  txns: Transaction[],
  account: string,
  baseCurrency: string,
  allTxns: Transaction[],
  tradeLinks: TradeLink[],
  convert: ConvertToBase,
): Promise<void> {
  const linkMap = new Map<string, string>();
  for (const link of tradeLinks) {
    linkMap.set(link.txn_id_a, link.txn_id_b);
    linkMap.set(link.txn_id_b, link.txn_id_a);
  }

  const partnerReqs: { txnIdx: number; commodity: string; amount: number; datetime: string }[] = [];
  txns.forEach((t, i) => {
    const txnId = txnIdFromMeta(t.meta);
    // Saved-link gate (matches the swap-row merge gate in resolveSwapPartner).
    if (!txnId || !linkMap.has(txnId)) return;
    // Rust-stamped partner reference: (hash, commodity) is unique per ledger row.
    const swapRef = swapPartnerRefFromMeta(t.meta);
    if (!swapRef) return;
    const partner = allTxns.find(
      (pt) =>
        pt !== t &&
        txnIdFromMeta(pt.meta) === swapRef.partnerTxnId &&
        pt.amount_commodity === swapRef.partnerCommodity,
    );
    if (!partner) return;
    const pp = partner.postings.find((p) => matchesPrefix(p.account, account));
    if (!pp || pp.commodity === t.postings.find((p) => matchesPrefix(p.account, account))?.commodity) return;
    partnerReqs.push({ txnIdx: i, commodity: pp.commodity, amount: Math.abs(pp.amount), datetime: partner.datetime });
  });

  if (partnerReqs.length === 0) return;
  const partnerValues = await convert(
    baseCurrency,
    partnerReqs.map((r) => ({ commodity: r.commodity, amount: r.amount, datetime: r.datetime })),
  );
  partnerValues.forEach((pv, j) => {
    if (pv === null) return;
    const i = partnerReqs[j].txnIdx;
    const key = txnValueKey(txns[i]);
    const posting = txns[i].postings.find((p) => matchesPrefix(p.account, account));
    const sign = posting && posting.amount < 0 ? -1 : 1;
    valMap.set(key, sign * pv);
  });
}

/** Rows whose Value the ledger view should compute: the windowed rows actually
 *  on screen. `state.parse` is NOT populated in account/search views, so
 *  sourcing from it left every Value cell empty. */
function rowsForValuation(state: ValueLoadState): Transaction[] {
  return state.searchFilteredTransactions ?? [];
}

/** Where to resolve a trade-link swap partner: the full ledger when loaded, else
 *  the windowed rows on screen. */
function partnerLookupRows(state: ValueLoadState): Transaction[] {
  return state.parse?.transactions ?? state.searchFilteredTransactions ?? [];
}

/** Compute the per-transaction base-currency value map for the selected account,
 *  or undefined when there is nothing to value (no base currency / account, or
 *  the account's own commodity already IS the base currency). Behaviour is a
 *  verbatim extraction of the former `loadTransactionValues`. */
export async function computeTransactionValues(
  state: ValueLoadState,
  convert: ConvertToBase,
): Promise<Map<string, number | null> | undefined> {
  const baseCurrency = state.displayConfig?.base_currency;
  const selected = state.sidebarView === "categories" ? state.selectedCategory : state.selectedAccount;
  if (!baseCurrency || !selected) {
    return undefined;
  }

  const account = selected;
  const txns = filterByAccountPrefix(rowsForValuation(state), account);
  // Selected account's commodity already is the base currency → no conversion.
  const samplePosting = txns
    .find((t) => t.postings.some((p) => matchesPrefix(p.account, account)))
    ?.postings.find((p) => matchesPrefix(p.account, account));
  if (samplePosting && samplePosting.commodity === baseCurrency) {
    return undefined;
  }

  const plan = buildLegValueRequests(txns, account, baseCurrency);

  try {
    const values = plan.requests.length > 0 ? await convert(baseCurrency, plan.requests) : [];
    const valMap = applyLegValues(plan, txns, values);

    // Trade-linked rows take their partner's counterparty value. Resolve the
    // partner in the full ledger when loaded, else the windowed rows on screen.
    if (state.tradeLinks?.length) {
      await applyTradeLinkPartnerValues(
        valMap,
        txns,
        account,
        baseCurrency,
        partnerLookupRows(state),
        state.tradeLinks,
        convert,
      );
    }

    return valMap;
  } catch {
    return undefined;
  }
}
