/**
 * Pure helpers for working with the comma-separated `meta` string that the
 * pipeline stamps onto every transaction (e.g. "txn:HASH,rule:R1,swap:txn:OTHER").
 *
 * These are extracted from src/main.ts so they can be imported and unit-tested
 * directly (main.ts has no exports because it owns mutable module-level state).
 */

/** True when `acct` equals `prefix` or is a child (e.g. "a:b:c" matches "a:b"). */
export function matchesPrefix(acct: string, prefix: string): boolean {
  return acct === prefix || acct.startsWith(prefix + ":");
}

/** Extract the `rule:<id>` value, or undefined when absent. */
export function extractRuleId(meta?: string | null): string | undefined {
  if (!meta) return undefined;
  const match = meta.split(",").map((s) => s.trim()).find((s) => s.startsWith("rule:"));
  return match ? match.slice(5) : undefined;
}

/** Extract the `txn:<id>` entry from a comma-separated meta string. Returns "" when absent. */
export function txnIdFromMeta(meta: string | null | undefined): string {
  return (meta ?? "").split(",").map((p) => p.trim()).find((p) => p.startsWith("txn:")) ?? "";
}

/**
 * Extract the `leg:<id>` entry — the per-leg id the pipeline stamps when several
 * legs of one on-chain transaction share a `txn:` id (swap / wrap / multi-hop).
 * Returns "" when absent (single-leg rows are uniquely identified by `txn:`).
 */
export function legIdFromMeta(meta: string | null | undefined): string {
  return (meta ?? "").split(",").map((p) => p.trim()).find((p) => p.startsWith("leg:")) ?? "";
}

/**
 * Extract a `key:value` segment from comma-separated meta. Returns the part
 * after the prefix (without the prefix itself), or undefined when absent.
 * For prefix `swap:` this returns the partner's `txn:<HASH>` segment.
 */
export function metaSegmentValue(meta: string | null | undefined, prefix: string): string | undefined {
  if (!meta) return undefined;
  for (const seg of meta.split(",")) {
    const trimmed = seg.trim();
    if (trimmed.startsWith(prefix)) return trimmed.slice(prefix.length);
  }
  return undefined;
}

/**
 * Read the Rust-stamped swap partner reference from a transaction's meta.
 * Returns `{ partnerTxnId, partnerCommodity }` when both segments are present —
 * the pipeline writes them together for every paired transaction so consumers
 * can find the partner unambiguously even when several legs share an on-chain
 * hash. Returns undefined for transactions that aren't part of a swap pair.
 */
export function swapPartnerRefFromMeta(meta: string | null | undefined):
  | { partnerTxnId: string; partnerCommodity: string }
  | undefined {
  const swapRef = metaSegmentValue(meta, "swap:");
  const partnerCommodity = metaSegmentValue(meta, "swap_partner_commodity:");
  if (!swapRef || !partnerCommodity) return undefined;
  return { partnerTxnId: swapRef, partnerCommodity };
}

/** Stable key for a transaction that works across different deserialization sources. */
export function txnValueKey(
  t: { meta?: string | null; datetime: string; amount: number; narration?: string | null },
): string {
  return `${txnIdFromMeta(t.meta)}|${t.datetime}|${t.amount}|${t.narration ?? ""}`;
}
