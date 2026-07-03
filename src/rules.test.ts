import { describe, it, expect } from "vitest";
import { wildcardMatch, ruleMatchesFields, computeRulePreview, extractRuleConfigPills, draftConditionsToPills, isDefaultRule, buildRuleFromDraft, categoryEditRuleMatch, RuleInfo, parseAmountCondition, amountMatches, derivePatternAndPayeeCondition, mergeRuleConditions } from "./rules";
import { parseSmartInput, toFlatString } from "./smart-search";
import { accountPrefixForScope, filterByAccountPrefix } from "./account-search";

describe("wildcardMatch", () => {
  it("exact match (case-insensitive)", () => {
    expect(wildcardMatch("hello", "Hello")).toBe(true);
    expect(wildcardMatch("hello", "world")).toBe(false);
  });

  it("prefix wildcard", () => {
    expect(wildcardMatch("hello*", "Hello World")).toBe(true);
    expect(wildcardMatch("hello*", "World Hello")).toBe(false);
  });

  it("suffix wildcard", () => {
    expect(wildcardMatch("*world", "Hello World")).toBe(true);
    expect(wildcardMatch("*world", "World Hello")).toBe(false);
  });

  it("contains wildcard", () => {
    expect(wildcardMatch("*ello*", "Hello World")).toBe(true);
    expect(wildcardMatch("*xyz*", "Hello World")).toBe(false);
  });

  it("star matches everything", () => {
    expect(wildcardMatch("*", "anything")).toBe(true);
  });

  it("OR alternatives", () => {
    expect(wildcardMatch("*0xabc*|*0xdef*", "tx to 0xabc123")).toBe(true);
    expect(wildcardMatch("*0xabc*|*0xdef*", "tx to 0xdef456")).toBe(true);
    expect(wildcardMatch("*0xabc*|*0xdef*", "tx to 0x999999")).toBe(false);
  });

  it("OR with spaces", () => {
    expect(wildcardMatch("*abc* | *def*", "has abc here")).toBe(true);
    expect(wildcardMatch("*abc* | *def*", "has def here")).toBe(true);
  });

  it("multiple wildcards", () => {
    expect(wildcardMatch("a*b*c", "aXbYc")).toBe(true);
    expect(wildcardMatch("a*b*c", "aXYc")).toBe(false);
  });
});

describe("ruleMatchesFields", () => {
  it("general rule matches narration", () => {
    const rule: RuleInfo = { id: "r1", pattern: "*coffee*" };
    expect(ruleMatchesFields(rule, { narration: "Coffee Shop" })).toBe(true);
  });

  it("general rule matches payee", () => {
    const rule: RuleInfo = { id: "r1", pattern: "*cafe*" };
    expect(ruleMatchesFields(rule, { payee: "Cafe Nero", narration: "card payment" })).toBe(true);
  });

  it("field-specific rule only matches that field", () => {
    const rule: RuleInfo = { id: "r1", pattern: "*coffee*", match_field: "meta" };
    expect(ruleMatchesFields(rule, { narration: "Coffee Shop", meta: "txn:csv-123" })).toBe(false);
    expect(ruleMatchesFields(rule, { narration: "Coffee Shop", meta: "coffee-order" })).toBe(true);
  });
});

describe("parseAmountCondition", () => {
  it("parses greater than", () => {
    expect(parseAmountCondition(">100")).toEqual({ op: "gt", value: 100 });
  });
  it("parses greater than or equal", () => {
    expect(parseAmountCondition(">=50.5")).toEqual({ op: "gte", value: 50.5 });
  });
  it("parses less than", () => {
    expect(parseAmountCondition("<0")).toEqual({ op: "lt", value: 0 });
  });
  it("parses less than or equal", () => {
    expect(parseAmountCondition("<=50")).toEqual({ op: "lte", value: 50 });
  });
  it("parses equals", () => {
    expect(parseAmountCondition("=100")).toEqual({ op: "eq", value: 100 });
  });
  it("parses range", () => {
    expect(parseAmountCondition("10..500")).toEqual({ op: "range", lo: 10, hi: 500 });
  });
  it("returns null for empty", () => {
    expect(parseAmountCondition("")).toBeNull();
  });
  it("returns null for invalid", () => {
    expect(parseAmountCondition("abc")).toBeNull();
    expect(parseAmountCondition(">abc")).toBeNull();
  });
});

describe("amountMatches", () => {
  it("greater than uses signed value", () => {
    expect(amountMatches({ op: "gt", value: 100 }, 150)).toBe(true);
    expect(amountMatches({ op: "gt", value: 100 }, -150)).toBe(false); // negative is not > 100
    expect(amountMatches({ op: "gt", value: 100 }, 50)).toBe(false);
  });
  it("gte boundary", () => {
    expect(amountMatches({ op: "gte", value: 100 }, 100)).toBe(true);
    expect(amountMatches({ op: "gte", value: 100 }, 99.9)).toBe(false);
  });
  it("less than for outflows", () => {
    expect(amountMatches({ op: "lt", value: 0 }, -50)).toBe(true); // outflow
    expect(amountMatches({ op: "lt", value: 0 }, 50)).toBe(false); // inflow
    expect(amountMatches({ op: "lt", value: 100 }, 50)).toBe(true);
    expect(amountMatches({ op: "lt", value: 100 }, 150)).toBe(false);
  });
  it("lte boundary", () => {
    expect(amountMatches({ op: "lte", value: 50 }, 50)).toBe(true);
  });
  it("equals uses signed value", () => {
    expect(amountMatches({ op: "eq", value: 100 }, 100)).toBe(true);
    expect(amountMatches({ op: "eq", value: 100 }, -100)).toBe(false); // signed
    expect(amountMatches({ op: "eq", value: 100 }, 100.1)).toBe(false);
  });
  it("range uses signed value", () => {
    expect(amountMatches({ op: "range", lo: 10, hi: 500 }, 150)).toBe(true);
    expect(amountMatches({ op: "range", lo: 10, hi: 500 }, -150)).toBe(false); // signed
    expect(amountMatches({ op: "range", lo: 10, hi: 500 }, 5)).toBe(false);
    expect(amountMatches({ op: "range", lo: 10, hi: 500 }, 600)).toBe(false);
  });
  it("negative range for outflows", () => {
    expect(amountMatches({ op: "range", lo: -500, hi: -10 }, -150)).toBe(true);
    expect(amountMatches({ op: "range", lo: -500, hi: -10 }, 150)).toBe(false);
  });
});

describe("ruleMatchesFields with amount", () => {
  it("rule with amount condition filters by signed amount", () => {
    const rule: RuleInfo = { id: "r1", pattern: "*Payment*", amount_condition: "<-100" };
    expect(ruleMatchesFields(rule, { narration: "Payment Big", amount: -150 })).toBe(true);
    expect(ruleMatchesFields(rule, { narration: "Payment Small", amount: -50 })).toBe(false);
  });

  it("rule without amount condition ignores amount", () => {
    const rule: RuleInfo = { id: "r1", pattern: "*Payment*" };
    expect(ruleMatchesFields(rule, { narration: "Payment", amount: -50 })).toBe(true);
  });

  it("amount condition AND pattern must both match", () => {
    const rule: RuleInfo = { id: "r1", pattern: "*Coffee*", amount_condition: "<0" };
    expect(ruleMatchesFields(rule, { narration: "Coffee", amount: -200 })).toBe(true);
    expect(ruleMatchesFields(rule, { narration: "Coffee", amount: 5 })).toBe(false);
    expect(ruleMatchesFields(rule, { narration: "Tea", amount: -200 })).toBe(false);
  });
});

describe("ruleMatchesFields with payee_condition", () => {
  it("payee_condition restricts match to matching payee", () => {
    const rule: RuleInfo = { id: "r1", pattern: "*token_transfer*", payee_condition: "*Jupiter*" };
    expect(ruleMatchesFields(rule, { payee: "Jupiter", narration: "token_transfer USDC" })).toBe(true);
    expect(ruleMatchesFields(rule, { payee: "Raydium", narration: "token_transfer USDC" })).toBe(false);
  });

  it("payee_condition matches display_payee", () => {
    const rule: RuleInfo = { id: "r1", pattern: "*token_transfer*", payee_condition: "*Jupiter*" };
    expect(ruleMatchesFields(rule, { payee: "CD6M9PpxUExFy8Gwdn", display_payee: "Jupiter", narration: "token_transfer" })).toBe(true);
    expect(ruleMatchesFields(rule, { payee: "CD6M9PpxUExFy8Gwdn", display_payee: "Raydium", narration: "token_transfer" })).toBe(false);
  });

  it("no payee_condition means payee is not checked", () => {
    const rule: RuleInfo = { id: "r1", pattern: "*token_transfer*" };
    expect(ruleMatchesFields(rule, { payee: "Raydium", narration: "token_transfer USDC" })).toBe(true);
    expect(ruleMatchesFields(rule, { payee: "Jupiter", narration: "token_transfer USDC" })).toBe(true);
  });
});

describe("computeRulePreview with payee_condition", () => {
  it("payee_condition filters preview matches", () => {
    const txns = [
      { payee: "Jupiter", narration: "token_transfer USDC", meta: "", amount: 0, amount_commodity: "USDC", postings: [{ account: "assets:sol", commodity: "USDC" }] },
      { payee: "Raydium", narration: "token_transfer SOL", meta: "", amount: 0, amount_commodity: "SOL", postings: [{ account: "assets:sol", commodity: "SOL" }] },
      { payee: "Jupiter", narration: "token_transfer JUP", meta: "", amount: 0, amount_commodity: "JUP", postings: [{ account: "assets:sol", commodity: "JUP" }] },
    ];
    const result = computeRulePreview("*token_transfer*", "", "", [], txns, "", "", undefined, "*Jupiter*");
    expect(result).toHaveLength(2);
    expect(result[0].transactionIndex).toBe(0);
    expect(result[1].transactionIndex).toBe(2);
  });

  it("payee_condition alone (no pattern) still returns matches", () => {
    const txns = [
      { payee: "0x1849964c", display_payee: "SCAMMER", narration: "token_transfer:send USDC", meta: "", amount: 0, amount_commodity: "USDC", postings: [{ account: "assets:eth", commodity: "USDC" }] },
      { payee: "Binance", narration: "Deposit", meta: "", amount: 0, amount_commodity: "USDC", postings: [{ account: "assets:exchange", commodity: "USDC" }] },
    ];
    const result = computeRulePreview("", "", "", [], txns, "", "", undefined, "*SCAMMER*");
    expect(result).toHaveLength(1);
    expect(result[0].transactionIndex).toBe(0);
  });
});

describe("computeRulePreview", () => {
  const txns = [
    { payee: "Coffee Shop", narration: "card purchase", meta: "txn:csv-001", amount: 0, amount_commodity: "AUD", postings: [{ account: "expenses:unknown", commodity: "AUD" }] },
    { payee: "Tea House", narration: "card purchase", meta: "txn:csv-002", amount: 0, amount_commodity: "AUD", postings: [{ account: "expenses:unknown", commodity: "AUD" }] },
    { payee: "Coffee Palace", narration: "online order", meta: "txn:csv-003", amount: 0, amount_commodity: "AUD", postings: [{ account: "expenses:unknown", commodity: "AUD" }] },
  ];

  it("returns matching transactions", () => {
    const result = computeRulePreview("*coffee*", "", "", [], txns);
    expect(result.length).toBe(2);
    expect(result[0].transactionIndex).toBe(0);
    expect(result[1].transactionIndex).toBe(2);
  });

  it("detects shadowed transactions", () => {
    const existingRules: RuleInfo[] = [
      { id: "r-existing", pattern: "*Coffee Shop*", amount_account: "expenses:food" },
    ];
    const result = computeRulePreview("*coffee*", "", "r-new", existingRules, txns);
    // Both match, but txn 0 ("Coffee Shop") is shadowed by r-existing
    expect(result.length).toBe(2);
    expect(result[0].shadowed).toBe(true);
    expect(result[0].shadowedByRuleId).toBe("r-existing");
    expect(result[1].shadowed).toBe(false);
  });

  it("field-specific rules shadow general rules", () => {
    const existingRules: RuleInfo[] = [
      { id: "r-meta", pattern: "txn:csv-001", match_field: "meta", amount_account: "expenses:specific" },
    ];
    // Draft is a general rule matching *coffee*
    const result = computeRulePreview("*coffee*", "", "r-new", existingRules, txns);
    expect(result.length).toBe(2);
    expect(result[0].shadowed).toBe(true); // shadowed by field-specific r-meta
    expect(result[1].shadowed).toBe(false);
  });

  it("empty pattern returns no matches", () => {
    expect(computeRulePreview("", "", "", [], txns)).toEqual([]);
  });

  it("filters by amount condition in preview", () => {
    const amountTxns = [
      { payee: "Payment A", narration: "card", meta: "m1", amount: -50, amount_commodity: "AUD", postings: [{ account: "expenses:unknown", commodity: "AUD", amount: -50 }] },
      { payee: "Payment B", narration: "card", meta: "m2", amount: -150, amount_commodity: "AUD", postings: [{ account: "expenses:unknown", commodity: "AUD", amount: -150 }] },
      { payee: "Payment C", narration: "card", meta: "m3", amount: -200, amount_commodity: "AUD", postings: [{ account: "expenses:unknown", commodity: "AUD", amount: -200 }] },
    ];
    // <-100 matches outflows with magnitude > 100 (i.e. -150, -200)
    const result = computeRulePreview("*Payment*", "", "", [], amountTxns, "<-100");
    expect(result.length).toBe(2);
    expect(result[0].transactionIndex).toBe(1); // Payment B (-150)
    expect(result[1].transactionIndex).toBe(2); // Payment C (-200)
  });

  it("matches via raw payee for ethereum address pattern (display_payee set)", () => {
    const ethTxns = [
      {
        payee: "0x7bbda8b821fdb6aaa3bda748a2befefec68cd805",
        display_payee: "Self Transfer",
        narration: "token_transfer TRX",
        meta: "txn:csv-8d8fcf4b6431, rule:eth-self-wallet-13",
        amount: 0,
        amount_commodity: "TRX",
        postings: [{ account: "assets:ethereum", commodity: "TRX" }],
      },
      {
        payee: "0xdeadbeef",
        narration: "token_transfer ETH",
        meta: "txn:csv-other",
        amount: 0,
        amount_commodity: "ETH",
        postings: [{ account: "assets:ethereum", commodity: "ETH" }],
      },
    ];
    const allRules: RuleInfo[] = [
      { id: "eth-self-wallet-13", pattern: "*0x7bbda8b821fdb6aaa3bda748a2befefec68cd805*", payee: "Self Transfer", amount_account: "assets:ethereum:wallet" },
    ];
    const result = computeRulePreview(
      "*0x7bbda8b821fdb6aaa3bda748a2befefec68cd805*",
      "",
      "eth-self-wallet-13",
      allRules,
      ethTxns,
    );
    expect(result.length).toBe(1);
    expect(result[0].transactionIndex).toBe(0);
    expect(result[0].shadowed).toBe(false);
  });

  it("default catch-all rule does not shadow transactions", () => {
    const existingRules: RuleInfo[] = [
      { id: "r-default", pattern: "*", amount_account: "expenses:unknown" },
    ];
    const result = computeRulePreview("*coffee*", "", "r-new", existingRules, txns);
    expect(result.length).toBe(2);
    expect(result[0].shadowed).toBe(false);
    expect(result[1].shadowed).toBe(false);
  });
});

describe("accountPrefixForScope", () => {
  it("local returns exact account", () => {
    expect(accountPrefixForScope("assets:kraken:personal", "local")).toBe("assets:kraken:personal");
  });

  it("institution strips last segment", () => {
    expect(accountPrefixForScope("assets:kraken:personal", "institution")).toBe("assets:kraken");
  });

  it("institution on 2-segment account returns itself", () => {
    expect(accountPrefixForScope("assets:savings", "institution")).toBe("assets:savings");
  });

  it("global returns empty string", () => {
    expect(accountPrefixForScope("assets:kraken:personal", "global")).toBe("");
  });
});

describe("filterByAccountPrefix", () => {
  const mkTxn = (account: string) => ({
    date: "2025-01-01",
    datetime: "2025-01-01",
    postings: [{ account, amount: 100, commodity: "USD" }],
  }) as any;

  const txns = [
    mkTxn("assets:kraken:personal"),
    mkTxn("assets:kraken:business"),
    mkTxn("assets:commsec"),
    mkTxn("assets:ethereum"),
  ];

  it("empty prefix returns all", () => {
    expect(filterByAccountPrefix(txns, "")).toHaveLength(4);
  });

  it("exact match", () => {
    expect(filterByAccountPrefix(txns, "assets:kraken:personal")).toHaveLength(1);
  });

  it("prefix match includes children", () => {
    expect(filterByAccountPrefix(txns, "assets:kraken")).toHaveLength(2);
  });

  it("no partial segment match", () => {
    expect(filterByAccountPrefix(txns, "assets:krak")).toHaveLength(0);
  });

  it("all assets match", () => {
    expect(filterByAccountPrefix(txns, "assets")).toHaveLength(4);
  });

  it("handles transactions with empty postings array", () => {
    const withEmpty = [...txns, { postings: [] } as any];
    expect(filterByAccountPrefix(withEmpty, "assets:kraken")).toHaveLength(2);
  });

  it("non-matching prefix returns empty", () => {
    expect(filterByAccountPrefix(txns, "liabilities")).toHaveLength(0);
  });
});

describe("scoped preview (filterByAccountPrefix + computeRulePreview)", () => {
  // Simulates the rule editor workflow: filter transactions by scope, then preview matches
  const mkTxn = (account: string, narration: string) => ({
    narration,
    amount: 0,
    amount_commodity: "USD",
    postings: [{ account, commodity: "USD" }],
  });

  const allTxns = [
    mkTxn("assets:kraken:personal", "trade tradespot"),
    mkTxn("assets:kraken:personal", "deposit"),
    mkTxn("assets:kraken:business", "trade tradespot"),
    mkTxn("assets:commsec", "trade CommSec"),
    mkTxn("assets:commsec", "deposit"),
    mkTxn("assets:ethereum", "normal:deposit"),
  ];

  it("local scope only previews the selected account", () => {
    const prefix = accountPrefixForScope("assets:kraken:personal", "local");
    const scoped = filterByAccountPrefix(allTxns, prefix);
    expect(scoped).toHaveLength(2);
    const result = computeRulePreview("*trade*", "", "", [], scoped);
    expect(result).toHaveLength(1);
    expect(result[0].transactionIndex).toBe(0);
    // Index 0 in scoped array is kraken:personal "trade tradespot"
    expect(scoped[result[0].transactionIndex].narration).toBe("trade tradespot");
  });

  it("institution scope includes sibling accounts", () => {
    const prefix = accountPrefixForScope("assets:kraken:personal", "institution");
    const scoped = filterByAccountPrefix(allTxns, prefix);
    expect(scoped).toHaveLength(3); // personal + business
    const result = computeRulePreview("*trade*", "", "", [], scoped);
    expect(result).toHaveLength(2);
    expect(scoped[result[0].transactionIndex].narration).toBe("trade tradespot");
    expect(scoped[result[1].transactionIndex].narration).toBe("trade tradespot");
  });

  it("global scope searches all transactions", () => {
    const prefix = accountPrefixForScope("assets:kraken:personal", "global");
    const scoped = filterByAccountPrefix(allTxns, prefix);
    expect(scoped).toHaveLength(6);
    const result = computeRulePreview("*deposit*", "", "", [], scoped);
    expect(result).toHaveLength(3); // kraken:personal, commsec, ethereum
  });

  it("preview indices map correctly to scoped array, not full array", () => {
    // This is the exact bug that was fixed: indices must reference scopedTxns, not allTxns
    const prefix = accountPrefixForScope("assets:commsec", "local");
    const scoped = filterByAccountPrefix(allTxns, prefix);
    expect(scoped).toHaveLength(2);
    const result = computeRulePreview("*trade*", "", "", [], scoped);
    expect(result).toHaveLength(1);
    // Index should be into scoped array (commsec txns), not the full array
    const matched = scoped[result[0].transactionIndex];
    expect(matched.narration).toBe("trade CommSec");
    expect((matched.postings[0] as any).account).toBe("assets:commsec");
  });
});

// filterBySearch tests moved to Rust BDD: transaction_search.feature

describe("isDefaultRule", () => {
  it("catch-all with pattern * and no payee is default", () => {
    expect(isDefaultRule({ id: "r1", pattern: "*" })).toBe(true);
  });

  it("catch-all with expenses:unknown is default", () => {
    expect(isDefaultRule({ id: "r1", pattern: "*", amount_account: "expenses:unknown" })).toBe(true);
  });

  it("rule with specific pattern is NOT default", () => {
    expect(isDefaultRule({ id: "r1", pattern: "*Coffee*" })).toBe(false);
  });

  it("rule with payee is NOT default", () => {
    expect(isDefaultRule({ id: "r1", pattern: "*", payee: "Merchant" })).toBe(false);
  });

  it("rule with specific account is NOT default", () => {
    expect(isDefaultRule({ id: "r1", pattern: "*", amount_account: "expenses:groceries" })).toBe(false);
  });
});

describe("buildRuleFromDraft — all search bar filters saved to rule", () => {
  const keywords = ["payee", "narration", "meta", "date", "account", "commodity", "amount", "fee", "field"];

  function build(overrides: Partial<Parameters<typeof buildRuleFromDraft>[0]>) {
    return buildRuleFromDraft({
      pattern: "", matchField: "", amountCondition: "", feeCondition: "",
      payeeCondition: "", narrationCondition: "", commodityCondition: "", metaCondition: "",
      filterPills: [], comment: "",
      amountAccount: "", feeAccount: "", ...overrides,
    }, parseSmartInput, toFlatString, keywords);
  }

  it("saves payee_condition from draft state", () => {
    const rule = build({
      pattern: "*token_transfer:send USDC*",
      payeeCondition: "*Jupiter*",
      amountAccount: "assets:transfer",
    });
    expect(rule.pattern).toBe("*token_transfer:send USDC*");
    expect(rule.payee_condition).toBe("*Jupiter*");
    expect(rule.amount_account).toBe("assets:transfer");
  });

  it("saves amount_condition from draft state", () => {
    const rule = build({
      pattern: "*Payment*",
      amountCondition: "<-100",
    });
    expect(rule.amount_condition).toBe("<-100");
  });

  it("saves fee_condition from draft state", () => {
    const rule = build({
      pattern: "*swap*",
      feeCondition: ">0",
    });
    expect(rule.fee_condition).toBe(">0");
  });

  it("saves match_field from draft state", () => {
    const rule = build({
      pattern: "*coffee*",
      matchField: "narration",
    });
    expect(rule.match_field).toBe("narration");
  });

  it("saves all conditions together", () => {
    const rule = build({
      pattern: "*token_transfer*",
      matchField: "",
      payeeCondition: "*Swell Network*",
      amountCondition: ">100",
      feeCondition: ">0",
      amountAccount: "income:crypto:airdrop",
      comment: "airdrop",
    });
    expect(rule.pattern).toBe("*token_transfer*");
    expect(rule.payee_condition).toBe("*Swell Network*");
    expect(rule.amount_condition).toBe(">100");
    expect(rule.fee_condition).toBe(">0");
    expect(rule.amount_account).toBe("income:crypto:airdrop");
    expect(rule.comment).toBe("airdrop");
  });

  it("filter pills (like -account:ignore) do not leak into saved rule", () => {
    const rule = build({
      pattern: "*token_transfer*",
      filterPills: [{ key: "account", value: "ignore", negated: true }],
      amountAccount: "expenses:unknown",
    });
    expect(rule.pattern).toBe("*token_transfer*");
    expect(rule.amount_account).toBe("expenses:unknown");
    // Filter pills should not appear as any rule field
    expect(rule.match_field).toBeNull();
    expect(rule.payee_condition).toBeNull();
  });

  it("payee-only rule (no pattern) becomes payee match_field", () => {
    const rule = build({
      pattern: "",
      payeeCondition: "*Jupiter*",
      amountAccount: "income",
    });
    expect(rule.pattern).toBe("*Jupiter*");
    expect(rule.match_field).toBe("payee");
    expect(rule.payee_condition).toBeNull();
  });

  it("empty fields are saved as null", () => {
    const rule = build({ pattern: "*test*" });
    expect(rule.comment).toBeNull();
    expect(rule.amount_condition).toBeNull();
    expect(rule.fee_condition).toBeNull();
    expect(rule.payee_condition).toBeNull();
    expect(rule.match_field).toBeNull();
    expect(rule.amount_account).toBeNull();
    expect(rule.fee_account).toBeNull();
  });

  it("cross-field example: amount range + commodity + payee + account", () => {
    // amount:>0 amount:<10 commodity:SOL payee:Kraken produces one saved
    // rule: amount collapsed to 0..10 range, commodity promoted to pattern
    // + match_field=commodity, Kraken as the payee_condition (exact —
    // user did not type wildcards).
    const rule = build({
      pattern: "amount:>0 amount:<10 commodity:SOL payee:Kraken",
      amountAccount: "expenses:trading",
    });
    expect(rule.amount_condition).toBe("0..10");
    expect(rule.payee_condition).toBe("Kraken");
    expect(rule.pattern).toBe("SOL");
    expect(rule.match_field).toBe("commodity");
    expect(rule.amount_account).toBe("expenses:trading");
  });

  it("two amount pills typed in the editor merge into one range condition", () => {
    // Regression for: rule saved with `amount_condition: "<0"` after the
    // user added BOTH `amount:>-0.03` and `amount:<0` pills. The pattern
    // text holds both pills as inline keywords (the rule editor's smart
    // search re-parses the input on save), so both should land in the
    // saved rule as a single range.
    const rule = build({
      pattern: "*token_transfer:* amount:>-0.03 amount:<0",
      amountAccount: "expenses:crypto:fees",
    });
    expect(rule.pattern).toBe("*token_transfer:*");
    expect(rule.amount_condition).toBe("-0.03..0");
    expect(rule.amount_account).toBe("expenses:crypto:fees");
  });
});

// Integration test that mirrors the actual UI data flow: typed input →
// parseSmartInput → change handler runs extractRuleConfigPills and stores
// the combined values in `draft.amountCondition` / `draft.payeeCondition`
// while putting leftover non-config pills in `draft.filterPills` →
// readAndBuildRule passes that draft to buildRuleFromDraft.
//
// The earlier "buildRuleFromDraft" tests pass a pre-formatted string as
// `pattern`, which makes buildRuleFromDraft re-parse from scratch. That
// hides any bug where the change handler's split between draft.* and
// filterPills loses information when buildRuleFromDraft's second-pass
// extract runs on filterPills alone.
describe("rule editor full UI flow (parse → change handler → save)", () => {
  const ruleKeywords = ["payee", "narration", "meta", "date", "account", "commodity", "amount", "fee", "field"];

  /** Simulates what attachSmartSearch's change callback does in main.ts:
   *  parses user-typed input, runs extractRuleConfigPills, returns the
   *  shape of state.ruleEditorDraft after the keystroke settles. */
  function simulateChangeHandler(typed: string) {
    const { pills, text } = parseSmartInput(typed, ruleKeywords);
    const extracted = extractRuleConfigPills(pills);
    return {
      pattern: text,
      matchField: extracted.matchField,
      amountCondition: extracted.amountCondition,
      feeCondition: extracted.feeCondition,
      payeeCondition: extracted.payeeCondition,
      narrationCondition: extracted.narrationCondition,
      commodityCondition: extracted.commodityCondition,
      metaCondition: extracted.metaCondition,
      filterPills: extracted.filterPills,
    };
  }

  /** Simulates readAndBuildRule + Save: takes the post-change-handler
   *  draft state and runs it through buildRuleFromDraft. */
  function simulateSave(typed: string, amountAccount = "expenses:trading") {
    const draftFromHandler = simulateChangeHandler(typed);
    return buildRuleFromDraft({
      ...draftFromHandler,
      comment: "",
      amountAccount,
      feeAccount: "",
    }, parseSmartInput, toFlatString, ruleKeywords);
  }

  it("typing two amount pills + commodity + payee saves all four", () => {
    const rule = simulateSave("amount:>0 amount:<10 commodity:SOL payee:Kraken");
    expect(rule.amount_condition).toBe("0..10");
    expect(rule.payee_condition).toBe("Kraken");
    expect(rule.pattern).toBe("SOL");
    expect(rule.match_field).toBe("commodity");
  });

  it("typing two amounts + commodity (no payee) still saves both bounds", () => {
    const rule = simulateSave("amount:>0 amount:<10 commodity:SOL");
    expect(rule.amount_condition).toBe("0..10");
    expect(rule.pattern).toBe("SOL");
    expect(rule.match_field).toBe("commodity");
  });

  it("typing two amount pills with a narration pattern saves the range", () => {
    const rule = simulateSave("amount:>-0.03 amount:<0 *token_transfer*");
    expect(rule.amount_condition).toBe("-0.03..0");
    expect(rule.pattern).toBe("*token_transfer*");
  });

  it("the change-handler/save round-trip does not drop the second amount pill", () => {
    // The bug we shipped was: change handler stored only the first amount
    // pill, second went to filterPills, and on save buildRuleFromDraft's
    // re-extract from filterPills alone produced a single-bound condition.
    // After fix: combined value lives in draft.amountCondition and survives.
    const draft = simulateChangeHandler("amount:>0 amount:<10 commodity:SOL");
    expect(draft.amountCondition).toBe("0..10");
    // commodity is now a config field (commodity_condition), not a leftover
    expect(draft.filterPills).toEqual([]);
  });

  it("editing an existing narration rule and adding commodity:SOL keeps both", () => {
    // The actual bug the user hit: existing rule with pattern *token_transfer:*
    // (narration), user typed amount:>-0.003 amount:<0 commodity:SOL on top.
    // The saved rule had the amount range but NO commodity — silently dropped
    // because derivePatternFromPills returned early when pattern was non-empty.
    // After fix: commodity:SOL is saved as commodity_condition, ANDed with the
    // narration pattern at match time.
    const draftFromHandler = simulateChangeHandler("amount:>-0.003 amount:<0 commodity:SOL");
    // User did not change the pattern itself — it was loaded from the existing rule.
    const draftWithExistingPattern = { ...draftFromHandler, pattern: "*token_transfer:*", matchField: "narration" };
    const saved = buildRuleFromDraft({
      ...draftWithExistingPattern,
      comment: "", amountAccount: "expenses:crypto:fees", feeAccount: "",
    }, parseSmartInput, toFlatString, ruleKeywords);

    expect(saved.pattern).toBe("*token_transfer:*");
    expect(saved.match_field).toBe("narration");
    expect(saved.amount_condition).toBe("-0.003..0");
    expect(saved.commodity_condition).toBe("SOL");
    expect(saved.amount_account).toBe("expenses:crypto:fees");
  });

  it("commodity, narration, meta, payee conditions all save alongside the pattern", () => {
    const draftFromHandler = simulateChangeHandler("commodity:SOL narration:swap meta:abc123 payee:Kraken");
    const draftWithPattern = { ...draftFromHandler, pattern: "*token_transfer:*", matchField: "narration" };
    const saved = buildRuleFromDraft({
      ...draftWithPattern,
      comment: "", amountAccount: "expenses:trading", feeAccount: "",
    }, parseSmartInput, toFlatString, ruleKeywords);

    expect(saved.pattern).toBe("*token_transfer:*");
    expect(saved.commodity_condition).toBe("SOL");
    expect(saved.narration_condition).toBe("swap");
    expect(saved.meta_condition).toBe("abc123");
    expect(saved.payee_condition).toBe("Kraken");
  });

  it("promotes narration condition to pattern when pattern is empty", () => {
    // No literal pattern + only narration:swap → narration becomes pattern
    // with match_field=narration. Same shape as commodity promotion.
    const rule = simulateSave("narration:swap");
    expect(rule.pattern).toBe("swap");
    expect(rule.match_field).toBe("narration");
    expect(rule.narration_condition).toBe(null);
  });

  it("promotes meta condition to pattern when pattern is empty", () => {
    const rule = simulateSave("meta:abc123");
    expect(rule.pattern).toBe("abc123");
    expect(rule.match_field).toBe("meta");
    expect(rule.meta_condition).toBe(null);
  });

  it("promotes payee condition to pattern when no other condition wins", () => {
    const rule = simulateSave("payee:Kraken");
    expect(rule.pattern).toBe("Kraken");
    expect(rule.match_field).toBe("payee");
    expect(rule.payee_condition).toBe(null);
  });
});

describe("categoryEditRuleMatch", () => {
  it("prefers the per-leg id so the rule binds to a single leg", () => {
    // A swap/multi-hop leg shares its txn: hash with siblings; the leg: anchor
    // must win so allocating one leg doesn't bleed onto the others.
    expect(categoryEditRuleMatch("leg:l-abc123", "txn:0xhash", "trade DYDX")).toEqual({
      pattern: "leg:l-abc123",
      matchField: "meta",
    });
  });

  it("uses the txn id when there is no leg id (single-leg row)", () => {
    // Editing a single-leg posting creates a rule that matches ONLY this
    // transaction by its unique id, not every future row sharing the narration.
    expect(categoryEditRuleMatch("", "txn:csv-abc123", "card purchase")).toEqual({
      pattern: "txn:csv-abc123",
      matchField: "meta",
    });
  });

  it("falls back to a narration wildcard when no id is available", () => {
    expect(categoryEditRuleMatch("", "", "card purchase")).toEqual({
      pattern: "*card purchase*",
      matchField: null,
    });
  });
});

describe("extractRuleConfigPills", () => {
  it("extracts field, amount, fee, payee from pills verbatim (no auto-wrap)", () => {
    // Auto-wrapping `payee:Jupiter` to `*Jupiter*` silently turns every
    // user-typed value into a substring match. The user typed Jupiter, so
    // the saved condition is exact "Jupiter" — substring needs explicit `*`s.
    const pills = [
      { key: "field", value: "narration" },
      { key: "amount", value: ">100" },
      { key: "fee", value: ">0" },
      { key: "payee", value: "Jupiter" },
      { key: "commodity", value: "ETH" },
    ];
    const result = extractRuleConfigPills(pills);
    expect(result.matchField).toBe("narration");
    expect(result.amountCondition).toBe(">100");
    expect(result.feeCondition).toBe(">0");
    expect(result.payeeCondition).toBe("Jupiter");
    expect(result.commodityCondition).toBe("ETH");
    expect(result.filterPills).toEqual([]);
  });

  it("preserves substring wildcards when user types them explicitly", () => {
    const pills = [
      { key: "payee", value: "*Orca*" },
      { key: "narration", value: "*token_transfer:receive*" },
      { key: "meta", value: "*csv-abc*" },
    ];
    const result = extractRuleConfigPills(pills);
    expect(result.payeeCondition).toBe("*Orca*");
    expect(result.narrationCondition).toBe("*token_transfer:receive*");
    expect(result.metaCondition).toBe("*csv-abc*");
  });

  it("negated pills are not extracted as config", () => {
    const pills = [{ key: "amount", value: ">100", negated: true }];
    const result = extractRuleConfigPills(pills);
    expect(result.amountCondition).toBe("");
    expect(result.filterPills).toEqual([{ key: "amount", value: ">100", negated: true }]);
  });

  it("combines `>` lower-bound and `<` upper-bound into a range", () => {
    // Regression: user adds two amount pills (>-0.03 AND <0) but only one
    // ends up in the saved rule. Two bound-style pills should merge into
    // a single range condition that encodes both bounds.
    const pills = [
      { key: "amount", value: ">-0.03" },
      { key: "amount", value: "<0" },
    ];
    const result = extractRuleConfigPills(pills);
    expect(result.amountCondition).toBe("-0.03..0");
    expect(result.filterPills).toEqual([]);
  });

  it("combines `>=` and `<=` into a range regardless of pill order", () => {
    const pills = [
      { key: "amount", value: "<=20" },
      { key: "amount", value: ">=10" },
    ];
    const result = extractRuleConfigPills(pills);
    expect(result.amountCondition).toBe("10..20");
    expect(result.filterPills).toEqual([]);
  });

  it("multiple non-rangeable amount pills: last edit wins (no silent drop)", () => {
    // Two `>` pills can't form a range — keep the user's most recent edit
    // rather than silently dropping it (the prior bug was first-wins, which
    // hid the user's intended change).
    const pills = [
      { key: "amount", value: ">100" },
      { key: "amount", value: ">50" },
    ];
    const result = extractRuleConfigPills(pills);
    expect(result.amountCondition).toBe(">50");
    expect(result.filterPills).toEqual([]);
  });

  it("combines two `fee` pills into a range the same way", () => {
    const pills = [
      { key: "fee", value: ">0" },
      { key: "fee", value: "<5" },
    ];
    const result = extractRuleConfigPills(pills);
    expect(result.feeCondition).toBe("0..5");
    expect(result.filterPills).toEqual([]);
  });

  it("empty pills returns empty config", () => {
    const result = extractRuleConfigPills([]);
    expect(result.matchField).toBe("");
    expect(result.amountCondition).toBe("");
    expect(result.feeCondition).toBe("");
    expect(result.payeeCondition).toBe("");
    expect(result.filterPills).toEqual([]);
  });
});

describe("draftConditionsToPills", () => {
  it("converts all conditions to display pills, preserving wildcards verbatim", () => {
    // Wildcards must stay visible — stripping them silently turned a
    // substring rule (*Orca* matches "Francium lyfOrca Program") into
    // something that looked like an exact-match rule in the editor.
    const pills = draftConditionsToPills({
      matchField: "narration", amountCondition: ">100",
      feeCondition: ">0", payeeCondition: "*Jupiter*",
    });
    expect(pills).toEqual([
      { key: "field", value: "narration" },
      { key: "amount", value: ">100" },
      { key: "fee", value: ">0" },
      { key: "payee", value: "*Jupiter*" },
    ]);
  });

  it("preserves *...*  wildcards on payeeCondition for visibility", () => {
    const pills = draftConditionsToPills({
      matchField: "", amountCondition: "", feeCondition: "",
      payeeCondition: "*Swell Network*",
    });
    expect(pills).toEqual([{ key: "payee", value: "*Swell Network*" }]);
  });

  it("preserves wildcards on narration and meta conditions too", () => {
    const pills = draftConditionsToPills({
      matchField: "", amountCondition: "", feeCondition: "",
      payeeCondition: "",
      narrationCondition: "*swap*",
      metaCondition: "*csv-abc*",
    });
    expect(pills).toEqual([
      { key: "narration", value: "*swap*" },
      { key: "meta", value: "*csv-abc*" },
    ]);
  });

  it("shows exact-match conditions (no stars) as the literal value", () => {
    const pills = draftConditionsToPills({
      matchField: "", amountCondition: "", feeCondition: "",
      payeeCondition: "Orca",
    });
    expect(pills).toEqual([{ key: "payee", value: "Orca" }]);
  });

  it("skips empty conditions", () => {
    const pills = draftConditionsToPills({
      matchField: "", amountCondition: ">100", feeCondition: "", payeeCondition: "",
    });
    expect(pills).toEqual([{ key: "amount", value: ">100" }]);
  });
});

describe("derivePatternAndPayeeCondition", () => {
  it("narration + label: pattern matches narration, payee_condition is the label", () => {
    const out = derivePatternAndPayeeCondition("Coffee", "Starbucks", "0xabc...123", "");
    expect(out.pattern).toBe("*Coffee*");
    expect(out.matchField).toBe("");
    expect(out.payeeCondition).toBe("*Starbucks*");
  });

  it("narration + no label: payee_condition falls back to the address", () => {
    const out = derivePatternAndPayeeCondition("Coffee", "", "0xabc...123", "");
    expect(out.pattern).toBe("*Coffee*");
    expect(out.payeeCondition).toBe("*0xabc...123*");
  });

  it("narration + no payee at all: payee_condition empty", () => {
    const out = derivePatternAndPayeeCondition("Coffee", "", "", "");
    expect(out.pattern).toBe("*Coffee*");
    expect(out.payeeCondition).toBe("");
  });

  it("no narration + label: pattern becomes the label, matchField=payee", () => {
    const out = derivePatternAndPayeeCondition("", "Starbucks", "0xabc...123", "");
    expect(out.pattern).toBe("*Starbucks*");
    expect(out.matchField).toBe("payee");
    expect(out.payeeCondition).toBe("");
  });

  it("no narration + no label: pattern falls back to the address", () => {
    const out = derivePatternAndPayeeCondition("", "", "0xabc...123", "");
    expect(out.pattern).toBe("*0xabc...123*");
    expect(out.matchField).toBe("payee");
  });

  it("an existing payeeCondition (from a search pill) wins over the row-derived payee", () => {
    const out = derivePatternAndPayeeCondition("Coffee", "Starbucks", "0xabc", "*Costa*");
    expect(out.pattern).toBe("*Coffee*");
    expect(out.payeeCondition).toBe("*Costa*");
  });

  it("everything empty yields an empty draft", () => {
    const out = derivePatternAndPayeeCondition("", "", "", "");
    expect(out).toEqual({ pattern: "", matchField: "", payeeCondition: "" });
  });
});

describe("mergeRuleConditions", () => {
  const emptyDraft = {
    matchField: "",
    amountCondition: "",
    feeCondition: "",
    payeeCondition: "",
    narrationCondition: "",
    commodityCondition: "",
    metaCondition: "",
    filterPills: [],
  };

  it("preserves commodityCondition already promoted into the draft (regression)", () => {
    // Repro for the rule editor preview ignoring `commodity:FEE`:
    // pills typed in the search bar are extracted into draft.commodityCondition
    // by the keystroke handler, but on the next render the text input is empty
    // (textPills = []), so the merged conditions must come from the draft.
    const draft = { ...emptyDraft, payeeCondition: "Kraken", commodityCondition: "FEE" };
    const { conditions } = mergeRuleConditions(draft, []);
    expect(conditions).toMatchObject({ payeeCondition: "Kraken", commodityCondition: "FEE" });
  });

  it("preserves narrationCondition and metaCondition from the draft", () => {
    const draft = { ...emptyDraft, narrationCondition: "*tea*", metaCondition: "*csv-001*" };
    const { conditions } = mergeRuleConditions(draft, []);
    expect(conditions).toMatchObject({ narrationCondition: "*tea*", metaCondition: "*csv-001*" });
  });

  it("text-pill conditions fill in when the draft slot is empty", () => {
    const { conditions } = mergeRuleConditions(emptyDraft, [{ key: "commodity", value: "ETH" }]);
    expect(conditions).toMatchObject({ commodityCondition: "ETH" });
  });
});
