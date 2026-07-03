// @vitest-environment jsdom
import { describe, it, expect, beforeEach, vi } from "vitest";
import { attachAccountInput, type CommitKind } from "./account-input";

const SUGGESTIONS = [
  "expenses:groceries",
  "expenses:rent",
  "expenses:utilities",
  "income:salary",
  "assets:bank:savings",
  "assets:bank:checking",
];

interface Ctx {
  input: HTMLInputElement;
  commits: Array<{ value: string; kind: CommitKind }>;
  controller: ReturnType<typeof attachAccountInput>;
}

function setup(initialValue: string, opts: Parameters<typeof attachAccountInput>[1]): Ctx {
  document.body.innerHTML = "";
  const input = document.createElement("input");
  input.value = initialValue;
  document.body.appendChild(input);
  const commits: Ctx["commits"] = [];
  const controller = attachAccountInput(input, opts, (value, kind) => {
    commits.push({ value, kind });
  });
  return { input, commits, controller };
}

function dropdown(): HTMLDivElement | null {
  return document.body.querySelector<HTMLDivElement>(".accountInput-dropdown");
}

function items(): HTMLDivElement[] {
  return Array.from(document.body.querySelectorAll<HTMLDivElement>(".accountInput-dropdown__item"));
}

function typeInto(input: HTMLInputElement, value: string): void {
  input.value = value;
  input.dispatchEvent(new Event("input"));
}

function press(input: HTMLInputElement, key: string): void {
  const e = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true });
  input.dispatchEvent(e);
}

function mousedown(el: Element): void {
  el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
}

beforeEach(() => {
  vi.useFakeTimers();
  document.body.innerHTML = "";
});

describe("attachAccountInput — filtering and dropdown", () => {
  it("opens with case-insensitive substring matches", () => {
    const { input } = setup("", { suggestions: SUGGESTIONS });
    input.focus();
    typeInto(input, "EXP");
    const labels = items().map((el) => el.textContent);
    expect(labels).toEqual(["expenses:groceries", "expenses:rent", "expenses:utilities"]);
  });

  it("closes when no matches and allowCreate is false", () => {
    const { input } = setup("", { suggestions: SUGGESTIONS });
    input.focus();
    typeInto(input, "zzz");
    expect(dropdown()).toBeNull();
  });

  it("auto-highlights the first match so Enter has an obvious target", () => {
    const { input } = setup("", { suggestions: SUGGESTIONS });
    input.focus();
    typeInto(input, "exp");
    const active = items().find((el) => el.classList.contains("accountInput-dropdown__item--active"));
    expect(active?.textContent).toBe("expenses:groceries");
  });
});

describe('attachAccountInput — "+ Add" requires explicit selection', () => {
  it('does not show "+ Add" when allowCreate is false', () => {
    const { input } = setup("", { suggestions: SUGGESTIONS });
    input.focus();
    typeInto(input, "wholly-new-account");
    expect(dropdown()).toBeNull();
  });

  it('shows "+ Add" only when typed value has no exact match (case-insensitive)', () => {
    const { input } = setup("", { suggestions: SUGGESTIONS, allowCreate: true });
    input.focus();

    typeInto(input, "Expenses:Groceries");
    expect(items().some((el) => el.classList.contains("accountInput-dropdown__item--add"))).toBe(false);

    typeInto(input, "expenses:new");
    const addItem = items().find((el) => el.classList.contains("accountInput-dropdown__item--add"));
    expect(addItem?.textContent).toBe('+ Add "expenses:new"');
  });

  it('does not commit "new" on blur — typed text reverts when ambiguous', () => {
    const { input, commits } = setup("expenses:rent", { suggestions: SUGGESTIONS, allowCreate: true });
    input.focus();
    typeInto(input, "expenses:newthing");
    input.dispatchEvent(new Event("blur"));
    vi.runAllTimers();
    expect(commits).toEqual([{ value: "expenses:rent", kind: "revert" }]);
    expect(input.value).toBe("expenses:rent");
  });

  it('commits "new" only on explicit click of the "+ Add" item', () => {
    const { input, commits } = setup("", { suggestions: SUGGESTIONS, allowCreate: true });
    input.focus();
    typeInto(input, "expenses:novel");
    const addItem = items().find((el) => el.classList.contains("accountInput-dropdown__item--add"))!;
    mousedown(addItem);
    expect(commits).toEqual([{ value: "expenses:novel", kind: "new" }]);
    expect(input.value).toBe("expenses:novel");
  });

  it('commits "new" when "+ Add" is highlighted and Enter is pressed', () => {
    const { input, commits } = setup("", { suggestions: SUGGESTIONS, allowCreate: true });
    input.focus();
    typeInto(input, "expenses:novel");
    // "+ Add" is the only item shown when no existing matches; auto-highlight points to it
    expect(items()[0].classList.contains("accountInput-dropdown__item--add")).toBe(true);
    press(input, "Enter");
    expect(commits).toEqual([{ value: "expenses:novel", kind: "new" }]);
  });
});

describe("attachAccountInput — selection commits the canonical value", () => {
  it("clicking an existing suggestion sets the input value to the suggestion (not the typed prefix)", () => {
    const { input, commits } = setup("", { suggestions: SUGGESTIONS, allowCreate: true });
    input.focus();
    typeInto(input, "gro"); // partial — should NOT end up in input.value after selection
    const target = items().find((el) => el.textContent === "expenses:groceries")!;
    mousedown(target);
    expect(input.value).toBe("expenses:groceries");
    expect(commits).toEqual([{ value: "expenses:groceries", kind: "existing" }]);
  });

  it('Enter on highlighted "+ Add" carries the typed value, not the "+ Add ..." label', () => {
    const { input, commits } = setup("", { suggestions: SUGGESTIONS, allowCreate: true });
    input.focus();
    typeInto(input, "expenses:bespoke");
    press(input, "Enter");
    expect(input.value).toBe("expenses:bespoke");
    expect(commits[0]).toEqual({ value: "expenses:bespoke", kind: "new" });
  });
});

describe("attachAccountInput — blur is forgiving, not aggressive", () => {
  it("blurring with text that exactly matches an existing account commits the canonical form", () => {
    const { input, commits } = setup("", { suggestions: SUGGESTIONS });
    input.focus();
    // mixed case — should canonicalize on blur
    typeInto(input, "EXPENSES:RENT");
    input.dispatchEvent(new Event("blur"));
    vi.runAllTimers();
    expect(commits).toEqual([{ value: "expenses:rent", kind: "existing" }]);
    expect(input.value).toBe("expenses:rent");
  });

  it("blurring with partial text reverts to the initial value (does not auto-commit a partial)", () => {
    const { input, commits } = setup("expenses:groceries", { suggestions: SUGGESTIONS, allowCreate: true });
    input.focus();
    typeInto(input, "exp");
    input.dispatchEvent(new Event("blur"));
    vi.runAllTimers();
    expect(input.value).toBe("expenses:groceries");
    expect(commits).toEqual([{ value: "expenses:groceries", kind: "revert" }]);
  });

  it("a click on a suggestion wins the race against blur", () => {
    const { input, commits } = setup("", { suggestions: SUGGESTIONS });
    input.focus();
    typeInto(input, "rent");
    const target = items().find((el) => el.textContent === "expenses:rent")!;
    mousedown(target); // synchronous commit
    input.dispatchEvent(new Event("blur"));
    vi.runAllTimers();
    // Only the selection commit fires; blur's deferred settle no-ops because committed=true
    expect(commits).toEqual([{ value: "expenses:rent", kind: "existing" }]);
  });
});

describe("attachAccountInput — keyboard navigation", () => {
  it("ArrowDown / ArrowUp move the highlight within the dropdown", () => {
    const { input } = setup("", { suggestions: SUGGESTIONS });
    input.focus();
    typeInto(input, "exp");
    press(input, "ArrowDown");
    expect(items()[1].classList.contains("accountInput-dropdown__item--active")).toBe(true);
    press(input, "ArrowDown");
    expect(items()[2].classList.contains("accountInput-dropdown__item--active")).toBe(true);
    press(input, "ArrowUp");
    expect(items()[1].classList.contains("accountInput-dropdown__item--active")).toBe(true);
  });

  it("Escape cancels and reverts to the initial value", () => {
    const { input, commits } = setup("expenses:rent", { suggestions: SUGGESTIONS });
    input.focus();
    typeInto(input, "groceries");
    press(input, "Escape");
    expect(input.value).toBe("expenses:rent");
    expect(commits[commits.length - 1]).toEqual({ value: "expenses:rent", kind: "cancel" });
  });
});

describe("attachAccountInput — revertOnBlur:false (modal mode)", () => {
  it("preserves the typed value on blur when there is no exact match", () => {
    const { input, commits } = setup("", { suggestions: SUGGESTIONS, allowCreate: true, revertOnBlur: false });
    input.focus();
    typeInto(input, "bank:newaccount");
    input.dispatchEvent(new Event("blur"));
    vi.runAllTimers();
    // Value left intact; no commit fires (caller reads input.value via its own button)
    expect(input.value).toBe("bank:newaccount");
    expect(commits).toEqual([]);
  });

  it("still canonicalizes an exact case-insensitive match on blur", () => {
    const { input, commits } = setup("", { suggestions: SUGGESTIONS, allowCreate: true, revertOnBlur: false });
    input.focus();
    typeInto(input, "EXPENSES:RENT");
    input.dispatchEvent(new Event("blur"));
    vi.runAllTimers();
    expect(input.value).toBe("expenses:rent");
    expect(commits).toEqual([{ value: "expenses:rent", kind: "existing" }]);
  });
});

describe("attachAccountInput — prefix mode (Add Account modal)", () => {
  it("filters suggestions to those under the prefix and strips it for display", () => {
    const { input } = setup("", { suggestions: SUGGESTIONS, prefix: "assets:" });
    input.focus();
    typeInto(input, "bank");
    expect(items().map((el) => el.textContent)).toEqual(["bank:savings", "bank:checking"]);
  });

  it("re-attaches the prefix on commit", () => {
    const { input, commits } = setup("", { suggestions: SUGGESTIONS, prefix: "assets:" });
    input.focus();
    typeInto(input, "bank:savings");
    const target = items().find((el) => el.textContent === "bank:savings")!;
    mousedown(target);
    expect(input.value).toBe("bank:savings"); // input shows the unprefixed form
    expect(commits).toEqual([{ value: "assets:bank:savings", kind: "existing" }]); // callback gets the canonical form
  });
});

describe("attachAccountInput — controller", () => {
  it("destroy() removes listeners and the dropdown", () => {
    const { input, controller, commits } = setup("", { suggestions: SUGGESTIONS });
    input.focus();
    typeInto(input, "exp");
    expect(dropdown()).not.toBeNull();
    controller.destroy();
    expect(dropdown()).toBeNull();
    // After destroy, blur should not fire settle — no further commits
    input.dispatchEvent(new Event("blur"));
    vi.runAllTimers();
    expect(commits).toEqual([]);
  });

  it("refreshSuggestions() updates the source list", () => {
    const { input, controller } = setup("", { suggestions: ["expenses:rent"] });
    input.focus();
    typeInto(input, "groc");
    expect(dropdown()).toBeNull();
    controller.refreshSuggestions(SUGGESTIONS);
    typeInto(input, "groc");
    expect(items()[0].textContent).toBe("expenses:groceries");
  });

  it("re-attaching to the same input refreshes onCommit without dropping listeners or dropdown state", () => {
    // Regression: the rule editor calls attachAccountInput on every render
    // because morphdom preserves the input element AND bindOnceDuring
    // strips its element listeners. Re-attach must (a) re-bind listeners
    // so the input still works, (b) route commits to the latest closure,
    // and (c) preserve any open dropdown so an unrelated re-render
    // doesn't close the user's autocomplete mid-typing.
    document.body.innerHTML = "";
    const input = document.createElement("input");
    document.body.appendChild(input);
    const firstCommits: Array<{ value: string; kind: CommitKind }> = [];
    const secondCommits: Array<{ value: string; kind: CommitKind }> = [];

    const c1 = attachAccountInput(input, { suggestions: SUGGESTIONS }, (v, k) => firstCommits.push({ value: v, kind: k }));
    input.focus();
    typeInto(input, "exp");
    expect(dropdown()).not.toBeNull(); // dropdown is open

    // Simulate the host framework's behavior between renders: bind-once
    // strips the prior element listeners. Then attachAccountInput is called
    // again with a fresh closure.
    input.removeEventListener("input", () => {}); // (no-op; jsdom doesn't expose tracked listeners)
    const c2 = attachAccountInput(input, { suggestions: SUGGESTIONS }, (v, k) => secondCommits.push({ value: v, kind: k }));

    // Same controller instance — refreshed in place
    expect(c2).toBe(c1);
    // Dropdown survived the re-attach
    expect(dropdown()).not.toBeNull();

    // Subsequent commits route to the latest onCommit
    typeInto(input, "EXPENSES:RENT");
    input.dispatchEvent(new Event("blur"));
    vi.runAllTimers();
    expect(firstCommits).toEqual([]);
    expect(secondCommits).toEqual([{ value: "expenses:rent", kind: "existing" }]);
    expect(document.body.querySelectorAll(".accountInput-dropdown").length).toBe(0);
  });
});
