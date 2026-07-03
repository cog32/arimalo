import type { AccountBalance, CommodityAmount } from "./types";
import { formatMoneyWhole } from "./format";

export const ROOT_FOLDER_PATH = "assets";

type TreeLike = {
  name: string;
  fullPath: string;
  isLeaf: boolean;
  children: TreeLike[];
};

function find(tree: TreeLike[], fullPath: string): TreeLike | undefined {
  const parts = fullPath.split(":");
  let cur: TreeLike | undefined = tree.find((r) => r.name === parts[0]);
  for (let i = 1; cur && i < parts.length; i++) {
    cur = cur.children.find((c) => c.name === parts[i]);
  }
  return cur;
}

export function isFolderSelection(selectedAccount: string | undefined, tree: TreeLike[]): boolean {
  if (!selectedAccount) return false;
  const node = find(tree, selectedAccount);
  if (!node) return false;
  return node.children.length > 0 || !node.isLeaf;
}

export function folderDisplayName(folderPath: string, accountSetLabel: string): string {
  if (folderPath === ROOT_FOLDER_PATH) return accountSetLabel;
  const parts = folderPath.split(":");
  return parts[parts.length - 1];
}

export function folderTotalAud(
  folderPath: string | undefined,
  treeBaseTotals: Map<string, number>,
): number | undefined {
  if (!folderPath) return undefined;
  return treeBaseTotals.get(folderPath);
}

/** Header total for a folder selection. Sums the base-currency total of every
 * visible immediate child in the sidebar tree, so the displayed header equals
 * the sum of the rows the user can see — even when some leaves are missing
 * from the filtered tree (e.g. non-folder-backed accounts that exist in raw
 * balances but never appear in the sidebar). Returns undefined when the
 * folder isn't in the tree or has no children. */
export function folderHeaderTotal(
  folderPath: string,
  tree: TreeLike[],
  treeBaseTotals: Map<string, number>,
): number | undefined {
  const node = find(tree, folderPath);
  if (!node || node.children.length === 0) return undefined;
  let sum = 0;
  let any = false;
  for (const child of node.children) {
    const t = treeBaseTotals.get(child.fullPath);
    if (t === undefined) continue;
    sum += t;
    any = true;
  }
  return any ? sum : undefined;
}

const CURRENCY_SYMBOLS: Record<string, string> = {
  AUD: "$", USD: "$", NZD: "$", CAD: "$", SGD: "$", HKD: "$",
  EUR: "€", GBP: "£", JPY: "¥", CNY: "¥", CNH: "¥",
};

/** Formats a folder-level balance with thousand separators and a leading
 * currency symbol when the commodity is a known fiat code. Unknown commodities
 * get the number alone — callers render the commodity ticker separately. */
export function formatFolderAmount(amount: number, commodity: string): string {
  const symbol = CURRENCY_SYMBOLS[commodity] ?? "";
  const sign = amount < 0 ? "-" : "";
  return `${sign}${symbol}${formatMoneyWhole(Math.abs(amount))}`;
}

/** Synthesizes an AccountBalance for a folder node by summing commodity totals
 * across every leaf account beneath the folder prefix. Returns undefined when
 * the folder has no descendant leaves (i.e. the path is itself a leaf account). */
export function syntheticFolderBalance(
  folderPath: string,
  balances: AccountBalance[],
): AccountBalance | undefined {
  const prefix = folderPath + ":";
  const totals = new Map<string, number>();
  let matched = false;
  for (const b of balances) {
    if (b.account === folderPath) continue;
    if (!b.account.startsWith(prefix)) continue;
    matched = true;
    for (const t of b.totals) {
      totals.set(t.commodity, (totals.get(t.commodity) ?? 0) + t.amount);
    }
  }
  if (!matched) return undefined;
  const result: CommodityAmount[] = [...totals.entries()].map(([commodity, amount]) => ({ commodity, amount }));
  return { account: folderPath, totals: result };
}
