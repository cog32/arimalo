/**
 * Selector for the per-transaction "AI suggest category" sparkle buttons.
 *
 * The "Configure CSV Transform" modal's "Generate with AI" button
 * (`#aiTransformBtn`) reuses the `.ai-sparkle-btn` class for its sparkle
 * styling, but it is NOT a per-row button: it carries no `data-ai-txn-id`
 * and must invoke `ai_suggest_transform`, never `ai_suggest_categorisation`.
 *
 * Scoping the row categorisation handler on `[data-ai-txn-id]` keeps it from
 * binding to that transform button — otherwise pressing "Generate with AI"
 * also fires a categorisation request against an empty transaction.
 */
export const ROW_AI_SPARKLE_SELECTOR = ".ai-sparkle-btn[data-ai-txn-id]";
