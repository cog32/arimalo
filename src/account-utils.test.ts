import { describe, it, expect } from "vitest";
import {
  shortAccountPath,
  shortAddress,
  buildManualPostings,
  computeManualBalance,
  formatCash,
  type ManualDraftCore,
} from "./account-utils";

// All examples use the single-char ellipsis "…" consistent with shortAddress().
// Budgets are absolute character counts.

describe("shortAccountPath — regular paths", () => {
  it("returns the full path when it fits", () => {
    expect(shortAccountPath("assets:crypto:bitcoin", 30)).toBe("assets:crypto:bitcoin");
  });

  it("returns the full path when it equals the budget exactly", () => {
    expect(shortAccountPath("assets:crypto:bitcoin", 21)).toBe("assets:crypto:bitcoin");
  });

  it("keeps the full leaf and fills with as much parent context as fits, prefixed with ellipsis", () => {
    // budget 15 = "…" (1) + "crypto:" (7) + "bitcoin" (7)
    expect(shortAccountPath("assets:crypto:bitcoin", 15)).toBe("…crypto:bitcoin");
  });

  it("packs as little parent context as 1 colon when the leaf nearly fills the budget", () => {
    // budget 10: leaf 7, ellipsis 1, room for 2 chars of parent → "o:"
    expect(shortAccountPath("assets:crypto:bitcoin", 10)).toBe("…o:bitcoin");
  });

  it("emits a leading ellipsis + bare leaf when there's no room for parent context", () => {
    // budget 8 = "…" (1) + "bitcoin" (7)
    expect(shortAccountPath("assets:crypto:bitcoin", 8)).toBe("…bitcoin");
  });

  it("truncates the leaf itself from the right when even the leaf is too big", () => {
    // budget 7: leaf "bitcoin" (7) + trailing ellipsis (1) = 8 > 7. Take leaf[0..6] + "…"
    expect(shortAccountPath("assets:crypto:bitcoin", 7)).toBe("bitcoi…");
  });

  it("truncates leaf hard at very tight budgets", () => {
    expect(shortAccountPath("assets:crypto:bitcoin", 4)).toBe("bit…");
  });
});

describe("shortAccountPath — address leaves", () => {
  // shortAddress("0x1234567890abcdef1234567890") — 28-char hex
  // → "0x12345678" + "…" + "567890" = 17 chars
  const ADDR = "0x1234567890abcdef1234567890";
  const SHORT_ADDR = shortAddress(ADDR);

  it("baseline: shortAddress collapses long addresses to first10…last6", () => {
    expect(SHORT_ADDR).toBe("0x12345678…567890");
    expect(SHORT_ADDR.length).toBe(17);
  });

  it("returns the full path with no leading ellipsis when parent fits in the budget", () => {
    // parent "assets:eth:" (11) + leaf 17 = 28. budget 30 has slack — no ellipsis.
    expect(shortAccountPath(`assets:eth:${ADDR}`, 30)).toBe(`assets:eth:${SHORT_ADDR}`);
  });

  it("trims the parent from the left with a leading ellipsis when it doesn't fully fit", () => {
    // budget 25: leaf 17, ellipsis 1, room for 7 chars of parent → "ts:eth:"
    expect(shortAccountPath(`assets:eth:${ADDR}`, 25)).toBe(`…ts:eth:${SHORT_ADDR}`);
  });

  it("further truncates the abbreviated address with a trailing ellipsis when the budget can't hold it", () => {
    // budget 15: short leaf 17 doesn't fit. leaf[0..14] + "…" — XXXX…XX… pattern.
    expect(shortAccountPath(`assets:eth:${ADDR}`, 15)).toBe("0x12345678…567…");
  });

  it("returns the bare abbreviated address (no parent) when budget exactly equals short-address + leading ellipsis", () => {
    // budget 18 = "…" (1) + 17. No parent room.
    expect(shortAccountPath(`assets:eth:${ADDR}`, 18)).toBe(`…${SHORT_ADDR}`);
  });

  it("returns full path unchanged when address path fits", () => {
    const acct = `assets:eth:${ADDR}`;
    expect(shortAccountPath(acct, acct.length)).toBe(acct);
  });
});

// ── Manual transaction posting construction ──

function draft(over: Partial<ManualDraftCore>): ManualDraftCore {
  return {
    mode: "value",
    account: "assets:bank",
    cashCommodity: "AUD",
    amount: "",
    tradeCommodity: "",
    quantity: "",
    price: "",
    contras: [],
    ...over,
  };
}

describe("formatCash — computed money formatting", () => {
  it("pads clean integers to 2 decimals", () => {
    expect(formatCash(3650)).toBe("3650.00");
    expect(formatCash(-500)).toBe("-500.00");
    expect(formatCash(10.5)).toBe("10.50");
  });
  it("keeps extra precision beyond 2 decimals, trimming trailing zeros", () => {
    expect(formatCash(1.66665)).toBe("1.66665");
    expect(formatCash(-3679.95)).toBe("-3679.95");
  });
  it("normalizes zero and -0", () => {
    expect(formatCash(0)).toBe("0.00");
    expect(formatCash(-0)).toBe("0.00");
  });
  it("rounds away floating-point noise at 8dp", () => {
    expect(formatCash(10000 * 0.365)).toBe("3650.00"); // 3650.0000000000005
  });
});

describe("buildManualPostings — value mode", () => {
  it("auto-fills a single blank contra to balance", () => {
    const r = buildManualPostings(
      draft({ amount: "500.00", contras: [{ account: "assets:transfer:cash", amount: "" }] }),
    );
    expect(r).toEqual({
      ok: true,
      postings: [
        { account: "assets:bank", amount: "500.00", commodity: "AUD", remainder: null },
        { account: "assets:transfer:cash", amount: "-500.00", commodity: "AUD", remainder: null },
      ],
    });
  });

  it("auto-fills the opposite sign for a negative amount", () => {
    const r = buildManualPostings(
      draft({ amount: "-50", contras: [{ account: "income:interest", amount: "" }] }),
    );
    expect(r.ok && r.postings[1].amount).toBe("50.00");
  });

  it("keeps user-typed contra amounts verbatim when a split already balances", () => {
    const r = buildManualPostings(
      draft({
        amount: "500",
        contras: [
          { account: "assets:a", amount: "-300" },
          { account: "assets:b", amount: "-200" },
        ],
      }),
    );
    expect(r).toEqual({
      ok: true,
      postings: [
        { account: "assets:bank", amount: "500", commodity: "AUD", remainder: null },
        { account: "assets:a", amount: "-300", commodity: "AUD", remainder: null },
        { account: "assets:b", amount: "-200", commodity: "AUD", remainder: null },
      ],
    });
  });

  it("rejects an invalid amount", () => {
    expect(buildManualPostings(draft({ amount: "abc", contras: [{ account: "x", amount: "" }] }))).toEqual({
      ok: false,
      error: "Enter a valid amount.",
    });
  });

  it("rejects an out-of-balance split with no blank row", () => {
    const r = buildManualPostings(
      draft({ amount: "500", contras: [{ account: "assets:a", amount: "-300" }] }),
    );
    expect(r).toEqual({ ok: false, error: "Out of balance by 200.00 AUD." });
  });

  it("rejects more than one blank row", () => {
    const r = buildManualPostings(
      draft({
        amount: "500",
        contras: [
          { account: "assets:a", amount: "" },
          { account: "assets:b", amount: "" },
        ],
      }),
    );
    expect(r).toEqual({ ok: false, error: "Enter an amount for all but one of the other accounts." });
  });

  it("requires at least one other account", () => {
    expect(buildManualPostings(draft({ amount: "500", contras: [] }))).toEqual({
      ok: false,
      error: "Add at least one account for the other side.",
    });
    expect(
      buildManualPostings(draft({ amount: "500", contras: [{ account: "", amount: "" }] })),
    ).toEqual({ ok: false, error: "Add at least one account for the other side." });
  });

  it("requires an account on a row that has an amount", () => {
    const r = buildManualPostings(draft({ amount: "500", contras: [{ account: "", amount: "-500" }] }));
    expect(r).toEqual({ ok: false, error: "Every posting needs an account." });
  });
});

describe("buildManualPostings — trade mode (matches generated CommSec format)", () => {
  const commsec = "assets:equity:broker:commsec:personal";

  it("builds a 3-leg buy with @@ total and auto-filled cash leg", () => {
    const r = buildManualPostings(
      draft({
        mode: "trade",
        account: commsec,
        cashCommodity: "AUD",
        tradeCommodity: "BQT",
        quantity: "10000",
        price: "0.365",
        contras: [
          { account: "expenses:fees:brokerage", amount: "29.95" },
          { account: "assets:transfer:cash", amount: "" },
        ],
      }),
    );
    expect(r).toEqual({
      ok: true,
      postings: [
        { account: commsec, amount: "10000", commodity: "BQT", remainder: "@@ 3650.00 AUD" },
        { account: "expenses:fees:brokerage", amount: "29.95", commodity: "AUD", remainder: null },
        { account: "assets:transfer:cash", amount: "-3679.95", commodity: "AUD", remainder: null },
      ],
    });
  });

  it("builds a sell from a negative quantity (positive @@ total, cash received)", () => {
    const r = buildManualPostings(
      draft({
        mode: "trade",
        account: commsec,
        cashCommodity: "AUD",
        tradeCommodity: "BQT",
        quantity: "-10000",
        price: "0.39",
        contras: [
          { account: "expenses:fees:brokerage", amount: "29.95" },
          { account: "assets:transfer:cash", amount: "" },
        ],
      }),
    );
    expect(r).toEqual({
      ok: true,
      postings: [
        { account: commsec, amount: "-10000", commodity: "BQT", remainder: "@@ 3900.00 AUD" },
        { account: "expenses:fees:brokerage", amount: "29.95", commodity: "AUD", remainder: null },
        { account: "assets:transfer:cash", amount: "3870.05", commodity: "AUD", remainder: null },
      ],
    });
  });

  it("requires a commodity, a numeric quantity, and a non-negative price", () => {
    const base = {
      mode: "trade" as const,
      account: commsec,
      tradeCommodity: "BQT",
      quantity: "100",
      price: "5",
      contras: [{ account: "assets:transfer:cash", amount: "" }],
    };
    expect(buildManualPostings(draft({ ...base, tradeCommodity: "" })).ok).toBe(false);
    expect(buildManualPostings(draft({ ...base, quantity: "x" })).ok).toBe(false);
    expect(buildManualPostings(draft({ ...base, price: "-1" })).ok).toBe(false);
  });
});

describe("computeManualBalance", () => {
  it("reports one blank and is balanceable for an in-progress trade", () => {
    const b = computeManualBalance(
      draft({
        mode: "trade",
        tradeCommodity: "BQT",
        quantity: "10000",
        price: "0.365",
        contras: [
          { account: "expenses:fees:brokerage", amount: "29.95" },
          { account: "assets:transfer:cash", amount: "" },
        ],
      }),
    );
    expect(b.blanks).toBe(1);
    expect(b.balanceable).toBe(true);
    expect(b.remainder).toBeCloseTo(-3679.95, 6);
  });

  it("is not balanceable before the top leg is a valid number", () => {
    const b = computeManualBalance(draft({ amount: "", contras: [{ account: "x", amount: "" }] }));
    expect(Number.isNaN(b.topCash)).toBe(true);
    expect(b.balanceable).toBe(false);
  });

  it("is balanceable with zero blanks only when the cash nets to zero", () => {
    expect(
      computeManualBalance(
        draft({ amount: "500", contras: [{ account: "a", amount: "-500" }] }),
      ).balanceable,
    ).toBe(true);
    expect(
      computeManualBalance(
        draft({ amount: "500", contras: [{ account: "a", amount: "-300" }] }),
      ).balanceable,
    ).toBe(false);
  });
});
