// @vitest-environment jsdom
//
// Regression test (FIX.md) for the "Generate with AI" → wrong-prompt bug.
//
// The "Configure CSV Transform" modal's generate button (#aiTransformBtn)
// reuses the .ai-sparkle-btn class for styling. The per-row "AI suggest
// category" handler in main.ts (attachAiAccountHandlers) selected ALL
// .ai-sparkle-btn elements, so it also bound to the transform button — pressing
// "Generate with AI" then fired ai_suggest_categorisation against an empty
// transaction (the single-transaction classify prompt) instead of only
// ai_suggest_transform. Scoping the selector on [data-ai-txn-id] fixes it.
//
// This is the test of record for that scoping contract: the transform button
// must never be matched as a per-row AI button.
import { describe, it, expect, beforeEach } from "vitest";
import { ROW_AI_SPARKLE_SELECTOR } from "./ai-sparkle";

// Markup mirrors main.ts. Row button: renderTxnActionsCell (carries data-ai-txn-id
// and the other data-ai-* attributes the categorisation handler reads).
const ROW_BUTTON = `<button
  class="ai-sparkle-btn"
  data-ai-txn-id="txn-123"
  data-ai-payee="Coles"
  data-ai-narration="groceries"
  data-ai-amount="-50.00"
  data-ai-commodity="AUD"
  data-ai-date="2024-01-15"
  data-ai-datetime="2024-01-15 10:30:00"
  title="AI suggest category"
><svg></svg></button>`;

// Transform modal button: same styling class, no data-ai-* attributes.
const TRANSFORM_BUTTON =
  `<button id="aiTransformBtn" class="ai-sparkle-btn" title="Generate with AI"><svg></svg></button>`;

describe("ROW_AI_SPARKLE_SELECTOR — per-row AI button scoping", () => {
  beforeEach(() => {
    document.body.innerHTML = "";
  });

  it("matches a per-row sparkle button", () => {
    document.body.innerHTML = ROW_BUTTON;
    expect(document.querySelectorAll(ROW_AI_SPARKLE_SELECTOR).length).toBe(1);
  });

  it("does NOT match the transform modal's Generate-with-AI button", () => {
    document.body.innerHTML = TRANSFORM_BUTTON;
    expect(document.querySelectorAll(ROW_AI_SPARKLE_SELECTOR).length).toBe(0);
  });

  it("selects only the row button when both are present", () => {
    document.body.innerHTML = `${ROW_BUTTON}${TRANSFORM_BUTTON}`;

    // Root cause of the bug: both buttons share the .ai-sparkle-btn styling class,
    // so a bare ".ai-sparkle-btn" selector (the old code) matched both.
    expect(document.querySelectorAll(".ai-sparkle-btn").length).toBe(2);

    // The fix: the scoped selector binds the categorisation handler to the
    // transaction row button only — never the transform button.
    const matched = document.querySelectorAll<HTMLButtonElement>(ROW_AI_SPARKLE_SELECTOR);
    expect(matched.length).toBe(1);
    expect(matched[0].id).not.toBe("aiTransformBtn");
    expect(matched[0].dataset.aiTxnId).toBe("txn-123");
  });
});
