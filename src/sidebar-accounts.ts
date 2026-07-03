import type { AccountBalance } from "./types";

/** The sidebar only exposes folder-backed asset accounts for the active set.
 * Parent groups are derived by the tree builder from these leaf balances. */
export function filterSidebarBalances(
  allBalances: AccountBalance[],
  accountFoldersMap: Record<string, string>,
  accountSetMap: Record<string, string[]>,
  selectedAccountSet?: string,
): AccountBalance[] {
  const folderBackedAccounts = new Set(Object.keys(accountFoldersMap));
  const assetBalances = allBalances.filter((b) => b.account.startsWith("assets:"));

  if (!selectedAccountSet) {
    return assetBalances.filter((b) => folderBackedAccounts.has(b.account));
  }

  const ownedAccounts = new Set(accountSetMap[selectedAccountSet] ?? []);
  return assetBalances.filter((b) =>
    ownedAccounts.has(b.account) && folderBackedAccounts.has(b.account)
  );
}

/** The complement of {@link filterSidebarBalances}: the "Categories" pane exposes
 * every account NOT backed by a source folder — `income:*`, `expenses:*`,
 * `equity:*`, `liabilities:*`, and the synthetic `assets` contras (`assets:staking`,
 * `assets:lending`, `assets:transfer`, `assets:crypto:bridge`, …). `ignore:*` is
 * excluded unless "Show Ignored" is on. Cross-set by design — these nominal/contra
 * accounts are not partitioned by owner, matching the union query that backs them. */
export function filterCategoryBalances(
  allBalances: AccountBalance[],
  accountFoldersMap: Record<string, string>,
  showHidden: boolean,
): AccountBalance[] {
  const folderBackedAccounts = new Set(Object.keys(accountFoldersMap));
  return allBalances.filter((b) =>
    !folderBackedAccounts.has(b.account) &&
    (showHidden || !b.account.startsWith("ignore:"))
  );
}
