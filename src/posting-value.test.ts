import { describe, expect, it } from "vitest";
import { postingPriceValue } from "./posting-value";
import type { Posting } from "./types";

function makePosting(overrides: Partial<Posting>): Posting {
  return {
    account: "assets:test",
    amount: 0,
    commodity: "BHP",
    ...overrides,
  };
}

describe("postingPriceValue", () => {
  it("multiplies units by unit price for @ (is_total=false)", () => {
    const p = makePosting({
      amount: 100,
      price: { is_total: false, amount: 45.2, amount_text: "45.20", commodity: "AUD" },
    });
    expect(postingPriceValue(p)).toBe(4520);
  });

  it("returns the total directly for @@ (is_total=true)", () => {
    const p = makePosting({
      amount: 10000,
      price: { is_total: true, amount: 3650, amount_text: "3650.00", commodity: "AUD" },
    });
    expect(postingPriceValue(p)).toBe(3650);
  });

  it("absolute-values negative quantities for @ pricing (sell legs)", () => {
    const p = makePosting({
      amount: -100,
      price: { is_total: false, amount: 50, amount_text: "50.00", commodity: "AUD" },
    });
    expect(postingPriceValue(p)).toBe(5000);
  });

  it("returns the @@ total unchanged for negative-quantity legs", () => {
    const p = makePosting({
      amount: -10000,
      price: { is_total: true, amount: 9000, amount_text: "9000.00", commodity: "AUD" },
    });
    expect(postingPriceValue(p)).toBe(9000);
  });

  it("returns null when the price is missing", () => {
    expect(postingPriceValue(makePosting({ amount: 100 }))).toBeNull();
  });

  it("returns null when the price amount is zero", () => {
    const p = makePosting({
      amount: 100,
      price: { is_total: false, amount: 0, amount_text: "0", commodity: "AUD" },
    });
    expect(postingPriceValue(p)).toBeNull();
  });

  it("returns null when the posting is null/undefined", () => {
    expect(postingPriceValue(undefined)).toBeNull();
    expect(postingPriceValue(null)).toBeNull();
  });
});
