// Locks down the JS↔Rust contract for the save_rule Tauri command.
//
// The bug we shipped: the live-app save lost commodity_condition. The
// rules.test.ts integration tests covered parse → handler → buildRuleFromDraft,
// but didn't exercise the args object actually sent over IPC. If the camelCase
// keys here drift from the snake_case Rust signature in src-tauri/src/main.rs
// `async fn save_rule`, fields silently disappear on save.
//
// The Rust side accepts these snake_case params; Tauri auto-converts from
// camelCase. The keys below are the canonical contract — change one and
// this test fails.

import { describe, it, expect } from "vitest";
import { buildSaveRuleArgs } from "./save-rule-args";
import { buildRuleFromDraft, type SavedRule } from "./rules";
import { parseSmartInput, toFlatString } from "./smart-search";

const RULE_KEYWORDS = ["payee", "narration", "meta", "date", "account", "commodity", "amount", "fee", "field"];

const EXPECTED_SAVE_RULE_KEYS = [
  "nowYyyymm",
  "accountFolder",
  "pattern",
  "payee",
  "commodity",
  "matchField",
  "amountCondition",
  "feeCondition",
  "payeeCondition",
  "narrationCondition",
  "commodityCondition",
  "metaCondition",
  "amountAccount",
  "feeAccount",
  "comment",
  "accountSet",
];

function makeSavedRule(overrides: Partial<SavedRule> = {}): SavedRule {
  return {
    pattern: "*test*",
    match_field: null,
    comment: null,
    amount_condition: null,
    fee_condition: null,
    payee_condition: null,
    narration_condition: null,
    commodity_condition: null,
    meta_condition: null,
    amount_account: null,
    fee_account: null,
    ...overrides,
  };
}

describe("save_rule IPC contract", () => {
  it("buildSaveRuleArgs sends every key the Rust signature expects", () => {
    const saved = makeSavedRule({
      pattern: "*token_transfer:*",
      match_field: "narration",
      amount_condition: "-0.003..0",
      payee_condition: "*Kraken*",
      narration_condition: "*swap*",
      commodity_condition: "SOL",
      meta_condition: "*csv-abc*",
      amount_account: "expenses:crypto:fees",
    });
    const args = buildSaveRuleArgs(saved, "richard/crypto/wallet/solana", "default", "202601");
    expect(Object.keys(args).sort()).toEqual([...EXPECTED_SAVE_RULE_KEYS].sort());
  });

  it("commodity_condition value flows through unchanged", () => {
    const saved = makeSavedRule({ pattern: "*token_transfer:*", commodity_condition: "SOL" });
    const args = buildSaveRuleArgs(saved, "x", "y", "202601");
    expect(args.commodityCondition).toBe("SOL");
    expect(args.narrationCondition).toBe(null);
    expect(args.metaCondition).toBe(null);
    expect(args.payeeCondition).toBe(null);
  });

  it("narration_condition and meta_condition flow through unchanged", () => {
    const saved = makeSavedRule({
      pattern: "*p*",
      narration_condition: "*swap*",
      meta_condition: "*csv-abc*",
    });
    const args = buildSaveRuleArgs(saved, "x", "y", "202601");
    expect(args.narrationCondition).toBe("*swap*");
    expect(args.metaCondition).toBe("*csv-abc*");
  });
});

describe("user scenario end-to-end (parse → save → invoke args)", () => {
  // Reproduces the live-app failure: existing rule had pattern
  // *token_transfer:*; user adds amount:>-0.003 amount:<0 commodity:SOL.
  // The args object sent to save_rule must include commodity_condition: "SOL"
  // alongside the merged amount range.
  it("user types amount + amount + commodity over an existing narration pattern", () => {
    // Step 1: simulate the change handler running on the typed pills.
    const typed = "amount:>-0.003 amount:<0 commodity:SOL";
    const { pills } = parseSmartInput(typed, RULE_KEYWORDS);
    expect(pills).toEqual([
      { key: "amount", value: ">-0.003" },
      { key: "amount", value: "<0" },
      { key: "commodity", value: "SOL" },
    ]);

    // Step 2: build a draft as the change handler would, on top of the
    // already-loaded existing rule (pattern *token_transfer:*).
    const draft = {
      pattern: "*token_transfer:*",
      matchField: "narration",
      amountCondition: "-0.003..0", // what extractRuleConfigPills produces
      feeCondition: "",
      payeeCondition: "",
      narrationCondition: "",
      commodityCondition: "SOL", // what extractRuleConfigPills produces
      metaCondition: "",
      filterPills: [],
      comment: "",
      amountAccount: "expenses:crypto:fees",
      feeAccount: "",
    };

    // Step 3: build the SavedRule (what readAndBuildRule does).
    const saved = buildRuleFromDraft(draft, parseSmartInput, toFlatString, RULE_KEYWORDS);
    expect(saved.pattern).toBe("*token_transfer:*");
    expect(saved.amount_condition).toBe("-0.003..0");
    expect(saved.commodity_condition).toBe("SOL");

    // Step 4: build the IPC args (what invokeRuleSave does). This is the
    // exact object sent over Tauri's bridge.
    const args = buildSaveRuleArgs(saved, "richard/crypto/wallet/solana", "default", "202601");
    expect(args.commodityCondition).toBe("SOL");
    expect(args.amountCondition).toBe("-0.003..0");
    expect(args.pattern).toBe("*token_transfer:*");
    expect(args.amountAccount).toBe("expenses:crypto:fees");
    // matchField stays — pattern was non-empty so derive doesn't promote.
    expect(args.matchField).toBe("narration");
  });

  it("the same scenario for narration_condition and meta_condition", () => {
    const draft = {
      pattern: "*token_transfer:*",
      matchField: "narration",
      amountCondition: "",
      feeCondition: "",
      payeeCondition: "",
      narrationCondition: "*swap*",
      commodityCondition: "",
      metaCondition: "*csv-abc*",
      filterPills: [],
      comment: "",
      amountAccount: "expenses:test",
      feeAccount: "",
    };
    const saved = buildRuleFromDraft(draft, parseSmartInput, toFlatString, RULE_KEYWORDS);
    const args = buildSaveRuleArgs(saved, "x", "y", "202601");
    expect(args.narrationCondition).toBe("*swap*");
    expect(args.metaCondition).toBe("*csv-abc*");
  });
});
