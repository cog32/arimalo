// Candidate list for account-name autocomplete — shared by the rule editor's
// Amount/Fee Account fields, the inline category-cell editor, and the Add
// Account modal.
//
// Suggestions are GLOBAL by design: every account known anywhere in the vault
// is offered, not just accounts with a balance in the currently selected set.
// That way a category you've used in one account — e.g. income:dividends,
// booked only against the CBA CDIA account — still autocompletes when you edit
// a rule in any other account. The global pool comes from the pipeline's
// generated summaries via load_account_tree(""), passed in as `allAccounts`.

export interface AccountSuggestionSources {
  /** owner_accounts from the pipeline (asset accounts, keyed by owner). */
  accountSetMap: Record<string, string[]>;
  /** Closing balances for the currently selected account set. */
  balances?: { account: string }[];
  /** Postings in the currently loaded ledger (empty on the account-tree view). */
  transactions?: { postings: { account: string }[] }[];
  /** Every account across the whole vault (load_account_tree("")). */
  allAccounts?: string[];
}

export function collectAccountSuggestions(src: AccountSuggestionSources): string[] {
  const accounts = new Set<string>();
  for (const names of Object.values(src.accountSetMap)) {
    for (const n of names) accounts.add(n);
  }
  for (const b of src.balances ?? []) accounts.add(b.account);
  for (const t of src.transactions ?? []) {
    for (const p of t.postings) accounts.add(p.account);
  }
  for (const a of src.allAccounts ?? []) accounts.add(a);
  return [...accounts].sort();
}
