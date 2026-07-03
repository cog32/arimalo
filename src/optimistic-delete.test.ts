// End-to-end test for the optimistic delete flow.
//
// Simulates the click handler's state mutations + the queue's drain
// without involving the DOM, and asserts:
//   1. After the click the txn is filtered out before the pipeline returns.
//   2. After the pipeline returns and refreshView updates the search list
//      to NOT include the txn (because the rule re-categorised it), the
//      txn stays filtered.
//   3. After pendingDeletes is cleared (the .finally), the txn stays
//      gone — because the search list no longer includes it.

import { describe, it, expect, vi } from "vitest";
import { MutationQueue } from "./mutation-queue";
import { filterPendingDeletes } from "./optimistic";

interface SimState {
  pendingMutations: number;
  status?: string;
  pendingDeletes: Set<string>;
  searchFilteredTransactions: Array<{ meta: string | null; payee: string }>;
}

function makeState(): SimState {
  return {
    pendingMutations: 0,
    pendingDeletes: new Set(),
    searchFilteredTransactions: [
      { meta: "txn:abc, ofx_id:foo", payee: "Original Row" },
      { meta: "txn:other", payee: "Other Row" },
    ],
  };
}

/** What buildTxTableBody does with state.pendingDeletes and the search list. */
function visibleRows(state: SimState): Array<{ payee: string }> {
  return filterPendingDeletes(
    state.searchFilteredTransactions,
    state.pendingDeletes,
  );
}

describe("optimistic delete end-to-end", () => {
  it("filters the row before the pipeline returns and keeps it filtered after", async () => {
    const state = makeState();
    const renderSpy = vi.fn();
    const renderSnapshots: Array<string[]> = [];
    const queue = new MutationQueue({
      render: () => {
        renderSpy();
        renderSnapshots.push(visibleRows(state).map((r) => r.payee));
      },
    });

    let backendDone: () => void = () => {};
    const backendPromise = new Promise<void>((res) => { backendDone = res; });

    // Simulate the click handler:
    const txnId = "txn:abc";
    state.pendingDeletes.add(txnId);
    const enqueued = queue.enqueue(state, "Hiding...", async () => {
      await backendPromise;
      // Simulate applyPipelineResponse: the rule has re-categorised the
      // txn so the next refreshView returns a list without it.
      state.searchFilteredTransactions = state.searchFilteredTransactions.filter(
        (t) => !t.meta?.includes(txnId),
      );
    }).finally(() => {
      state.pendingDeletes.delete(txnId);
    });

    // Renders are deferred (requestIdleCallback / setTimeout) so the
    // queue's flow doesn't block on heavy render work. Flush.
    await new Promise((r) => setTimeout(r, 20));

    // First snapshot: optimistic filter active, row gone even though
    // searchFilteredTransactions still contains it.
    expect(renderSnapshots[0]).toEqual(["Other Row"]);
    expect(state.searchFilteredTransactions.map((t) => t.payee)).toContain("Original Row");

    // Pipeline finishes; queue's post-mutation render is also deferred.
    backendDone();
    await enqueued;
    await new Promise((r) => setTimeout(r, 20));

    // After everything settles, the search list reflects the post-rule
    // state (without the hidden txn). pendingDeletes is empty but the
    // row is still filtered out because it's not in the data.
    const finalSnapshot = renderSnapshots[renderSnapshots.length - 1];
    expect(finalSnapshot).toEqual(["Other Row"]);
    expect(state.pendingDeletes.size).toBe(0);
    expect(state.searchFilteredTransactions.map((t) => t.payee)).toEqual(["Other Row"]);
  });

  it("does NOT bring the row back even if a stray render fires after pendingDeletes is cleared", () => {
    // Regression: if any render fires after the .finally clears
    // pendingDeletes (e.g. from the file watcher's pipeline-rebuilt
    // event), the row should still be gone — because by then the
    // search list reflects the new ledger state.
    const state = makeState();

    // After pipeline applied the rule, the txn is no longer in the
    // search results.
    state.searchFilteredTransactions = state.searchFilteredTransactions.filter(
      (t) => !t.meta?.includes("txn:abc"),
    );
    // pendingDeletes is empty (cleared by .finally).
    state.pendingDeletes = new Set();

    expect(visibleRows(state).map((r) => r.payee)).toEqual(["Other Row"]);
  });

  it("DOES bring the row back if the search list still contains it (rule didn't apply)", () => {
    // Diagnostic: this is the failure mode the user described —
    // optimistic vanish then reappearance. Happens when the search
    // list still contains the txn after the pipeline rebuild AND
    // pendingDeletes has been cleared.
    const state = makeState();
    state.pendingDeletes = new Set();
    // searchFilteredTransactions intentionally NOT mutated — the rule
    // did NOT re-categorise the txn for some reason.

    expect(visibleRows(state).map((r) => r.payee)).toContain("Original Row");
  });
});
