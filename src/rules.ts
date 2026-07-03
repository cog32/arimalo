/**
 * TypeScript port of the Rust wildcard_match from src-tauri/src/rules.rs.
 * Used for client-side rule preview in the rules editor page.
 */

/** Walk through glob parts (from `pattern.split("*")`) confirming each
 *  non-empty part appears in order within text. Anchors the first part to
 *  the start and the last part to the end. */
function matchWildcardParts(parts: string[], text: string): boolean {
  let remaining = text;
  for (let i = 0; i < parts.length; i++) {
    const part = parts[i];
    if (part === "") continue;
    if (i === 0) {
      if (!remaining.startsWith(part)) return false;
      remaining = remaining.slice(part.length);
    } else if (i === parts.length - 1) {
      if (!remaining.endsWith(part)) return false;
    } else {
      const pos = remaining.indexOf(part);
      if (pos === -1) return false;
      remaining = remaining.slice(pos + part.length);
    }
  }
  return true;
}

/** Match a single glob pattern (no `|` OR) against text. Case-insensitive. */
function wildcardMatchSingle(pattern: string, text: string): boolean {
  const p = pattern.toLowerCase();
  const t = text.toLowerCase();

  if (!p.includes("*")) return p === t;

  const parts = p.split("*");
  if (parts.length === 2) {
    const [prefix, suffix] = parts;
    if (prefix === "" && suffix === "") return true; // "*" matches everything
    if (prefix === "") return t.endsWith(suffix);
    if (suffix === "") return t.startsWith(prefix);
    return t.startsWith(prefix) && t.endsWith(suffix) && t.length >= prefix.length + suffix.length;
  }
  return matchWildcardParts(parts, t);
}

/** Match a glob pattern (supports `|` for OR alternatives) against text. Case-insensitive. */
export function wildcardMatch(pattern: string, text: string): boolean {
  if (!pattern) return true;
  if (pattern.includes("|")) {
    return pattern.split("|").some((p) => wildcardMatchSingle(p.trim(), text));
  }
  return wildcardMatchSingle(pattern, text);
}

export type MatchFields = {
  payee?: string;
  display_payee?: string;
  narration?: string;
  meta?: string;
  commodity?: string;
  display_commodity?: string;
  amount?: number;
  fee?: number;
};

export type AmountCondition =
  | { op: "gt"; value: number }
  | { op: "gte"; value: number }
  | { op: "lt"; value: number }
  | { op: "lte"; value: number }
  | { op: "eq"; value: number }
  | { op: "range"; lo: number; hi: number };

export function parseAmountCondition(s: string): AmountCondition | null {
  const t = s.trim();
  if (!t) return null;
  // Range: "10..500"
  const rangeIdx = t.indexOf("..");
  if (rangeIdx !== -1) {
    const lo = parseFloat(t.slice(0, rangeIdx).trim());
    const hi = parseFloat(t.slice(rangeIdx + 2).trim());
    if (isNaN(lo) || isNaN(hi)) return null;
    return { op: "range", lo, hi };
  }
  if (t.startsWith(">=")) {
    const v = parseFloat(t.slice(2).trim());
    return isNaN(v) ? null : { op: "gte", value: v };
  }
  if (t.startsWith(">")) {
    const v = parseFloat(t.slice(1).trim());
    return isNaN(v) ? null : { op: "gt", value: v };
  }
  if (t.startsWith("<=")) {
    const v = parseFloat(t.slice(2).trim());
    return isNaN(v) ? null : { op: "lte", value: v };
  }
  if (t.startsWith("<")) {
    const v = parseFloat(t.slice(1).trim());
    return isNaN(v) ? null : { op: "lt", value: v };
  }
  if (t.startsWith("=")) {
    const v = parseFloat(t.slice(1).trim());
    return isNaN(v) ? null : { op: "eq", value: v };
  }
  return null;
}

export function amountMatches(condition: AmountCondition, amount: number): boolean {
  switch (condition.op) {
    case "gt": return amount > condition.value;
    case "gte": return amount >= condition.value;
    case "lt": return amount < condition.value;
    case "lte": return amount <= condition.value;
    case "eq": {
      const eps = Math.max(1e-15, Math.abs(condition.value) * 1e-9);
      return Math.abs(amount - condition.value) < eps;
    }
    case "range": return amount >= condition.lo && amount <= condition.hi;
  }
}

export type RuleInfo = {
  id: string;
  pattern: string;
  match_field?: string;
  payee?: string;
  commodity?: string;
  comment?: string;
  amount_condition?: string;
  fee_condition?: string;
  payee_condition?: string;
  narration_condition?: string;
  commodity_condition?: string;
  meta_condition?: string;
  amount_account?: string;
  fee_account?: string;
};

/** A default/catch-all rule that shouldn't be opened in the editor when
 *  clicking a transaction row. Instead, open a new rule draft. */
export function isDefaultRule(rule: RuleInfo): boolean {
  return rule.pattern === "*"
    && !rule.payee
    && (!rule.amount_account || rule.amount_account === "expenses:unknown");
}

/** Does the rule's pattern match any of the relevant fields, honouring
 *  `match_field` if set (with display-value fallbacks for payee/commodity)? */
function patternMatchesFields(rule: RuleInfo, fields: MatchFields): boolean {
  if (rule.match_field) {
    const target =
      rule.match_field === "meta" ? fields.meta :
      rule.match_field === "payee" ? fields.payee :
      rule.match_field === "narration" ? fields.narration :
      rule.match_field === "commodity" ? fields.commodity :
      undefined;
    const fallback =
      rule.match_field === "payee" ? fields.display_payee :
      rule.match_field === "commodity" ? fields.display_commodity :
      undefined;
    return (target != null && wildcardMatch(rule.pattern, target))
      || (fallback != null && wildcardMatch(rule.pattern, fallback));
  }
  const allFields = [fields.payee, fields.display_payee, fields.narration, fields.meta]
    .filter((f): f is string => f != null);
  return allFields.some((f) => wildcardMatch(rule.pattern, f));
}

function payeeConditionMatches(rule: RuleInfo, fields: MatchFields): boolean {
  if (!rule.payee_condition) return true;
  return (fields.payee != null && wildcardMatch(rule.payee_condition, fields.payee))
    || (fields.display_payee != null && wildcardMatch(rule.payee_condition, fields.display_payee));
}

/** Each of narration_condition / commodity_condition / meta_condition (when
 *  set) must match its corresponding field. ANDed with the rule's main
 *  pattern so a single rule can constrain multiple fields. */
function otherConditionsMatch(rule: RuleInfo, fields: MatchFields): boolean {
  if (rule.narration_condition) {
    if (fields.narration == null || !wildcardMatch(rule.narration_condition, fields.narration)) {
      return false;
    }
  }
  if (rule.commodity_condition) {
    const matched = (fields.commodity != null && wildcardMatch(rule.commodity_condition, fields.commodity))
      || (fields.display_commodity != null && wildcardMatch(rule.commodity_condition, fields.display_commodity));
    if (!matched) return false;
  }
  if (rule.meta_condition) {
    if (fields.meta == null || !wildcardMatch(rule.meta_condition, fields.meta)) {
      return false;
    }
  }
  return true;
}

/** Evaluate an amount/fee condition string. Unparseable strings are
 *  treated as no-op (match), preserving prior behaviour. */
function numericConditionMatches(conditionStr: string | undefined, value: number | undefined): boolean {
  if (!conditionStr) return true;
  const cond = parseAmountCondition(conditionStr);
  if (!cond) return true;
  if (value == null) return false;
  return amountMatches(cond, value);
}

/**
 * Test whether a single rule matches the given fields,
 * respecting the match_field constraint.
 */
export function ruleMatchesFields(rule: RuleInfo, fields: MatchFields): boolean {
  if (!patternMatchesFields(rule, fields)) return false;
  if (!payeeConditionMatches(rule, fields)) return false;
  if (!otherConditionsMatch(rule, fields)) return false;
  if (!numericConditionMatches(rule.amount_condition, fields.amount)) return false;
  if (!numericConditionMatches(rule.fee_condition, fields.fee)) return false;
  return true;
}

export type PreviewMatch = {
  transactionIndex: number;
  shadowed: boolean;
  shadowedByRuleId?: string;
  shadowedByPattern?: string;
};

type PreviewTransaction = {
  payee?: string | null;
  display_payee?: string | null;
  narration?: string | null;
  meta?: string | null;
  amount: number;
  amount_commodity: string;
  display_amount_commodity?: string | null;
  fee?: number | null;
  postings: Array<{ account: string; commodity: string; amount?: number }>;
};

type DraftConditionInputs = {
  amountCondition?: string;
  feeCondition?: string;
  payeeCondition?: string;
  narrationCondition?: string;
  commodityCondition?: string;
  metaCondition?: string;
};

function buildDraftRule(
  pattern: string,
  matchField: string,
  id: string,
  c: DraftConditionInputs,
): RuleInfo {
  return {
    id: id || "__draft__",
    pattern: pattern.trim() || "*",
    match_field: matchField || undefined,
    amount_condition: c.amountCondition || undefined,
    fee_condition: c.feeCondition || undefined,
    payee_condition: c.payeeCondition || undefined,
    narration_condition: c.narrationCondition || undefined,
    commodity_condition: c.commodityCondition || undefined,
    meta_condition: c.metaCondition || undefined,
  };
}

/** Rules appearing before the draft have higher priority. New (not-yet-saved)
 *  drafts are appended at the end, so every existing rule outranks them. */
function higherPriorityRulesFor(allRules: RuleInfo[], draftRuleId: string): RuleInfo[] {
  const draftIndex = allRules.findIndex((r) => r.id === draftRuleId);
  return draftIndex >= 0 ? allRules.slice(0, draftIndex) : allRules;
}

/** Use the sum of postings under the selected account (matches the amount
 *  displayed in the UI) instead of the transaction's raw amount. */
function effectiveAmountForAccount(t: PreviewTransaction, selectedAccount: string | undefined): number {
  if (!selectedAccount) return t.amount;
  const matchingPostings = t.postings.filter((p) =>
    p.account === selectedAccount || p.account.startsWith(selectedAccount + ":"));
  if (matchingPostings.length === 0) return t.amount;
  return matchingPostings.reduce((sum, p) => sum + (p.amount ?? 0), 0);
}

function transactionToMatchFields(t: PreviewTransaction, effectiveAmount: number): MatchFields {
  return {
    payee: t.payee || undefined,
    display_payee: t.display_payee || undefined,
    narration: t.narration || undefined,
    meta: t.meta || undefined,
    commodity: t.amount_commodity,
    display_commodity: t.display_amount_commodity || undefined,
    amount: effectiveAmount,
    fee: t.fee ?? undefined,
  };
}

/** Transform rules perform data normalization (display_payee/display_commodity),
 *  not categorization — they should never be counted as shadowing. */
function isTransformRule(r: RuleInfo): boolean {
  return (r.match_field === "commodity" && !!r.commodity)
    || (!!r.payee && !r.amount_account && !r.fee_account && !r.commodity);
}

function scanForShadow(
  rules: RuleInfo[],
  fields: MatchFields,
  wantFieldSpecific: boolean,
): RuleInfo | undefined {
  for (const r of rules) {
    if (wantFieldSpecific ? !r.match_field : !!r.match_field) continue;
    if (isTransformRule(r)) continue;
    if (isDefaultRule(r)) continue;
    if (ruleMatchesFields(r, fields)) return r;
  }
  return undefined;
}

/** Two-pass shadow lookup: field-specific rules outrank general ones.
 *  The general pass only runs when the draft is also a general rule, since a
 *  field-specific draft and a general rule address different concerns. */
function findShadowingRule(
  higherPriorityRules: RuleInfo[],
  fields: MatchFields,
  draftHasMatchField: boolean,
): RuleInfo | undefined {
  const fieldShadow = scanForShadow(higherPriorityRules, fields, true);
  if (fieldShadow) return fieldShadow;
  if (draftHasMatchField) return undefined;
  return scanForShadow(higherPriorityRules, fields, false);
}

/**
 * Compute which transactions match the draft rule pattern,
 * and whether each is shadowed by a higher-priority rule.
 *
 * Mirrors the two-pass priority system from find_match_prioritized:
 *  - Pass 1: field-specific rules (match_field set) have higher priority
 *  - Pass 2: general rules (no match_field)
 */
export function computeRulePreview(
  draftPattern: string,
  draftMatchField: string,
  draftRuleId: string,
  allRules: RuleInfo[],
  transactions: PreviewTransaction[],
  draftAmountCondition?: string,
  draftFeeCondition?: string,
  selectedAccount?: string,
  draftPayeeCondition?: string,
  draftNarrationCondition?: string,
  draftCommodityCondition?: string,
  draftMetaCondition?: string,
): PreviewMatch[] {
  const anyCondition = draftAmountCondition || draftFeeCondition || draftPayeeCondition
    || draftNarrationCondition || draftCommodityCondition || draftMetaCondition;
  if (!draftPattern.trim() && !anyCondition) return [];

  const draftRule = buildDraftRule(draftPattern, draftMatchField, draftRuleId, {
    amountCondition: draftAmountCondition,
    feeCondition: draftFeeCondition,
    payeeCondition: draftPayeeCondition,
    narrationCondition: draftNarrationCondition,
    commodityCondition: draftCommodityCondition,
    metaCondition: draftMetaCondition,
  });
  const higherPriorityRules = higherPriorityRulesFor(allRules, draftRule.id);
  const matches: PreviewMatch[] = [];

  for (let i = 0; i < transactions.length; i++) {
    const t = transactions[i];
    const effectiveAmount = effectiveAmountForAccount(t, selectedAccount);
    const fields = transactionToMatchFields(t, effectiveAmount);
    if (!ruleMatchesFields(draftRule, fields)) continue;
    const shadowRule = findShadowingRule(higherPriorityRules, fields, !!draftRule.match_field);
    matches.push({
      transactionIndex: i,
      shadowed: !!shadowRule,
      shadowedByRuleId: shadowRule?.id,
      shadowedByPattern: shadowRule?.pattern,
    });
  }
  return matches;
}

/**
 * Derive the `pattern`, `matchField`, and `payeeCondition` for a NEW rule
 * draft seeded from a transaction row. Prefers the payee label
 * (`display_payee`) and falls back to the raw payee (address) when no
 * label is set. When narration is present the pattern matches narration
 * and the payee is added as a separate condition; when narration is
 * absent the payee becomes the pattern itself with `matchField=payee`.
 *
 * `existingPayeeCondition` lets a payee filter pill the user already
 * has active take precedence over the row-derived payee.
 */
export function derivePatternAndPayeeCondition(
  narration: string,
  displayPayee: string,
  sourcePayee: string,
  existingPayeeCondition: string,
): { pattern: string; matchField: string; payeeCondition: string } {
  const payeeForRule = displayPayee || sourcePayee;
  if (narration) {
    const payeeCondition = existingPayeeCondition
      || (payeeForRule ? `*${payeeForRule}*` : "");
    return { pattern: `*${narration}*`, matchField: "", payeeCondition };
  }
  if (payeeForRule) {
    return { pattern: `*${payeeForRule}*`, matchField: "payee", payeeCondition: existingPayeeCondition };
  }
  return { pattern: "", matchField: "", payeeCondition: existingPayeeCondition };
}

/**
 * Partition pills into rule-config fields and remaining filter pills.
 * Multi-amount/fee pills collapse to a range when shape allows.
 * narration/commodity/meta pills become text-pattern conditions
 * (`*value*` for narration/meta, bare value for commodity) — first wins,
 * duplicates stay in filterPills.
 */
export type RuleConfigExtraction = {
  matchField: string;
  amountCondition: string;
  feeCondition: string;
  payeeCondition: string;
  narrationCondition: string;
  commodityCondition: string;
  metaCondition: string;
  filterPills: Array<{ key: string; value: string; negated?: boolean }>;
};

type RuleConfigSlots = {
  matchField: string;
  payeeCondition: string;
  narrationCondition: string;
  commodityCondition: string;
  metaCondition: string;
  fieldSet: boolean;
  payeeSet: boolean;
  narrationSet: boolean;
  commoditySet: boolean;
  metaSet: boolean;
  amountValues: string[];
  feeValues: string[];
};

/** Per-key handlers: each returns `true` if the pill was consumed into a
 *  rule-config slot; `false` falls through to the freeform filterPills
 *  bucket (e.g. negated pills, repeated single-slot keys, unknown keys). */
const RULE_CONFIG_SLOT_HANDLERS: Record<
  string,
  (s: RuleConfigSlots, value: string) => boolean
> = {
  field: (s, v) => { if (s.fieldSet) return false; s.matchField = v; s.fieldSet = true; return true; },
  amount: (s, v) => { s.amountValues.push(v); return true; },
  fee: (s, v) => { s.feeValues.push(v); return true; },
  payee: (s, v) => { if (s.payeeSet) return false; s.payeeCondition = v; s.payeeSet = true; return true; },
  narration: (s, v) => { if (s.narrationSet) return false; s.narrationCondition = v; s.narrationSet = true; return true; },
  commodity: (s, v) => { if (s.commoditySet) return false; s.commodityCondition = v; s.commoditySet = true; return true; },
  meta: (s, v) => { if (s.metaSet) return false; s.metaCondition = v; s.metaSet = true; return true; },
};

export function extractRuleConfigPills(
  pills: Array<{ key: string; value: string; negated?: boolean }>,
): RuleConfigExtraction {
  const slots: RuleConfigSlots = {
    matchField: "", payeeCondition: "", narrationCondition: "",
    commodityCondition: "", metaCondition: "",
    fieldSet: false, payeeSet: false, narrationSet: false,
    commoditySet: false, metaSet: false,
    amountValues: [], feeValues: [],
  };
  const filterPills: Array<{ key: string; value: string; negated?: boolean }> = [];

  for (const p of pills) {
    const handler = !p.negated ? RULE_CONFIG_SLOT_HANDLERS[p.key] : undefined;
    if (!handler || !handler(slots, p.value)) filterPills.push(p);
  }

  return {
    matchField: slots.matchField,
    amountCondition: combineNumericConditions(slots.amountValues),
    feeCondition: combineNumericConditions(slots.feeValues),
    payeeCondition: slots.payeeCondition,
    narrationCondition: slots.narrationCondition,
    commodityCondition: slots.commodityCondition,
    metaCondition: slots.metaCondition,
    filterPills,
  };
}

/** Merge multiple `amount:`/`fee:` pills into a single condition string.
 *  A `>`/`>=` lower bound + `<`/`<=` upper bound becomes a `lo..hi` range
 *  (the schema only supports one condition per field). When the values
 *  can't form a range (e.g. two `>` pills), the most recent pill wins so
 *  the user's latest edit isn't silently dropped. */
function combineNumericConditions(values: string[]): string {
  if (values.length === 0) return "";
  if (values.length === 1) return values[0];
  const range = tryFormRange(values);
  if (range) return range;
  return values[values.length - 1];
}

function tryFormRange(values: string[]): string | null {
  if (values.length !== 2) return null;
  const a = parseAmountCondition(values[0]);
  const b = parseAmountCondition(values[1]);
  if (!a || !b) return null;
  const low = isLowerBound(a) ? a : isLowerBound(b) ? b : null;
  const high = isUpperBound(a) ? a : isUpperBound(b) ? b : null;
  if (!low || !high) return null;
  return `${(low as { value: number }).value}..${(high as { value: number }).value}`;
}

function isLowerBound(c: AmountCondition): boolean {
  return c.op === "gt" || c.op === "gte";
}
function isUpperBound(c: AmountCondition): boolean {
  return c.op === "lt" || c.op === "lte";
}

/**
 * Convert rule-config conditions back to display pills.
 * Wildcards are preserved verbatim — the `*` characters are part of the
 * condition's meaning (substring vs exact match) and must stay visible
 * so the user can see what they're editing.
 */
export function draftConditionsToPills(
  draft: {
    matchField: string;
    amountCondition: string;
    feeCondition: string;
    payeeCondition: string;
    narrationCondition?: string;
    commodityCondition?: string;
    metaCondition?: string;
  },
): Array<{ key: string; value: string }> {
  const pills: Array<{ key: string; value: string }> = [];
  if (draft.matchField) pills.push({ key: "field", value: draft.matchField });
  if (draft.amountCondition) pills.push({ key: "amount", value: draft.amountCondition });
  if (draft.feeCondition) pills.push({ key: "fee", value: draft.feeCondition });
  if (draft.payeeCondition) pills.push({ key: "payee", value: draft.payeeCondition });
  if (draft.narrationCondition) pills.push({ key: "narration", value: draft.narrationCondition });
  if (draft.commodityCondition) pills.push({ key: "commodity", value: draft.commodityCondition });
  if (draft.metaCondition) pills.push({ key: "meta", value: draft.metaCondition });
  return pills;
}

/**
 * Convenience wrapper for computeRulePreview that takes a structured
 * draft object instead of 9 positional arguments.
 */
export type DraftPreviewInput = {
  pattern: string;
  matchField: string;
  ruleId: string;
  amountCondition?: string;
  feeCondition?: string;
  payeeCondition?: string;
  narrationCondition?: string;
  commodityCondition?: string;
  metaCondition?: string;
};

export function computeRulePreviewFromDraft(
  draft: DraftPreviewInput,
  allRules: RuleInfo[],
  transactions: Parameters<typeof computeRulePreview>[4],
  selectedAccount?: string,
): PreviewMatch[] {
  return computeRulePreview(
    draft.pattern, draft.matchField, draft.ruleId,
    allRules, transactions,
    draft.amountCondition, draft.feeCondition,
    selectedAccount, draft.payeeCondition,
    draft.narrationCondition, draft.commodityCondition, draft.metaCondition,
  );
}

/** Merge a rule editor draft with the just-parsed text-input pills into the
 *  shape `computeRulePreviewFromDraft` expects: structured rule conditions
 *  plus residual filter pills. Pills already promoted into the draft
 *  (payee/commodity/etc) take precedence over what's currently in the text
 *  input, so a stale typed character can't override a committed pill. */
export type MergeRuleConditionsDraft = {
  matchField: string;
  amountCondition: string;
  feeCondition: string;
  payeeCondition: string;
  narrationCondition: string;
  commodityCondition: string;
  metaCondition: string;
  filterPills: Array<{ key: string; value: string; negated?: boolean }>;
};

export function mergeRuleConditions(
  draft: MergeRuleConditionsDraft,
  textPills: Array<{ key: string; value: string; negated?: boolean }>,
) {
  const extracted = extractRuleConfigPills(textPills);
  return {
    conditions: {
      matchField: extracted.matchField || draft.matchField,
      amountCondition: draft.amountCondition || extracted.amountCondition,
      feeCondition: draft.feeCondition || extracted.feeCondition,
      payeeCondition: draft.payeeCondition || extracted.payeeCondition,
      narrationCondition: draft.narrationCondition || extracted.narrationCondition,
      commodityCondition: draft.commodityCondition || extracted.commodityCondition,
      metaCondition: draft.metaCondition || extracted.metaCondition,
    },
    filterPills: [...draft.filterPills, ...extracted.filterPills],
  };
}

/**
 * Build a rule object from the rule editor draft state.
 * Extracted from the save handler so the logic is testable.
 */
export type RuleDraft = {
  pattern: string;
  matchField: string;
  amountCondition: string;
  feeCondition: string;
  payeeCondition: string;
  narrationCondition: string;
  commodityCondition: string;
  metaCondition: string;
  filterPills: Array<{ key: string; value: string; negated?: boolean }>;
  comment: string;
  amountAccount: string;
  feeAccount: string;
};

export type SavedRule = {
  pattern: string;
  match_field: string | null;
  comment: string | null;
  amount_condition: string | null;
  fee_condition: string | null;
  payee_condition: string | null;
  narration_condition: string | null;
  commodity_condition: string | null;
  meta_condition: string | null;
  amount_account: string | null;
  fee_account: string | null;
};

type DerivedPattern = {
  pattern: string;
  matchField: string;
  payeeCondition: string;
  narrationCondition: string;
  commodityCondition: string;
  metaCondition: string;
};

/** A rule must have a non-empty pattern to be saved. When the user has
 *  only filter conditions (no main pattern), promote one of them — in
 *  preference order commodity → narration → meta → payee — to be the
 *  pattern, and clear the corresponding *Condition so it isn't applied
 *  twice. The promoted pattern uses match_field so the engine knows
 *  which field to match against. */
function derivePatternFromPills(
  currentPattern: string,
  matchField: string,
  conds: { payeeCondition: string; narrationCondition: string; commodityCondition: string; metaCondition: string },
): DerivedPattern {
  if (currentPattern.trim()) {
    return { pattern: currentPattern, matchField, ...conds };
  }
  if (conds.commodityCondition) {
    return { pattern: conds.commodityCondition, matchField: "commodity", ...conds, commodityCondition: "" };
  }
  if (conds.narrationCondition) {
    return { pattern: conds.narrationCondition, matchField: "narration", ...conds, narrationCondition: "" };
  }
  if (conds.metaCondition) {
    return { pattern: conds.metaCondition, matchField: "meta", ...conds, metaCondition: "" };
  }
  if (conds.payeeCondition) {
    return { pattern: conds.payeeCondition, matchField: "payee", ...conds, payeeCondition: "" };
  }
  return { pattern: currentPattern, matchField, ...conds };
}

const emptyToNull = (s: string | null | undefined): string | null => s ? s : null;

export function buildRuleFromDraft(
  draft: RuleDraft,
  parseSmartInput: (raw: string, keywords: string[]) => { pills: Array<{ key: string; value: string; negated?: boolean }>; text: string },
  toFlatString: (pills: Array<{ key: string; value: string; negated?: boolean }>, text: string) => string,
  keywords: string[],
): SavedRule {
  const fullInput = toFlatString(draft.filterPills, draft.pattern);
  const { pills: savePills, text: savePattern } = parseSmartInput(fullInput, keywords);
  const extracted = extractRuleConfigPills(savePills);
  const initialMatchField = extracted.matchField || draft.matchField;
  const amountCondition = extracted.amountCondition || draft.amountCondition;
  const feeCondition = extracted.feeCondition || draft.feeCondition;
  const conds = {
    payeeCondition: extracted.payeeCondition || (draft.payeeCondition ?? ""),
    narrationCondition: extracted.narrationCondition || (draft.narrationCondition ?? ""),
    commodityCondition: extracted.commodityCondition || (draft.commodityCondition ?? ""),
    metaCondition: extracted.metaCondition || (draft.metaCondition ?? ""),
  };
  const derived = derivePatternFromPills(savePattern, initialMatchField, conds);
  return {
    pattern: derived.pattern,
    match_field: emptyToNull(derived.matchField),
    comment: emptyToNull(draft.comment),
    amount_condition: emptyToNull(amountCondition),
    fee_condition: emptyToNull(feeCondition),
    payee_condition: emptyToNull(derived.payeeCondition),
    narration_condition: emptyToNull(derived.narrationCondition),
    commodity_condition: emptyToNull(derived.commodityCondition),
    meta_condition: emptyToNull(derived.metaCondition),
    amount_account: emptyToNull(draft.amountAccount),
    fee_account: emptyToNull(draft.feeAccount),
  };
}

/**
 * Compute (pattern, matchField) for a rule saved by an inline category-cell
 * edit. Prefers the per-leg `leg:<id>` anchor so the rule binds to exactly one
 * leg — crucial when several legs of one on-chain transaction share a `txn:`
 * id (swap / wrap / multi-hop), where a `txn:`-anchored rule would bleed across
 * every leg. Falls back to the `txn:` id (unique for single-leg rows), then to
 * a narration wildcard when no id is available.
 */
export function categoryEditRuleMatch(
  legId: string,
  txnId: string,
  narration: string,
): { pattern: string; matchField: string | null } {
  if (legId) return { pattern: legId, matchField: "meta" };
  if (txnId) return { pattern: txnId, matchField: "meta" };
  return { pattern: `*${narration}*`, matchField: null };
}
