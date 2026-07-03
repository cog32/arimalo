/** Tree-level base currency total computation — converts leaf balances and aggregates upward. */

import type { AccountBalance } from "./types";

type ConversionIndex = { balanceIdx: number; totalIdx: number };
type ConversionRequest = { commodity: string; amount: number; datetime: string };

/** Build flat conversion request arrays from all leaf balance totals. */
function buildConversionRequests(
  balances: AccountBalance[],
  datetime: string,
): { indexed: ConversionIndex[]; requests: ConversionRequest[] } {
  const indexed: ConversionIndex[] = [];
  const requests: ConversionRequest[] = [];
  for (let bi = 0; bi < balances.length; bi++) {
    for (let ti = 0; ti < balances[bi].totals.length; ti++) {
      const t = balances[bi].totals[ti];
      indexed.push({ balanceIdx: bi, totalIdx: ti });
      requests.push({ commodity: t.commodity, amount: Math.abs(t.amount), datetime });
    }
  }
  return { indexed, requests };
}

/** Compute per-leaf totals then aggregate upward through the account hierarchy. */
function aggregateConvertedTotals(
  balances: AccountBalance[],
  indexed: ConversionIndex[],
  values: (number | null)[],
): Map<string, number> {
  // Compute per-leaf-account base currency total
  const leafTotals = new Map<string, number>();
  for (let i = 0; i < indexed.length; i++) {
    const { balanceIdx, totalIdx } = indexed[i];
    const balance = balances[balanceIdx];
    const originalAmount = balance.totals[totalIdx].amount;
    const converted = values[i];
    if (converted !== null) {
      const signed = originalAmount < 0 ? -converted : converted;
      leafTotals.set(balance.account, (leafTotals.get(balance.account) ?? 0) + signed);
    }
  }

  // Walk account hierarchy to aggregate upward
  const allTotals = new Map<string, number>();
  for (const [account, total] of leafTotals) {
    allTotals.set(account, total);
    const parts = account.split(":");
    for (let i = 1; i < parts.length; i++) {
      const ancestor = parts.slice(0, i).join(":");
      allTotals.set(ancestor, (allTotals.get(ancestor) ?? 0) + total);
    }
  }
  return allTotals;
}

/** Load base-currency totals for the full account tree. Returns undefined if not available. */
export async function loadTreeBaseCurrencyTotals(
  baseCurrency: string | undefined,
  balances: AccountBalance[],
  invoker: <T>(cmd: string, args: Record<string, unknown>) => Promise<T>,
): Promise<Map<string, number> | undefined> {
  if (!baseCurrency || balances.length === 0) return undefined;

  const now = new Date().toISOString().slice(0, 19);
  const { indexed, requests } = buildConversionRequests(balances, now);
  if (requests.length === 0) return undefined;

  try {
    const result = await invoker<{ values: (number | null)[] }>("convert_to_base_currency", {
      baseCurrency, requests,
    });
    return aggregateConvertedTotals(balances, indexed, result.values);
  } catch {
    return undefined;
  }
}
