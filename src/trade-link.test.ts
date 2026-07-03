import { describe, it, expect } from "vitest";
import { resetStateForAccountNavigation, detectTradePairs, detectGroupTradePairs, sortTxnIdsByAbsAmount } from "./account-utils";
import { paginateWithGroups, buildTxnIdMap } from "./tx-grouping";
import type { TxRowItem, TxGroup } from "./tx-grouping";
import type { Transaction } from "./types";
import { postingPriceValue } from "./posting-value";

describe("trade link selection during account navigation", () => {
  it("should preserve tradeLinkSelection when navigating to a different account", () => {
    const state = {
      selectedAccount: "assets:crypto:wallet:solana:abc",
      prefixQuery: undefined as unknown,
      transactionValues: undefined as unknown,
      accountTotalValue: undefined as unknown,
      tradeLinkSelection: "txn:csv-sell-eth-123",
      txWindowStart: 50,
      txExpandedGroups: new Set<string>(["g1"]),
    };

    resetStateForAccountNavigation(state, "assets:crypto:wallet:solana:def");

    expect(state.selectedAccount).toBe("assets:crypto:wallet:solana:def");
    expect(state.tradeLinkSelection).toBe("txn:csv-sell-eth-123");
    expect(state.txWindowStart).toBe(0);
    expect(state.txExpandedGroups.size).toBe(0);
  });

  it("should reset other navigation state when changing accounts", () => {
    const state = {
      selectedAccount: "assets:crypto:wallet:solana:abc",
      prefixQuery: { some: "query" } as unknown,
      transactionValues: new Map() as unknown,
      accountTotalValue: { total: 100 } as unknown,
      tradeLinkSelection: undefined as string | undefined,
      txWindowStart: 50,
      txExpandedGroups: new Set<string>(["g1"]),
    };

    resetStateForAccountNavigation(state, "assets:crypto:exchange:kraken");

    expect(state.selectedAccount).toBe("assets:crypto:exchange:kraken");
    expect(state.prefixQuery).toBeUndefined();
    expect(state.transactionValues).toBeUndefined();
    expect(state.accountTotalValue).toBeUndefined();
    expect(state.txWindowStart).toBe(0);
  });
});

describe("detectTradePairs", () => {
  it("detects a pair with same datetime, different commodities, opposite signs", () => {
    const txns = [
      { datetime: "2024-10-17 05:47:34", amount: -6.78, amount_commodity: "SOL" },
      { datetime: "2024-10-17 05:47:34", amount: 1763.02, amount_commodity: "SKBDI" },
    ];
    const pairs = detectTradePairs(txns);
    expect(pairs.has(0)).toBe(true);
    expect(pairs.size).toBe(1);
  });

  it("does not pair transactions with different datetimes", () => {
    const txns = [
      { datetime: "2024-10-17 05:47:34", amount: -6.78, amount_commodity: "SOL" },
      { datetime: "2024-10-17 05:47:35", amount: 1763.02, amount_commodity: "SKBDI" },
    ];
    expect(detectTradePairs(txns).size).toBe(0);
  });

  it("does not pair transactions with same commodity", () => {
    const txns = [
      { datetime: "2024-10-17 05:47:34", amount: -6.78, amount_commodity: "SOL" },
      { datetime: "2024-10-17 05:47:34", amount: 3.22, amount_commodity: "SOL" },
    ];
    expect(detectTradePairs(txns).size).toBe(0);
  });

  it("does not pair transactions with same sign", () => {
    const txns = [
      { datetime: "2024-10-17 05:47:34", amount: 6.78, amount_commodity: "SOL" },
      { datetime: "2024-10-17 05:47:34", amount: 1763.02, amount_commodity: "SKBDI" },
    ];
    expect(detectTradePairs(txns).size).toBe(0);
  });

  it("skips zero-amount transactions", () => {
    const txns = [
      { datetime: "2024-10-17 05:47:34", amount: 0, amount_commodity: "SOL" },
      { datetime: "2024-10-17 05:47:34", amount: 1763.02, amount_commodity: "SKBDI" },
    ];
    expect(detectTradePairs(txns).size).toBe(0);
  });

  it("detects multiple pairs in a sequence", () => {
    const txns = [
      { datetime: "2024-10-17 05:47:34", amount: -6.78, amount_commodity: "SOL" },
      { datetime: "2024-10-17 05:47:34", amount: 1763.02, amount_commodity: "SKBDI" },
      { datetime: "2024-10-17 05:48:00", amount: -100, amount_commodity: "USDC" },
      { datetime: "2024-10-17 05:48:00", amount: 0.5, amount_commodity: "ETH" },
    ];
    const pairs = detectTradePairs(txns);
    expect(pairs.has(0)).toBe(true);
    expect(pairs.has(2)).toBe(true);
    expect(pairs.size).toBe(2);
  });

  it("does not pair non-adjacent transactions separated by a gas fee", () => {
    const txns = [
      { datetime: "2024-10-17 05:47:34", amount: -6.78, amount_commodity: "SOL" },
      { datetime: "2024-10-17 05:47:34", amount: 0.001, amount_commodity: "SOL" }, // gas fee between them
      { datetime: "2024-10-17 05:47:34", amount: 1763.02, amount_commodity: "SKBDI" },
    ];
    const pairs = detectTradePairs(txns);
    // 0-1: same commodity → no. 1-2: same sign (both positive) → no.
    expect(pairs.size).toBe(0);
  });
});

describe("value fallback from price annotation", () => {
  it("uses posting price annotation when PriceGraph returns null", () => {
    const posting = {
      account: "assets:wallet",
      amount: -63934.19,
      commodity: "MNGO",
      price: { is_total: false, amount: 0.0228, amount_text: "0.0228", commodity: "AUD" },
    };
    const backendValue = null;
    const priceVal = postingPriceValue(posting);
    const sign = posting.amount < 0 ? -1 : 1;
    let computedValue: number | null = null;
    if (backendValue !== null) {
      computedValue = backendValue;
    } else if (priceVal !== null) {
      computedValue = sign * priceVal;
    }
    expect(computedValue).not.toBeNull();
    expect(computedValue).toBeCloseTo(-1457.70, 0);
  });

  it("prefers price annotation in base currency over PriceGraph value", () => {
    const posting = {
      account: "assets:wallet",
      amount: -19029,
      commodity: "PENGU",
      price: { is_total: false, amount: 0.010594, amount_text: "0.010594", commodity: "AUD" },
    };
    const baseCurrency = "AUD";
    const backendValue = 151.52;
    const sign = posting.amount < 0 ? -1 : 1;
    const priceVal = postingPriceValue(posting);
    let computedValue: number | null = null;
    if (priceVal !== null && posting.price?.commodity === baseCurrency) {
      computedValue = sign * priceVal;
    } else if (backendValue !== null) {
      computedValue = sign * backendValue;
    }
    expect(computedValue).toBeCloseTo(-201.61, 0);
  });

  it("prefers PriceGraph when annotation commodity differs from base currency", () => {
    const posting = {
      account: "assets:wallet",
      amount: -19029,
      commodity: "PENGU",
      price: { is_total: false, amount: 0.0067, amount_text: "0.0067", commodity: "USD" },
    };
    const baseCurrency = "AUD";
    const backendValue = 151.52;
    const sign = posting.amount < 0 ? -1 : 1;
    const priceVal = postingPriceValue(posting);
    let computedValue: number | null = null;
    if (priceVal !== null && posting.price?.commodity === baseCurrency) {
      computedValue = sign * priceVal;
    } else if (backendValue !== null) {
      computedValue = sign * backendValue;
    }
    expect(computedValue).toBeCloseTo(-151.52, 0);
  });

  it("returns null when neither PriceGraph nor annotation available", () => {
    const posting = {
      account: "assets:wallet",
      amount: 1000,
      commodity: "UNKNOWN",
    };
    const backendValue = null;
    const priceVal = postingPriceValue(posting as any);
    let computedValue: number | null = null;
    if (backendValue !== null) {
      computedValue = backendValue;
    } else if (priceVal !== null) {
      computedValue = priceVal;
    }
    expect(computedValue).toBeNull();
  });

  it("honours @@ (is_total) annotation as the total, not per-unit", () => {
    const posting = {
      account: "assets:equity:broker:commsec:personal",
      amount: 10000,
      commodity: "BQT",
      price: { is_total: true, amount: 3650, amount_text: "3650.00", commodity: "AUD" },
    };
    const baseCurrency = "AUD";
    const backendValue = null;
    const sign = posting.amount < 0 ? -1 : 1;
    const priceVal = postingPriceValue(posting);
    let computedValue: number | null = null;
    if (priceVal !== null && posting.price.commodity === baseCurrency) {
      computedValue = sign * priceVal;
    } else if (backendValue !== null) {
      computedValue = sign * backendValue;
    } else if (priceVal !== null) {
      computedValue = sign * priceVal;
    }
    expect(computedValue).toBe(3650);
  });
});

describe("buildTxnIdMap", () => {
  function makeTxn(meta: string, commodity: string, amount: number): Transaction {
    return {
      date: "2024-10-17", datetime: "2024-10-17 05:47:34",
      payee: "Exchange", narration: "trade",
      meta, amount, amount_commodity: commodity,
      postings: [{ account: "assets:wallet", amount, commodity }],
    };
  }

  it("indexes transactions by txn: metadata", () => {
    const txnA = makeTxn("txn:abc123", "SOL", -6.78);
    const txnB = makeTxn("txn:def456", "USDC", 954);
    const map = buildTxnIdMap([txnA, txnB]);
    expect(map.get("txn:abc123")).toEqual([txnA]);
    expect(map.get("txn:def456")).toEqual([txnB]);
  });

  it("groups multiple transactions with same txn ID", () => {
    const txnA = makeTxn("txn:same-hash", "MNGO", -63934);
    const txnB = makeTxn("txn:same-hash", "USDC", 954);
    const map = buildTxnIdMap([txnA, txnB]);
    expect(map.get("txn:same-hash")).toEqual([txnA, txnB]);
  });

  it("skips transactions without txn: metadata", () => {
    const txnA = makeTxn("txn:has-id", "SOL", 10);
    const txnB = makeTxn("", "USDC", 20);
    const txnC = makeTxn("other-tag", "ETH", 30);
    const map = buildTxnIdMap([txnA, txnB, txnC]);
    expect(map.size).toBe(1);
    expect(map.get("txn:has-id")).toEqual([txnA]);
  });

  it("handles comma-separated meta with txn: tag", () => {
    const txnA = makeTxn("source:bybit, txn:multi-meta", "HNT", 26.75);
    const map = buildTxnIdMap([txnA]);
    expect(map.get("txn:multi-meta")).toEqual([txnA]);
  });
});

describe("swap partner lookup via map", () => {
  function makeTxn(meta: string, commodity: string, amount: number): Transaction {
    return {
      date: "2024-10-17", datetime: "2024-10-17 05:47:34",
      payee: "Exchange", narration: "trade",
      meta, amount, amount_commodity: commodity,
      postings: [{ account: "assets:wallet", amount, commodity }],
    };
  }

  it("finds partner with different commodity via map lookup", () => {
    const txnA = makeTxn("txn:same-hash", "MNGO", -63934);
    const txnB = makeTxn("txn:same-hash", "USDC", 954);
    const map = buildTxnIdMap([txnA, txnB]);
    const partnerId = "txn:same-hash";

    const partner = map.get(partnerId)?.find(
      (pt) => pt !== txnA && pt.amount_commodity !== txnA.amount_commodity,
    );

    expect(partner).toBe(txnB);
  });

  it("does not match self even with same txn ID", () => {
    const txnA = makeTxn("txn:same-hash", "MNGO", -63934);
    const map = buildTxnIdMap([txnA]);
    const partnerId = "txn:same-hash";

    const partner = map.get(partnerId)?.find(
      (pt) => pt !== txnA && pt.amount_commodity !== txnA.amount_commodity,
    );

    expect(partner).toBeUndefined();
  });

  it("does not match partner with same commodity", () => {
    const txnA = makeTxn("txn:hash-a", "SOL", -6.78);
    const txnB = makeTxn("txn:hash-a", "SOL", 0.001);
    const map = buildTxnIdMap([txnA, txnB]);
    const partnerId = "txn:hash-a";

    const partner = map.get(partnerId)?.find(
      (pt) => pt !== txnA && pt.amount_commodity !== txnA.amount_commodity,
    );

    expect(partner).toBeUndefined();
  });

  it("renderKey prevents duplicate rendering of same-ID swap pairs", () => {
    const rendered = new Set<string>();
    const txnA = makeTxn("txn:same-hash", "MNGO", -63934);
    const txnB = makeTxn("txn:same-hash", "USDC", 954);

    // First row (MNGO) renders and marks partner
    const renderKeyA = `txn:same-hash|MNGO`;
    expect(rendered.has(renderKeyA)).toBe(false);
    rendered.add(`txn:same-hash|USDC`); // mark partner

    // Second row (USDC) should be skipped
    const renderKeyB = `txn:same-hash|USDC`;
    expect(rendered.has(renderKeyB)).toBe(true);
  });
});

function makeGroup(overrides: Partial<TxGroup> & { commodity: string; netAmount: number }): TxGroup {
  return {
    key: `2025-11-13|Bybit|${overrides.commodity}|`,
    date: "2025-11-13",
    venueName: "Bybit",
    narration: "",
    transactions: [],
    totalIn: 0,
    totalOut: 0,
    ...overrides,
  };
}

describe("detectGroupTradePairs", () => {
  it("detects adjacent group-headers with same date+venue, different commodities, opposite net amounts", () => {
    const items: TxRowItem[] = [
      { kind: "group-header", group: makeGroup({ commodity: "HNT", netAmount: 80.24 }) },
      { kind: "group-header", group: makeGroup({ commodity: "USDT", netAmount: -178.46 }) },
    ];
    const pairs = detectGroupTradePairs(items);
    expect(pairs.has(0)).toBe(true);
    expect(pairs.size).toBe(1);
  });

  it("does not pair groups with same commodity", () => {
    const items: TxRowItem[] = [
      { kind: "group-header", group: makeGroup({ commodity: "HNT", netAmount: 80.24 }) },
      { kind: "group-header", group: makeGroup({ commodity: "HNT", netAmount: -10 }) },
    ];
    const pairs = detectGroupTradePairs(items);
    expect(pairs.size).toBe(0);
  });

  it("does not pair groups with same net sign", () => {
    const items: TxRowItem[] = [
      { kind: "group-header", group: makeGroup({ commodity: "HNT", netAmount: 80.24 }) },
      { kind: "group-header", group: makeGroup({ commodity: "USDT", netAmount: 178.46 }) },
    ];
    const pairs = detectGroupTradePairs(items);
    expect(pairs.size).toBe(0);
  });

  it("does not pair groups with different dates", () => {
    const items: TxRowItem[] = [
      { kind: "group-header", group: makeGroup({ date: "2025-11-13", commodity: "HNT", netAmount: 80.24 }) },
      { kind: "group-header", group: makeGroup({ date: "2025-11-14", commodity: "USDT", netAmount: -178.46 }) },
    ];
    const pairs = detectGroupTradePairs(items);
    expect(pairs.size).toBe(0);
  });

  it("does not pair groups with different venues", () => {
    const items: TxRowItem[] = [
      { kind: "group-header", group: makeGroup({ venueName: "Bybit", commodity: "HNT", netAmount: 80.24 }) },
      { kind: "group-header", group: makeGroup({ venueName: "Binance", commodity: "USDT", netAmount: -178.46 }) },
    ];
    const pairs = detectGroupTradePairs(items);
    expect(pairs.size).toBe(0);
  });

  it("does not pair a group-header with a single item", () => {
    const items: TxRowItem[] = [
      { kind: "group-header", group: makeGroup({ commodity: "HNT", netAmount: 80.24 }) },
      { kind: "single", transaction: { date: "2025-11-13", datetime: "2025-11-13T18:44:35", payee: "Bybit", narration: "BUY", meta: null, postings: [], amount: -178.46, amount_commodity: "USDT" } },
    ];
    const pairs = detectGroupTradePairs(items);
    expect(pairs.size).toBe(0);
  });

  it("skips groups where net amount is zero", () => {
    const items: TxRowItem[] = [
      { kind: "group-header", group: makeGroup({ commodity: "HNT", netAmount: 0 }) },
      { kind: "group-header", group: makeGroup({ commodity: "USDT", netAmount: -178.46 }) },
    ];
    const pairs = detectGroupTradePairs(items);
    expect(pairs.size).toBe(0);
  });
});

function makeTxnWithPosting(meta: string, account: string, amount: number, commodity: string): Transaction {
  return {
    date: "2025-11-13",
    datetime: "2025-11-13T18:44:35",
    payee: "Bybit",
    narration: "BUY HNTUSDT",
    meta,
    postings: [{ account, amount, commodity }],
    amount,
    amount_commodity: commodity,
  };
}

describe("sortTxnIdsByAbsAmount", () => {
  it("sorts transaction IDs by absolute posting amount ascending", () => {
    const txns = [
      makeTxnWithPosting("txn:big", "assets:ex", 26.75, "HNT"),
      makeTxnWithPosting("txn:small", "assets:ex", 7.20, "HNT"),
      makeTxnWithPosting("txn:mid", "assets:ex", 9.96, "HNT"),
    ];
    const ids = sortTxnIdsByAbsAmount(txns, "assets:ex");
    expect(ids).toEqual(["txn:small", "txn:mid", "txn:big"]);
  });

  it("filters out transactions without txn: meta", () => {
    const txns = [
      makeTxnWithPosting("txn:has-id", "assets:ex", 10, "HNT"),
      makeTxnWithPosting("", "assets:ex", 20, "HNT"),
      makeTxnWithPosting("other-tag", "assets:ex", 30, "HNT"),
    ];
    const ids = sortTxnIdsByAbsAmount(txns, "assets:ex");
    expect(ids).toEqual(["txn:has-id"]);
  });

  it("uses absolute amount for negative postings", () => {
    const txns = [
      makeTxnWithPosting("txn:big-sell", "assets:ex", -59.22, "USDT"),
      makeTxnWithPosting("txn:small-sell", "assets:ex", -16.03, "USDT"),
    ];
    const ids = sortTxnIdsByAbsAmount(txns, "assets:ex");
    expect(ids).toEqual(["txn:small-sell", "txn:big-sell"]);
  });
});

describe("group trade pair integration", () => {
  const noTradeLinks = new Map<string, { link: { id: string }; partnerId: string }>();
  const noExpanded = new Set<string>();

  it("multi-fill produces two group-headers detected as a pair", () => {
    const sorted: Transaction[] = [
      makeTxnWithPosting("txn:hnt1", "assets:ex", 26.75, "HNT"),
      makeTxnWithPosting("txn:hnt2", "assets:ex", 9.96, "HNT"),
      makeTxnWithPosting("txn:hnt3", "assets:ex", 7.20, "HNT"),
      makeTxnWithPosting("txn:usdt1", "assets:ex", -59.22, "USDT"),
      makeTxnWithPosting("txn:usdt2", "assets:ex", -22.15, "USDT"),
      makeTxnWithPosting("txn:usdt3", "assets:ex", -16.03, "USDT"),
    ];

    const { items } = paginateWithGroups(sorted, "assets:ex", noTradeLinks, noExpanded, 100);
    const groupPairs = detectGroupTradePairs(items);
    expect(groupPairs.size).toBe(1);
    expect(groupPairs.has(0)).toBe(true);
  });

  it("after linking, trade-linked txns are excluded from groups", () => {
    const tradeLinks = new Map([
      ["txn:hnt1", { link: { id: "l1" }, partnerId: "txn:usdt1" }],
      ["txn:usdt1", { link: { id: "l1" }, partnerId: "txn:hnt1" }],
      ["txn:hnt2", { link: { id: "l2" }, partnerId: "txn:usdt2" }],
      ["txn:usdt2", { link: { id: "l2" }, partnerId: "txn:hnt2" }],
      ["txn:hnt3", { link: { id: "l3" }, partnerId: "txn:usdt3" }],
      ["txn:usdt3", { link: { id: "l3" }, partnerId: "txn:hnt3" }],
    ]);
    const sorted: Transaction[] = [
      makeTxnWithPosting("txn:hnt1", "assets:ex", 26.75, "HNT"),
      makeTxnWithPosting("txn:hnt2", "assets:ex", 9.96, "HNT"),
      makeTxnWithPosting("txn:hnt3", "assets:ex", 7.20, "HNT"),
      makeTxnWithPosting("txn:usdt1", "assets:ex", -59.22, "USDT"),
      makeTxnWithPosting("txn:usdt2", "assets:ex", -22.15, "USDT"),
      makeTxnWithPosting("txn:usdt3", "assets:ex", -16.03, "USDT"),
    ];

    const { items } = paginateWithGroups(sorted, "assets:ex", tradeLinks, noExpanded, 100);
    // All trade-linked transactions should be singles, no group headers
    const headers = items.filter((i) => i.kind === "group-header");
    expect(headers).toHaveLength(0);
    const singles = items.filter((i) => i.kind === "single");
    expect(singles).toHaveLength(6);
  });

  it("unequal group sizes: only min(sell, buy) pairs possible", () => {
    const sellIds = sortTxnIdsByAbsAmount([
      makeTxnWithPosting("txn:s1", "assets:ex", -16.03, "USDT"),
      makeTxnWithPosting("txn:s2", "assets:ex", -22.15, "USDT"),
      makeTxnWithPosting("txn:s3", "assets:ex", -59.22, "USDT"),
    ], "assets:ex");
    const buyIds = sortTxnIdsByAbsAmount([
      makeTxnWithPosting("txn:b1", "assets:ex", 7.20, "HNT"),
      makeTxnWithPosting("txn:b2", "assets:ex", 26.75, "HNT"),
    ], "assets:ex");
    const pairCount = Math.min(sellIds.length, buyIds.length);
    expect(pairCount).toBe(2);
    expect(sellIds).toHaveLength(3);
    expect(buyIds).toHaveLength(2);
  });
});
