// Shared autocomplete behavior for account-name inputs.
//
// Used by the inline category-cell editor on the accounts page, the rule
// editor's Amount/Fee Account fields, and the Add Account modal. The single
// component fixes the long-standing problems each call site had on its own:
//
//   - Dropdown rendered behind subsequent rows (parent stacking context).
//     Fixed by rendering the dropdown via position: fixed, attached to the
//     document body and positioned against the input's bounding rect.
//
//   - Partially-typed text auto-committing on blur, silently creating bogus
//     "expense" or "exp" accounts. Fixed by reverting on blur unless the
//     typed value exactly matches an existing suggestion (case-insensitive).
//
//   - "+ Add" leaking into the value. Fixed by carrying the typed value on
//     the item's dataset and using it directly on commit, rather than
//     re-extracting from textContent.
//
// The component mutates `input.value` to the chosen value and then invokes
// `onCommit(value, kind)`. Callers decide what "commit" means for them
// (save a rule, update a draft, do nothing and rely on the modal's button).

export type CommitKind = "existing" | "new" | "revert" | "cancel";

export interface AccountInputOptions {
  /** Pool of existing account names to suggest. Full canonical paths. */
  suggestions: string[];
  /** When true, show a "+ Add ..." item at the bottom for unknown values. */
  allowCreate?: boolean;
  /**
   * Optional visible prefix (e.g. "assets:") rendered outside the input.
   * Suggestions are filtered to those starting with the prefix and
   * displayed without it; the prefix is re-attached on commit.
   */
  prefix?: string;
  /** Select the input's contents on attach. */
  selectOnMount?: boolean;
  /**
   * Default true. When false, blur preserves the typed value instead of
   * reverting on no-match. Used by the Add Account modal where the user
   * intends to create a new name and a separate "Add" button commits it.
   * Exact case-insensitive matches still canonicalize on blur.
   */
  revertOnBlur?: boolean;
}

export interface AccountInputController {
  destroy(): void;
  refreshSuggestions(next: string[]): void;
  /**
   * Reapply options and onCommit to an already-attached controller. Used
   * when the caller's framework re-runs every render and the input
   * element is preserved (morphdom). Element listeners are re-bound so
   * they survive a bind-once-driven listener strip; dropdown state and
   * the committed flag are preserved so an open dropdown isn't closed
   * by an unrelated re-render.
   */
  refresh(opts: AccountInputOptions, onCommit: (value: string, kind: CommitKind) => void): void;
}

const DROPDOWN_CLASS = "accountInput-dropdown";
const ITEM_CLASS = "accountInput-dropdown__item";
const ITEM_ACTIVE_CLASS = "accountInput-dropdown__item--active";
const ITEM_ADD_CLASS = "accountInput-dropdown__item--add";

const controllers = new WeakMap<HTMLInputElement, AccountInputController>();

export function attachAccountInput(
  input: HTMLInputElement,
  initialOpts: AccountInputOptions,
  initialOnCommit: (value: string, kind: CommitKind) => void,
): AccountInputController {
  const existing = controllers.get(input);
  if (existing) {
    existing.refresh(initialOpts, initialOnCommit);
    return existing;
  }

  const initialValue = input.value;
  let opts: AccountInputOptions = initialOpts;
  let onCommit = initialOnCommit;
  let suggestions = stripPrefix(opts.suggestions, opts.prefix);
  let dropdown: HTMLDivElement | null = null;
  let highlightIdx = -1;
  let committed = false;
  let blurTimer: ReturnType<typeof setTimeout> | null = null;

  function withPrefix(typed: string): string {
    return opts.prefix ? `${opts.prefix}${typed}` : typed;
  }

  function close(): void {
    if (dropdown) {
      dropdown.remove();
      dropdown = null;
      window.removeEventListener("scroll", onScrollOrResize, true);
      window.removeEventListener("resize", onScrollOrResize);
    }
    highlightIdx = -1;
  }

  function position(): void {
    if (!dropdown) return;
    const rect = input.getBoundingClientRect();
    if (rect.bottom < 0 || rect.top > window.innerHeight) {
      close();
      return;
    }
    dropdown.style.top = `${rect.bottom + 2}px`;
    dropdown.style.left = `${rect.left}px`;
    dropdown.style.minWidth = `${rect.width}px`;
  }

  function open(): void {
    const query = input.value.trim().toLowerCase();
    const matches = query
      ? suggestions.filter((s) => s.toLowerCase().includes(query))
      : suggestions.slice();
    const exactMatch = !!query && suggestions.some((s) => s.toLowerCase() === query);
    const showAdd = !!opts.allowCreate && !!query && !exactMatch;

    close();
    if (matches.length === 0 && !showAdd) return;

    dropdown = document.createElement("div");
    dropdown.className = DROPDOWN_CLASS;
    dropdown.setAttribute("role", "listbox");

    matches.forEach((s) => {
      const item = makeItem(s, "existing", s);
      item.addEventListener("mousedown", (e) => {
        e.preventDefault();
        commit(s, "existing");
      });
      dropdown!.appendChild(item);
    });

    if (showAdd) {
      const typed = input.value.trim();
      const item = makeItem(`+ Add "${typed}"`, "new", typed);
      item.classList.add(ITEM_ADD_CLASS);
      item.addEventListener("mousedown", (e) => {
        e.preventDefault();
        commit(typed, "new");
      });
      dropdown!.appendChild(item);
    }

    document.body.appendChild(dropdown);
    window.addEventListener("scroll", onScrollOrResize, true);
    window.addEventListener("resize", onScrollOrResize);
    position();
    highlight(0);
  }

  function makeItem(label: string, kind: "existing" | "new", value: string): HTMLDivElement {
    const item = document.createElement("div");
    item.className = ITEM_CLASS;
    item.dataset.kind = kind;
    item.dataset.value = value;
    item.textContent = label;
    item.setAttribute("role", "option");
    return item;
  }

  function highlight(idx: number): void {
    if (!dropdown) return;
    const items = dropdown.querySelectorAll<HTMLDivElement>(`.${ITEM_CLASS}`);
    if (items.length === 0) {
      highlightIdx = -1;
      return;
    }
    highlightIdx = Math.max(0, Math.min(idx, items.length - 1));
    items.forEach((el, i) => {
      el.classList.toggle(ITEM_ACTIVE_CLASS, i === highlightIdx);
    });
    items[highlightIdx]?.scrollIntoView?.({ block: "nearest" });
  }

  function commit(value: string, kind: CommitKind): void {
    if (committed) return;
    committed = true;
    if (blurTimer !== null) {
      clearTimeout(blurTimer);
      blurTimer = null;
    }
    const finalDisplay = kind === "revert" || kind === "cancel" ? initialValue : value;
    input.value = finalDisplay;
    close();
    onCommit(withPrefix(finalDisplay), kind);
  }

  /**
   * Resolve an ambiguous state (blur, Enter without selection):
   *   - exact case-insensitive match → commit the canonical form
   *   - anything else → revert to the initial value (default), or preserve
   *     the typed text when revertOnBlur is false (modal mode)
   *
   * Notably this does NOT auto-create when allowCreate is on. Creating a
   * new account requires the user to explicitly pick the "+ Add" item.
   */
  function settle(): void {
    if (committed) return;
    const typed = input.value.trim();
    const match = suggestions.find((s) => s.toLowerCase() === typed.toLowerCase());
    if (match) {
      commit(match, "existing");
      return;
    }
    if (opts.revertOnBlur === false) {
      close();
      return;
    }
    commit(initialValue, "revert");
  }

  function onInputChange(): void {
    committed = false;
    open();
  }

  function onKeyDown(e: KeyboardEvent): void {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      if (!dropdown) {
        open();
      } else {
        highlight(highlightIdx + 1);
      }
      return;
    }
    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (dropdown) highlight(highlightIdx - 1);
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      commit(initialValue, "cancel");
      return;
    }
    if (e.key === "Enter") {
      e.preventDefault();
      const items = dropdown?.querySelectorAll<HTMLDivElement>(`.${ITEM_CLASS}`);
      if (items && highlightIdx >= 0 && items[highlightIdx]) {
        const el = items[highlightIdx];
        const kind = el.dataset.kind === "new" ? "new" : "existing";
        commit(el.dataset.value ?? "", kind);
      } else {
        settle();
      }
    }
  }

  function onBlur(): void {
    blurTimer = setTimeout(() => {
      blurTimer = null;
      settle();
    }, 120);
  }

  function onFocus(): void {
    if (blurTimer !== null) {
      clearTimeout(blurTimer);
      blurTimer = null;
    }
    open();
  }

  function onScrollOrResize(): void {
    position();
  }

  function attachInputListeners(): void {
    input.addEventListener("input", onInputChange);
    input.addEventListener("keydown", onKeyDown);
    input.addEventListener("blur", onBlur);
    input.addEventListener("focus", onFocus);
  }

  function detachInputListeners(): void {
    input.removeEventListener("input", onInputChange);
    input.removeEventListener("keydown", onKeyDown);
    input.removeEventListener("blur", onBlur);
    input.removeEventListener("focus", onFocus);
  }

  attachInputListeners();
  if (opts.selectOnMount) input.select();

  const controller: AccountInputController = {
    destroy(): void {
      detachInputListeners();
      if (blurTimer !== null) clearTimeout(blurTimer);
      close();
      controllers.delete(input);
    },
    refreshSuggestions(next: string[]): void {
      suggestions = stripPrefix(next, opts.prefix);
      if (dropdown) open();
    },
    refresh(newOpts, newOnCommit): void {
      opts = newOpts;
      onCommit = newOnCommit;
      suggestions = stripPrefix(newOpts.suggestions, newOpts.prefix);
      // Re-bind: the host framework's render cycle (bind-once) may have
      // stripped our prior element listeners.
      detachInputListeners();
      attachInputListeners();
    },
  };
  controllers.set(input, controller);
  return controller;
}

function stripPrefix(list: string[], prefix?: string): string[] {
  if (!prefix) return list.slice();
  return list.filter((s) => s.startsWith(prefix)).map((s) => s.slice(prefix.length));
}
