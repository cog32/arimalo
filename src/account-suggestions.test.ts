import { describe, it, expect } from "vitest";
import { collectAccountSuggestions } from "./account-suggestions";

describe("collectAccountSuggestions", () => {
  it("offers a vault-wide account absent from the current set (regression: income:dividends)", () => {
    // Editing a rule on a bank account: its loaded balances and owner accounts
    // are all assets, so income:dividends — booked only in another account —
    // is reachable solely through the global `allAccounts` pool.
    const result = collectAccountSuggestions({
      accountSetMap: { richard: ["assets:cash:bank:ubank"] },
      balances: [{ account: "assets:cash:bank:ubank" }],
      transactions: [],
      allAccounts: ["assets:cash:bank:ubank", "income:dividends", "expenses:fees:bank"],
    });
    expect(result).toContain("income:dividends");
  });

  it("merges, de-duplicates and sorts across all sources", () => {
    const result = collectAccountSuggestions({
      accountSetMap: { r: ["assets:b"] },
      balances: [{ account: "assets:b" }, { account: "assets:a" }],
      transactions: [{ postings: [{ account: "expenses:x" }] }],
      allAccounts: ["income:z", "assets:a"],
    });
    expect(result).toEqual(["assets:a", "assets:b", "expenses:x", "income:z"]);
  });

  it("handles missing optional sources", () => {
    expect(collectAccountSuggestions({ accountSetMap: {} })).toEqual([]);
    expect(collectAccountSuggestions({ accountSetMap: { r: ["assets:a"] } })).toEqual(["assets:a"]);
  });
});
