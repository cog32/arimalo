import { describe, expect, it } from "vitest";
import {
  extractRuleId,
  legIdFromMeta,
  matchesPrefix,
  metaSegmentValue,
  swapPartnerRefFromMeta,
  txnIdFromMeta,
  txnValueKey,
} from "./meta";

describe("matchesPrefix", () => {
  it("matches exact account", () => {
    expect(matchesPrefix("assets:cash", "assets:cash")).toBe(true);
  });
  it("matches child accounts", () => {
    expect(matchesPrefix("assets:cash:aud", "assets:cash")).toBe(true);
  });
  it("does not match sibling prefixes", () => {
    expect(matchesPrefix("assets:cashbox", "assets:cash")).toBe(false);
  });
  it("does not match unrelated accounts", () => {
    expect(matchesPrefix("expenses:food", "assets:cash")).toBe(false);
  });
});

describe("extractRuleId", () => {
  it("returns undefined for null/empty meta", () => {
    expect(extractRuleId(null)).toBeUndefined();
    expect(extractRuleId(undefined)).toBeUndefined();
    expect(extractRuleId("")).toBeUndefined();
  });
  it("extracts the rule id", () => {
    expect(extractRuleId("txn:abc,rule:R1,swap:txn:def")).toBe("R1");
  });
  it("tolerates surrounding whitespace", () => {
    expect(extractRuleId("txn:abc , rule:R42 , foo:bar")).toBe("R42");
  });
  it("returns undefined when no rule segment present", () => {
    expect(extractRuleId("txn:abc,swap:txn:def")).toBeUndefined();
  });
});

describe("txnIdFromMeta", () => {
  it("returns empty string for null/undefined", () => {
    expect(txnIdFromMeta(null)).toBe("");
    expect(txnIdFromMeta(undefined)).toBe("");
  });
  it("extracts the txn:HASH segment", () => {
    expect(txnIdFromMeta("txn:0xABC,rule:R1")).toBe("txn:0xABC");
  });
  it("ignores the swap partner's txn segment", () => {
    // The swap segment is "swap:txn:HASH" — startsWith("txn:") must be false for it.
    expect(txnIdFromMeta("swap:txn:OTHER,txn:MINE")).toBe("txn:MINE");
  });
});

describe("legIdFromMeta", () => {
  it("returns empty string when no leg segment (single-leg row)", () => {
    expect(legIdFromMeta(null)).toBe("");
    expect(legIdFromMeta(undefined)).toBe("");
    expect(legIdFromMeta("txn:0xABC,rule:R1")).toBe("");
  });
  it("extracts the leg:ID segment for a shared-txn leg", () => {
    expect(legIdFromMeta("txn:0xABC, leg:l-deadbeef0001, rule:R1")).toBe("leg:l-deadbeef0001");
  });
});

describe("metaSegmentValue", () => {
  it("returns the suffix after the prefix", () => {
    expect(metaSegmentValue("txn:abc,swap:txn:def", "swap:")).toBe("txn:def");
  });
  it("returns undefined when prefix missing", () => {
    expect(metaSegmentValue("txn:abc", "swap:")).toBeUndefined();
  });
  it("returns undefined for null/empty meta", () => {
    expect(metaSegmentValue(null, "swap:")).toBeUndefined();
    expect(metaSegmentValue("", "swap:")).toBeUndefined();
  });
});

describe("swapPartnerRefFromMeta", () => {
  it("returns partner ref when both segments present", () => {
    const meta = "txn:A,swap:txn:B,swap_partner_commodity:ETH";
    expect(swapPartnerRefFromMeta(meta)).toEqual({
      partnerTxnId: "txn:B",
      partnerCommodity: "ETH",
    });
  });
  it("returns undefined when commodity segment missing", () => {
    expect(swapPartnerRefFromMeta("txn:A,swap:txn:B")).toBeUndefined();
  });
  it("returns undefined when swap segment missing", () => {
    expect(swapPartnerRefFromMeta("txn:A,swap_partner_commodity:ETH")).toBeUndefined();
  });
  it("returns undefined for unrelated meta", () => {
    expect(swapPartnerRefFromMeta("txn:A,rule:R1")).toBeUndefined();
  });
});

describe("txnValueKey", () => {
  it("produces a stable key combining id, datetime, amount, narration", () => {
    expect(
      txnValueKey({
        meta: "txn:abc",
        datetime: "2026-01-01 12:00:00",
        amount: 123.45,
        narration: "lunch",
      }),
    ).toBe("txn:abc|2026-01-01 12:00:00|123.45|lunch");
  });
  it("treats missing narration as empty", () => {
    expect(
      txnValueKey({ meta: "txn:abc", datetime: "2026-01-01", amount: 1 }),
    ).toBe("txn:abc|2026-01-01|1|");
  });
  it("uses empty txn id when meta absent", () => {
    expect(
      txnValueKey({ datetime: "2026-01-01", amount: 1, narration: "x" }),
    ).toBe("|2026-01-01|1|x");
  });
});
