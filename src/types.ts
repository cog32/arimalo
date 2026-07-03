/** Shared types — single source of truth for types used across modules. */

// ── Core data types ──

export type Diagnostic = {
  line: number;
  column: number;
  message: string;
  file?: string;
};

export type Posting = {
  account: string;
  amount: number;
  amount_text?: string;
  commodity: string;
  remainder?: string | null;
  price?: { is_total: boolean; amount: number; amount_text: string; commodity: string } | null;
  cost?: { is_total: boolean; amount?: number; commodity?: string } | null;
};

export type Transaction = {
  date: string;
  datetime: string;
  status?: string | null;
  payee?: string | null;
  narration?: string | null;
  meta?: string | null;
  postings: Posting[];
  display_payee?: string | null;
  amount: number;
  amount_commodity: string;
  display_amount_commodity?: string | null;
  fee?: number | null;
  fee_commodity?: string | null;
};

export type CommodityAmount = {
  commodity: string;
  amount: number;
};

export type AccountBalance = {
  account: string;
  totals: CommodityAmount[];
};

export type AccountProperties = {
  name?: string | null;
};

// ── Trade types ──

export type TradeLink = { id: string; txn_id_a: string; txn_id_b: string };
export type TradeSuggestion = { txn_id_a: string; txn_id_b: string; summary: string };

// ── Parse / Pipeline types ──

export type ParseResponse = {
  ok: boolean;
  diagnostics: Diagnostic[];
  transactions: Transaction[];
  balances: AccountBalance[];
  accounts_with_opening: string[];
  account_properties: Record<string, AccountProperties>;
};

export type QueryResult = {
  transactions: Transaction[];
  balances: AccountBalance[];
  aggregated_balance: CommodityAmount[];
  accounts: string[];
  transaction_count: number;
};

export type PipelineResult = {
  csv_transformed: number;
  csv_cached: number;
  manual_count: number;
  total_written: number;
  output_files_written: number;
  warnings: string[];
  owner_accounts: Record<string, string[]>;
  account_folders: Record<string, string>;
  account_properties: Record<string, AccountProperties>;
};

export type PipelineResponse = {
  result: PipelineResult;
  parse: ParseResponse;
  warnings: string[];
};

// ── Report types ──

export type CgtEvent = {
  sell_date: string;
  buy_date: string;
  commodity: string;
  quantity: number;
  cost_basis: number;
  sale_proceeds: number;
  capital_gain: number;
  holding_days: number;
  discount_eligible: boolean;
  discounted_gain: number;
  trade_link_id: string;
  sell_txn_id: string;
  sell_account: string;
};

export type CgtReport = {
  financial_year: string;
  events: CgtEvent[];
  total_gains: number;
  total_losses: number;
  short_term_gains: number;
  long_term_gains: number;
  net_capital_gain: number;
  total_discounted_gain: number;
  warnings: string[];
};

export type TaxCategory = {
  account: string;
  total: number;
  base_currency: string;
};

export type IncomeEvent = {
  date: string;
  account: string;
  commodity: string;
  quantity: number;
  price: number;
  value: number;
  base_currency: string;
  txn_id: string;
  asset_account: string;
};

export type IncomeTaxReport = {
  financial_year: string;
  income_categories: TaxCategory[];
  expense_categories: TaxCategory[];
  events: IncomeEvent[];
  expense_events: IncomeEvent[];
  total_income: number;
  total_expenses: number;
  net: number;
  warnings: string[];
};

export type CommodityAccountBalance = {
  account: string;
  quantity: number;
  value: number;
};

export type CoinBalance = {
  commodity: string;
  quantity: number;
  price: number;
  price_date: string;
  value: number;
  portfolio_weight: number;
  accounts: CommodityAccountBalance[];
};

export type BalancesReport = {
  as_of_date: string;
  base_currency: string;
  base_account_scope: string | null;
  holdings: CoinBalance[];
  total_value: number;
  warnings: string[];
};

// One FIFO acquisition lot still held — the Tax Savings per-parcel drill-down.
export type LotDetail = {
  acquisition_date: string;
  quantity: number;
  cost_per_unit: number;
  cost_basis: number;
  value: number;
  unrealised: number;
};

// Performance report — mirrors src-tauri/src/reports.rs structs (snake_case).
export type CommodityHolding = {
  commodity: string;
  quantity: number;
  cost_basis: number;
  price: number;
  price_date: string;
  value: number;
  unrealised: number;
  has_price: boolean;
  lots: LotDetail[];
};

export type PerformancePoint = {
  date: string;
  label: string;
  realised_gain: number;
  income: number;
  portfolio_value: number;
  cost_basis: number;
  unrealised_gain: number;
  unrealised_change: number;
};

export type AccountValueSeries = {
  account: string;
  values: number[];
};

export type CommodityPerformance = {
  commodity: string;
  realised_gain: number;
  unrealised_change: number;
  total: number;
  closing_value: number;
  closing_unrealised: number;
};

export type PerformanceReport = {
  label: string;
  date_from: string;
  date_to: string;
  base_currency: string;
  base_account_scope: string | null;
  points: PerformancePoint[];
  closing_holdings: CommodityHolding[];
  attribution: CommodityPerformance[];
  total_realised_gain: number;
  total_income: number;
  unrealised_change: number;
  value_open: number;
  closing_value: number;
  closing_cost_basis: number;
  total_return: number;
  total_return_pct: number | null;
  account_breakdown: AccountValueSeries[];
  warnings: string[];
};

// Tax Savings (loss-harvesting) report — mirrors src-tauri/src/reports.rs.
export type LossPosition = {
  commodity: string;
  quantity: number;
  cost_basis: number;
  value: number;
  unrealised_loss: number;
  pct_below_cost: number;
  price: number;
  price_date: string;
  lots: LotDetail[];
};

export type UnderwaterParcel = {
  commodity: string;
  acquisition_date: string;
  quantity: number;
  cost_per_unit: number;
  cost_basis: number;
  value: number;
  unrealised_loss: number;
  pct_below_cost: number;
};

export type LossHarvestReport = {
  as_of_date: string;
  financial_year: string;
  base_currency: string;
  base_account_scope: string | null;
  positions: LossPosition[];
  total_realisable_loss: number;
  realised_net_gain: number;
  realised_short_gains: number;
  realised_long_gains: number;
  offset_now: number;
  carry_forward: number;
  marginal_rate_percent: number;
  cgt_discount_percent: number;
  estimated_tax_saved: number;
  underwater_parcels: UnderwaterParcel[];
  total_parcel_loss: number;
  warnings: string[];
};

export type TaxConfig = {
  financial_year_end_month: number;
  financial_year_end_day: number;
  cgt_discount_percent: number;
  cgt_discount_holding_months: number;
  non_taxable_accounts: string[];
  non_deductible_accounts: string[];
  marginal_tax_rate_percent: number;
};

// ── Sort types ──

export type SortDirection = "asc" | "desc";
export type GenericSortState<C extends string> = { column: C; direction: SortDirection };

export type CgtSortColumn = "sell_date" | "buy_date" | "commodity" | "quantity"
  | "cost_basis" | "sale_proceeds" | "capital_gain" | "holding_days";
export type IncomeSortColumn =
  | "date" | "account" | "commodity" | "quantity" | "price" | "value";
export type BalancesSortColumn = "commodity" | "quantity" | "price" | "value" | "portfolio_weight";

export type SortColumn = "date" | "party" | "notes" | "category" | "amount";
export type SortState = GenericSortState<SortColumn>;
