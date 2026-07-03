import { describe, it, expect, vi } from "vitest";
import { MutationQueue, type QueueState } from "./mutation-queue";

function makeState(): QueueState {
  return { pendingMutations: 0 };
}

function deferred<T = void>(): {
  promise: Promise<T>;
  resolve: (v: T) => void;
  reject: (e: unknown) => void;
} {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

describe("MutationQueue", () => {
  it("runs a single mutation and resolves", async () => {
    const state = makeState();
    const render = vi.fn();
    const q = new MutationQueue({ render });

    let ran = false;
    await q.enqueue(state, "do thing", async () => { ran = true; });

    expect(ran).toBe(true);
    expect(state.pendingMutations).toBe(0);
  });

  it("serializes two mutations enqueued back-to-back", async () => {
    const state = makeState();
    const q = new MutationQueue({ render: () => {} });

    const order: string[] = [];
    const d1 = deferred();
    const d2 = deferred();

    const p1 = q.enqueue(state, "first", async () => {
      order.push("first-start");
      await d1.promise;
      order.push("first-end");
    });
    const p2 = q.enqueue(state, "second", async () => {
      order.push("second-start");
      await d2.promise;
      order.push("second-end");
    });

    // Microtask flush — first should have started, second should NOT
    await Promise.resolve();
    await Promise.resolve();
    expect(order).toEqual(["first-start"]);

    d1.resolve();
    await p1;
    // After first finishes, second starts
    await Promise.resolve();
    await Promise.resolve();
    expect(order).toEqual(["first-start", "first-end", "second-start"]);

    d2.resolve();
    await p2;
    expect(order).toEqual(["first-start", "first-end", "second-start", "second-end"]);
  });

  it("counts pendingMutations across the queue lifetime", async () => {
    const state = makeState();
    const q = new MutationQueue({ render: () => {} });

    const d1 = deferred();
    const d2 = deferred();

    const counts: number[] = [];
    counts.push(state.pendingMutations); // 0

    const p1 = q.enqueue(state, "a", async () => { await d1.promise; });
    counts.push(state.pendingMutations); // 1 after enqueue

    const p2 = q.enqueue(state, "b", async () => { await d2.promise; });
    counts.push(state.pendingMutations); // 2 after enqueue

    d1.resolve();
    await p1;
    counts.push(state.pendingMutations); // 1 after first done

    d2.resolve();
    await p2;
    counts.push(state.pendingMutations); // 0 after both done

    expect(counts).toEqual([0, 1, 2, 1, 0]);
  });

  it("schedules at least one render per mutation (deferred to idle)", async () => {
    const state = makeState();
    const render = vi.fn();
    const q = new MutationQueue({ render });

    await q.enqueue(state, "x", async () => {});
    // Renders are deferred via requestIdleCallback (or setTimeout(0))
    // so the click flow doesn't block. Flush the timer queue.
    await new Promise((r) => setTimeout(r, 20));
    expect(render.mock.calls.length).toBeGreaterThanOrEqual(1);
  });

  it("reflects label in state.status when each item starts", async () => {
    const state = makeState();
    const seen: string[] = [];
    const q = new MutationQueue({
      render: () => { if (state.status) seen.push(state.status); },
    });

    await q.enqueue(state, "label-1", async () => {});
    await q.enqueue(state, "label-2", async () => {});
    // Flush deferred renders.
    await new Promise((r) => setTimeout(r, 20));

    expect(seen).toContain("label-2");
  });

  it("does NOT call render synchronously inside enqueue", () => {
    // Critical invariant for responsive UI: on a large vault the full
    // render takes long enough that the OS shows a wait cursor (macOS
    // beach ball) if it runs before the click handler returns. enqueue
    // must update state synchronously but defer the render so the
    // calling handler completes immediately.
    const state = makeState();
    const renderCalls: string[] = [];
    const q = new MutationQueue({ render: () => renderCalls.push("rendered") });

    const d = deferred();
    void q.enqueue(state, "x", async () => { await d.promise; });

    expect(state.pendingMutations).toBe(1);
    expect(state.status).toBe("x");
    expect(renderCalls.length).toBe(0);
  });

  it("recovers and continues processing after a thrown error", async () => {
    const state = makeState();
    const q = new MutationQueue({ render: () => {} });

    let ran2 = false;
    const p1 = q.enqueue(state, "boom", async () => { throw new Error("boom"); });
    const p2 = q.enqueue(state, "ok", async () => { ran2 = true; });

    await expect(p1).rejects.toThrow("boom");
    await p2;

    expect(ran2).toBe(true);
    expect(state.pendingMutations).toBe(0);
  });
});
