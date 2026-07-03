use crate::ledger_parser::{Posting, PriceGraph, Transaction};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

const EPSILON: f64 = 1e-10;

/// Default marginal tax rate (%) used by the Tax Savings report when a vault's
/// config predates the field. 47 = top AU marginal rate incl. Medicare levy.
fn default_marginal_rate() -> u32 {
    47
}

/// Tax configuration stored in config.json under "tax" key.
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
pub struct TaxConfig {
    pub financial_year_end_month: u32,
    pub financial_year_end_day: u32,
    pub cgt_discount_percent: u32,
    pub cgt_discount_holding_months: u32,
    #[serde(default)]
    pub non_taxable_accounts: Vec<String>,
    #[serde(default)]
    pub non_deductible_accounts: Vec<String>,
    /// Marginal tax rate (%) applied to the offsettable capital-loss reduction
    /// in the Tax Savings (loss-harvesting) report to estimate dollars saved.
    #[serde(default = "default_marginal_rate")]
    pub marginal_tax_rate_percent: u32,
}

impl Default for TaxConfig {
    fn default() -> Self {
        Self {
            financial_year_end_month: 6,
            financial_year_end_day: 30,
            cgt_discount_percent: 50,
            cgt_discount_holding_months: 12,
            non_taxable_accounts: Vec::new(),
            non_deductible_accounts: Vec::new(),
            marginal_tax_rate_percent: default_marginal_rate(),
        }
    }
}

/// A single CGT disposal event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CgtEvent {
    pub sell_date: String,
    pub buy_date: String,
    pub commodity: String,
    pub quantity: f64,
    pub cost_basis: f64,
    pub sale_proceeds: f64,
    pub capital_gain: f64,
    pub holding_days: i64,
    pub discount_eligible: bool,
    pub discounted_gain: f64,
    pub trade_link_id: String,
    /// Transaction ID of the sell transaction (from meta txn:xxx).
    pub sell_txn_id: String,
    /// Account that held the disposed asset.
    pub sell_account: String,
}

/// Full CGT report for a financial year.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CgtReport {
    pub financial_year: String,
    pub events: Vec<CgtEvent>,
    pub total_gains: f64,
    pub total_losses: f64,
    /// Gains on events held <12mo (`!discount_eligible && gain>0`).
    pub short_term_gains: f64,
    /// Gains on events held ≥12mo (`discount_eligible && gain>0`).
    pub long_term_gains: f64,
    pub net_capital_gain: f64,
    pub total_discounted_gain: f64,
    pub warnings: Vec<String>,
}

/// A single income/expense category line.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaxCategory {
    pub account: String,
    pub total: f64,
    pub base_currency: String,
}

/// A single income or expense event — one posting to an `income:*` /
/// `expenses:*` account, exposed in the report so the UI can group by asset
/// (commodity) and show date / quantity / price / value line items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomeEvent {
    pub date: String,
    pub account: String,
    pub commodity: String,
    pub quantity: f64,
    pub price: f64,
    pub value: f64,
    pub base_currency: String,
    pub txn_id: String,
    pub asset_account: String,
}

/// Full income tax report for a financial year.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomeTaxReport {
    pub financial_year: String,
    pub income_categories: Vec<TaxCategory>,
    pub expense_categories: Vec<TaxCategory>,
    pub events: Vec<IncomeEvent>,
    pub expense_events: Vec<IncomeEvent>,
    pub total_income: f64,
    pub total_expenses: f64,
    pub net: f64,
    pub warnings: Vec<String>,
}

/// Per-leaf-account contribution to a commodity holding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalance {
    pub account: String,
    pub quantity: f64,
    pub value: f64,
}

/// A single per-commodity holding line in the Balances report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoinBalance {
    pub commodity: String,
    pub quantity: f64,
    /// Per-unit spot price in `base_currency` used to value this holding.
    pub price: f64,
    /// Datetime of the P-directive the price was sourced from (ISO). For the
    /// identity case (commodity == base_currency) this is the report's as-of
    /// date.
    pub price_date: String,
    /// `quantity * price`, in `base_currency`.
    pub value: f64,
    /// Share of `total_value` held in this commodity (0.0 – 1.0).
    pub portfolio_weight: f64,
    /// Leaf-account breakdown — quantities sum to `quantity`. Sorted by
    /// |quantity| desc.
    pub accounts: Vec<AccountBalance>,
}

/// Point-in-time portfolio snapshot: per-commodity quantities and their AUD
/// (or base-currency) valuations as of a given date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalancesReport {
    pub as_of_date: String,
    pub base_currency: String,
    pub base_account_scope: Option<String>,
    pub holdings: Vec<CoinBalance>,
    pub total_value: f64,
    pub warnings: Vec<String>,
}

/// One FIFO acquisition lot still held, valued at the holding's spot price.
/// The building block for the Tax Savings report's per-parcel drill-down.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LotDetail {
    /// Acquisition date of the parcel (FIFO key).
    pub acquisition_date: String,
    pub quantity: f64,
    pub cost_per_unit: f64,
    /// `quantity × cost_per_unit`, in base currency.
    pub cost_basis: f64,
    /// `quantity × price` (the holding's spot), in base currency.
    pub value: f64,
    /// `value − cost_basis`: positive = this parcel is in gain, negative =
    /// underwater. Under FIFO the *oldest* parcels are disposed first, so a
    /// position can be net-underwater while its earliest parcels sit in gain.
    pub unrealised: f64,
}

/// A single per-commodity holding with FIFO cost basis and mark-to-market
/// valuation, as of a point in time. Used by the Performance report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommodityHolding {
    pub commodity: String,
    pub quantity: f64,
    /// Σ remaining-lot (quantity × cost_per_unit), in base currency.
    pub cost_basis: f64,
    /// Per-unit spot price in base currency; 0.0 when unpriced.
    pub price: f64,
    /// Datetime of the P-directive used, or the as-of date when carried/identity.
    pub price_date: String,
    /// `quantity * price` when priced; carried at `cost_basis` when unpriced.
    pub value: f64,
    /// `value − cost_basis`; 0.0 when unpriced (carried at cost).
    pub unrealised: f64,
    /// false → no price found; value carried at cost, unrealised unreliable.
    pub has_price: bool,
    /// FIFO remaining lots, oldest-first (disposal order). Empty when unpriced
    /// or for a net short with no lots.
    pub lots: Vec<LotDetail>,
}

/// Point-in-time FIFO holdings snapshot: current (still-held) lots, their cost
/// basis, and mark-to-market value as of a date. Building block for the
/// Performance report's value-vs-cost-basis series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoldingsSnapshot {
    pub as_of_date: String,
    pub base_currency: String,
    pub holdings: Vec<CommodityHolding>,
    pub total_value: f64,
    pub total_cost_basis: f64,
    pub total_unrealised: f64,
    pub warnings: Vec<String>,
}

/// One month-end point in a Performance report time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformancePoint {
    /// Snapshot date (month-end "YYYY-MM-DD", or the exact window end).
    pub date: String,
    /// Human label, e.g. "Jun 2026".
    pub label: String,
    /// Realised capital gain booked in this month (raw, pre-discount).
    pub realised_gain: f64,
    /// Income (dividends/staking/interest) booked in this month.
    pub income: f64,
    /// Mark-to-market value of held positions (ex-cash) at the snapshot.
    pub portfolio_value: f64,
    /// FIFO cost basis of held positions at the snapshot.
    pub cost_basis: f64,
    /// `portfolio_value − cost_basis` (embedded paper gain at the snapshot).
    pub unrealised_gain: f64,
    /// Change in unrealised gain vs the previous snapshot — this month's
    /// mark-to-market contribution to performance. Monthly changes telescope to
    /// the window's total `unrealised_change`.
    pub unrealised_change: f64,
}

/// Per-direct-child market-value series for the Performance "growth by category"
/// chart: one entry per account one level below `base_account_scope`, aligned
/// 1:1 with [`PerformanceReport::points`]. Carries RAW values; the frontend
/// rebases each to a 0%-at-open cumulative-growth line so differently-sized
/// categories compare on a single percentage axis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountValueSeries {
    /// Child-group account key, e.g. `"assets:crypto"` (scope `"assets"`) or, at
    /// the top level, `"assets"` / `"equity"` / `"liabilities"`.
    pub account: String,
    /// Market value at each snapshot; `values[i]` corresponds to `points[i]`
    /// (base-currency cash at face, priced commodities at market, unpriced
    /// dropped — matching the Balances report).
    pub values: Vec<f64>,
}

/// Per-commodity contribution to the window's performance: realised capital
/// gain booked in the window plus the change in unrealised gain (close −
/// open). `Σ total + total_income` reconciles to `total_return`, so this shows
/// *where* the return came from — distinct from the closing unrealised *level*.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommodityPerformance {
    pub commodity: String,
    /// Realised capital gain booked on this commodity over the window (raw).
    pub realised_gain: f64,
    /// Change in unrealised gain on this commodity (closing − opening).
    pub unrealised_change: f64,
    /// `realised_gain + unrealised_change` — total window contribution.
    pub total: f64,
    /// Mark-to-market value of this commodity still held at the window close.
    pub closing_value: f64,
    /// Embedded unrealised gain still in the holding at close (level, context).
    pub closing_unrealised: f64,
}

/// A performance report over an arbitrary window (default 12 months): realised
/// capital gains + income flows bucketed by month, plus mark-to-market value vs
/// FIFO cost basis at each month-end. Computed live (not cached).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceReport {
    pub label: String,
    pub date_from: String,
    pub date_to: String,
    pub base_currency: String,
    pub base_account_scope: Option<String>,
    /// Chronological month-end points across the window.
    pub points: Vec<PerformancePoint>,
    /// Per-commodity holdings at the window end (drill-down).
    pub closing_holdings: Vec<CommodityHolding>,
    /// Per-commodity attribution of the window's return (realised +
    /// Δunrealised), sorted by total contribution. `Σ total + total_income =
    /// total_return` — shows where the performance came from.
    pub attribution: Vec<CommodityPerformance>,
    /// Σ realised capital gain over the window (net of losses).
    pub total_realised_gain: f64,
    /// Σ income over the window.
    pub total_income: f64,
    /// Change in unrealised gain over the window (closing − opening). The
    /// lifetime embedded gain in current holdings is intentionally NOT here —
    /// that's a tax/CGT concern, not period performance.
    pub unrealised_change: f64,
    /// Mark-to-market value of holdings at the window open (prior-period close).
    pub value_open: f64,
    /// Mark-to-market value of holdings as of `date_to` (window close).
    pub closing_value: f64,
    pub closing_cost_basis: f64,
    /// Headline period return = `total_realised_gain + total_income +
    /// unrealised_change`. A dollar P&L for the window — not a time-weighted
    /// return (it doesn't weight when capital was added).
    pub total_return: f64,
    /// `total_return / value_open`; None when opening value ≈ 0.
    pub total_return_pct: Option<f64>,
    /// Per-direct-child market-value series one level below `base_account_scope`,
    /// aligned with `points`. Empty when the scope has no qualifying child
    /// groups. Feeds the "growth by category" chart (frontend rebases to %).
    pub account_breakdown: Vec<AccountValueSeries>,
    pub warnings: Vec<String>,
}

/// A single underwater holding in the Tax Savings (loss-harvesting) report: a
/// position whose mark-to-market value sits below its FIFO cost basis, so
/// selling it would realise a capital loss.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LossPosition {
    pub commodity: String,
    pub quantity: f64,
    /// FIFO remaining-lot cost basis, in base currency.
    pub cost_basis: f64,
    /// Current mark-to-market value (`quantity * price`), in base currency.
    pub value: f64,
    /// Positive magnitude of the unrealised loss (`cost_basis − value`).
    pub unrealised_loss: f64,
    /// `unrealised_loss / cost_basis` (0.0 – 1.0); 0.0 when cost_basis ≤ 0.
    pub pct_below_cost: f64,
    /// Per-unit spot price used to value the holding.
    pub price: f64,
    pub price_date: String,
    /// FIFO remaining parcels, oldest-first — the per-lot drill-down.
    pub lots: Vec<LotDetail>,
}

/// A single FIFO parcel that is currently below its cost, in the Tax Savings
/// parcel-scan view. Unlike [`LossPosition`] (whole-position, net), these are
/// listed across ALL holdings — including ones that are net in gain — so
/// underwater parcels hidden inside a profitable position are surfaced.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnderwaterParcel {
    pub commodity: String,
    pub acquisition_date: String,
    pub quantity: f64,
    pub cost_per_unit: f64,
    pub cost_basis: f64,
    pub value: f64,
    /// Positive magnitude of the parcel's unrealised loss (`cost_basis − value`).
    pub unrealised_loss: f64,
    /// `unrealised_loss / cost_basis` (0.0 – 1.0).
    pub pct_below_cost: f64,
}

/// Tax Savings (loss-harvesting) report: current underwater holdings plus an
/// estimate of the tax saved by realising those losses against the financial
/// year's realised capital gains. Composed from `holdings_as_of` (the
/// positions) and the CGT report (the gains the losses offset). Computed live.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LossHarvestReport {
    pub as_of_date: String,
    /// FY label or "from to" range label, mirroring the CGT report.
    pub financial_year: String,
    pub base_currency: String,
    pub base_account_scope: Option<String>,
    /// Underwater holdings, largest loss first.
    pub positions: Vec<LossPosition>,
    /// Σ `unrealised_loss` across positions — the total realisable capital loss.
    pub total_realisable_loss: f64,
    /// This FY's realised net capital gain (gains − losses, pre-discount).
    pub realised_net_gain: f64,
    /// Gross realised gains on parcels held <12mo (taxed in full).
    pub realised_short_gains: f64,
    /// Gross realised gains on parcels held ≥12mo (CGT-discount eligible).
    pub realised_long_gains: f64,
    /// Portion of the harvestable loss that offsets gains still standing this
    /// year (after existing realised losses) — the part that saves tax now.
    pub offset_now: f64,
    /// Portion exceeding available gains; carries forward (no benefit this year).
    pub carry_forward: f64,
    /// Marginal tax rate (%) used for the estimate.
    pub marginal_rate_percent: u32,
    /// CGT discount (%) used to halve the long-term offset.
    pub cgt_discount_percent: u32,
    /// Estimated tax saved this year = `(offset_short + discount_factor *
    /// offset_long) * marginal_rate`.
    pub estimated_tax_saved: f64,
    /// Every FIFO parcel below its cost, across ALL in-scope holdings (not just
    /// the net-underwater `positions`), largest loss first — the parcel-scan
    /// view that surfaces harvestable losses inside net-positive holdings.
    pub underwater_parcels: Vec<UnderwaterParcel>,
    /// Σ `unrealised_loss` across `underwater_parcels` (gross, pre-netting).
    pub total_parcel_loss: f64,
    pub warnings: Vec<String>,
}

/// Trade link with two transaction IDs (from MetadataStore).
#[derive(Debug, Default, Clone)]
pub struct TradeLinkRef {
    pub id: String,
    pub txn_id_a: String,
    pub txn_id_b: String,
}

// === FIFO inventory types ===

/// A single acquisition lot in the FIFO queue.
#[derive(Debug, Clone)]
struct Lot {
    quantity: f64,
    cost_per_unit: f64,
    acquisition_date: String,
    txn_id: String,
}

/// Per-commodity FIFO inventory.
#[derive(Debug, Default)]
struct CommodityInventory {
    lots: VecDeque<Lot>,
}

impl CommodityInventory {
    /// Add a new lot to the back of the queue.
    fn push(&mut self, lot: Lot) {
        self.lots.push_back(lot);
    }

    /// Consume lots FIFO from the front. Returns consumed portions and the
    /// total quantity actually consumed (may be less than requested).
    fn consume_fifo(&mut self, quantity: f64) -> (Vec<Lot>, f64) {
        let mut remaining = quantity;
        let mut consumed = Vec::new();
        while remaining > EPSILON && !self.lots.is_empty() {
            let lot = self.lots.front_mut().unwrap();
            let consumed_qty = lot.quantity.min(remaining);
            consumed.push(Lot {
                quantity: consumed_qty,
                cost_per_unit: lot.cost_per_unit,
                acquisition_date: lot.acquisition_date.clone(),
                txn_id: lot.txn_id.clone(),
            });
            lot.quantity -= consumed_qty;
            if lot.quantity < EPSILON {
                self.lots.pop_front();
            }
            remaining -= consumed_qty;
        }
        let total_consumed = quantity - remaining;
        (consumed, total_consumed)
    }
}

// === Helpers ===

/// Compute the FY date range.  For Australian FY "2025" with end month=6, day=30:
/// start = 2025-07-01, end = 2026-06-30.
fn fy_date_range(fy: &str, tax_config: &TaxConfig) -> (String, String) {
    let fy_year: i32 = fy.parse().expect("financial_year must be a valid integer");
    let end_month = tax_config.financial_year_end_month;
    let end_day = tax_config.financial_year_end_day;

    // FY year = the year the FY ends. E.g. FY2025 with end_month=6 → Jul 2024 – Jun 2025
    let start_month = end_month + 1;
    let (start_year, start_month) = if start_month > 12 {
        (fy_year, 1) // calendar year: Jan–Dec of fy_year
    } else {
        (fy_year - 1, start_month) // split year: starts previous calendar year
    };

    let start = format!("{start_year}-{start_month:02}-01");
    let end = format!("{fy_year}-{end_month:02}-{end_day:02}");

    (start, end)
}

/// Extract the `txn:xxx` id from a transaction's meta field.
fn extract_txn_id(meta: &Option<String>) -> Option<String> {
    meta.as_ref().and_then(|m| {
        m.split(',')
            .map(|p| p.trim())
            .find(|p| p.starts_with("txn:"))
            .map(String::from)
    })
}

/// Extract the `swap:txn:xxx` reference from a transaction's meta field.
/// Returns the referenced txn ID (e.g. "txn:csv-abc123").
fn extract_swap_ref(meta: &Option<String>) -> Option<String> {
    meta.as_ref().and_then(|m| {
        m.split(',')
            .map(|p| p.trim())
            .find(|p| p.starts_with("swap:"))
            .map(|p| p.strip_prefix("swap:").unwrap_or(p).to_string())
    })
}

/// Parse a date string "YYYY-MM-DD" into (year, month, day).
fn parse_date(d: &str) -> Option<(i32, u32, u32)> {
    let parts: Vec<&str> = d.split('-').collect();
    if parts.len() < 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// Days between two dates (approximate).
fn days_between(d1: &str, d2: &str) -> i64 {
    let (y1, m1, day1) = parse_date(d1).unwrap_or((2000, 1, 1));
    let (y2, m2, day2) = parse_date(d2).unwrap_or((2000, 1, 1));
    let days1 = y1 as i64 * 365 + (y1 as i64 / 4) - (y1 as i64 / 100)
        + (y1 as i64 / 400)
        + month_day_offset(m1)
        + day1 as i64;
    let days2 = y2 as i64 * 365 + (y2 as i64 / 4) - (y2 as i64 / 100)
        + (y2 as i64 / 400)
        + month_day_offset(m2)
        + day2 as i64;
    (days2 - days1).abs()
}

fn month_day_offset(m: u32) -> i64 {
    const OFFSETS: [i64; 13] = [0, 0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    OFFSETS.get(m as usize).copied().unwrap_or(0)
}

/// Months between two dates, accounting for the day component.
/// If the sell day is before the buy day, one month is subtracted
/// (e.g. Aug 11 → Aug 10 = 11 months, not 12).
fn months_between(d1: &str, d2: &str) -> i64 {
    let (y1, m1, day1) = parse_date(d1).unwrap_or((2000, 1, 1));
    let (y2, m2, day2) = parse_date(d2).unwrap_or((2000, 1, 1));
    let (early_day, late_day) = if (y2, m2, day2) >= (y1, m1, day1) {
        (day1, day2)
    } else {
        (day2, day1)
    };
    let raw_months = ((y2 as i64 * 12 + m2 as i64) - (y1 as i64 * 12 + m1 as i64)).abs();
    if late_day < early_day {
        (raw_months - 1).max(0)
    } else {
        raw_months
    }
}

/// Check if a transaction is a transfer for a given commodity.
/// A transfer has two or more asset postings in the same commodity that net to zero.
fn is_transfer_for_commodity(txn: &Transaction, commodity: &str) -> bool {
    let asset_amounts: Vec<f64> = txn
        .postings
        .iter()
        .filter(|p| p.account.starts_with("assets:") && p.commodity == commodity)
        .map(|p| p.amount)
        .collect();
    if asset_amounts.len() >= 2 {
        let sum: f64 = asset_amounts.iter().sum();
        sum.abs() < EPSILON
    } else {
        false
    }
}

fn resolve_annotation_value(
    annotation_amount: f64,
    annotation_commodity: &str,
    annotation_is_total: bool,
    quantity: f64,
    price_graph: &PriceGraph,
    datetime: &str,
    base_currency: &str,
) -> f64 {
    let total = if annotation_is_total {
        annotation_amount
    } else {
        annotation_amount * quantity
    };
    if annotation_commodity == base_currency {
        return total;
    }
    price_graph
        .convert_to_base(annotation_commodity, total, datetime, base_currency)
        .unwrap_or(total)
}

/// Convert a posting amount to the base currency.
/// Returns None if conversion is not possible (caller should add a warning).
fn resolve_posting_value(
    posting: &Posting,
    txn: &Transaction,
    price_graph: &PriceGraph,
    base_currency: &str,
) -> Option<f64> {
    let sign = posting.amount.signum();
    let amount = posting.amount.abs();
    // 1. Already in base currency
    if posting.commodity == base_currency {
        return Some(sign * amount);
    }
    // 2. Cost annotation
    if let Some(ref cost) = posting.cost {
        if let Some(cost_amount) = cost.amount {
            let commodity = cost.commodity.as_deref().unwrap_or(base_currency);
            return Some(sign * resolve_annotation_value(
                cost_amount,
                commodity,
                cost.is_total,
                amount,
                price_graph,
                &txn.datetime,
                base_currency,
            ));
        }
    }
    // 3. Price annotation
    if let Some(ref price) = posting.price {
        return Some(sign * resolve_annotation_value(
            price.amount,
            &price.commodity,
            price.is_total,
            amount,
            price_graph,
            &txn.datetime,
            base_currency,
        ));
    }
    // 4. Counterparty posting in base currency
    for p in &txn.postings {
        if p.commodity == base_currency && p.account != posting.account {
            // Use the counterparty's absolute amount, with the original posting's sign
            if (p.amount.signum() != posting.amount.signum()) || p.account.starts_with("equity:") {
                return Some(sign * p.amount.abs());
            }
        }
    }
    // 5. PriceGraph conversion
    if let Some(converted) =
        price_graph.convert_to_base(&posting.commodity, amount, &txn.datetime, base_currency)
    {
        return Some(sign * converted);
    }
    // 6. Cannot convert
    None
}

fn find_equity_swap_sibling<'a>(
    txn: &Transaction,
    sibling_txns: &'a [&'a Transaction],
    equity_leg: &str,
    sibling_leg: &str,
    posting: &Posting,
    amount_sign_positive: bool,
) -> Option<&'a Transaction> {
    let has_leg = txn.postings.iter().any(|p| {
        p.account.starts_with("equity:trading")
            && p.account.contains(equity_leg)
            && p.commodity == posting.commodity
            && if amount_sign_positive {
                p.amount > 0.0
            } else {
                p.amount < 0.0
            }
    });
    if !has_leg {
        return None;
    }

    // Prefer swap-referenced sibling for precise matching (avoids double-matching
    // when multiple fills share a timestamp)
    if let Some(swap_ref) = extract_swap_ref(&txn.meta) {
        for sibling in sibling_txns {
            if let Some(sib_txn_id) = extract_txn_id(&sibling.meta) {
                if sib_txn_id == swap_ref {
                    return Some(sibling);
                }
            }
        }
    }

    // Fallback: match by sorted rank among same-datetime siblings.
    // When multiple fills occur at the same timestamp, pair each current-leg
    // txn with its corresponding target-leg txn by sorting both groups by
    // equity posting amount and matching by rank (smallest↔smallest, etc.).
    let mut current_group: Vec<&Transaction> = Vec::new();
    let mut target_group: Vec<&Transaction> = Vec::new();

    for sibling in sibling_txns {
        if sibling.datetime != txn.datetime {
            continue;
        }
        if sibling.postings.iter().any(|p| {
            p.account.starts_with("equity:trading")
                && p.account.contains(equity_leg)
                && p.commodity == posting.commodity
                && if amount_sign_positive {
                    p.amount > 0.0
                } else {
                    p.amount < 0.0
                }
        }) {
            current_group.push(sibling);
        }
        if sibling.postings.iter().any(|p| {
            p.account.starts_with("equity:trading") && p.account.contains(sibling_leg)
        }) {
            target_group.push(sibling);
        }
    }

    if current_group.len() > 1 && current_group.len() == target_group.len() {
        current_group.sort_by(|a, b| {
            let amt = |t: &&Transaction| {
                t.postings
                    .iter()
                    .find(|p| {
                        p.account.starts_with("equity:trading")
                            && p.account.contains(equity_leg)
                            && p.commodity == posting.commodity
                    })
                    .map(|p| p.amount.abs())
                    .unwrap_or(0.0)
            };
            amt(a).partial_cmp(&amt(b)).unwrap_or(std::cmp::Ordering::Equal)
        });

        target_group.sort_by(|a, b| {
            let amt = |t: &&Transaction| {
                t.postings
                    .iter()
                    .find(|p| {
                        p.account.starts_with("equity:trading")
                            && p.account.contains(sibling_leg)
                    })
                    .map(|p| p.amount.abs())
                    .unwrap_or(0.0)
            };
            amt(a).partial_cmp(&amt(b)).unwrap_or(std::cmp::Ordering::Equal)
        });

        if let Some(rank) =
            current_group
                .iter()
                .position(|s| s.postings.as_ptr() == txn.postings.as_ptr())
        {
            return Some(target_group[rank]);
        }
    }

    // Single-fill fallback: first sibling at same datetime with matching leg
    for sibling in sibling_txns {
        if sibling.datetime == txn.datetime && sibling.postings.as_ptr() != txn.postings.as_ptr() {
            let has_sibling_leg = sibling.postings.iter().any(|p| {
                p.account.starts_with("equity:trading") && p.account.contains(sibling_leg)
            });
            if has_sibling_leg {
                return Some(sibling);
            }
        }
    }

    // Same-leg fallback: when both legs share the same equity:trading suffix
    // (e.g. both equity:trading:sell due to a category rule that didn't
    // differentiate sign), the equity-suffix-based matching above can't
    // separate buy from sell. Pair siblings by `assets:` commodity instead,
    // and when both sides are multi-fill, rank-pair by `assets:` amount so
    // each disposal gets its own counter-leg's value rather than every
    // disposal sharing the FIRST sibling's proceeds.
    let mut same_leg_current: Vec<&Transaction> = Vec::new();
    let mut same_leg_target: Vec<&Transaction> = Vec::new();
    for sibling in sibling_txns {
        if sibling.datetime != txn.datetime {
            continue;
        }
        let has_equity = sibling
            .postings
            .iter()
            .any(|p| p.account.starts_with("equity:trading"));
        if !has_equity {
            continue;
        }
        let has_same_commodity = sibling
            .postings
            .iter()
            .any(|p| p.account.starts_with("assets:") && p.commodity == posting.commodity);
        let has_different_commodity = sibling
            .postings
            .iter()
            .any(|p| p.account.starts_with("assets:") && p.commodity != posting.commodity);
        if has_same_commodity {
            same_leg_current.push(sibling);
        }
        if has_different_commodity {
            same_leg_target.push(sibling);
        }
    }

    if same_leg_current.len() > 1 && same_leg_current.len() == same_leg_target.len() {
        let asset_amount = |t: &&Transaction, want_same: bool| -> f64 {
            t.postings
                .iter()
                .find(|p| {
                    p.account.starts_with("assets:")
                        && (if want_same {
                            p.commodity == posting.commodity
                        } else {
                            p.commodity != posting.commodity
                        })
                })
                .map(|p| p.amount.abs())
                .unwrap_or(0.0)
        };
        same_leg_current.sort_by(|a, b| {
            asset_amount(a, true)
                .partial_cmp(&asset_amount(b, true))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        same_leg_target.sort_by(|a, b| {
            asset_amount(a, false)
                .partial_cmp(&asset_amount(b, false))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if let Some(rank) = same_leg_current
            .iter()
            .position(|s| s.postings.as_ptr() == txn.postings.as_ptr())
        {
            return Some(same_leg_target[rank]);
        }
    }

    // Single-fill same-leg: first counter-commodity sibling at the same
    // datetime. Preserves the original behaviour for groups of one.
    for sibling in sibling_txns {
        if sibling.datetime == txn.datetime && sibling.postings.as_ptr() != txn.postings.as_ptr() {
            let has_equity = sibling
                .postings
                .iter()
                .any(|p| p.account.starts_with("equity:trading"));
            let has_different_commodity = sibling.postings.iter().any(|p| {
                p.account.starts_with("assets:") && p.commodity != posting.commodity
            });
            if has_equity && has_different_commodity {
                return Some(sibling);
            }
        }
    }
    None
}

fn resolve_acquisition_cost(
    posting: &Posting,
    txn: &Transaction,
    price_graph: &PriceGraph,
    base_currency: &str,
    warnings: &mut Vec<String>,
    sibling_txns: &[&Transaction],
) -> f64 {
    let quantity = posting.amount;

    // 1a. Cost annotation on posting
    if let Some(ref cost) = posting.cost {
        if let Some(amount) = cost.amount {
            let commodity = cost.commodity.as_deref().unwrap_or(base_currency);
            return resolve_annotation_value(
                amount,
                commodity,
                cost.is_total,
                quantity.abs(),
                price_graph,
                &txn.datetime,
                base_currency,
            );
        }
    }

    // 1b. Price annotation on posting (e.g. "@ 0.77 AUD" from swap auto-link)
    if let Some(ref price) = posting.price {
        return resolve_annotation_value(
            price.amount,
            &price.commodity,
            price.is_total,
            quantity.abs(),
            price_graph,
            &txn.datetime,
            base_currency,
        );
    }

    // 2. Counterparty posting in base currency (negative amount = payment)
    for p in &txn.postings {
        if p.commodity == base_currency && p.amount < 0.0 && p.account != posting.account {
            return p.amount.abs();
        }
    }

    // 3. Counterparty disposal posting with price annotation
    for p in &txn.postings {
        if p.account.starts_with("assets:") && p.amount < 0.0 && p.commodity != posting.commodity {
            if let Some(ref price) = p.price {
                return resolve_annotation_value(
                    price.amount,
                    &price.commodity,
                    price.is_total,
                    p.amount.abs(),
                    price_graph,
                    &txn.datetime,
                    base_currency,
                );
            }
        }
    }

    // 3b. Counterparty posting in non-base currency, converted via PriceGraph
    for p in &txn.postings {
        if p.amount < 0.0
            && p.commodity != posting.commodity
            && p.commodity != base_currency
            && p.account != posting.account
        {
            if let Some(value) = price_graph.convert_to_base(
                &p.commodity,
                p.amount.abs(),
                &txn.datetime,
                base_currency,
            ) {
                return value;
            }
        }
    }

    // 4. Equity:trading swap — find sibling sell transaction
    if let Some(sibling) =
        find_equity_swap_sibling(txn, sibling_txns, "buy", "sell", posting, false)
    {
        for p in &sibling.postings {
            if p.account.starts_with("assets:")
                && p.amount < 0.0
                && p.commodity != posting.commodity
            {
                let disposed_qty = p.amount.abs();
                if let Some(ref price) = p.price {
                    return resolve_annotation_value(
                        price.amount,
                        &price.commodity,
                        price.is_total,
                        disposed_qty,
                        price_graph,
                        &txn.datetime,
                        base_currency,
                    );
                }
                if let Some(value) = price_graph.convert_to_base(
                    &p.commodity,
                    disposed_qty,
                    &txn.datetime,
                    base_currency,
                ) {
                    return value;
                }
            }
        }
    }

    // 5. PriceGraph conversion
    if let Some(value) =
        price_graph.convert_to_base(&posting.commodity, quantity, &txn.datetime, base_currency)
    {
        return value;
    }

    // 6. Zero + warning
    warnings.push(format!(
        "No cost basis for acquisition of {:.4} {} on {} - using 0",
        quantity, posting.commodity, txn.date
    ));
    0.0
}

fn resolve_sale_proceeds(
    posting: &Posting,
    txn: &Transaction,
    price_graph: &PriceGraph,
    base_currency: &str,
    warnings: &mut Vec<String>,
    sibling_txns: &[&Transaction],
) -> f64 {
    let quantity = posting.amount.abs();

    // 1. Price annotation on posting
    if let Some(ref price) = posting.price {
        return resolve_annotation_value(
            price.amount,
            &price.commodity,
            price.is_total,
            quantity,
            price_graph,
            &txn.datetime,
            base_currency,
        );
    }

    // 2. Counterparty posting in base currency (positive amount = receipt)
    for p in &txn.postings {
        if p.commodity == base_currency && p.amount > 0.0 && p.account != posting.account {
            return p.amount;
        }
    }

    // 3. Counterparty acquisition posting with cost annotation
    for p in &txn.postings {
        if p.account.starts_with("assets:") && p.amount > 0.0 && p.commodity != posting.commodity {
            if let Some(ref cost) = p.cost {
                if let Some(amount) = cost.amount {
                    return resolve_annotation_value(
                        amount,
                        cost.commodity.as_deref().unwrap_or(base_currency),
                        cost.is_total,
                        p.amount.abs(),
                        price_graph,
                        &txn.datetime,
                        base_currency,
                    );
                }
            }
        }
    }

    // 3b. Counterparty posting in non-base currency, converted via PriceGraph
    for p in &txn.postings {
        if p.amount > 0.0
            && p.commodity != posting.commodity
            && p.commodity != base_currency
            && p.account != posting.account
        {
            if let Some(value) =
                price_graph.convert_to_base(&p.commodity, p.amount, &txn.datetime, base_currency)
            {
                return value;
            }
        }
    }

    // 4. Equity:trading swap — find sibling buy transaction
    if let Some(sibling) = find_equity_swap_sibling(txn, sibling_txns, "sell", "buy", posting, true)
    {
        for p in &sibling.postings {
            if p.account.starts_with("assets:")
                && p.amount > 0.0
                && p.commodity != posting.commodity
            {
                if p.commodity == base_currency {
                    return p.amount;
                }
                if let Some(ref cost) = p.cost {
                    if let Some(amount) = cost.amount {
                        return resolve_annotation_value(
                            amount,
                            cost.commodity.as_deref().unwrap_or(base_currency),
                            cost.is_total,
                            p.amount,
                            price_graph,
                            &txn.datetime,
                            base_currency,
                        );
                    }
                }
                if let Some(value) = price_graph.convert_to_base(
                    &p.commodity,
                    p.amount,
                    &txn.datetime,
                    base_currency,
                ) {
                    return value;
                }
            }
        }
    }

    // 5. PriceGraph conversion
    if let Some(value) =
        price_graph.convert_to_base(&posting.commodity, quantity, &txn.datetime, base_currency)
    {
        return value;
    }

    // 6. Zero + warning
    warnings.push(format!(
        "No sale proceeds for disposal of {:.4} {} on {} - using 0",
        quantity, posting.commodity, txn.date
    ));
    0.0
}

/// Generate a CGT report using FIFO lot matching.
///
/// Walks all transactions chronologically, building per-commodity FIFO inventories.
/// When a non-base commodity is disposed (negative asset posting), lots are consumed
/// oldest first and CGT events are emitted for disposals within the financial year.
pub fn generate_cgt_report(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    tax_config: &TaxConfig,
    financial_year: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
) -> CgtReport {
    let (fy_start, fy_end) = fy_date_range(financial_year, tax_config);
    generate_cgt_report_range(
        transactions,
        price_graph,
        tax_config,
        financial_year,
        &fy_start,
        &fy_end,
        base_currency,
        base_account_scope,
    )
}

pub fn generate_cgt_report_range(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    tax_config: &TaxConfig,
    label: &str,
    date_from: &str,
    date_to: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
) -> CgtReport {
    let (fy_start, fy_end) = (date_from.to_string(), date_to.to_string());

    // Sort transactions chronologically, optionally filtered by scope
    let mut sorted_txns: Vec<&Transaction> = transactions.iter()
        .filter(|txn| match base_account_scope {
            Some(scope) => txn.postings.iter().any(|p| {
                p.account == scope || p.account.starts_with(&format!("{scope}:"))
            }),
            None => true,
        })
        .collect();
    sorted_txns.sort_by(|a, b| a.datetime.cmp(&b.datetime));

    // Group transactions by datetime for two-leg equity:trading swap matching
    let mut by_datetime: HashMap<String, Vec<&Transaction>> = HashMap::new();
    for txn in &sorted_txns {
        by_datetime
            .entry(txn.datetime.clone())
            .or_default()
            .push(txn);
    }

    let mut inventories: HashMap<String, CommodityInventory> = HashMap::new();
    let mut events = Vec::new();
    let mut warnings = Vec::new();

    for txn in &sorted_txns {
        // Determine which commodities are transfers in this transaction
        let mut transfer_commodities = std::collections::HashSet::new();
        let commodities_in_txn: std::collections::HashSet<String> = txn
            .postings
            .iter()
            .filter(|p| p.account.starts_with("assets:") && p.commodity != base_currency)
            .map(|p| p.commodity.clone())
            .collect();
        for commodity in &commodities_in_txn {
            if is_transfer_for_commodity(txn, commodity) {
                transfer_commodities.insert(commodity.clone());
            }
        }

        // Process disposals first (before acquisitions within the same transaction)
        for posting in &txn.postings {
            if !posting.account.starts_with("assets:") {
                continue;
            }
            if posting.commodity == base_currency {
                continue;
            }
            if transfer_commodities.contains(&posting.commodity) {
                continue;
            }
            if posting.amount >= 0.0 {
                continue;
            }

            let commodity = &posting.commodity;
            let quantity = posting.amount.abs();
            let siblings = by_datetime
                .get(&txn.datetime)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let proceeds = resolve_sale_proceeds(
                posting,
                txn,
                price_graph,
                base_currency,
                &mut warnings,
                siblings,
            );
            let proceeds_per_unit = if quantity > EPSILON {
                proceeds / quantity
            } else {
                0.0
            };

            let inv = inventories.entry(commodity.clone()).or_default();
            let (consumed_lots, total_consumed) = inv.consume_fifo(quantity);

            let unmatched = quantity - total_consumed;
            if unmatched > EPSILON {
                warnings.push(format!(
                    "Sold more than held for {}: {:.4} units unmatched (using 0 cost basis)",
                    commodity, unmatched
                ));
            }

            // Emit CGT events for each consumed lot (if disposal is within FY)
            let in_fy =
                txn.date.as_str() >= fy_start.as_str() && txn.date.as_str() <= fy_end.as_str();
            if in_fy {
                // Emit a zero-cost event for unmatched units (no buy found)
                if unmatched > EPSILON {
                    let sale_proceeds = unmatched * proceeds_per_unit;
                    events.push(CgtEvent {
                        sell_date: txn.date.clone(),
                        buy_date: txn.date.clone(),
                        commodity: commodity.clone(),
                        quantity: unmatched,
                        cost_basis: 0.0,
                        sale_proceeds,
                        capital_gain: sale_proceeds,
                        holding_days: 0,
                        discount_eligible: false,
                        discounted_gain: sale_proceeds,
                        trade_link_id: String::new(),
                        sell_txn_id: extract_txn_id(&txn.meta).unwrap_or_default(),
                        sell_account: posting.account.clone(),
                    });
                }

                for lot in &consumed_lots {
                    let cost_basis = lot.quantity * lot.cost_per_unit;
                    let sale_proceeds = lot.quantity * proceeds_per_unit;
                    let capital_gain = sale_proceeds - cost_basis;
                    let holding_days = days_between(&lot.acquisition_date, &txn.date);
                    let holding_months = months_between(&lot.acquisition_date, &txn.date);
                    let discount_eligible = capital_gain > 0.0
                        && holding_months >= tax_config.cgt_discount_holding_months as i64;
                    let discounted_gain = if discount_eligible {
                        capital_gain * (1.0 - tax_config.cgt_discount_percent as f64 / 100.0)
                    } else {
                        capital_gain
                    };

                    events.push(CgtEvent {
                        sell_date: txn.date.clone(),
                        buy_date: lot.acquisition_date.clone(),
                        commodity: commodity.clone(),
                        quantity: lot.quantity,
                        cost_basis,
                        sale_proceeds,
                        capital_gain,
                        holding_days,
                        discount_eligible,
                        discounted_gain,
                        trade_link_id: lot.txn_id.clone(),
                        sell_txn_id: extract_txn_id(&txn.meta).unwrap_or_default(),
                        sell_account: posting.account.clone(),
                    });
                }
            }
        }

        // Process acquisitions second
        for posting in &txn.postings {
            if !posting.account.starts_with("assets:") {
                continue;
            }
            if posting.commodity == base_currency {
                continue;
            }
            if transfer_commodities.contains(&posting.commodity) {
                continue;
            }
            if posting.amount <= 0.0 {
                continue;
            }

            let commodity = &posting.commodity;
            let quantity = posting.amount;
            let siblings = by_datetime
                .get(&txn.datetime)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let cost = resolve_acquisition_cost(
                posting,
                txn,
                price_graph,
                base_currency,
                &mut warnings,
                siblings,
            );
            let cost_per_unit = if quantity > EPSILON {
                cost / quantity
            } else {
                0.0
            };
            let txn_id = extract_txn_id(&txn.meta).unwrap_or_default();

            inventories.entry(commodity.clone()).or_default().push(Lot {
                quantity,
                cost_per_unit,
                acquisition_date: txn.date.clone(),
                txn_id,
            });
        }
    }

    // Sort by sell date, then by buy date for stability
    events.sort_by(|a, b| {
        a.sell_date
            .cmp(&b.sell_date)
            .then(a.buy_date.cmp(&b.buy_date))
    });

    let total_gains: f64 = events
        .iter()
        .filter(|e| e.capital_gain > 0.0)
        .map(|e| e.capital_gain)
        .sum();
    let total_losses: f64 = events
        .iter()
        .filter(|e| e.capital_gain < 0.0)
        .map(|e| e.capital_gain.abs())
        .sum();
    let net_capital_gain = total_gains - total_losses;
    // ATO method: capital losses offset gains BEFORE the CGT discount is applied.
    // Apply losses to non-discount-eligible gains first (taxpayer-favorable, since
    // those gains would otherwise be taxed at the full rate), then to eligible
    // gains, then halve the remaining eligible gains.
    let eligible_gains: f64 = events
        .iter()
        .filter(|e| e.discount_eligible && e.capital_gain > 0.0)
        .map(|e| e.capital_gain)
        .sum();
    let non_eligible_gains: f64 = events
        .iter()
        .filter(|e| !e.discount_eligible && e.capital_gain > 0.0)
        .map(|e| e.capital_gain)
        .sum();
    let losses_after_non_eligible = (total_losses - non_eligible_gains).max(0.0);
    let remaining_non_eligible = (non_eligible_gains - total_losses).max(0.0);
    let remaining_eligible = (eligible_gains - losses_after_non_eligible).max(0.0);
    let discount_factor = 1.0 - tax_config.cgt_discount_percent as f64 / 100.0;
    let total_discounted_gain = remaining_non_eligible + remaining_eligible * discount_factor;

    CgtReport {
        financial_year: label.to_string(),
        events,
        total_gains,
        total_losses,
        short_term_gains: non_eligible_gains,
        long_term_gains: eligible_gains,
        net_capital_gain,
        total_discounted_gain,
        warnings,
    }
}

/// Section a CGT event list into (short-term gains, long-term gains, losses).
/// Single source of truth for the per-event bucketing rule used by both the UI
/// and the CSV export: anything <0 is a loss regardless of holding period;
/// remaining events split by `discount_eligible` (≥12mo).
pub fn partition_cgt_events(events: &[CgtEvent]) -> (Vec<&CgtEvent>, Vec<&CgtEvent>, Vec<&CgtEvent>) {
    let mut short = Vec::new();
    let mut long = Vec::new();
    let mut losses = Vec::new();
    for e in events {
        if e.capital_gain < 0.0 {
            losses.push(e);
        } else if e.discount_eligible {
            long.push(e);
        } else {
            short.push(e);
        }
    }
    (short, long, losses)
}

/// Generate an income tax report.
pub fn generate_income_report(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    tax_config: &TaxConfig,
    financial_year: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
) -> IncomeTaxReport {
    let (fy_start, fy_end) = fy_date_range(financial_year, tax_config);
    generate_income_report_range(
        transactions,
        price_graph,
        tax_config,
        financial_year,
        &fy_start,
        &fy_end,
        base_currency,
        base_account_scope,
    )
}

pub fn generate_income_report_range(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    tax_config: &TaxConfig,
    label: &str,
    date_from: &str,
    date_to: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
) -> IncomeTaxReport {
    let (fy_start, fy_end) = (date_from.to_string(), date_to.to_string());

    let mut income_totals: std::collections::BTreeMap<String, f64> =
        std::collections::BTreeMap::new();
    let mut expense_totals: std::collections::BTreeMap<String, f64> =
        std::collections::BTreeMap::new();
    let mut income_events: Vec<IncomeEvent> = Vec::new();
    let mut expense_events: Vec<IncomeEvent> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    for txn in transactions {
        // Filter to FY
        if txn.date.as_str() < fy_start.as_str() || txn.date.as_str() > fy_end.as_str() {
            continue;
        }

        // Filter by base account scope
        if let Some(scope) = base_account_scope {
            let in_scope = txn.postings.iter().any(|p| {
                p.account == scope || p.account.starts_with(&format!("{scope}:"))
            });
            if !in_scope {
                continue;
            }
        }

        let txn_id = extract_txn_id(&txn.meta).unwrap_or_default();

        for posting in &txn.postings {
            if posting.account.starts_with("income:") {
                if tax_config.non_taxable_accounts.contains(&posting.account) {
                    continue;
                }
                match resolve_posting_value(posting, txn, price_graph, base_currency) {
                    Some(value) => {
                        // Negate: income postings are credits (negative) in the ledger,
                        // but we display income as positive in the report.
                        let signed = -value;
                        *income_totals.entry(posting.account.clone()).or_default() += signed;
                        income_events.push(make_income_event(
                            posting,
                            txn,
                            signed,
                            base_currency,
                            &txn_id,
                        ));
                    }
                    None => {
                        warnings.push(format!(
                            "Could not convert {} {} ({}) to {} on {}",
                            posting.amount,
                            posting.commodity,
                            posting.account,
                            base_currency,
                            txn.date
                        ));
                    }
                }
            } else if posting.account.starts_with("expenses:") {
                if tax_config
                    .non_deductible_accounts
                    .contains(&posting.account)
                {
                    continue;
                }
                match resolve_posting_value(posting, txn, price_graph, base_currency) {
                    Some(value) => {
                        *expense_totals.entry(posting.account.clone()).or_default() += value;
                        expense_events.push(make_income_event(
                            posting,
                            txn,
                            value,
                            base_currency,
                            &txn_id,
                        ));
                    }
                    None => {
                        warnings.push(format!(
                            "Could not convert {} {} ({}) to {} on {}",
                            posting.amount,
                            posting.commodity,
                            posting.account,
                            base_currency,
                            txn.date
                        ));
                    }
                }
            }
        }
    }

    let income_categories: Vec<TaxCategory> = income_totals
        .into_iter()
        .map(|(account, total)| TaxCategory {
            account,
            total,
            base_currency: base_currency.to_string(),
        })
        .collect();

    let expense_categories: Vec<TaxCategory> = expense_totals
        .into_iter()
        .map(|(account, total)| TaxCategory {
            account,
            total,
            base_currency: base_currency.to_string(),
        })
        .collect();

    let total_income: f64 = income_categories.iter().map(|c| c.total).sum();
    let total_expenses: f64 = expense_categories.iter().map(|c| c.total).sum();

    IncomeTaxReport {
        financial_year: label.to_string(),
        income_categories,
        expense_categories,
        events: income_events,
        expense_events,
        total_income,
        total_expenses,
        net: total_income - total_expenses,
        warnings,
    }
}

/// Build an `IncomeEvent` from a posting + its resolved base-currency value.
///
/// `signed_value` is the posting's contribution to the category total in the
/// base currency: positive for normal income (a credit on `income:*`) and
/// expenses (a debit on `expenses:*`), negative for "negative income" rows
/// like fees recorded as positive on `income:*` (which offset gross income).
///
/// `quantity` mirrors that sign so each event can be summed without surprises:
/// `Σ event.value` equals `category.total`. `price` stays a positive per-unit
/// number — display layers can derive sign from the value column.
fn make_income_event(
    posting: &Posting,
    txn: &Transaction,
    signed_value: f64,
    base_currency: &str,
    txn_id: &str,
) -> IncomeEvent {
    // Income postings are credits in the ledger (negative amount → positive
    // contribution); expenses are debits (positive amount → positive
    // contribution). Flipping income preserves the relation
    // `value = quantity * price` (with `price` always positive).
    let quantity = if posting.account.starts_with("income:") {
        -posting.amount
    } else {
        posting.amount
    };
    let value = signed_value;
    let abs_q = quantity.abs();
    let price = if abs_q > EPSILON {
        value.abs() / abs_q
    } else {
        0.0
    };
    // Counterparty asset account, used for the "click to navigate" link in the
    // UI. Prefer the same-commodity sibling with the opposite sign; otherwise
    // fall back to the first non-income/expense posting.
    let asset_account = txn
        .postings
        .iter()
        .find(|p| {
            p.account != posting.account
                && p.commodity == posting.commodity
                && p.amount.signum() != posting.amount.signum()
        })
        .or_else(|| {
            txn.postings.iter().find(|p| {
                !p.account.starts_with("income:") && !p.account.starts_with("expenses:")
            })
        })
        .map(|p| p.account.clone())
        .unwrap_or_default();
    IncomeEvent {
        date: txn.date.clone(),
        account: posting.account.clone(),
        commodity: posting.commodity.clone(),
        quantity,
        price,
        value,
        base_currency: base_currency.to_string(),
        txn_id: txn_id.to_string(),
        asset_account,
    }
}

/// Set of accounts that count toward holdings / balances / performance.
///
/// `exact` holds the auto-derived source-folder accounts (historical
/// behaviour). `prefixes` holds user-configured extras
/// (`extra_primary_account_prefixes` in the global config) that also match any
/// sub-account. Membership is the UNION of the two, so configuring prefixes only
/// ever includes MORE accounts — it can never drop a folder-backed account.
#[derive(Debug, Clone, Default)]
pub struct AccountAllowlist {
    pub exact: HashSet<String>,
    pub prefixes: Vec<String>,
}

impl AccountAllowlist {
    /// Build from an exact set plus raw prefix strings. Prefixes are trimmed and
    /// empties dropped, so a blank entry can't accidentally match every account
    /// via the `starts_with` path.
    pub fn new(exact: HashSet<String>, prefixes: &[String]) -> Self {
        let prefixes = prefixes
            .iter()
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        Self { exact, prefixes }
    }

    /// True if `account` is an exact member, or sits at or under any configured
    /// prefix. Prefix matching is segment-aware: `assets:staking` matches
    /// `assets:staking` and `assets:staking:lido`, but NOT `assets:stakingpool`.
    pub fn allows(&self, account: &str) -> bool {
        if self.exact.contains(account) {
            return true;
        }
        self.prefixes
            .iter()
            .any(|p| account == p || account.starts_with(&format!("{p}:")))
    }
}

/// Generate a Balances report for the end of `financial_year`.
///
/// Resolves the FY-end date via `tax_config`, then delegates to the range form.
pub fn generate_balances_report(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    tax_config: &TaxConfig,
    financial_year: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
    allowed_accounts: Option<&AccountAllowlist>,
) -> BalancesReport {
    let (_, fy_end) = fy_date_range(financial_year, tax_config);
    generate_balances_report_range(
        transactions,
        price_graph,
        &fy_end,
        base_currency,
        base_account_scope,
        allowed_accounts,
    )
}

/// Generate a Balances report as of `as_of_date`.
///
/// Walks all transactions with `datetime <= as_of_date`, aggregating postings
/// whose account is under `base_account_scope` (if given) AND, when
/// `allowed_accounts` is supplied, whose account is in that set. The allowlist
/// keeps reconciliation totals tied to primary source-folder accounts and
/// excludes routing contras (transfer / lending / bridge / wrap / staking)
/// that share the scope prefix.
pub fn generate_balances_report_range(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    as_of_date: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
    allowed_accounts: Option<&AccountAllowlist>,
) -> BalancesReport {
    // Aggregate at the posting level so both date and scope filters propagate
    // through to the final balances (unlike `QueryResult.aggregated_balance`,
    // which is all-time per-account and ignores filters).
    //
    // Transactions touching any `ignore:*` account (spam tokens, approvals,
    // failed/no-op events) are skipped entirely — they don't contribute to
    // holdings, and they don't trigger missing-price warnings either.
    let mut by_commodity: HashMap<String, f64> = HashMap::new();
    let mut by_commodity_account: HashMap<(String, String), f64> = HashMap::new();
    for txn in transactions {
        // Date comparison on ISO strings — take the date portion so a bare
        // "YYYY-MM-DD" as-of date compares correctly against
        // "YYYY-MM-DDTHH:MM:SS" transaction datetimes.
        let txn_date_len = txn.datetime.len().min(10);
        let txn_date = &txn.datetime[..txn_date_len];
        if txn_date > as_of_date {
            continue;
        }
        if txn
            .postings
            .iter()
            .any(|p| p.account == "ignore" || p.account.starts_with("ignore:"))
        {
            continue;
        }
        for posting in &txn.postings {
            let in_scope = match base_account_scope {
                Some(scope) => {
                    posting.account == scope
                        || posting.account.starts_with(&format!("{scope}:"))
                }
                None => true,
            };
            let allowed = match allowed_accounts {
                Some(al) => al.allows(&posting.account),
                None => true,
            };
            if in_scope && allowed {
                *by_commodity.entry(posting.commodity.clone()).or_insert(0.0) +=
                    posting.amount;
                *by_commodity_account
                    .entry((posting.commodity.clone(), posting.account.clone()))
                    .or_insert(0.0) += posting.amount;
            }
        }
    }

    let datetime_key = format!("{as_of_date}T23:59:59");
    let mut holdings: Vec<CoinBalance> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let build_accounts = |commodity: &str, price: f64| -> Vec<AccountBalance> {
        // Names of all accounts that hold this commodity (regardless of sign).
        // DETERMINISM: filter then sort. Iterating `by_commodity_account.keys()`
        // is HashMap iteration order, which is randomized per-process; collecting
        // into a Vec and sorting makes the per-commodity account ordering stable.
        let mut names: Vec<&String> = by_commodity_account
            .keys()
            .filter(|(c, _)| c == commodity)
            .map(|(_, a)| a)
            .collect();
        names.sort();
        // Drop parents — any account that is a strict prefix of another
        // account in the same commodity is an aggregation, not a wallet.
        let is_leaf = |a: &str| -> bool {
            !names
                .iter()
                .any(|other| other.len() > a.len() && other.starts_with(a) && other.as_bytes()[a.len()] == b':')
        };
        // DETERMINISM: collect a sorted list of (account, qty) before mapping
        // to `AccountBalance`. The final secondary sort below is by abs(quantity)
        // desc, which is non-stable on f64 ties; with input pre-sorted by account,
        // ties resolve in account-alphabetical order rather than HashMap order.
        let mut entries: Vec<(&String, &f64)> = by_commodity_account
            .iter()
            .filter(|((c, a), q)| c == commodity && q.abs() >= EPSILON && is_leaf(a))
            .map(|((_, a), q)| (a, q))
            .collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let mut out: Vec<AccountBalance> = entries
            .into_iter()
            .map(|(a, q)| AccountBalance {
                account: a.clone(),
                quantity: *q,
                value: *q * price,
            })
            .collect();
        out.sort_by(|a, b| {
            b.quantity
                .abs()
                .partial_cmp(&a.quantity.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    };

    // DETERMINISM: iterate `by_commodity` in sorted-key order. The downstream
    // `total_value` sum (line below) is order-dependent in f64 arithmetic — two
    // different HashMap iteration orders give `total_value` differing by ULPs,
    // and that propagates into every `portfolio_weight = h.value / total_value`
    // making every balances report file change run-to-run. See invariant test
    // `pipeline_is_deterministic_across_two_cold_runs`.
    let mut commodities: Vec<(&String, &f64)> = by_commodity.iter().collect();
    commodities.sort_by(|a, b| a.0.cmp(b.0));
    for (commodity, quantity) in commodities {
        let quantity = *quantity;
        if quantity.abs() < EPSILON {
            continue;
        }
        if commodity == base_currency {
            let accounts = build_accounts(commodity, 1.0);
            holdings.push(CoinBalance {
                commodity: commodity.clone(),
                quantity,
                price: 1.0,
                price_date: as_of_date.to_string(),
                value: quantity,
                portfolio_weight: 0.0,
                accounts,
            });
            continue;
        }
        match price_graph.convert_to_base(commodity, 1.0, &datetime_key, base_currency) {
            Some(price) => {
                let price_date = price_graph
                    .lookup(commodity, &datetime_key)
                    .map(|p| p.datetime.clone())
                    .unwrap_or_else(|| as_of_date.to_string());
                let accounts = build_accounts(commodity, price);
                holdings.push(CoinBalance {
                    commodity: commodity.clone(),
                    quantity,
                    price,
                    price_date,
                    value: quantity * price,
                    portfolio_weight: 0.0,
                    accounts,
                });
            }
            None => {
                warnings.push(format!(
                    "No {base_currency} price for {commodity} as of {as_of_date}"
                ));
            }
        }
    }

    let total_value: f64 = holdings.iter().map(|h| h.value).sum();
    if total_value.abs() > EPSILON {
        for h in &mut holdings {
            h.portfolio_weight = h.value / total_value;
        }
    }

    // Largest absolute value first — long positions typically top the list, but
    // sizeable shorts / liabilities in scope also surface.
    holdings.sort_by(|a, b| {
        b.value
            .abs()
            .partial_cmp(&a.value.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    BalancesReport {
        as_of_date: as_of_date.to_string(),
        base_currency: base_currency.to_string(),
        base_account_scope: base_account_scope.map(|s| s.to_string()),
        holdings,
        total_value,
        warnings,
    }
}

const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Format an ISO date "YYYY-MM-DD" as a human label, e.g. "Jun 2026".
fn month_label(date: &str) -> String {
    match parse_date(date) {
        Some((y, m, _)) if (1..=12).contains(&m) => {
            format!("{} {}", MONTH_ABBR[(m - 1) as usize], y)
        }
        _ => date.to_string(),
    }
}

/// The ISO date one day before `date` (prior-period close), via chrono. Falls
/// back to `date` unchanged if it can't be parsed.
fn day_before(date: &str) -> String {
    use chrono::NaiveDate;
    parse_date(date)
        .and_then(|(y, m, d)| NaiveDate::from_ymd_opt(y, m, d))
        .and_then(|nd| nd.pred_opt())
        .map(|prev| prev.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| date.to_string())
}

/// Snapshot dates for a performance window: the last calendar day of each month
/// that falls within `[date_from, date_to]`, plus `date_to` itself when it isn't
/// already a month-end (so a mid-month window end is always represented). Never
/// returns an empty vec.
fn month_end_snapshot_dates(date_from: &str, date_to: &str) -> Vec<String> {
    use chrono::NaiveDate;
    let (fy, fm) = match parse_date(date_from) {
        Some((y, m, _)) => (y, m),
        None => return vec![date_to.to_string()],
    };
    let (ty, tm) = match parse_date(date_to) {
        Some((y, m, _)) => (y, m),
        None => return vec![date_to.to_string()],
    };

    let mut out: Vec<String> = Vec::new();
    let (mut y, mut m) = (fy, fm);
    while (y, m) <= (ty, tm) {
        // Last day of (y, m) = day before the 1st of the following month.
        let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
        if let Some(first_next) = NaiveDate::from_ymd_opt(ny, nm, 1) {
            if let Some(last) = first_next.pred_opt() {
                let d = last.format("%Y-%m-%d").to_string();
                if d.as_str() >= date_from && d.as_str() <= date_to {
                    out.push(d);
                }
            }
        }
        if m == 12 {
            y += 1;
            m = 1;
        } else {
            m += 1;
        }
    }

    // Guarantee the exact window end is represented (mid-month `date_to`).
    if out.last().map(String::as_str) != Some(date_to) {
        out.push(date_to.to_string());
    }
    out
}

/// Point-in-time FIFO holdings snapshot as of `as_of_date`.
///
/// Walks transactions chronologically up to (and including) `as_of_date`,
/// building per-commodity FIFO inventories (acquisitions push lots, disposals
/// consume oldest-first via [`CommodityInventory::consume_fifo`]), then reads out
/// the REMAINING lots as current holdings. Per held commodity:
///   - `cost_basis` = Σ remaining-lot (quantity × cost_per_unit)
///   - `value`      = quantity × spot price at `as_of_date`
///   - `unrealised` = value − cost_basis
///
/// Filtering matches [`generate_balances_report_range`] so the value line
/// reconciles with the Balances report: `ignore:*` transactions are skipped;
/// `base_account_scope` + `allowed_accounts` are applied at posting level;
/// base-currency cash is excluded (it has no cost basis). A missing price →
/// the holding is CARRIED AT COST (`value := cost_basis`, `unrealised := 0`,
/// `has_price = false`) so the performance series stays continuous across months.
pub fn holdings_as_of(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    as_of_date: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
    allowed_accounts: Option<&AccountAllowlist>,
) -> HoldingsSnapshot {
    // In-window transactions (date-portion compare, like Balances), skipping any
    // txn that touches an `ignore:*` account, sorted chronologically.
    let mut sorted_txns: Vec<&Transaction> = transactions
        .iter()
        .filter(|txn| {
            let len = txn.datetime.len().min(10);
            &txn.datetime[..len] <= as_of_date
        })
        .filter(|txn| {
            !txn.postings
                .iter()
                .any(|p| p.account == "ignore" || p.account.starts_with("ignore:"))
        })
        .collect();
    sorted_txns.sort_by(|a, b| a.datetime.cmp(&b.datetime));

    // Group by datetime so equity-swap sibling cost resolution works (as in CGT).
    let mut by_datetime: HashMap<String, Vec<&Transaction>> = HashMap::new();
    for txn in &sorted_txns {
        by_datetime
            .entry(txn.datetime.clone())
            .or_default()
            .push(txn);
    }

    let mut warnings: Vec<String> = Vec::new();
    let mut inventories: HashMap<String, CommodityInventory> = HashMap::new();

    let in_scope = |account: &str| -> bool {
        match base_account_scope {
            Some(scope) => account == scope || account.starts_with(&format!("{scope}:")),
            None => true,
        }
    };
    let is_allowed = |account: &str| -> bool {
        match allowed_accounts {
            Some(al) => al.allows(account),
            None => true,
        }
    };

    // Base-currency cash held in asset accounts (in scope/allowed). It's part of
    // portfolio value with book cost = face, so it lifts value and cost equally
    // (no unrealised). Summed for EVERY txn — including cash-only ones like income
    // receipts that carry no non-base holdings (and would be skipped below).
    let mut cash: f64 = 0.0;
    // Net balance per non-base commodity (Σ of in-scope/allowed asset postings).
    // This — NOT FIFO remaining lots — is what each holding is VALUED at, matching
    // the Balances report and the account total. FIFO remaining floors an
    // over-disposed position at 0; the net balance counts it as the (negative)
    // short it actually is, so the totals reconcile. FIFO is still used for the
    // cost basis below.
    let mut net_qty: HashMap<String, f64> = HashMap::new();

    for txn in &sorted_txns {
        for p in &txn.postings {
            if p.account.starts_with("assets:")
                && p.commodity == base_currency
                && in_scope(&p.account)
                && is_allowed(&p.account)
            {
                cash += p.amount;
            }
        }

        // Asset postings that participate in holdings: non-base commodity, in
        // scope, allowed. Transfer detection runs over THIS filtered set, so an
        // in-scope two-leg transfer nets to zero (skipped, lot cost preserved),
        // while a scope-crossing transfer (one leg) correctly moves holdings.
        let relevant: Vec<&Posting> = txn
            .postings
            .iter()
            .filter(|p| {
                p.account.starts_with("assets:")
                    && p.commodity != base_currency
                    && in_scope(&p.account)
                    && is_allowed(&p.account)
            })
            .collect();
        if relevant.is_empty() {
            continue;
        }
        // Net balance includes every leg (transfer legs cancel naturally).
        for p in &relevant {
            *net_qty.entry(p.commodity.clone()).or_insert(0.0) += p.amount;
        }

        // Transfer detection runs over ALL of the transaction's asset postings
        // for the commodity (via `is_transfer_for_commodity`, the same test the
        // CGT engine uses) — NOT the allowlist-filtered `relevant` set. A move
        // routed through a contra the holdings allowlist excludes (e.g.
        // `assets:transfer` for a Kraken earn-autoallocation sweep, often split
        // across two single-leg transactions) still nets to zero and is skipped,
        // so the lot's original cost basis and acquisition date are preserved
        // instead of being re-booked at the transfer-date spot price. Detecting
        // over the filtered set was the bug: with the contra dropped, each leg
        // looked like a standalone disposal + re-acquisition.
        let transfer_commodities: HashSet<&str> = relevant
            .iter()
            .map(|p| p.commodity.as_str())
            .filter(|c| is_transfer_for_commodity(txn, c))
            .collect();

        let siblings = by_datetime
            .get(&txn.datetime)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);

        // Disposals first (consume FIFO; we only need what remains).
        for posting in &relevant {
            if transfer_commodities.contains(posting.commodity.as_str()) || posting.amount >= 0.0 {
                continue;
            }
            let quantity = posting.amount.abs();
            let inv = inventories.entry(posting.commodity.clone()).or_default();
            let (_, total_consumed) = inv.consume_fifo(quantity);
            let unmatched = quantity - total_consumed;
            if unmatched > EPSILON {
                warnings.push(format!(
                    "Disposed more than held for {} as of {}: {:.4} units short (not valued)",
                    posting.commodity, as_of_date, unmatched
                ));
            }
        }

        // Acquisitions second (push new lots with resolved cost basis).
        for posting in &relevant {
            if transfer_commodities.contains(posting.commodity.as_str()) || posting.amount <= 0.0 {
                continue;
            }
            let quantity = posting.amount;
            let cost = resolve_acquisition_cost(
                posting,
                txn,
                price_graph,
                base_currency,
                &mut warnings,
                siblings,
            );
            let cost_per_unit = if quantity > EPSILON { cost / quantity } else { 0.0 };
            inventories
                .entry(posting.commodity.clone())
                .or_default()
                .push(Lot {
                    quantity,
                    cost_per_unit,
                    acquisition_date: txn.date.clone(),
                    txn_id: extract_txn_id(&txn.meta).unwrap_or_default(),
                });
        }
    }

    // Build holdings from NET balances (the held positions), valued at the as-of
    // price; cost basis comes from FIFO remaining lots. DETERMINISM: build in
    // sorted-commodity order and sum totals before the display sort.
    let datetime_key = format!("{as_of_date}T23:59:59");
    let mut commodities: Vec<&String> = net_qty.keys().collect();
    commodities.sort();

    let mut holdings: Vec<CommodityHolding> = Vec::new();
    for commodity in commodities {
        let quantity = net_qty[commodity];
        if quantity.abs() < EPSILON {
            continue;
        }
        // For a net-LONG holding, reconcile FIFO-remaining lots to the net held
        // quantity. A disposal that exceeds the lots on hand is floored to zero
        // (you can't dispose what you don't hold), leaving the unmatched units
        // un-consumed; later re-acquisitions then push FIFO-remaining ABOVE the
        // net balance. Since a holding is valued on its net balance but its cost
        // basis is summed from FIFO-remaining lots, that surplus would attach the
        // cost of coins NO LONGER IN THE BALANCE and manufacture a fictitious
        // unrealised P&L (a stablecoin showed ~49% "below cost"). Drop the oldest
        // surplus lots so cost basis and value count the same units. Net shorts
        // are handled below — carried at market (see the cost-basis block).
        if quantity > EPSILON {
            if let Some(inv) = inventories.get_mut(commodity) {
                let remaining: f64 = inv.lots.iter().map(|l| l.quantity).sum();
                if remaining - quantity > EPSILON {
                    inv.consume_fifo(remaining - quantity);
                }
            }
        }
        match price_graph.convert_to_base(commodity, 1.0, &datetime_key, base_currency) {
            Some(price) => {
                let price_date = price_graph
                    .lookup(commodity, &datetime_key)
                    .map(|p| p.datetime.clone())
                    .unwrap_or_else(|| as_of_date.to_string());
                let value = quantity * price;
                // Cost basis + per-parcel detail. A net-SHORT position has no
                // FIFO cost basis the engine can establish (it doesn't track
                // short lots), so carry it at market — cost = value, unrealised 0
                // — rather than summing the orphan lots left by floored
                // over-disposals / allowlist-hidden transfer legs, which would
                // book the whole short value as a fictitious unrealised swing
                // (SOL and BNB showed six-figure phantom gains on tiny net
                // positions). A net-LONG holding values each remaining FIFO lot at
                // this spot, oldest-first (the order they'd be disposed under FIFO).
                let (cost_basis, lots): (f64, Vec<LotDetail>) = if quantity > EPSILON {
                    inventories
                        .get(commodity)
                        .map(|inv| {
                            let mut cb = 0.0;
                            let lots = inv
                                .lots
                                .iter()
                                .map(|l| {
                                    let lot_cost = l.quantity * l.cost_per_unit;
                                    let lot_value = l.quantity * price;
                                    cb += lot_cost;
                                    LotDetail {
                                        acquisition_date: l.acquisition_date.clone(),
                                        quantity: l.quantity,
                                        cost_per_unit: l.cost_per_unit,
                                        cost_basis: lot_cost,
                                        value: lot_value,
                                        unrealised: lot_value - lot_cost,
                                    }
                                })
                                .collect();
                            (cb, lots)
                        })
                        .unwrap_or((0.0, Vec::new()))
                } else {
                    (value, Vec::new())
                };
                holdings.push(CommodityHolding {
                    commodity: commodity.clone(),
                    quantity,
                    cost_basis,
                    price,
                    price_date,
                    value,
                    unrealised: value - cost_basis,
                    has_price: true,
                    lots,
                });
            }
            None => {
                // Exclude unpriced commodities from value — matching the Balances
                // report (which drops them with a warning), so the performance
                // total reconciles with Balances and the account total.
                warnings.push(format!(
                    "No {base_currency} price for {commodity} as of {as_of_date}"
                ));
            }
        }
    }

    // Fold cash into the totals so total_value is the FULL portfolio value
    // (matching the Balances report). Cash cancels in total_unrealised. The
    // per-commodity `holdings` list stays investments-only (cash isn't a lot).
    let total_value: f64 = holdings.iter().map(|h| h.value).sum::<f64>() + cash;
    let total_cost_basis: f64 = holdings.iter().map(|h| h.cost_basis).sum::<f64>() + cash;

    holdings.sort_by(|a, b| {
        b.value
            .abs()
            .partial_cmp(&a.value.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    HoldingsSnapshot {
        as_of_date: as_of_date.to_string(),
        base_currency: base_currency.to_string(),
        holdings,
        total_value,
        total_cost_basis,
        total_unrealised: total_value - total_cost_basis,
        warnings,
    }
}

/// Generate a Tax Savings (loss-harvesting) report for a financial year:
/// current underwater holdings as of the FY end, offset against the gains
/// realised in that FY.
pub fn generate_loss_harvest_report(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    tax_config: &TaxConfig,
    financial_year: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
    allowed_accounts: Option<&AccountAllowlist>,
) -> LossHarvestReport {
    let (_, fy_end) = fy_date_range(financial_year, tax_config);
    let cgt = generate_cgt_report(
        transactions,
        price_graph,
        tax_config,
        financial_year,
        base_currency,
        base_account_scope,
    );
    let snapshot = holdings_as_of(
        transactions,
        price_graph,
        &fy_end,
        base_currency,
        base_account_scope,
        allowed_accounts,
    );
    assemble_loss_harvest(snapshot, &cgt, tax_config, &fy_end, financial_year, base_account_scope)
}

/// Range variant: underwater holdings as of `date_to`, offset against capital
/// gains realised in `[date_from, date_to]`.
pub fn generate_loss_harvest_report_range(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    tax_config: &TaxConfig,
    date_from: &str,
    date_to: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
    allowed_accounts: Option<&AccountAllowlist>,
) -> LossHarvestReport {
    let label = format!("{date_from} to {date_to}");
    let cgt = generate_cgt_report_range(
        transactions,
        price_graph,
        tax_config,
        &label,
        date_from,
        date_to,
        base_currency,
        base_account_scope,
    );
    let snapshot = holdings_as_of(
        transactions,
        price_graph,
        date_to,
        base_currency,
        base_account_scope,
        allowed_accounts,
    );
    assemble_loss_harvest(snapshot, &cgt, tax_config, date_to, &label, base_account_scope)
}

/// Shared assembly: filter a holdings snapshot to underwater positions and
/// estimate the tax saved by offsetting those losses against `cgt`'s realised
/// gains. Mirrors `generate_cgt_report`'s ATO method so the figures reconcile.
fn assemble_loss_harvest(
    snapshot: HoldingsSnapshot,
    cgt: &CgtReport,
    tax_config: &TaxConfig,
    as_of_date: &str,
    label: &str,
    base_account_scope: Option<&str>,
) -> LossHarvestReport {
    // Underwater = priced, positive holding whose value is below its cost basis.
    let mut positions: Vec<LossPosition> = snapshot
        .holdings
        .iter()
        .filter(|h| h.has_price && h.quantity > EPSILON && h.unrealised < -EPSILON)
        .map(|h| {
            let unrealised_loss = -h.unrealised;
            let pct_below_cost = if h.cost_basis > EPSILON {
                unrealised_loss / h.cost_basis
            } else {
                0.0
            };
            LossPosition {
                commodity: h.commodity.clone(),
                quantity: h.quantity,
                cost_basis: h.cost_basis,
                value: h.value,
                unrealised_loss,
                pct_below_cost,
                price: h.price,
                price_date: h.price_date.clone(),
                lots: h.lots.clone(),
            }
        })
        .collect();
    positions.sort_by(|a, b| {
        b.unrealised_loss
            .partial_cmp(&a.unrealised_loss)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total_realisable_loss: f64 = positions.iter().map(|p| p.unrealised_loss).sum();

    // Parcel scan: every FIFO parcel below its cost across ALL holdings (not
    // only the net-underwater positions), so underwater parcels hidden inside a
    // net-positive holding are surfaced. Largest loss first.
    let mut underwater_parcels: Vec<UnderwaterParcel> = snapshot
        .holdings
        .iter()
        .filter(|h| h.has_price)
        .flat_map(|h| {
            h.lots
                .iter()
                .filter(|l| l.unrealised < -EPSILON)
                .map(move |l| {
                    let loss = -l.unrealised;
                    UnderwaterParcel {
                        commodity: h.commodity.clone(),
                        acquisition_date: l.acquisition_date.clone(),
                        quantity: l.quantity,
                        cost_per_unit: l.cost_per_unit,
                        cost_basis: l.cost_basis,
                        value: l.value,
                        unrealised_loss: loss,
                        pct_below_cost: if l.cost_basis > EPSILON {
                            loss / l.cost_basis
                        } else {
                            0.0
                        },
                    }
                })
        })
        .collect();
    underwater_parcels.sort_by(|a, b| {
        b.unrealised_loss
            .partial_cmp(&a.unrealised_loss)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total_parcel_loss: f64 = underwater_parcels.iter().map(|p| p.unrealised_loss).sum();

    // Offset the harvestable loss against the FY's realised gains, mirroring the
    // ATO method in `generate_cgt_report`: losses apply BEFORE the discount,
    // against non-discount-eligible (short-term) gains first (otherwise taxed at
    // the full marginal rate), then discount-eligible (long-term) gains. The
    // FY's already-realised losses have consumed some gains, so net those out
    // first to find the gains still standing.
    let discount_factor = 1.0 - tax_config.cgt_discount_percent as f64 / 100.0;
    let losses_after_short = (cgt.total_losses - cgt.short_term_gains).max(0.0);
    let avail_short = (cgt.short_term_gains - cgt.total_losses).max(0.0);
    let avail_long = (cgt.long_term_gains - losses_after_short).max(0.0);

    let offset_short = total_realisable_loss.min(avail_short);
    let offset_long = (total_realisable_loss - offset_short).min(avail_long);
    let offset_now = offset_short + offset_long;
    let carry_forward = total_realisable_loss - offset_now;

    let taxable_reduction = offset_short + discount_factor * offset_long;
    let estimated_tax_saved =
        taxable_reduction * tax_config.marginal_tax_rate_percent as f64 / 100.0;

    LossHarvestReport {
        as_of_date: as_of_date.to_string(),
        financial_year: label.to_string(),
        base_currency: snapshot.base_currency,
        base_account_scope: base_account_scope.map(str::to_string),
        positions,
        total_realisable_loss,
        realised_net_gain: cgt.net_capital_gain,
        realised_short_gains: cgt.short_term_gains,
        realised_long_gains: cgt.long_term_gains,
        offset_now,
        carry_forward,
        marginal_rate_percent: tax_config.marginal_tax_rate_percent,
        cgt_discount_percent: tax_config.cgt_discount_percent,
        estimated_tax_saved,
        underwater_parcels,
        total_parcel_loss,
        warnings: snapshot.warnings,
    }
}

/// Account-segment names that route value but aren't real holdings (in-flight
/// transfers, lending positions, bridge/wrap/staking contras). They're hidden as
/// standalone growth lines — their value still rolls into the parent group at a
/// higher scope. Mirrors the contra set the Balances allowlist excludes (see the
/// doc comment on [`generate_balances_report_range`]).
const CONTRA_SEGMENTS: [&str; 5] = ["transfer", "lending", "bridge", "wrap", "staking"];

/// Top-level groups shown at the root scope: balance-sheet categories only.
/// `income`/`expenses` are flows, not holdings, so they never get a growth line.
const ROOT_GROUPS: [&str; 3] = ["assets", "equity", "liabilities"];

/// Map `account` to its "child group" one level below `scope` — the unit each
/// growth line aggregates — or `None` when the account isn't a qualifying child.
///
/// The key is `account` truncated to `scope_depth + 1` colon-segments:
///   scope `Some("assets")`   → `assets:crypto:eth` ⇒ `"assets:crypto"`
///   scope `None`/`""` (root) → `assets:crypto:eth` ⇒ `"assets"`, `income:x` ⇒ `None`
///
/// Returns `None` for root non-balance-sheet groups and for contra sub-accounts
/// that appear as incidental siblings (final segment is a contra keyword).
/// Scoping *into* a contra (e.g. `scope="assets:lending"`) still shows its
/// children, since then the keyword is part of the scope prefix, not the key tail.
fn child_group_key(account: &str, scope: Option<&str>) -> Option<String> {
    let scope_depth = match scope {
        Some(s) if !s.is_empty() => s.split(':').count(),
        _ => 0,
    };
    let key = account
        .split(':')
        .take(scope_depth + 1)
        .collect::<Vec<_>>()
        .join(":");
    if key.is_empty() {
        return None;
    }
    if scope_depth == 0 && !ROOT_GROUPS.contains(&key.as_str()) {
        return None;
    }
    let last = key.rsplit(':').next().unwrap_or(&key);
    if CONTRA_SEGMENTS.iter().any(|c| c.eq_ignore_ascii_case(last)) {
        return None;
    }
    Some(key)
}

/// Market value at `as_of_date` bucketed by child group (one level below
/// `base_account_scope`). Mirrors the Balances valuation
/// ([`generate_balances_report_range`]): base-currency cash at face, priced
/// commodities at market (`{date}T23:59:59`), unpriced silently dropped, `ignore:*`
/// transactions skipped. Deliberately takes NO allowlist — the root `equity` /
/// `liabilities` lines the growth chart needs would be filtered out by the
/// folder-derived primary-accounts allowlist (so the chart's `assets` total may
/// differ slightly from the allowlist-filtered `portfolio_value` line, which is
/// fine: the lines are independently rebased on a separate chart). Non-holding
/// groups (income/expenses) and contras are excluded by [`child_group_key`].
fn child_group_values_as_of(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    as_of_date: &str,
    base_currency: &str,
    base_account_scope: Option<&str>,
) -> std::collections::BTreeMap<String, f64> {
    // (group, commodity) → net quantity. BTreeMap so the downstream per-group
    // f64 sums are computed in a stable order (same determinism invariant the
    // Balances report guards — see `pipeline_is_deterministic_across_two_cold_runs`).
    let mut by_group_commodity: std::collections::BTreeMap<(String, String), f64> =
        std::collections::BTreeMap::new();
    for txn in transactions {
        let txn_date_len = txn.datetime.len().min(10);
        if &txn.datetime[..txn_date_len] > as_of_date {
            continue;
        }
        if txn
            .postings
            .iter()
            .any(|p| p.account == "ignore" || p.account.starts_with("ignore:"))
        {
            continue;
        }
        for posting in &txn.postings {
            let in_scope = match base_account_scope {
                Some(scope) => {
                    posting.account == scope
                        || posting.account.starts_with(&format!("{scope}:"))
                }
                None => true,
            };
            if !in_scope {
                continue;
            }
            if let Some(group) = child_group_key(&posting.account, base_account_scope) {
                *by_group_commodity
                    .entry((group, posting.commodity.clone()))
                    .or_insert(0.0) += posting.amount;
            }
        }
    }

    let datetime_key = format!("{as_of_date}T23:59:59");
    let mut values: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    for ((group, commodity), qty) in &by_group_commodity {
        if commodity == base_currency {
            *values.entry(group.clone()).or_insert(0.0) += *qty;
        } else if let Some(price) =
            price_graph.convert_to_base(commodity, 1.0, &datetime_key, base_currency)
        {
            *values.entry(group.clone()).or_insert(0.0) += *qty * price;
        }
        // Unpriced non-base commodity: dropped (matches Balances/holdings_as_of).
    }
    values
}

/// Inputs for [`generate_performance_report_range`]. Bundled into a struct
/// because the report needs the full tax / pricing / scope context — more
/// distinct parameters than a positional arg list should carry — and so the
/// call site stays self-documenting.
pub struct PerformanceReportParams<'a> {
    pub transactions: &'a [Transaction],
    pub price_graph: &'a PriceGraph,
    pub tax_config: &'a TaxConfig,
    pub label: &'a str,
    pub date_from: &'a str,
    pub date_to: &'a str,
    pub base_currency: &'a str,
    pub base_account_scope: Option<&'a str>,
    pub allowed_accounts: Option<&'a AccountAllowlist>,
}

/// Generate a performance report over `[date_from, date_to]`.
///
/// Reuses the existing range reports for the flows and the new
/// [`holdings_as_of`] snapshot for the stocks:
///   - realised CG per month: one [`generate_cgt_report_range`] call, events
///     bucketed by `sell_date` month (raw `capital_gain`, pre-discount);
///   - income per month: one [`generate_income_report_range`] call, events
///     bucketed by `date` month (`value`);
///   - value + cost basis at each month-end: one [`holdings_as_of`] per point.
///
/// `total_return = total_realised_gain + total_income + closing_unrealised_gain`
/// — see [`PerformanceReport::total_return`] for the (deliberate) semantics.
pub fn generate_performance_report_range(
    params: PerformanceReportParams<'_>,
) -> PerformanceReport {
    let PerformanceReportParams {
        transactions,
        price_graph,
        tax_config,
        label,
        date_from,
        date_to,
        base_currency,
        base_account_scope,
        allowed_accounts,
    } = params;

    // Realised capital gains over the window (one walk), bucketed by month.
    let cgt = generate_cgt_report_range(
        transactions,
        price_graph,
        tax_config,
        label,
        date_from,
        date_to,
        base_currency,
        base_account_scope,
    );
    // Realised is computed window-relative (rebased to the window-open value)
    // after the opening snapshot — see `realised_window_by_month` below.

    // Income over the window (one walk), bucketed by month.
    let income = generate_income_report_range(
        transactions,
        price_graph,
        tax_config,
        label,
        date_from,
        date_to,
        base_currency,
        base_account_scope,
    );
    let mut income_by_month: HashMap<String, f64> = HashMap::new();
    for e in &income.events {
        if e.date.len() >= 7 {
            *income_by_month.entry(e.date[..7].to_string()).or_insert(0.0) += e.value;
        }
    }

    // Window-open baseline: holdings as of the day before date_from (prior-
    // period close). Anchors the period return and the per-month unrealised
    // change. Its own warnings are dropped (only the closing snapshot's kept).
    let opening_date = day_before(date_from);
    let opening = holdings_as_of(
        transactions,
        price_graph,
        &opening_date,
        base_currency,
        base_account_scope,
        allowed_accounts,
    );
    let unrealised_open = opening.total_unrealised;
    let value_open = opening.total_value;
    // Per-child-group opening values — the growth chart's baseline, aligned with
    // the opening `points[0]`. Same scope filter as the `opening` snapshot above.
    let opening_groups = child_group_values_as_of(
        transactions,
        price_graph,
        &opening_date,
        base_currency,
        base_account_scope,
    );

    // Window-relative realised: a performance report measures gains from the
    // report start, so lots acquired BEFORE the window are rebased to their
    // window-open value (the pre-window gain belongs in the tax/CGT report). Lots
    // acquired during the window keep their cost basis (already window-relative).
    //   realised_window = proceeds − window-open value          (pre-window lots)
    //   realised_window = proceeds − cost basis = capital_gain  (in-window lots)
    let mut open_price: HashMap<String, f64> = HashMap::new();
    for h in &opening.holdings {
        open_price.insert(h.commodity.clone(), h.price);
    }
    let mut realised_window_by_month: HashMap<String, f64> = HashMap::new();
    let mut realised_window_by_commodity: HashMap<String, f64> = HashMap::new();
    let mut open_gain_sold_by_month: HashMap<String, f64> = HashMap::new();
    let mut open_gain_sold_by_commodity: HashMap<String, f64> = HashMap::new();
    for e in &cgt.events {
        // Pre-window embedded gain on the sold lot (open value − cost). Subtracted
        // from lifetime realised so only the in-window movement remains.
        let open_gain_sold = if e.buy_date.as_str() < date_from {
            match open_price.get(&e.commodity) {
                Some(op) => op * e.quantity - e.cost_basis,
                None => 0.0, // unpriced at open: cannot rebase, keep lifetime gain
            }
        } else {
            0.0
        };
        let realised_window = e.capital_gain - open_gain_sold;
        if e.sell_date.len() >= 7 {
            let mk = e.sell_date[..7].to_string();
            *realised_window_by_month.entry(mk.clone()).or_insert(0.0) += realised_window;
            *open_gain_sold_by_month.entry(mk).or_insert(0.0) += open_gain_sold;
        }
        *realised_window_by_commodity
            .entry(e.commodity.clone())
            .or_insert(0.0) += realised_window;
        *open_gain_sold_by_commodity
            .entry(e.commodity.clone())
            .or_insert(0.0) += open_gain_sold;
    }

    // Month-end snapshots: value + cost basis at each point, plus the unrealised
    // *change* over that month (snapshot minus the previous one, starting from
    // the window-open baseline).
    let dates = month_end_snapshot_dates(date_from, date_to);
    let mut points: Vec<PerformancePoint> = Vec::new();
    // Opening baseline as the first point, so the chart and table start at the
    // window open (prior-period close) and the curve shows the FULL window — not
    // the first month-end, which can sit just after a big early move. Zero flows;
    // it's a starting marker.
    points.push(PerformancePoint {
        date: opening_date.clone(),
        label: "Open".to_string(),
        realised_gain: 0.0,
        income: 0.0,
        portfolio_value: opening.total_value,
        cost_basis: opening.total_cost_basis,
        unrealised_gain: opening.total_unrealised,
        unrealised_change: 0.0,
    });
    let mut warnings: Vec<String> = Vec::new();
    warnings.extend(cgt.warnings.iter().cloned());
    warnings.extend(income.warnings.iter().cloned());

    let mut closing_holdings: Vec<CommodityHolding> = Vec::new();
    let mut closing_value = value_open;
    let mut closing_cost_basis = opening.total_cost_basis;
    let mut closing_unrealised = unrealised_open;
    let mut prev_unrealised = unrealised_open;
    let last_idx = dates.len().saturating_sub(1);
    // Per-child-group values at each month-end, pushed in lock-step with the
    // points loop so `month_group_maps[i]` lines up with `points[i + 1]`.
    let mut month_group_maps: Vec<std::collections::BTreeMap<String, f64>> = Vec::new();

    for (i, d) in dates.iter().enumerate() {
        let snap = holdings_as_of(
            transactions,
            price_graph,
            d,
            base_currency,
            base_account_scope,
            allowed_accounts,
        );
        month_group_maps.push(child_group_values_as_of(
            transactions,
            price_graph,
            d,
            base_currency,
            base_account_scope,
        ));

        let month_key = &d[..7.min(d.len())];
        let realised_gain = realised_window_by_month.get(month_key).copied().unwrap_or(0.0);
        let income_amt = income_by_month.get(month_key).copied().unwrap_or(0.0);
        // Window-relative MtM: raw change this month + the pre-window gain on lots
        // SOLD this month (which left the unrealised bucket as they realised).
        let unrealised_change = (snap.total_unrealised - prev_unrealised)
            + open_gain_sold_by_month.get(month_key).copied().unwrap_or(0.0);
        prev_unrealised = snap.total_unrealised;

        if i == last_idx {
            closing_holdings = snap.holdings.clone();
            closing_value = snap.total_value;
            closing_cost_basis = snap.total_cost_basis;
            closing_unrealised = snap.total_unrealised;
            // Only the closing snapshot's warnings: intermediate month-ends
            // re-walk the same history and would multiply every persistent
            // issue (short positions, carried-at-cost) across all ~12 points.
            warnings.extend(snap.warnings.iter().cloned());
        }

        points.push(PerformancePoint {
            date: d.clone(),
            label: month_label(d),
            realised_gain,
            income: income_amt,
            portfolio_value: snap.total_value,
            cost_basis: snap.total_cost_basis,
            unrealised_gain: snap.total_unrealised,
            unrealised_change,
        });
    }

    // Per-commodity attribution, split window-relative: realised = proceeds −
    // window-open value (sold units); unrealised = current − window-open value
    // (held units). `Σ total + total_income` reconciles to total_return, so the
    // table shows where the window's performance came from — distinct from the
    // closing unrealised *level* (e.g. revalued property held all year shows ~0
    // here even though its level is large).
    let mut open_unreal: HashMap<String, f64> = HashMap::new();
    for h in &opening.holdings {
        open_unreal.insert(h.commodity.clone(), h.unrealised);
    }
    let mut close_unreal: HashMap<String, f64> = HashMap::new();
    let mut close_value: HashMap<String, f64> = HashMap::new();
    for h in &closing_holdings {
        close_unreal.insert(h.commodity.clone(), h.unrealised);
        close_value.insert(h.commodity.clone(), h.value);
    }
    let mut attr_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    attr_keys.extend(realised_window_by_commodity.keys().cloned());
    attr_keys.extend(open_unreal.keys().cloned());
    attr_keys.extend(close_unreal.keys().cloned());
    let mut attribution: Vec<CommodityPerformance> = attr_keys
        .into_iter()
        .map(|c| {
            let realised = realised_window_by_commodity.get(&c).copied().unwrap_or(0.0);
            let open_gain_sold = open_gain_sold_by_commodity.get(&c).copied().unwrap_or(0.0);
            let open_u = open_unreal.get(&c).copied().unwrap_or(0.0);
            let close_u = close_unreal.get(&c).copied().unwrap_or(0.0);
            let close_v = close_value.get(&c).copied().unwrap_or(0.0);
            // Window-relative unrealised on still-held units: raw Δ + the
            // pre-window gain on sold units (which moved out to realised).
            let unrealised_change = (close_u - open_u) + open_gain_sold;
            CommodityPerformance {
                commodity: c,
                realised_gain: realised,
                unrealised_change,
                total: realised + unrealised_change,
                closing_value: close_v,
                closing_unrealised: close_u,
            }
        })
        .collect();
    // Drop dead rows (no contribution and nothing held); largest contribution
    // first, commodity name as a deterministic tiebreaker.
    attribution.retain(|c| c.total.abs() > EPSILON || c.closing_value.abs() > EPSILON);
    attribution.sort_by(|a, b| {
        b.total
            .partial_cmp(&a.total)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.commodity.cmp(&b.commodity))
    });

    // Dedup identical warnings, then cap so a messy ledger (unresolved cost
    // bases, illiquid tokens carried at cost) can't flood the UI with thousands
    // of notices — the Capital Gains report carries the full detail.
    warnings.sort();
    warnings.dedup();
    const MAX_WARNINGS: usize = 50;
    if warnings.len() > MAX_WARNINGS {
        let extra = warnings.len() - MAX_WARNINGS;
        warnings.truncate(MAX_WARNINGS);
        warnings.push(format!(
            "\u{2026} and {extra} more warnings (see the Capital Gains report for full detail)"
        ));
    }

    // Period return: realised + income flows + the *change* in unrealised gain
    // over the window (NOT lifetime embedded gain, which belongs in the tax/CGT
    // report). total_return uses the lifetime-realised + Δunrealised identity;
    // the headline realised / unrealised are then re-split window-relative
    // (rebased to window-open) so they match the per-holding attribution and
    // still sum to the same total_return. Percentage is on opening value.
    let total_income = income.total_income;
    let total_return =
        cgt.net_capital_gain + total_income + (closing_unrealised - unrealised_open);
    let total_realised_gain: f64 = realised_window_by_commodity.values().sum();
    let unrealised_change = total_return - total_income - total_realised_gain;
    let total_return_pct = if value_open.abs() > EPSILON {
        Some(total_return / value_open)
    } else {
        None
    };

    // Per-direct-child value series for the growth-by-category chart, aligned
    // 1:1 with `points` (opening baseline first, then each month-end). Raw values
    // — the frontend rebases each to a 0%-at-open growth line. Groups that never
    // carry value across the window are dropped so the legend stays clean.
    let account_breakdown = {
        let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        keys.extend(opening_groups.keys().cloned());
        for m in &month_group_maps {
            keys.extend(m.keys().cloned());
        }
        let mut series: Vec<AccountValueSeries> = keys
            .into_iter()
            .map(|account| {
                let mut values = Vec::with_capacity(points.len());
                values.push(opening_groups.get(&account).copied().unwrap_or(0.0));
                for m in &month_group_maps {
                    values.push(m.get(&account).copied().unwrap_or(0.0));
                }
                AccountValueSeries { account, values }
            })
            .filter(|s| s.values.iter().any(|v| v.abs() > EPSILON))
            .collect();
        // Largest current category first; account name as a stable tiebreaker
        // (mirrors the attribution sort).
        series.sort_by(|a, b| {
            let av = a.values.last().copied().unwrap_or(0.0);
            let bv = b.values.last().copied().unwrap_or(0.0);
            bv.partial_cmp(&av)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.account.cmp(&b.account))
        });
        series
    };

    PerformanceReport {
        label: label.to_string(),
        date_from: date_from.to_string(),
        date_to: date_to.to_string(),
        base_currency: base_currency.to_string(),
        base_account_scope: base_account_scope.map(|s| s.to_string()),
        points,
        closing_holdings,
        attribution,
        total_realised_gain,
        total_income,
        unrealised_change,
        value_open,
        closing_value,
        closing_cost_basis,
        total_return,
        total_return_pct,
        account_breakdown,
        warnings,
    }
}

/// Collect all distinct income and expense account names from transactions.
pub fn list_report_accounts(transactions: &[Transaction]) -> (Vec<String>, Vec<String>) {
    let mut income: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut expenses: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for txn in transactions {
        for posting in &txn.postings {
            if posting.account.starts_with("income:") {
                income.insert(posting.account.clone());
            } else if posting.account.starts_with("expenses:") {
                expenses.insert(posting.account.clone());
            }
        }
    }
    (income.into_iter().collect(), expenses.into_iter().collect())
}
