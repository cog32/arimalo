import { describe, it, expect } from "vitest";
import {
  filterPendingDeletes,
  pruneExpandedKeysForTxn,
  resetSearchPaging,
  buildOptimisticTransaction,
  pendingAddsForView,
  applyPendingBalanceDelta,
  nextOptimisticTempId,
} from "./optimistic";
import type { BuiltPosting } from "./account-utils";
import type { Transaction } from "./types";

type T = { meta?: string | null; payee: string };

describe("filterPendingDeletes", () => {
  it("returns the input array unchanged when the set is empty", () => {
    const txns: T[] = [
      { meta: "txn:a", payee: "x" },
      { meta: "txn:b", payee: "y" },
    ];
    expect(filterPendingDeletes(txns, new Set())).toBe(txns);
  });

  it("removes txns whose id is in the pending set", () => {
    const txns: T[] = [
      { meta: "txn:a", payee: "x" },
      { meta: "txn:b", payee: "y" },
      { meta: "txn:c", payee: "z" },
    ];
    const out = filterPendingDeletes(txns, new Set(["txn:b"]));
    expect(out).toHaveLength(2);
    expect(out.map((t) => t.payee)).toEqual(["x", "z"]);
  });

  it("keeps txns whose meta does not contain a txn id", () => {
    const txns: T[] = [
      { meta: "no-id-here", payee: "x" },
      { meta: null, payee: "y" },
      { meta: undefined, payee: "z" },
    ];
    expect(filterPendingDeletes(txns, new Set(["txn:abc"]))).toHaveLength(3);
  });

  it("matches when txn:id is buried in a comma-separated meta string", () => {
    const txns: T[] = [
      { meta: "ofx_id:foo, txn:abc, payee:Bar", payee: "x" },
    ];
    expect(filterPendingDeletes(txns, new Set(["txn:abc"]))).toHaveLength(0);
  });

  it("removes multiple matching txns in one pass", () => {
    const txns: T[] = [
      { meta: "txn:a", payee: "x" },
      { meta: "txn:b", payee: "y" },
      { meta: "txn:c", payee: "z" },
    ];
    const out = filterPendingDeletes(txns, new Set(["txn:a", "txn:c"]));
    expect(out.map((t) => t.payee)).toEqual(["y"]);
  });
});

describe("pruneExpandedKeysForTxn — keep txExpandedRows in sync with delete", () => {
  it("removes the single expandKey for a deleted single-leg txn", () => {
    const expanded = new Set(["txn:a|2025-01-01|10|note", "txn:b|2025-01-02|5|other"]);
    pruneExpandedKeysForTxn(expanded, "txn:a");
    expect([...expanded]).toEqual(["txn:b|2025-01-02|5|other"]);
  });

  it("removes ALL expandKeys for a multi-leg txn (every leg shares the txn id)", () => {
    // Regression: a single on-chain txn renders as multiple <tr>s
    // (different postings → different expandKeys). hide_transaction
    // hides the whole txn, so every leg's expandKey must go too — or
    // the surviving stale entry surfaces the row when it re-appears
    // (e.g. show-ignored toggled back on).
    const expanded = new Set([
      "txn:swap|2025-01-01|10|leg-out",
      "txn:swap|2025-01-01|10|leg-in",
      "txn:other|2025-01-02|5|x",
    ]);
    pruneExpandedKeysForTxn(expanded, "txn:swap");
    expect([...expanded]).toEqual(["txn:other|2025-01-02|5|x"]);
  });

  it("matches a key that equals the txnId exactly (no datetime suffix)", () => {
    const expanded = new Set(["txn:a", "txn:b|x"]);
    pruneExpandedKeysForTxn(expanded, "txn:a");
    expect([...expanded]).toEqual(["txn:b|x"]);
  });

  it("does not over-match when a different txn id is a prefix substring", () => {
    // "txn:ab" must not be wiped when we prune "txn:a" — the pipe
    // delimiter is what enforces the boundary.
    const expanded = new Set(["txn:a|x", "txn:abcd|y"]);
    pruneExpandedKeysForTxn(expanded, "txn:a");
    expect([...expanded]).toEqual(["txn:abcd|y"]);
  });

  it("is a no-op for an empty set or empty txnId", () => {
    const empty = new Set<string>();
    pruneExpandedKeysForTxn(empty, "txn:a");
    expect(empty.size).toBe(0);
    const populated = new Set(["txn:a|x"]);
    pruneExpandedKeysForTxn(populated, "");
    expect([...populated]).toEqual(["txn:a|x"]);
    pruneExpandedKeysForTxn(undefined, "txn:a");
  });
});

describe("resetSearchPaging — guards against the missing-rows bug", () => {
  it("resets txWindowStart so the next runSearchFilter fetches from offset 0", () => {
    // Regression: switching the date filter (or any other search-affecting
    // input) while txWindowStart > 0 used to fetch offset=N..N+TX_WINDOW
    // of the NEW query, silently dropping the first N rows. The user
    // saw an apparent gap in the date-sorted list — rows weren't
    // missing from the data, the frontend was just looking at the
    // wrong page.
    const state: Parameters<typeof resetSearchPaging>[0] = {
      txWindowStart: 500,
      searchFilteredTransactions: [
        { date: "", datetime: "", postings: [], amount: 0, amount_commodity: "USD", meta: "txn:old" },
      ],
      searchFilteredCount: 700,
      searchFilteredOffset: 500,
    };
    resetSearchPaging(state);
    expect(state.txWindowStart).toBe(0);
    expect(state.searchFilteredTransactions).toBeUndefined();
    expect(state.searchFilteredCount).toBeUndefined();
    expect(state.searchFilteredOffset).toBeUndefined();
  });

  it("is idempotent on a fresh state", () => {
    const state = {};
    resetSearchPaging(state);
    expect(state).toEqual({
      txWindowStart: 0,
      searchFilteredTransactions: undefined,
      searchFilteredCount: undefined,
      searchFilteredOffset: undefined,
    });
  });
});

// ── Optimistic add (1b) ──

const valuePostings: BuiltPosting[] = [
  { account: "assets:cash", amount: "-3.50", commodity: "USD", remainder: null },
  { account: "expenses:coffee", amount: "3.50", commodity: "USD", remainder: null },
];

function mkTemp(account = "assets:cash", id = "tmp-1"): Transaction {
  return buildOptimisticTransaction(
    { datetime: "2025-02-01 09:00:00", payee: "Coffee Hut", narration: "Flat white" },
    valuePostings,
    account,
    id,
  );
}

describe("buildOptimisticTransaction", () => {
  it("synthesizes a Transaction with the temp id, date, and selected-account amount", () => {
    const t = mkTemp();
    expect(t.meta).toBe("txn:tmp-1");
    expect(t.date).toBe("2025-02-01");
    expect(t.payee).toBe("Coffee Hut");
    expect(t.postings).toHaveLength(2);
    expect(t.postings[0]).toMatchObject({
      account: "assets:cash",
      amount: -3.5,
      commodity: "USD",
      amount_text: "-3.50",
    });
    // Amount column reflects the SELECTED account's leg.
    expect(t.amount).toBe(-3.5);
    expect(t.amount_commodity).toBe("USD");
  });
});

describe("pendingAddsForView", () => {
  it("returns temps whose postings touch the selected account", () => {
    const temp = mkTemp();
    expect(pendingAddsForView([temp], "assets:cash", [])).toEqual([temp]);
  });
  it("excludes temps for a different account", () => {
    expect(pendingAddsForView([mkTemp()], "assets:bank", [])).toEqual([]);
  });
  it("dedups a temp once a content-matching real row is present (no double-row flash)", () => {
    const temp = mkTemp();
    const real: Transaction = { ...temp, meta: "txn:man-abc" }; // same content, real id
    expect(pendingAddsForView([temp], "assets:cash", [real])).toEqual([]);
  });
  it("keeps the temp when the real row's content differs", () => {
    const temp = mkTemp();
    const realDifferent: Transaction = {
      ...temp,
      meta: "txn:man-abc",
      postings: temp.postings.map((p) => ({ ...p, amount: p.amount * 2 })),
    };
    expect(pendingAddsForView([temp], "assets:cash", [realDifferent])).toEqual([temp]);
  });
});

describe("applyPendingBalanceDelta", () => {
  it("folds the selected-account leg into an existing commodity total", () => {
    const out = applyPendingBalanceDelta([{ commodity: "USD", amount: 100 }], [mkTemp()], "assets:cash");
    expect(out).toEqual([{ commodity: "USD", amount: 96.5 }]); // 100 + (-3.50)
  });
  it("adds a new commodity entry when absent", () => {
    expect(applyPendingBalanceDelta([], [mkTemp()], "assets:cash")).toEqual([{ commodity: "USD", amount: -3.5 }]);
  });
  it("returns the input unchanged when there are no pending adds", () => {
    const totals = [{ commodity: "USD", amount: 100 }];
    expect(applyPendingBalanceDelta(totals, [], "assets:cash")).toBe(totals);
  });
});

describe("nextOptimisticTempId", () => {
  it("returns monotonic tmp- ids", () => {
    const a = nextOptimisticTempId();
    const b = nextOptimisticTempId();
    expect(a).toMatch(/^tmp-\d+$/);
    expect(a).not.toBe(b);
  });
});
