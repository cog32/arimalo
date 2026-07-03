/**
 * Smart search bar with pill-based keyword highlighting.
 * Used by both the rule editor and the accounts page search.
 *
 * Keywords like `field:narration`, `amount:>100`, `fee:>0` are rendered as
 * removable pills. Remaining text is the pattern / free-text search.
 */

export type SearchPill = { key: string; value: string; negated?: boolean };

export type SmartSearchConfig = {
  /** Valid keyword prefixes */
  keywords: string[];
  /** Value suggestions per keyword (e.g., { field: ["narration","payee","meta"] }) */
  valueSuggestions?: Record<string, string[]>;
  placeholder?: string;
};

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/**
 * A pill value is "complete" if it's not just bare operators.
 * Rejects: ">", ">=", "<", "<=", "=", ".."
 * Accepts: ">0", ">=100", "narration", "10..500", etc.
 */
export function isPillValueComplete(value: string, key?: string): boolean {
  if (!value) return false;
  // Quoted values: complete only when closing quote is present
  if (value.startsWith('"')) return value.length > 1 && value.endsWith('"');
  // Reject values that are only comparison operators with no number
  if (/^[><=.]+$/.test(value)) return false;
  // For amount/fee pills, require an operator prefix (>, <, >=, <=, =, or N..N range)
  if (key === "amount" || key === "fee") {
    if (/^[><=]/.test(value) || value.includes("..")) return true;
    return false; // bare number — not a valid condition
  }
  return true;
}

/** Strip surrounding quotes from a pill value. */
function unquote(value: string): string {
  if (value.startsWith('"') && value.endsWith('"') && value.length > 1) {
    return value.slice(1, -1);
  }
  return value;
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

function makePill(key: string, rawValue: string, negated: boolean): SearchPill {
  const value = unquote(rawValue);
  return negated ? { key, value, negated } : { key, value };
}

/** Extend a value that starts with `"` but hasn't closed yet, by appending
 *  follow-on tokens until one ends with `"`. Only advances the index when a
 *  closing quote is actually found. */
function extendQuotedValue(
  tokens: string[], startI: number, initialValue: string,
): { value: string; endI: number } {
  let value = initialValue;
  for (let j = startI + 1; j < tokens.length; j++) {
    value += " " + tokens[j];
    if (tokens[j].endsWith('"')) return { value, endI: j };
  }
  return { value, endI: startI };
}

/** Extend a complete, unquoted value by appending follow-on plain-text tokens,
 *  stopping at wildcards or another keyword token. */
function extendUnquotedValue(
  tokens: string[], startI: number, initialValue: string, keySet: Set<string>,
): { value: string; endI: number } {
  let value = initialValue;
  let endI = startI;
  for (let j = startI + 1; j < tokens.length; j++) {
    const next = tokens[j];
    if (next.includes("*") || next.includes("?")) break;
    const nextClean = next.startsWith("-") && next.length > 1 ? next.slice(1) : next;
    const nextColon = nextClean.indexOf(":");
    if (nextColon > 0 && keySet.has(nextClean.slice(0, nextColon).toLowerCase())) break;
    value += " " + next;
    endI = j;
  }
  return { value, endI };
}

/** Handle "keyword: value" where the value is in one or more following tokens.
 *  Supports both quoted spans (`payee: "Solend Main"`) and bare values
 *  (`fee: >0`). */
function parseStandaloneValue(
  tokens: string[], i: number, key: string, negated: boolean,
): { pill: SearchPill; endI: number } | null {
  if (i + 1 >= tokens.length) return null;
  let nextVal = tokens[i + 1];
  if (nextVal.startsWith('"') && !nextVal.endsWith('"')) {
    for (let j = i + 2; j < tokens.length; j++) {
      nextVal += " " + tokens[j];
      if (tokens[j].endsWith('"')) {
        if (isPillValueComplete(nextVal, key)) {
          return { pill: makePill(key, nextVal, negated), endI: j };
        }
        break;
      }
    }
  }
  if (!nextVal.includes(":") && isPillValueComplete(nextVal, key)) {
    return { pill: makePill(key, nextVal, negated), endI: i + 1 };
  }
  return null;
}

/** Given a recognised keyword at tokens[i] with an initial (possibly empty)
 *  value, try to form a pill, extending the value across follow-on tokens as
 *  needed for quoted spans or unquoted multi-word values. */
function parseKeywordToken(
  tokens: string[], i: number, key: string, negated: boolean,
  initialValue: string, keySet: Set<string>,
): { pill: SearchPill; endI: number } | null {
  let value = initialValue;
  let endI = i;

  if (value.startsWith('"') && !value.endsWith('"')) {
    const ext = extendQuotedValue(tokens, i, value);
    value = ext.value;
    endI = ext.endI;
  }
  if (value && !value.startsWith('"') && isPillValueComplete(value, key)) {
    const ext = extendUnquotedValue(tokens, endI, value, keySet);
    value = ext.value;
    endI = ext.endI;
  }
  if (value && isPillValueComplete(value, key)) {
    return { pill: makePill(key, value, negated), endI };
  }
  if (!value) return parseStandaloneValue(tokens, i, key, negated);
  return null;
}

/** Attempt to parse tokens[i] (possibly consuming follow-on tokens) as a pill. */
function tryParsePillAt(
  tokens: string[], i: number, keySet: Set<string>,
): { pill: SearchPill; endI: number } | null {
  const token = tokens[i];
  const negated = token.startsWith("-") && token.length > 1;
  const cleanToken = negated ? token.slice(1) : token;
  const colonIdx = cleanToken.indexOf(":");
  if (colonIdx <= 0) return null;
  const key = cleanToken.slice(0, colonIdx).toLowerCase();
  if (!keySet.has(key)) return null;
  const initialValue = cleanToken.slice(colonIdx + 1);
  return parseKeywordToken(tokens, i, key, negated, initialValue, keySet);
}

/** Parse raw input text into pills + remaining text. */
export function parseSmartInput(raw: string, keywords: string[]): { pills: SearchPill[]; text: string } {
  const keySet = new Set(keywords.map((k) => k.toLowerCase()));
  const pills: SearchPill[] = [];
  const textParts: string[] = [];
  const tokens = raw.split(/\s+/).filter(Boolean);
  for (let i = 0; i < tokens.length; i++) {
    const parsed = tryParsePillAt(tokens, i, keySet);
    if (parsed) {
      pills.push(parsed.pill);
      i = parsed.endI;
      continue;
    }
    textParts.push(tokens[i]);
  }
  return { pills, text: textParts.join(" ") };
}

/** Reconstruct a flat string from pills + text. */
export function toFlatString(pills: SearchPill[], text: string): string {
  const parts = pills.map((p) => {
    const v = p.value.includes(" ") ? `"${p.value}"` : p.value;
    return `${p.negated ? "-" : ""}${p.key}:${v}`;
  });
  if (text.trim()) parts.push(text.trim());
  return parts.join(" ");
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

function esc(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/** Render the smart search bar HTML. */
export function renderSmartSearch(
  id: string,
  pills: SearchPill[],
  text: string,
  config: SmartSearchConfig,
  disabled = false,
): string {
  const dis = disabled ? "disabled" : "";
  const pillsHtml = pills
    .map(
      (p, i) => `<span class="smartSearch__pill${p.negated ? " smartSearch__pill--negated" : ""}" data-pill-index="${i}">` +
        `<span class="smartSearch__pillLabel">${p.negated ? "-" : ""}${esc(p.key)}:${esc(p.value)}</span>` +
        `<button class="smartSearch__pillX" data-pill-remove="${i}" type="button" ${dis}>\u00d7</button>` +
        `</span>`,
    )
    .join("");
  const placeholder = pills.length === 0 ? (config.placeholder ?? "") : "";
  return (
    `<div class="smartSearch" id="${id}">` +
    `<input class="smartSearch__input" id="${id}Input" value="${esc(text)}" placeholder="${esc(placeholder)}" ${dis} autocomplete="off" />` +
    pillsHtml +
    `<div class="smartSearch__dropdown" id="${id}Dropdown"></div>` +
    `</div>`
  );
}

// ---------------------------------------------------------------------------
// Interaction
// ---------------------------------------------------------------------------

export type SmartSearchChange = { pills: SearchPill[]; text: string };

/**
 * Attach event handlers to a rendered smart search bar.
 * Returns a cleanup function. `onChange` fires whenever pills or text change.
 * Pills are managed directly in the DOM — no parent re-render needed.
 */
export function attachSmartSearch(
  id: string,
  pills: SearchPill[],
  config: SmartSearchConfig,
  onChange: (change: SmartSearchChange) => void,
): () => void {
  const container = document.getElementById(id);
  const input = document.getElementById(`${id}Input`) as HTMLInputElement | null;
  const dropdown = document.getElementById(`${id}Dropdown`) as HTMLDivElement | null;
  if (!container || !input) return () => {};

  const keySet = new Set(config.keywords.map((k) => k.toLowerCase()));
  let currentPills = [...pills];
  let highlightIdx = -1;

  /** Rebuild pill DOM elements in the container (after the input). */
  function rebuildPillsDom() {
    // Remove existing pill elements
    container!.querySelectorAll(".smartSearch__pill").forEach((p) => p.remove());
    // Insert pills after the input (before the dropdown)
    const ref = dropdown || null;
    for (let i = 0; i < currentPills.length; i++) {
      const p = currentPills[i];
      const span = document.createElement("span");
      span.className = p.negated ? "smartSearch__pill smartSearch__pill--negated" : "smartSearch__pill";
      span.setAttribute("data-pill-index", String(i));
      span.innerHTML =
        `<span class="smartSearch__pillLabel">${p.negated ? "-" : ""}${esc(p.key)}:${esc(p.value)}</span>` +
        `<button class="smartSearch__pillX" data-pill-remove="${i}" type="button">\u00d7</button>`;
      container!.insertBefore(span, ref);
    }
    // Update placeholder
    input!.placeholder = currentPills.length === 0 ? (config.placeholder ?? "") : "";
  }

  // Focus the input when clicking the container
  function onContainerClick(e: MouseEvent) {
    if ((e.target as HTMLElement).closest(".smartSearch__pillX")) return;
    input!.focus();
  }

  // Remove pill by × button
  function onPillRemove(e: MouseEvent) {
    const btn = (e.target as HTMLElement).closest("[data-pill-remove]") as HTMLElement | null;
    if (!btn) return;
    const idx = parseInt(btn.getAttribute("data-pill-remove") ?? "", 10);
    if (isNaN(idx)) return;
    currentPills.splice(idx, 1);
    rebuildPillsDom();
    onChange({ pills: [...currentPills], text: input!.value });
  }

  // Handle input: check for keyword:value tokens on Space
  function onInput() {
    const val = input!.value;
    // Check if last character is space and last word is a keyword:value
    if (val.endsWith(" ")) {
      // Re-parse the full input to correctly handle quoted multi-word values
      const parsed = parseSmartInput(val.trimEnd(), [...keySet]);
      if (parsed.pills.length > 0) {
        for (const pill of parsed.pills) currentPills.push(pill);
        input!.value = parsed.text + (parsed.text ? " " : "");
        rebuildPillsDom();
        hideDropdown();
        onChange({ pills: [...currentPills], text: input!.value });
        return;
      }
    }
    updateDropdown();
    onChange({ pills: [...currentPills], text: val });
  }

  // Backspace at position 0 removes last pill
  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Backspace" && input!.selectionStart === 0 && input!.selectionEnd === 0 && currentPills.length > 0) {
      e.preventDefault();
      currentPills.pop();
      rebuildPillsDom();
      onChange({ pills: [...currentPills], text: input!.value });
      return;
    }
    // Navigate dropdown
    if (dropdown && dropdown.childElementCount > 0) {
      const items = dropdown.querySelectorAll<HTMLElement>(".smartSearch__dropdownItem");
      if (e.key === "ArrowDown") {
        e.preventDefault();
        highlightIdx = Math.min(highlightIdx + 1, items.length - 1);
        highlightItem(items);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        highlightIdx = Math.max(highlightIdx - 1, 0);
        highlightItem(items);
      } else if ((e.key === "Enter" || e.key === "Tab") && highlightIdx >= 0 && items[highlightIdx]) {
        e.preventDefault();
        applyDropdownItem(items[highlightIdx].getAttribute("data-insert") ?? "");
      }
    }
  }

  function highlightItem(items: NodeListOf<HTMLElement>) {
    items.forEach((it, i) => it.classList.toggle("smartSearch__dropdownItem--active", i === highlightIdx));
    if (items[highlightIdx]) items[highlightIdx].scrollIntoView({ block: "nearest" });
  }

  function applyDropdownItem(insert: string) {
    const val = input!.value;
    const words = val.split(/\s+/);
    words.pop(); // remove partial word
    words.push(insert);
    input!.value = words.join(" ");
    hideDropdown();
    input!.focus();
    // Don't trigger onChange yet — user still needs to type the value
  }

  // Dropdown suggestions
  function updateDropdown() {
    if (!dropdown) return;
    const val = input!.value;
    const words = val.split(/\s+/);
    const rawPartial = (words[words.length - 1] ?? "").toLowerCase();
    const negPrefix = rawPartial.startsWith("-") && rawPartial.length > 1;
    const partial = negPrefix ? rawPartial.slice(1) : rawPartial;
    if (!partial || partial.includes(":")) {
      hideDropdown();
      return;
    }
    // Build suggestions: keywords that start with partial
    const prefix = negPrefix ? "-" : "";
    const suggestions: { label: string; insert: string }[] = [];
    for (const kw of config.keywords) {
      if (kw.toLowerCase().startsWith(partial)) {
        const vals = config.valueSuggestions?.[kw];
        if (vals) {
          for (const v of vals) {
            suggestions.push({ label: `${prefix}${kw}:${v}`, insert: `${prefix}${kw}:${v} ` });
          }
        } else {
          suggestions.push({ label: `${prefix}${kw}:`, insert: `${prefix}${kw}:` });
        }
      }
    }
    if (suggestions.length === 0) {
      hideDropdown();
      return;
    }
    highlightIdx = -1;
    dropdown.innerHTML = suggestions
      .slice(0, 8)
      .map((s) => `<div class="smartSearch__dropdownItem" data-insert="${esc(s.insert)}">${esc(s.label)}</div>`)
      .join("");
    dropdown.style.display = "block";

    // Click handler for dropdown items
    dropdown.querySelectorAll<HTMLElement>(".smartSearch__dropdownItem").forEach((item) => {
      item.addEventListener("mousedown", (e) => {
        e.preventDefault();
        applyDropdownItem(item.getAttribute("data-insert") ?? "");
      });
    });
  }

  function hideDropdown() {
    if (dropdown) {
      dropdown.innerHTML = "";
      dropdown.style.display = "none";
    }
    highlightIdx = -1;
  }

  function onBlur() {
    setTimeout(hideDropdown, 150);
  }

  function onFocus() {
    updateDropdown();
  }

  container.addEventListener("click", onContainerClick);
  container.addEventListener("click", onPillRemove);
  input.addEventListener("input", onInput);
  input.addEventListener("keydown", onKeydown);
  input.addEventListener("blur", onBlur);
  input.addEventListener("focus", onFocus);

  return () => {
    container.removeEventListener("click", onContainerClick);
    container.removeEventListener("click", onPillRemove);
    input.removeEventListener("input", onInput);
    input.removeEventListener("keydown", onKeydown);
    input.removeEventListener("blur", onBlur);
    input.removeEventListener("focus", onFocus);
  };
}
