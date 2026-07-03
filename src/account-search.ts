// Account search and filtering.
// Scope resolution and transaction filtering by account prefix.

/** Derive the account name prefix for a scope selection.
 *  e.g. accountPrefixForScope("assets:crypto:exchange:kraken:personal", "institution") → "assets:crypto:exchange:kraken" */
export function accountPrefixForScope(accountName: string, scope: "local" | "institution" | "global"): string {
  if (scope === "global") return "";
  if (scope === "institution") {
    const parts = accountName.split(":");
    return parts.length > 2 ? parts.slice(0, -1).join(":") : accountName;
  }
  return accountName;
}

/** Filter transactions where the first posting's account matches the given prefix.
 *  Used by both the accounts search and the rule editor preview. */
export function filterByAccountPrefix<T extends { postings: { account: string }[] }>(
  txns: T[],
  accountPrefix: string,
): T[] {
  if (!accountPrefix) return txns;
  return txns.filter((t) => {
    const acct = t.postings[0]?.account ?? "";
    return acct === accountPrefix || acct.startsWith(accountPrefix + ":");
  });
}
