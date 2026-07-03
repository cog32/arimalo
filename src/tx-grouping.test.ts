import { describe, it, expect } from "vitest";
import { paginateWithGroups, type TxRowItem } from "./tx-grouping";
import type { Transaction } from "./types";
import { TX_WINDOW } from "./virtual-scroll";

function txn(overrides: Partial<Transaction> & { postings: Transaction["postings"] }): Transaction {
  const { postings, ...rest } = overrides;
  return {
    date: "2025-11-19",
    datetime: "2025-11-19T16:23:56",
    payee: "Bybit",
    narration: "BUY",
    meta: null,
    postings,
    amount: 0,
    amount_commodity: "USD",
    ...rest,
  };
}

function posting(account: string, amount: number, commodity: string) {
  return { account, amount, commodity };
}

const noTradeLinks = new Map<string, { link: { id: string }; partnerId: string }>();
const noExpanded = new Set<string>();

describe("paginateWithGroups", () => {
  it("groups 3+ transactions with same date+venue+commodity", () => {
    const sorted = [
      txn({ postings: [posting("assets:exchange", 10, "HNT")] }),
      txn({ postings: [posting("assets:exchange", 20, "HNT")] }),
      txn({ postings: [posting("assets:exchange", 30, "HNT")] }),
    ];

    const { items, totalTopLevel } = paginateWithGroups(sorted, "assets:exchange", noTradeLinks, noExpanded, 100);

    expect(totalTopLevel).toBe(1);
    expect(items).toHaveLength(1);
    expect(items[0].kind).toBe("group-header");
    if (items[0].kind === "group-header") {
      expect(items[0].group.transactions).toHaveLength(3);
      expect(items[0].group.totalIn).toBe(60);
      expect(items[0].group.commodity).toBe("HNT");
    }
  });

  it("does not group fewer than 3 transactions", () => {
    const sorted = [
      txn({ postings: [posting("assets:exchange", 10, "HNT")] }),
      txn({ postings: [posting("assets:exchange", 20, "HNT")] }),
    ];

    const { items, totalTopLevel } = paginateWithGroups(sorted, "assets:exchange", noTradeLinks, noExpanded, 100);

    expect(totalTopLevel).toBe(2);
    expect(items).toHaveLength(2);
    expect(items[0].kind).toBe("single");
    expect(items[1].kind).toBe("single");
  });

  it("groups interleaved commodities from same date+venue", () => {
    // This is the Bybit scenario: HNT and USDT trades interleaved
    const sorted = [
      txn({ postings: [posting("assets:exchange", 26.75, "HNT")] }),
      txn({ postings: [posting("assets:exchange", -55.10, "USDT")] }),
      txn({ postings: [posting("assets:exchange", 7.21, "HNT")] }),
      txn({ postings: [posting("assets:exchange", -1.06, "USDT")] }),
      txn({ postings: [posting("assets:exchange", 1.06, "HNT")] }),
      txn({ postings: [posting("assets:exchange", -2.19, "USDT")] }),
      txn({ postings: [posting("assets:exchange", 38.23, "HNT")] }),
      txn({ postings: [posting("assets:exchange", -18.48, "USDT")] }),
    ];

    const { items, totalTopLevel } = paginateWithGroups(sorted, "assets:exchange", noTradeLinks, noExpanded, 100);

    expect(totalTopLevel).toBe(2); // one HNT group + one USDT group
    const headers = items.filter((i): i is Extract<TxRowItem, { kind: "group-header" }> => i.kind === "group-header");
    expect(headers).toHaveLength(2);

    const hntGroup = headers.find((h) => h.group.commodity === "HNT")!;
    expect(hntGroup.group.transactions).toHaveLength(4);
    expect(hntGroup.group.totalIn).toBeCloseTo(73.25);

    const usdtGroup = headers.find((h) => h.group.commodity === "USDT")!;
    expect(usdtGroup.group.transactions).toHaveLength(4);
    expect(usdtGroup.group.totalOut).toBeCloseTo(76.83);
  });

  it("separates groups by date even with same venue+commodity", () => {
    const sorted = [
      txn({ date: "2025-11-18", datetime: "2025-11-18T10:00:00", postings: [posting("assets:ex", 10, "HNT")] }),
      txn({ date: "2025-11-18", datetime: "2025-11-18T10:00:01", postings: [posting("assets:ex", 20, "HNT")] }),
      txn({ date: "2025-11-18", datetime: "2025-11-18T10:00:02", postings: [posting("assets:ex", 30, "HNT")] }),
      txn({ date: "2025-11-19", datetime: "2025-11-19T10:00:00", postings: [posting("assets:ex", 40, "HNT")] }),
      txn({ date: "2025-11-19", datetime: "2025-11-19T10:00:01", postings: [posting("assets:ex", 50, "HNT")] }),
      txn({ date: "2025-11-19", datetime: "2025-11-19T10:00:02", postings: [posting("assets:ex", 60, "HNT")] }),
    ];

    const { items, totalTopLevel } = paginateWithGroups(sorted, "assets:ex", noTradeLinks, noExpanded, 100);

    expect(totalTopLevel).toBe(2); // two groups, one per date
    const headers = items.filter((i) => i.kind === "group-header");
    expect(headers).toHaveLength(2);
  });

  it("excludes trade-linked transactions from groups", () => {
    const tradeLinks = new Map([
      ["txn:linked-1", { link: { id: "link1" }, partnerId: "txn:linked-2" }],
      ["txn:linked-2", { link: { id: "link1" }, partnerId: "txn:linked-1" }],
    ]);
    const sorted = [
      txn({ meta: "txn:linked-1", postings: [posting("assets:ex", 10, "HNT")] }),
      txn({ postings: [posting("assets:ex", 20, "HNT")] }),
      txn({ postings: [posting("assets:ex", 30, "HNT")] }),
      txn({ postings: [posting("assets:ex", 40, "HNT")] }),
      txn({ meta: "txn:linked-2", postings: [posting("assets:ex", 50, "HNT")] }),
    ];

    const { items, totalTopLevel } = paginateWithGroups(sorted, "assets:ex", tradeLinks, noExpanded, 100);

    // 2 trade-linked singles + 1 group of 3
    expect(totalTopLevel).toBe(3);
    const singles = items.filter((i) => i.kind === "single");
    const headers = items.filter((i) => i.kind === "group-header");
    expect(singles).toHaveLength(2);
    expect(headers).toHaveLength(1);
  });

  it("expands group when key is in expandedGroups", () => {
    const sorted = [
      txn({ postings: [posting("assets:ex", 10, "HNT")] }),
      txn({ postings: [posting("assets:ex", 20, "HNT")] }),
      txn({ postings: [posting("assets:ex", 30, "HNT")] }),
    ];
    const expanded = new Set(["2025-11-19|Bybit|HNT|BUY"]);

    const { items } = paginateWithGroups(sorted, "assets:ex", noTradeLinks, expanded, 100);

    expect(items[0].kind).toBe("group-header");
    expect(items[1].kind).toBe("group-detail");
    expect(items[2].kind).toBe("group-detail");
    expect(items[3].kind).toBe("group-detail");
    expect(items).toHaveLength(4); // 1 header + 3 details
  });

  it("respects pageSize for top-level items", () => {
    // 9 transactions = 3 groups of 3
    const sorted = Array.from({ length: 9 }, (_, idx) =>
      txn({
        payee: `Venue${Math.floor(idx / 3)}`,
        postings: [posting("assets:ex", 10, "HNT")],
      }),
    );

    const { items, totalTopLevel } = paginateWithGroups(sorted, "assets:ex", noTradeLinks, noExpanded, 2);

    expect(totalTopLevel).toBe(3); // 3 groups total
    // Only 2 top-level items emitted
    const headers = items.filter((i) => i.kind === "group-header");
    expect(headers).toHaveLength(2);
  });

  it("calculates net amount correctly with in and out", () => {
    const sorted = [
      txn({ postings: [posting("assets:ex", 100, "USDT"), posting("equity:trading", -100, "USDT")] }),
      txn({ postings: [posting("assets:ex", -30, "USDT"), posting("equity:trading", 30, "USDT")] }),
      txn({ postings: [posting("assets:ex", 50, "USDT"), posting("equity:trading", -50, "USDT")] }),
    ];

    const { items } = paginateWithGroups(sorted, "assets:ex", noTradeLinks, noExpanded, 100);

    expect(items[0].kind).toBe("group-header");
    if (items[0].kind === "group-header") {
      expect(items[0].group.totalIn).toBe(150);
      expect(items[0].group.totalOut).toBe(30);
      expect(items[0].group.netAmount).toBe(120);
    }
  });

  it("does not group transactions with different narrations", () => {
    // Same date, venue, commodity but different narrations — should NOT be grouped together
    const sorted = [
      txn({ narration: "SELL SOLUSDT", postings: [posting("assets:exchange", -55, "USDT")] }),
      txn({ narration: "SELL SOLUSDT", postings: [posting("assets:exchange", -30, "USDT")] }),
      txn({ narration: "SELL SOLUSDT", postings: [posting("assets:exchange", -20, "USDT")] }),
      txn({ narration: "Interest USDT", postings: [posting("assets:exchange", 0.01, "USDT")] }),
      txn({ narration: "Interest USDT", postings: [posting("assets:exchange", 0.01, "USDT")] }),
      txn({ narration: "Interest USDT", postings: [posting("assets:exchange", 0.01, "USDT")] }),
    ];

    const { items, totalTopLevel } = paginateWithGroups(sorted, "assets:exchange", noTradeLinks, noExpanded, 100);

    expect(totalTopLevel).toBe(2); // two groups: SELL SOLUSDT (3) + Interest USDT (3)
    const headers = items.filter((i): i is Extract<TxRowItem, { kind: "group-header" }> => i.kind === "group-header");
    expect(headers).toHaveLength(2);
    expect(headers[0].group.narration).toBe("SELL SOLUSDT");
    expect(headers[0].group.transactions).toHaveLength(3);
    expect(headers[1].group.narration).toBe("Interest USDT");
    expect(headers[1].group.transactions).toHaveLength(3);
  });

  it("groups transactions with same narration together", () => {
    const sorted = [
      txn({ narration: "SELL SOLUSDT", postings: [posting("assets:exchange", -55, "USDT")] }),
      txn({ narration: "SELL SOLUSDT", postings: [posting("assets:exchange", -30, "USDT")] }),
      txn({ narration: "SELL SOLUSDT", postings: [posting("assets:exchange", -20, "USDT")] }),
    ];

    const { items, totalTopLevel } = paginateWithGroups(sorted, "assets:exchange", noTradeLinks, noExpanded, 100);

    expect(totalTopLevel).toBe(1);
    const headers = items.filter((i): i is Extract<TxRowItem, { kind: "group-header" }> => i.kind === "group-header");
    expect(headers).toHaveLength(1);
    expect(headers[0].group.narration).toBe("SELL SOLUSDT");
    expect(headers[0].group.transactions).toHaveLength(3);
  });

  it("rejects Infinity as pageSize", () => {
    expect(() =>
      paginateWithGroups([], "", noTradeLinks, noExpanded, Infinity),
    ).toThrow();
  });

  it("caps output at TX_WINDOW top-level items for large datasets", () => {
    // Create TX_WINDOW + 100 transactions with unique datetimes to avoid grouping
    const sorted = Array.from({ length: TX_WINDOW + 100 }, (_, i) =>
      txn({
        date: `2025-01-${String(1 + Math.floor(i / 50)).padStart(2, "0")}`,
        datetime: `2025-01-${String(1 + Math.floor(i / 50)).padStart(2, "0")}T${String(Math.floor(i / 60)).padStart(2, "0")}:${String(i % 60).padStart(2, "0")}:00`,
        payee: `Venue${i}`,
        postings: [posting("assets:exchange", 10, "HNT")],
      }),
    );
    const { items } = paginateWithGroups(sorted, "assets:exchange", noTradeLinks, noExpanded, TX_WINDOW);
    const topLevel = items.filter((i) => i.kind !== "group-detail");
    expect(topLevel.length).toBeLessThanOrEqual(TX_WINDOW);
  });

  it("uses display_payee for venue name when available", () => {
    const sorted = [
      txn({ payee: "0xabc", display_payee: "Uniswap", postings: [posting("assets:ex", 10, "ETH")] }),
      txn({ payee: "0xdef", display_payee: "Uniswap", postings: [posting("assets:ex", 20, "ETH")] }),
      txn({ payee: "0xghi", display_payee: "Uniswap", postings: [posting("assets:ex", 30, "ETH")] }),
    ];

    const { items } = paginateWithGroups(sorted, "assets:ex", noTradeLinks, noExpanded, 100);

    expect(items[0].kind).toBe("group-header");
    if (items[0].kind === "group-header") {
      expect(items[0].group.venueName).toBe("Uniswap");
    }
  });

  // Cross-scope visibility: a transaction must show the same gross magnitude
  // at every scope that contains it. Internal transfers (both legs inside the
  // scope) must NOT collapse to zero — that would hide unmatched/half-recorded
  // transfers, which are exactly the rows the user needs to see.
  describe("cross-scope amount consistency", () => {
    it("shows gross magnitude at the deeper scope (only one leg in scope)", () => {
      const sorted = [
        txn({ payee: "Binance", narration: "transfer", postings: [
          posting("assets:crypto:wallet:solana:abc", 7999.99, "SOL"),
          posting("assets:crypto:transfer", -7999.99, "SOL"),
        ]}),
        txn({ payee: "Binance", narration: "transfer", postings: [
          posting("assets:crypto:wallet:solana:abc", 1.0, "SOL"),
          posting("assets:crypto:transfer", -1.0, "SOL"),
        ]}),
        txn({ payee: "Binance", narration: "transfer", postings: [
          posting("assets:crypto:wallet:solana:abc", 0.5, "SOL"),
          posting("assets:crypto:transfer", -0.5, "SOL"),
        ]}),
      ];

      const { items } = paginateWithGroups(sorted, "assets:crypto:wallet:solana", noTradeLinks, noExpanded, 100);
      const header = items.find((i): i is Extract<TxRowItem, { kind: "group-header" }> => i.kind === "group-header")!;
      expect(header).toBeDefined();
      expect(header.group.totalIn).toBeCloseTo(8001.49);
      expect(header.group.totalOut).toBe(0);
    });

    it("shows the same gross magnitude at the broader scope (both legs in scope)", () => {
      // Reproduces the bug report. Same data as the test above, broader scope.
      // The user must see the 7999.99 SOL movement so they can fix the missing
      // outgoing leg on Binance — hiding it with `0` would defeat the ledger.
      const sorted = [
        txn({ payee: "Binance", narration: "transfer", postings: [
          posting("assets:crypto:wallet:solana:abc", 7999.99, "SOL"),
          posting("assets:crypto:transfer", -7999.99, "SOL"),
        ]}),
        txn({ payee: "Binance", narration: "transfer", postings: [
          posting("assets:crypto:wallet:solana:abc", 1.0, "SOL"),
          posting("assets:crypto:transfer", -1.0, "SOL"),
        ]}),
        txn({ payee: "Binance", narration: "transfer", postings: [
          posting("assets:crypto:wallet:solana:abc", 0.5, "SOL"),
          posting("assets:crypto:transfer", -0.5, "SOL"),
        ]}),
      ];

      const { items } = paginateWithGroups(sorted, "assets:crypto", noTradeLinks, noExpanded, 100);
      const header = items.find((i): i is Extract<TxRowItem, { kind: "group-header" }> => i.kind === "group-header")!;
      expect(header).toBeDefined();
      // Magnitude must match the deeper-scope view, NOT zero out.
      expect(header.group.totalIn).toBeCloseTo(8001.49);
      expect(header.group.totalOut).toBe(0);
    });

    it("preserves direction of buys at the asset scope (one leg in, equity contra out)", () => {
      const sorted = [
        txn({ payee: "Binance", narration: "buy", postings: [
          posting("assets:crypto:wallet:solana:abc", 1.0, "SOL"),
          posting("equity:trading:buy:trade-1", -1.0, "SOL"),
        ]}),
        txn({ payee: "Binance", narration: "buy", postings: [
          posting("assets:crypto:wallet:solana:abc", 2.0, "SOL"),
          posting("equity:trading:buy:trade-2", -2.0, "SOL"),
        ]}),
        txn({ payee: "Binance", narration: "buy", postings: [
          posting("assets:crypto:wallet:solana:abc", 3.0, "SOL"),
          posting("equity:trading:buy:trade-3", -3.0, "SOL"),
        ]}),
      ];

      const { items } = paginateWithGroups(sorted, "assets", noTradeLinks, noExpanded, 100);
      const header = items.find((i): i is Extract<TxRowItem, { kind: "group-header" }> => i.kind === "group-header")!;
      expect(header).toBeDefined();
      expect(header.group.totalIn).toBe(6);
      expect(header.group.totalOut).toBe(0);
    });
  });
});
