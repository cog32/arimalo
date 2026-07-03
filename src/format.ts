// Amount formatting helpers.
// Smart is the default: at least 2 significant digits after the decimal.
// Per-commodity decimals in displayConfig override smart formatting.

export type DisplayConfig = {
  commodities: Record<string, { decimals: number }>;
  default_decimals: number;
  base_currency?: string;
  default_account?: string;
};

export function formatAmount(amount: number, commodity: string, cfg?: DisplayConfig | null): string {
  const override = cfg?.commodities?.[commodity]?.decimals;
  if (override === undefined) return formatAmountSmart(amount);
  const result = amount.toFixed(override);
  if (amount !== 0 && parseFloat(result) === 0) return formatAmountSmart(amount);
  return result;
}

/** At least 2 significant digits after the decimal point.
 *  e.g. 123.45 → "123.45", 0.0034 → "0.0034", 0.00001 → "0.000010" */
export function formatAmountSmart(amount: number): string {
  if (amount === 0) return "0.00";
  const abs = Math.abs(amount);
  if (abs >= 0.01) return amount.toFixed(2);
  const decimals = Math.min(Math.ceil(-Math.log10(abs)) + 1, 10);
  return amount.toFixed(decimals);
}

// === Currency total formatting ===
// Single source of truth for fiat money TOTALS (report headers, summaries,
// table cells, chart tooltips, folder totals). Adds thousand separators.
// NOT for commodity quantities — use formatAmount for those (crypto precision).

/** Money with thousand separators and a fixed number of decimals.
 *  e.g. formatMoney(1150774.25) → "1,150,774.25", formatMoney(-16.4, 2) → "-16.40" */
export function formatMoney(amount: number, decimals = 2): string {
  const value = amount === 0 ? 0 : amount; // collapse -0 → "0"
  return new Intl.NumberFormat("en-US", {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  }).format(value);
}

/** Whole-dollar money (no decimals) with thousand separators, for headline /
 *  summary figures. e.g. formatMoneyWhole(1150774.25) → "1,150,774" */
export function formatMoneyWhole(amount: number): string {
  return formatMoney(amount, 0);
}
