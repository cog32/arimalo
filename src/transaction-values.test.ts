import { describe, expect, it } from "vitest";
import {
  buildLegValueRequests,
  computeTransactionValues,
  type ConvertToBase,
  type ValueLoadState,
} from "./transaction-values";
import { txnValueKey } from "./meta";
import type { Transaction } from "./types";

const WALLET = "assets:crypto:wallet:ethereum:0x6d25";
const PERP_QTY = 985.0348891476083;
const PERP_AUD = 0.0248; // ~swap-derived AUD/PERP

/** A PERP disposal in WALLET (wallet leg + equity:trading:sell contra), no price
 *  annotation — so its value comes from a `convert_to_base_currency` call. */
function perpSale(): Transaction {
  return {
    date: "2026-06-26",
    datetime: "2026-06-26 00:23:23",
    status: "*",
    payee: "Uniswap",
    narration: "token_transfer:trade PERP",
    meta: "txn:0x34ed",
    postings: [
      { account: WALLET, amount: -PERP_QTY, amount_text: `${-PERP_QTY}`, commodity: "PERP" },
      { account: "equity:trading:sell", amount: PERP_QTY, amount_text: `${PERP_QTY}`, commodity: "PERP" },
    ],
    amount: -PERP_QTY,
    amount_commodity: "PERP",
  };
}

/** Stub: every requested commodity amount → amount × PERP_AUD. */
const convertStub: ConvertToBase = async (_baseCurrency, requests) =>
  requests.map((r) => r.amount * PERP_AUD);

describe("buildLegValueRequests", () => {
  it("emits one conversion request for an unpriced in-scope leg", () => {
    const plan = buildLegValueRequests([perpSale()], WALLET, "AUD");
    expect(plan.requests).toEqual([
      { commodity: "PERP", amount: PERP_QTY, datetime: "2026-06-26 00:23:23" },
    ]);
  });
});

describe("computeTransactionValues", () => {
  it("values an account's rows via the injected converter (disposal → negative)", async () => {
    const txns = [perpSale()];
    const state: ValueLoadState = {
      displayConfig: { base_currency: "AUD" },
      parse: { transactions: txns },
      searchFilteredTransactions: txns,
      sidebarView: "accounts",
      selectedAccount: WALLET,
      tradeLinks: [],
    };
    const result = await computeTransactionValues(state, convertStub);
    expect(result).toBeDefined();
    expect(result?.get(txnValueKey(txns[0]))).toBeCloseTo(-PERP_QTY * PERP_AUD, 3);
  });

  it("returns undefined when no account is selected", async () => {
    const state: ValueLoadState = {
      displayConfig: { base_currency: "AUD" },
      parse: { transactions: [perpSale()] },
      sidebarView: "accounts",
      selectedAccount: undefined,
    };
    expect(await computeTransactionValues(state, convertStub)).toBeUndefined();
  });

  // REGRESSION GUARD for the empty "Value" column. In a windowed account/search
  // view `state.parse` is NOT populated — the rendered rows live in
  // `state.searchFilteredTransactions`, which is where the value-load now sources
  // from. Before the fix this failed (the load sourced from `state.parse` and
  // bailed on `!state.parse`), leaving every Value cell empty.
  it(
    "populates values from the on-screen (searchFiltered) rows when state.parse is empty",
    async () => {
      const txns = [perpSale()];
      const state: ValueLoadState = {
        displayConfig: { base_currency: "AUD" },
        parse: undefined, // windowed account/search view — not loaded
        searchFilteredTransactions: txns, // the rows actually rendered
        sidebarView: "accounts",
        selectedAccount: WALLET,
        tradeLinks: [],
      };
      const result = await computeTransactionValues(state, convertStub);
      expect(result).toBeDefined();
      expect(result?.get(txnValueKey(txns[0]))).toBeCloseTo(-PERP_QTY * PERP_AUD, 3);
    },
  );
});
