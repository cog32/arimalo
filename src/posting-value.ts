import type { Posting } from "./types";

/**
 * The value of a posting derived from its price annotation, in the
 * annotation's currency. Returns null when the posting has no usable price.
 *
 * `@@` (is_total) carries the leg total directly; `@` (per-unit) must be
 * multiplied by the absolute unit count.
 */
export function postingPriceValue(posting: Posting | undefined | null): number | null {
  if (!posting) return null;
  const price = posting.price;
  if (!price || !price.amount) return null;
  return price.is_total ? price.amount : Math.abs(posting.amount) * price.amount;
}
