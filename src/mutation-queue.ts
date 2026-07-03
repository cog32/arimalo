// Single-flight FIFO queue for pipeline mutations.
//
// Pipeline mutations (delete, hide, link, rule edit) all rebuild the
// generated ledger via run_pipeline, which holds a global lock on the
// Rust side. Issuing two of those calls in parallel just blocks the
// second invoke and freezes the UI until both round-trips complete.
//
// Routing handlers through this queue keeps clicks instant: a click
// always returns control to the event loop in the next microtask, the
// strip indicator (state.pendingMutations) lights up, and the work
// drains one item at a time in the background.
//
// Renders are NOT called synchronously anywhere in the mutation flow.
// On a populated vault the full render (buildMainHtml + morphdom over
// 500 visible txn rows + sidebar + everything) is 1-3 seconds of pure
// CPU work on the JS main thread. Two of those during one mutation =
// macOS beach ball.
//
// The optimistic DOM updates the click handler does directly (row
// removed, strip class set) are enough for visual feedback. The post-
// mutation render is just confirmation that DOM and state agree — it
// doesn't need to happen on the critical path. We schedule it via
// requestIdleCallback so it runs only when the browser has nothing
// else to do, never blocking the click flow.

export interface QueueState {
  pendingMutations: number;
  status?: string;
}

export interface QueueDeps {
  render: () => void;
}

export type MutationFn = () => Promise<void>;

interface QueueItem {
  label: string;
  fn: MutationFn;
  resolve: () => void;
  reject: (e: unknown) => void;
}

export class MutationQueue {
  private queue: QueueItem[] = [];
  private running = false;

  constructor(private deps: QueueDeps) {}

  get length(): number {
    return this.queue.length + (this.running ? 1 : 0);
  }

  enqueue(state: QueueState, label: string, fn: MutationFn): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      this.queue.push({ label, fn, resolve, reject });
      state.pendingMutations++;
      state.status = label;
      // No synchronous render here — drain yields once before the first
      // render so the calling click handler returns to the event loop
      // immediately and the browser stays responsive.
      void this.drain(state);
    });
  }

  private async drain(state: QueueState): Promise<void> {
    if (this.running) return;
    while (this.queue.length > 0) {
      const item = this.queue.shift()!;
      this.running = true;
      state.status = item.label;
      // Schedule the strip-indicator render for when the browser is
      // idle. Critically: do NOT await this. The mutation fn proceeds
      // immediately; if a render fires before the fn finishes that's
      // fine (state is already updated), and if it doesn't, the post-
      // mutation render below picks up everything.
      this.scheduleRender();
      try {
        await item.fn();
        item.resolve();
      } catch (err) {
        item.reject(err);
      } finally {
        state.pendingMutations--;
        this.running = false;
        this.scheduleRender();
      }
    }
  }

  private scheduleRender(): void {
    const fn = this.deps.render;
    const ric = (globalThis as { requestIdleCallback?: (cb: () => void, opts?: { timeout: number }) => void }).requestIdleCallback;
    if (typeof ric === "function") {
      ric(() => fn(), { timeout: 1000 });
    } else {
      setTimeout(fn, 0);
    }
  }
}
