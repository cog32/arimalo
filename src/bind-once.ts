// Idempotent re-binding of per-render event listeners.
//
// attachRenderHandlers re-runs on every render(); morphdom preserves
// elements across renders, so naively re-attaching listeners would
// stack a fresh closure on every render and click handlers would fire
// once for every prior render. We can't dedupe by `(element, type)`
// because the same element legitimately carries multiple distinct
// listeners on the same event (e.g. `.txTable` has three click
// handlers for row expansion, explorer-link clicks, and the
// "Edit Rule" button — collapsing them to one breaks the latter two).
//
// Instead, we track every listener attached during a bindOnceDuring
// window and remove them all before the next window opens. Each
// render therefore sees a clean slate: every listener attaches
// freshly, and no listeners stack across renders. Listeners attached
// outside the window (e.g. one-time wiring at app startup) are not
// tracked and are not removed.
//
// Why patch addEventListener rather than asking callers to register
// through a wrapper: attachRenderHandlers calls 100+ binding sites
// scattered across modules; rewriting each would be invasive and
// fragile. The patch is scoped tightly inside try/finally so no
// other code observes it.

type Tracked = {
  el: Element;
  type: string;
  listener: EventListenerOrEventListenerObject;
  options?: boolean | AddEventListenerOptions;
};

const _addEventListenerOriginal = Element.prototype.addEventListener;
let _trackedListeners: Tracked[] = [];

function _trackingAddEventListener(
  this: Element,
  type: string,
  listener: EventListenerOrEventListenerObject,
  options?: boolean | AddEventListenerOptions,
): void {
  _trackedListeners.push({ el: this, type, listener, options });
  return _addEventListenerOriginal.call(this, type, listener, options);
}

export function bindOnceDuring<T>(fn: () => T): T {
  // Shed listeners attached during the previous window so the new
  // bindings replace them rather than stack on top.
  for (const t of _trackedListeners) {
    t.el.removeEventListener(t.type, t.listener, t.options as EventListenerOptions);
  }
  _trackedListeners = [];

  Element.prototype.addEventListener = _trackingAddEventListener as typeof Element.prototype.addEventListener;
  try {
    return fn();
  } finally {
    Element.prototype.addEventListener = _addEventListenerOriginal;
  }
}

// Test-only escape hatch: forget tracked listeners without removing
// them, so independent test cases don't see each other's bindings.
export function _resetBindOnceForTests(): void {
  _trackedListeners = [];
}
