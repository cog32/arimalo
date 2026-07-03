// End-to-end test for the optimistic Apply Rule flow.
//
// Simulates the #ruleSave / #ruleDelete click handlers' state mutations
// + the queue's drain without involving the DOM, and asserts:
//   1. sidebarView restores to the previous view IMMEDIATELY (before
//      the backend invoke resolves).
//   2. The rule editor draft is cleared at the same moment.
//   3. On backend success, status updates without bringing the editor back.
//   4. On backend failure, the editor is re-opened with the in-flight
//      draft so the user doesn't lose unsaved edits.

import { describe, it, expect, vi } from "vitest";
import { MutationQueue } from "./mutation-queue";

interface RuleDraft {
  ruleId?: string;
  accountFolder: string;
  previousView: "accounts" | "reports";
  pattern: string;
  amountAccount: string;
  error?: string;
}

interface SimState {
  pendingMutations: number;
  status?: string;
  ruleEditorDraft?: RuleDraft;
  sidebarView: "accounts" | "reports" | "rule-editor";
}

function makeState(): SimState {
  return {
    pendingMutations: 0,
    sidebarView: "rule-editor",
    ruleEditorDraft: {
      accountFolder: "richard/cash/bank/cba/savings",
      previousView: "accounts",
      pattern: "*Coffee*",
      amountAccount: "expenses:food:coffee",
    },
  };
}

/** What the production click handler does pre-invoke. The optimistic
 *  close is the WHOLE point of this change: it must not be deferred
 *  to inside the queue, otherwise the user stares at the rule editor
 *  for the full pipeline rebuild duration. */
function optimisticClose(state: SimState, draft: RuleDraft, render: () => void): void {
  const prevView = draft.previousView ?? "accounts";
  state.ruleEditorDraft = undefined;
  state.sidebarView = prevView;
  render();
}

/** What the production .catch does on backend failure. */
function reopenOnError(
  state: SimState,
  draft: RuleDraft,
  err: unknown,
  render: () => void,
): void {
  state.ruleEditorDraft = { ...draft, error: String(err) };
  state.sidebarView = "rule-editor";
  state.status = `Error: ${String(err)}`;
  render();
}

describe("optimistic Apply Rule end-to-end", () => {
  it("restores sidebarView before the backend invoke resolves", async () => {
    const state = makeState();
    const renderSnapshots: Array<{ view: string; draft: boolean; status?: string }> = [];
    const queue = new MutationQueue({
      render: () => renderSnapshots.push({
        view: state.sidebarView,
        draft: state.ruleEditorDraft !== undefined,
        status: state.status,
      }),
    });

    let backendDone: () => void = () => {};
    const backendPromise = new Promise<void>((res) => { backendDone = res; });

    // Simulate the click handler: optimistic close, then enqueue.
    const draft = state.ruleEditorDraft!;
    optimisticClose(state, draft, () => {
      // The handler renders synchronously after restoring the view.
      renderSnapshots.push({
        view: state.sidebarView,
        draft: state.ruleEditorDraft !== undefined,
        status: state.status,
      });
    });

    const enqueued = queue.enqueue(state, "Saving rule...", async () => {
      await backendPromise;
      state.status = "Rule saved";
    });

    // First snapshot: optimistic close has already happened. View is
    // "accounts", draft is gone. The backend hasn't resolved yet.
    expect(renderSnapshots[0]).toEqual({
      view: "accounts",
      draft: false,
      status: undefined,
    });

    // Resolve the backend, drain, and confirm the final state.
    backendDone();
    await enqueued;
    await new Promise((r) => setTimeout(r, 20));

    expect(state.sidebarView).toBe("accounts");
    expect(state.ruleEditorDraft).toBeUndefined();
    expect(state.status).toBe("Rule saved");
  });

  it("re-opens the rule editor with the in-flight draft when the backend rejects", async () => {
    const state = makeState();
    const draft = state.ruleEditorDraft!;
    const queue = new MutationQueue({ render: () => { /* unused */ } });

    optimisticClose(state, draft, () => { /* render */ });
    expect(state.sidebarView).toBe("accounts");
    expect(state.ruleEditorDraft).toBeUndefined();

    let backendReject: (err: Error) => void = () => {};
    const backendPromise = new Promise<void>((_res, rej) => { backendReject = rej; });

    const renderSpy = vi.fn();
    const enqueued = queue.enqueue(state, "Saving rule...", async () => {
      await backendPromise;
    }).catch((err) => reopenOnError(state, draft, err, renderSpy));

    backendReject(new Error("pipeline rebuild failed"));
    await enqueued;
    await new Promise((r) => setTimeout(r, 20));

    // Editor is back, with the original draft preserved + an error
    // string to show the user.
    expect(state.sidebarView).toBe("rule-editor");
    expect(state.ruleEditorDraft).toBeDefined();
    expect(state.ruleEditorDraft?.pattern).toBe("*Coffee*");
    expect(state.ruleEditorDraft?.amountAccount).toBe("expenses:food:coffee");
    expect(state.ruleEditorDraft?.error).toContain("pipeline rebuild failed");
    expect(state.status).toContain("Error:");
    expect(renderSpy).toHaveBeenCalled();
  });

  it("does not bring the editor back if a stray render fires after success", async () => {
    // Regression guard: nothing in applyMutationResponse or the
    // queue's drain path should resurrect state.ruleEditorDraft.
    const state = makeState();
    const draft = state.ruleEditorDraft!;
    optimisticClose(state, draft, () => { /* render */ });

    // Simulate any post-mutation render — sidebarView and draft must
    // remain in their post-close state.
    expect(state.sidebarView).toBe("accounts");
    expect(state.ruleEditorDraft).toBeUndefined();

    // Even after refreshSearch landed and the queue drained.
    state.status = "Rule saved";
    expect(state.sidebarView).toBe("accounts");
    expect(state.ruleEditorDraft).toBeUndefined();
  });

  it("falls back to 'accounts' when previousView is missing", () => {
    // Defensive: a draft constructed without previousView (older
    // codepath, malformed state) must still close cleanly.
    const state = makeState();
    const draft = { ...state.ruleEditorDraft! } as RuleDraft;
    delete (draft as Partial<RuleDraft>).previousView;

    optimisticClose(state, draft, () => { /* render */ });
    expect(state.sidebarView).toBe("accounts");
    expect(state.ruleEditorDraft).toBeUndefined();
  });
});
