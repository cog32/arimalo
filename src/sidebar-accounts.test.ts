import { describe, expect, it } from "vitest";
import { filterSidebarBalances, filterCategoryBalances } from "./sidebar-accounts";
import type { AccountBalance } from "./types";

const balances: AccountBalance[] = [
  { account: "assets:cash:bank:savings", totals: [{ commodity: "USD", amount: 10 }] },
  { account: "assets:cash:bank:checking", totals: [{ commodity: "USD", amount: 20 }] },
  { account: "assets:cash", totals: [{ commodity: "USD", amount: 30 }] },
  { account: "liabilities:card", totals: [{ commodity: "USD", amount: -5 }] },
];

describe("filterSidebarBalances", () => {
  it("keeps only folder-backed asset accounts for the active set", () => {
    expect(
      filterSidebarBalances(
        balances,
        {
          "assets:cash:bank:savings": "richard/cash/bank/savings",
          "assets:cash:bank:checking": "richard/cash/bank/checking",
        },
        {
          richard: [
            "assets:cash:bank:savings",
            "assets:cash:bank:checking",
            "assets:cash",
          ],
        },
        "richard",
      ).map((b) => b.account),
    ).toEqual([
      "assets:cash:bank:savings",
      "assets:cash:bank:checking",
    ]);
  });

  it("falls back to all folder-backed assets when no set is selected", () => {
    expect(
      filterSidebarBalances(
        balances,
        {
          "assets:cash:bank:savings": "richard/cash/bank/savings",
        },
        {},
      ).map((b) => b.account),
    ).toEqual(["assets:cash:bank:savings"]);
  });
});

describe("filterCategoryBalances", () => {
  const catBalances: AccountBalance[] = [
    { account: "assets:cash:bank:savings", totals: [{ commodity: "USD", amount: 10 }] }, // folder-backed
    { account: "assets:staking", totals: [{ commodity: "LFNTY", amount: 11296 }] }, // contra
    { account: "assets:lending", totals: [{ commodity: "USDC", amount: 500 }] }, // contra
    { account: "income:crypto:staking", totals: [{ commodity: "SOL", amount: -3 }] },
    { account: "expenses:fees:trading", totals: [{ commodity: "USD", amount: -2 }] },
    { account: "equity:opening-balance", totals: [{ commodity: "USD", amount: -100 }] },
    { account: "liabilities:custody:james", totals: [{ commodity: "BTC", amount: -1 }] },
    { account: "ignore:spam", totals: [{ commodity: "SCAM", amount: 1000 }] },
  ];
  const folders = { "assets:cash:bank:savings": "richard/cash/bank/savings" };

  it("keeps non-folder-backed accounts and excludes ignore:* by default", () => {
    expect(
      filterCategoryBalances(catBalances, folders, false).map((b) => b.account),
    ).toEqual([
      "assets:staking",
      "assets:lending",
      "income:crypto:staking",
      "expenses:fees:trading",
      "equity:opening-balance",
      "liabilities:custody:james",
    ]);
  });

  it("includes ignore:* when showHidden is on", () => {
    expect(
      filterCategoryBalances(catBalances, folders, true).map((b) => b.account),
    ).toContain("ignore:spam");
  });

  it("never includes folder-backed asset accounts (the Accounts pane owns them)", () => {
    expect(
      filterCategoryBalances(catBalances, folders, true).map((b) => b.account),
    ).not.toContain("assets:cash:bank:savings");
  });
});
