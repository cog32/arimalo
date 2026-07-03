// Selecting which posting legs a transaction row represents for a given account.
//
// Most transactions have exactly one posting on the selected account, so a row
// shows that single leg. Two cases need more care when several postings fall
// within the account scope, and they pull in opposite directions:
//
//   • In-scope transfer — same commodity, opposite signs (e.g. a wallet leg and
//     its `…:transfer` contra both under `assets:crypto`). Summing nets to zero
//     and hides exactly the rows you need to spot a half-recorded transfer, so
//     we keep showing a single representative leg.
//
//   • Multi-commodity holding — distinct commodities held in one account (e.g. a
//     property booked as separate LAND/BUILDING legs). Each commodity is its own
//     asset, so every leg is shown and their base-currency values sum.
//
// The single-vs-multiple-commodity test cleanly separates the two.

import type { Posting, Transaction } from "./types";
import { matchesPrefix } from "./meta";

/** Postings of `t` whose account is at or below the selected account scope. */
export function inScopeLegs(t: Transaction, account: string | undefined): Posting[] {
  if (!account) return [];
  return t.postings.filter((p) => matchesPrefix(p.account, account));
}

/**
 * The posting leg(s) a transaction row should display for `account`:
 *  - 0 in-scope postings → [] (caller renders a zero amount).
 *  - 1 in-scope posting → that posting.
 *  - 2+ in-scope postings, one commodity → the first leg only (transfer case).
 *  - 2+ in-scope postings, multiple commodities → every leg (multi-commodity).
 */
export function representativeLegs(t: Transaction, account: string | undefined): Posting[] {
  const matching = inScopeLegs(t, account);
  if (matching.length <= 1) return matching;
  const commodities = new Set(matching.map((p) => p.commodity));
  return commodities.size <= 1 ? [matching[0]] : matching;
}

/**
 * The posting leg(s) a row displays for `account`: the in-scope representative
 * legs, or — when none are in scope — the transaction's first posting. The
 * fallback surfaces a revealed ignored row (both legs moved to `ignore:*`, so
 * nothing matches the account) with the real amount it moved instead of a 0.
 */
export function displayLegs(t: Transaction, account: string | undefined): Posting[] {
  const legs = representativeLegs(t, account);
  return legs.length > 0 ? legs : t.postings.slice(0, 1);
}

/**
 * The posting leg(s) whose base-currency values should be SUMMED to value a
 * transaction row for `account`. Refines {@link representativeLegs}: a
 * multi-commodity holding only sums to a meaningful number when its legs move
 * the same way.
 *
 *  - Co-held holdings (all legs the same direction, e.g. a property's separate
 *    land + building legs) keep every leg, so their values add up.
 *  - A swap (mixed-direction legs, e.g. a broker trade whose cash sub-account
 *    is in scope — shares in / cash out) keeps only the headline (first-leg)
 *    side. Summing opposite sides nets the acquired asset against its cash
 *    consideration, collapsing a real trade to a meaningless ~zero / mark-to-
 *    market markup instead of its value.
 */
export function valuationLegs(t: Transaction, account: string | undefined): Posting[] {
  const legs = representativeLegs(t, account);
  if (legs.length <= 1) return legs;
  const headingOut = legs[0].amount < 0;
  const sameSide = legs.filter((p) => (p.amount < 0) === headingOut);
  return sameSide.length === legs.length ? legs : sameSide;
}
