/**
 * Reusable filter row component for pattern/match-field/amount-condition.
 * Used by the rule editor, and designed to be reused by the transform editor.
 */

function escapeAttr(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

export type FilterRowState = {
  pattern: string;
  matchField: string;
  amountCondition: string;
};

/**
 * Render a filter row (pattern + match field + amount condition).
 * Uses prefixed IDs so multiple instances can coexist: `${prefix}Pattern`, etc.
 */
export function renderFilterRow(
  state: FilterRowState,
  prefix: string,
  disabled: boolean,
): string {
  const dis = disabled ? "disabled" : "";
  return `
    <label class="ruleEditorBar__field ruleEditorBar__field--pattern">
      <span class="ruleEditorBar__label">Pattern</span>
      <input id="${prefix}Pattern" class="ruleEditorBar__input ruleEditorBar__input--mono" value="${escapeAttr(state.pattern)}" ${dis} placeholder="*search text*" />
    </label>
    <label class="ruleEditorBar__field ruleEditorBar__field--match">
      <span class="ruleEditorBar__label">Match</span>
      <select id="${prefix}MatchField" class="ruleEditorBar__select" ${dis}>
        <option value="" ${state.matchField === "" ? "selected" : ""}>Any</option>
        <option value="narration" ${state.matchField === "narration" ? "selected" : ""}>Narration</option>
        <option value="payee" ${state.matchField === "payee" ? "selected" : ""}>Payee</option>
        <option value="meta" ${state.matchField === "meta" ? "selected" : ""}>Meta</option>
      </select>
    </label>
    <label class="ruleEditorBar__field ruleEditorBar__field--amount">
      <span class="ruleEditorBar__label">Amount</span>
      <input id="${prefix}AmountCondition" class="ruleEditorBar__input ruleEditorBar__input--mono" value="${escapeAttr(state.amountCondition)}" ${dis} placeholder="e.g. >100, <0, 50..200" />
    </label>`;
}

/**
 * Read current filter values from the DOM by prefix.
 * Falls back to the provided defaults if elements are missing.
 */
export function readFilterValues(prefix: string, defaults: FilterRowState): FilterRowState {
  return {
    pattern: document.querySelector<HTMLInputElement>(`#${prefix}Pattern`)?.value ?? defaults.pattern,
    matchField: document.querySelector<HTMLSelectElement>(`#${prefix}MatchField`)?.value ?? defaults.matchField,
    amountCondition: document.querySelector<HTMLInputElement>(`#${prefix}AmountCondition`)?.value ?? defaults.amountCondition,
  };
}
