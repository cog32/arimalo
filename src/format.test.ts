import { describe, it, expect } from "vitest";
import {
  formatAmount,
  formatAmountSmart,
  formatMoney,
  formatMoneyWhole,
  type DisplayConfig,
} from "./format";

const noOverrides: DisplayConfig = { commodities: {}, default_decimals: 2 };

describe("formatAmountSmart", () => {
  it("renders 2dp for amounts >= 0.01", () => {
    expect(formatAmountSmart(123.45)).toBe("123.45");
    expect(formatAmountSmart(9.241119)).toBe("9.24");
    expect(formatAmountSmart(0.0625)).toBe("0.06");
    expect(formatAmountSmart(28.608047)).toBe("28.61");
  });

  it("extends precision for sub-cent amounts to keep 2 sig figs", () => {
    expect(formatAmountSmart(0.0034)).toBe("0.0034");
    expect(formatAmountSmart(0.00001)).toBe("0.000010");
  });

  it("handles zero and negatives", () => {
    expect(formatAmountSmart(0)).toBe("0.00");
    expect(formatAmountSmart(-9.241119)).toBe("-9.24");
  });
});

describe("formatAmount — smart is the default", () => {
  it("uses smart formatting for commodities not in displayConfig (regression: ETH 6dp)", () => {
    expect(formatAmount(9.241119, "ETH", noOverrides)).toBe("9.24");
    expect(formatAmount(0.0625, "ETH", noOverrides)).toBe("0.06");
    expect(formatAmount(28.608047, "ETH", noOverrides)).toBe("28.61");
  });

  it("uses smart formatting when displayConfig is missing", () => {
    expect(formatAmount(9.241119, "ETH", null)).toBe("9.24");
    expect(formatAmount(9.241119, "ETH", undefined)).toBe("9.24");
  });

  it("does not consult default_decimals for unconfigured commodities", () => {
    const cfg: DisplayConfig = { commodities: {}, default_decimals: 6 };
    expect(formatAmount(9.241119, "ETH", cfg)).toBe("9.24");
  });

  it("honours an explicit per-commodity override", () => {
    const cfg: DisplayConfig = {
      commodities: { AUD: { decimals: 2 }, ETH: { decimals: 6 } },
      default_decimals: 2,
    };
    expect(formatAmount(100, "AUD", cfg)).toBe("100.00");
    expect(formatAmount(9.241119, "ETH", cfg)).toBe("9.241119");
    expect(formatAmount(0.0625, "ETH", cfg)).toBe("0.062500");
  });

  it("falls back to smart when override would round a non-zero value to zero", () => {
    const cfg: DisplayConfig = { commodities: { USD: { decimals: 2 } }, default_decimals: 2 };
    expect(formatAmount(0.0034, "USD", cfg)).toBe("0.0034");
  });
});

describe("formatMoney / formatMoneyWhole — currency totals", () => {
  it("adds thousand separators, 2 decimals by default", () => {
    expect(formatMoney(1150774.25)).toBe("1,150,774.25");
    expect(formatMoney(11280308.98)).toBe("11,280,308.98");
    expect(formatMoney(-16.4)).toBe("-16.40");
    expect(formatMoney(0)).toBe("0.00");
  });

  it("honours a custom decimal count", () => {
    expect(formatMoney(1234.5, 0)).toBe("1,235");
    expect(formatMoney(1234.5, 1)).toBe("1,234.5");
  });

  it("formatMoneyWhole rounds to whole dollars with separators", () => {
    expect(formatMoneyWhole(1150774.25)).toBe("1,150,774");
    expect(formatMoneyWhole(-390816.3)).toBe("-390,816");
    expect(formatMoneyWhole(0.6)).toBe("1");
  });

  it("collapses -0 to 0", () => {
    expect(formatMoney(-0, 2)).toBe("0.00");
    expect(formatMoneyWhole(-0)).toBe("0");
  });
});
