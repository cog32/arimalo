import { describe, it, expect } from "vitest";
import { displayLegs, inScopeLegs, representativeLegs, valuationLegs } from "./transaction-legs";
import type { Transaction } from "./types";

function txn(postings: { account: string; amount: number; commodity: string }[]): Transaction {
  return {
    date: "2020-07-22",
    datetime: "2020-07-22T00:00:00",
    payee: "Property",
    narration: "37 Hill St — purchase",
    postings,
    amount: postings[0]?.amount ?? 0,
    amount_commodity: postings[0]?.commodity ?? "",
  };
}

// Mirrors the real 37 Hill St purchase: land + building on one account, in two
// commodities, with an out-of-scope equity contra.
const property = txn([
  { account: "assets:property:hillst37", amount: 539.47, commodity: "LAND37HILL" },
  { account: "assets:property:hillst37", amount: 1, commodity: "BUILD37HILL" },
  { account: "equity:opening-balance", amount: -2925000, commodity: "AUD" },
]);

describe("inScopeLegs", () => {
  it("returns only postings at or below the account scope", () => {
    expect(inScopeLegs(property, "assets:property:hillst37").map((p) => p.commodity))
      .toEqual(["LAND37HILL", "BUILD37HILL"]);
  });

  it("returns [] for no account", () => {
    expect(inScopeLegs(property, undefined)).toEqual([]);
  });
});

describe("representativeLegs", () => {
  it("shows every leg for a multi-commodity holding (land + building)", () => {
    const legs = representativeLegs(property, "assets:property:hillst37");
    expect(legs.map((p) => p.commodity)).toEqual(["LAND37HILL", "BUILD37HILL"]);
  });

  it("matches a parent scope and still returns both commodity legs", () => {
    expect(representativeLegs(property, "assets:property").map((p) => p.commodity))
      .toEqual(["LAND37HILL", "BUILD37HILL"]);
  });

  it("returns the single in-scope leg for an ordinary transaction", () => {
    const t = txn([
      { account: "assets:cash:usd", amount: -10, commodity: "USD" },
      { account: "expenses:coffee", amount: 10, commodity: "USD" },
    ]);
    expect(representativeLegs(t, "assets:cash:usd").map((p) => p.amount)).toEqual([-10]);
  });

  it("keeps an in-scope same-commodity transfer single-leg (avoids netting to zero)", () => {
    // Wallet leg + its `…:transfer` contra both fall under `assets:crypto`.
    const transfer = txn([
      { account: "assets:crypto:wallet:eth", amount: 5, commodity: "ETH" },
      { account: "assets:crypto:transfer", amount: -5, commodity: "ETH" },
    ]);
    const legs = representativeLegs(transfer, "assets:crypto");
    expect(legs).toHaveLength(1);
    expect(legs[0].amount).toBe(5);
  });

  it("returns [] when the account has no in-scope posting", () => {
    expect(representativeLegs(property, "assets:cash:usd")).toEqual([]);
  });
});

describe("displayLegs", () => {
  it("returns the in-scope representative legs when present", () => {
    const t = txn([
      { account: "assets:cash:usd", amount: -10, commodity: "USD" },
      { account: "expenses:coffee", amount: 10, commodity: "USD" },
    ]);
    expect(displayLegs(t, "assets:cash:usd").map((p) => p.amount)).toEqual([-10]);
  });

  it("falls back to the first posting for a revealed ignored row (no in-scope leg)", () => {
    // A hidden row: both legs moved to ignore:hidden, nothing on the wallet.
    const hidden = txn([
      { account: "ignore:hidden", amount: 100, commodity: "AUD" },
      { account: "ignore:hidden", amount: -100, commodity: "AUD" },
    ]);
    const legs = displayLegs(hidden, "assets:savings");
    expect(legs).toHaveLength(1);
    expect(legs[0].amount).toBe(100);
    expect(legs[0].account).toBe("ignore:hidden");
  });
});

describe("valuationLegs", () => {
  // Mirrors the real IBKR MSFT buy: shares acquired + cash sub-account paid,
  // both in scope of the broker account, in opposite directions. Summing the
  // share leg (marked to market) against the cash leg nets to a meaningless
  // markup, so only the headline (shares) side is valued.
  const ibkrBuy = txn([
    { account: "assets:equity:broker:ibkr:personal", amount: 92, commodity: "MSFT" },
    { account: "assets:equity:broker:ibkr:personal:cash", amount: -33948.46, commodity: "USD" },
    { account: "expenses:fees:brokerage", amount: 0.46, commodity: "USD" },
  ]);

  it("values only the headline side of an in-scope swap (broker trade)", () => {
    const legs = valuationLegs(ibkrBuy, "assets:equity:broker:ibkr:personal");
    expect(legs.map((p) => p.commodity)).toEqual(["MSFT"]);
    expect(legs.map((p) => p.amount)).toEqual([92]);
  });

  it("values the headline side regardless of which direction leads (sell)", () => {
    const ibkrSell = txn([
      { account: "assets:equity:broker:ibkr:personal", amount: -300, commodity: "MSFT" },
      { account: "assets:equity:broker:ibkr:personal:cash", amount: 142276.44, commodity: "USD" },
    ]);
    const legs = valuationLegs(ibkrSell, "assets:equity:broker:ibkr:personal");
    expect(legs.map((p) => p.commodity)).toEqual(["MSFT"]);
    expect(legs.map((p) => p.amount)).toEqual([-300]);
  });

  it("keeps every leg for a co-held multi-commodity holding (land + building)", () => {
    const legs = valuationLegs(property, "assets:property:hillst37");
    expect(legs.map((p) => p.commodity)).toEqual(["LAND37HILL", "BUILD37HILL"]);
  });

  it("matches representativeLegs for ordinary single-leg transactions", () => {
    const t = txn([
      { account: "assets:cash:usd", amount: -10, commodity: "USD" },
      { account: "expenses:coffee", amount: 10, commodity: "USD" },
    ]);
    expect(valuationLegs(t, "assets:cash:usd")).toEqual(representativeLegs(t, "assets:cash:usd"));
  });
});
