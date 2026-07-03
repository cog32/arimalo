// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from "vitest";
import { bindOnceDuring, _resetBindOnceForTests } from "./bind-once";

describe("bindOnceDuring — multi-handler re-binding", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
    _resetBindOnceForTests();
  });

  it("preserves multiple distinct listeners on the same (element, event)", () => {
    // Regression: the previous implementation deduped by (element, type),
    // so a second `addEventListener("click", ...)` on the same element
    // was silently dropped. `.txTable` legitimately carries three click
    // handlers (row expand, explorer link, Edit Rule button) and
    // `.smartSearch` carries two (focus the input, remove pill). The
    // dropped handlers manifested as the Edit Rule button doing
    // nothing and the pill × failing to remove its pill.
    const el = document.createElement("div");
    document.body.appendChild(el);
    const calls: string[] = [];

    bindOnceDuring(() => {
      el.addEventListener("click", () => calls.push("expand"));
      el.addEventListener("click", () => calls.push("explorer"));
      el.addEventListener("click", () => calls.push("editRule"));
    });

    el.click();
    expect(calls).toEqual(["expand", "explorer", "editRule"]);
  });

  it("does not double-fire after a re-render attaches fresh closures", () => {
    // The whole point of bindOnceDuring: morphdom preserves the
    // element across renders, attachRenderHandlers re-runs and
    // creates fresh closures. Without dedup we'd attach two copies
    // and the click would fire twice.
    const el = document.createElement("div");
    document.body.appendChild(el);
    let calls = 0;

    bindOnceDuring(() => {
      el.addEventListener("click", () => { calls++; });
    });
    bindOnceDuring(() => {
      el.addEventListener("click", () => { calls++; });
    });

    el.click();
    expect(calls).toBe(1);
  });

  it("re-binds with the latest closure (so newest state is captured)", () => {
    // The fresh closure attached during render N must be the one
    // that fires on the next click; the stale closure from render
    // N-1 should be gone.
    const el = document.createElement("div");
    document.body.appendChild(el);
    let activeRender = 0;
    const fired: number[] = [];

    activeRender = 1;
    bindOnceDuring(() => {
      const captured = activeRender;
      el.addEventListener("click", () => fired.push(captured));
    });
    activeRender = 2;
    bindOnceDuring(() => {
      const captured = activeRender;
      el.addEventListener("click", () => fired.push(captured));
    });

    el.click();
    expect(fired).toEqual([2]);
  });

  it("conditional binding: a handler only attached on render 2 still fires", () => {
    const el = document.createElement("div");
    document.body.appendChild(el);
    const calls: string[] = [];

    bindOnceDuring(() => {
      el.addEventListener("click", () => calls.push("alwaysOn"));
    });
    bindOnceDuring(() => {
      el.addEventListener("click", () => calls.push("alwaysOn"));
      el.addEventListener("click", () => calls.push("conditional"));
    });

    el.click();
    expect(calls).toEqual(["alwaysOn", "conditional"]);
  });

  it("restores original addEventListener even when fn throws", () => {
    const el = document.createElement("div");
    document.body.appendChild(el);

    expect(() =>
      bindOnceDuring(() => {
        throw new Error("boom");
      }),
    ).toThrow("boom");

    // After the throw, addEventListener outside the window must
    // behave normally — i.e. no longer be the tracking variant.
    let calls = 0;
    el.addEventListener("click", () => { calls++; });
    el.addEventListener("click", () => { calls++; });
    el.click();
    expect(calls).toBe(2);
  });
});
