//! CSV serialisers for the three report types. Lives in its own module so the
//! pipeline-time writer (`report_templates`) and the on-demand Tauri export
//! command share one source of truth for the columns.

use crate::reports::{partition_cgt_events, BalancesReport, CgtReport, IncomeEvent, IncomeTaxReport};

/// Sectioned, human-readable CGT layout — one CSV file with a cover block, then
/// Short-Term and Long-Term sections, each grouped by commodity with subtotals.
/// Mirrors the on-screen / PDF report flow rather than a flat machine table.
pub fn cgt_to_csv(report: &CgtReport) -> Result<String, String> {
    let mut wtr = csv::WriterBuilder::new().flexible(true).from_writer(vec![]);
    let w = |wtr: &mut csv::Writer<Vec<u8>>, row: &[&str]| -> Result<(), String> {
        wtr.write_record(row).map_err(|e| format!("csv: {e}"))
    };

    let (short_term, long_term, losses) = partition_cgt_events(&report.events);

    // --- Cover sheet --- totals come from the report struct (single source of
    // truth shared with the UI), not re-derived here.
    w(&mut wtr, &["Capital Gains Report"])?;
    w(&mut wtr, &["Financial Year", &report.financial_year])?;
    w(&mut wtr, &["Inventory Method", "FIFO"])?;
    w(&mut wtr, &[])?;
    w(&mut wtr, &["Report Summary"])?;
    w(&mut wtr, &["Short Term Capital Gains", &fmt_money(report.short_term_gains)])?;
    w(&mut wtr, &["Long Term Capital Gains", &fmt_money(report.long_term_gains)])?;
    w(&mut wtr, &["Total Capital Losses", &fmt_money(-report.total_losses)])?;
    w(&mut wtr, &["Net Total Capital Gains", &fmt_money(report.net_capital_gain)])?;
    w(&mut wtr, &["Discounted Gain", &fmt_money(report.total_discounted_gain)])?;
    w(&mut wtr, &[])?;

    write_section(&mut wtr, "Short-Term", &short_term)?;
    w(&mut wtr, &[])?;
    write_section(&mut wtr, "Long-Term", &long_term)?;
    w(&mut wtr, &[])?;
    write_section(&mut wtr, "Losses", &losses)?;

    finish(wtr)
}

fn write_section(
    wtr: &mut csv::Writer<Vec<u8>>,
    title: &str,
    events: &[&crate::reports::CgtEvent],
) -> Result<(), String> {
    let w = |wtr: &mut csv::Writer<Vec<u8>>, row: &[&str]| -> Result<(), String> {
        wtr.write_record(row).map_err(|e| format!("csv: {e}"))
    };

    w(wtr, &[title])?;

    // Section totals row (above the per-commodity blocks). Quantity omitted
    // because it sums across commodities and is meaningless.
    let tot_cost: f64 = events.iter().map(|e| e.cost_basis).sum();
    let tot_proc: f64 = events.iter().map(|e| e.sale_proceeds).sum();
    let tot_gain: f64 = events.iter().map(|e| e.capital_gain).sum();
    w(
        wtr,
        &[
            "Section Total",
            "",
            "",
            "",
            &fmt_money(tot_cost),
            &fmt_money(tot_proc),
            &fmt_money(tot_gain),
        ],
    )?;
    w(wtr, &["Commodity", "Bought", "Sold", "Quantity", "Cost", "Proceeds", "Gain/Loss", "Days"])?;

    if events.is_empty() {
        w(wtr, &["(no events)"])?;
        return Ok(());
    }

    // Group by commodity, preserving first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<&crate::reports::CgtEvent>> =
        std::collections::HashMap::new();
    for e in events {
        if !groups.contains_key(&e.commodity) {
            order.push(e.commodity.clone());
        }
        groups.entry(e.commodity.clone()).or_default().push(*e);
    }

    for commodity in &order {
        let rows = &groups[commodity];
        w(wtr, &[commodity])?;
        let mut s_qty = 0.0;
        let mut s_cost = 0.0;
        let mut s_proc = 0.0;
        let mut s_gain = 0.0;
        for e in rows {
            w(
                wtr,
                &[
                    "",
                    &e.buy_date,
                    &e.sell_date,
                    &fmt_qty(e.quantity),
                    &fmt_money(e.cost_basis),
                    &fmt_money(e.sale_proceeds),
                    &fmt_money(e.capital_gain),
                    &e.holding_days.to_string(),
                ],
            )?;
            s_qty += e.quantity;
            s_cost += e.cost_basis;
            s_proc += e.sale_proceeds;
            s_gain += e.capital_gain;
        }
        w(
            wtr,
            &[
                &format!("{commodity} subtotal"),
                "",
                "",
                &fmt_qty(s_qty),
                &fmt_money(s_cost),
                &fmt_money(s_proc),
                &fmt_money(s_gain),
            ],
        )?;
    }
    Ok(())
}

/// Sectioned, human-readable Income/Expenses layout — cover block, then an
/// Income section and an Expenses section, each grouped by commodity with
/// per-commodity subtotals (mirrors `cgt_to_csv` and the on-screen layout).
pub fn income_to_csv(report: &IncomeTaxReport) -> Result<String, String> {
    let mut wtr = csv::WriterBuilder::new().flexible(true).from_writer(vec![]);
    let w = |wtr: &mut csv::Writer<Vec<u8>>, row: &[&str]| -> Result<(), String> {
        wtr.write_record(row).map_err(|e| format!("csv: {e}"))
    };

    // --- Cover sheet ---
    w(&mut wtr, &["Income Tax Report"])?;
    w(&mut wtr, &["Financial Year", &report.financial_year])?;
    w(&mut wtr, &[])?;
    w(&mut wtr, &["Report Summary"])?;
    w(&mut wtr, &["Total Income", &fmt_money(report.total_income)])?;
    w(&mut wtr, &["Total Expenses", &fmt_money(report.total_expenses)])?;
    w(&mut wtr, &["Net", &fmt_money(report.net)])?;
    w(&mut wtr, &[])?;

    write_income_section(&mut wtr, "Income", &report.events)?;
    w(&mut wtr, &[])?;
    write_income_section(&mut wtr, "Expenses", &report.expense_events)?;

    finish(wtr)
}

fn write_income_section(
    wtr: &mut csv::Writer<Vec<u8>>,
    title: &str,
    events: &[IncomeEvent],
) -> Result<(), String> {
    let w = |wtr: &mut csv::Writer<Vec<u8>>, row: &[&str]| -> Result<(), String> {
        wtr.write_record(row).map_err(|e| format!("csv: {e}"))
    };

    w(wtr, &[title])?;

    // Section total row above the per-commodity blocks. Quantity is omitted
    // because it sums across commodities and is meaningless.
    let tot_value: f64 = events.iter().map(|e| e.value).sum();
    w(
        wtr,
        &[
            "Section Total",
            "",
            "",
            "",
            "",
            "",
            &fmt_money(tot_value),
        ],
    )?;
    w(wtr, &["Commodity", "Date", "Where", "Category", "Quantity", "Price", "Value"])?;

    if events.is_empty() {
        w(wtr, &["(no events)"])?;
        return Ok(());
    }

    // Group by commodity, preserving first-seen order.
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<&IncomeEvent>> =
        std::collections::HashMap::new();
    for e in events {
        if !groups.contains_key(&e.commodity) {
            order.push(e.commodity.clone());
        }
        groups.entry(e.commodity.clone()).or_default().push(e);
    }

    for commodity in &order {
        let rows = &groups[commodity];
        w(wtr, &[commodity])?;
        let mut s_qty = 0.0;
        let mut s_value = 0.0;
        for e in rows {
            w(
                wtr,
                &[
                    "",
                    &e.date,
                    &e.asset_account,
                    &e.account,
                    &fmt_qty(e.quantity),
                    &fmt_money(e.price),
                    &fmt_money(e.value),
                ],
            )?;
            s_qty += e.quantity;
            s_value += e.value;
        }
        w(
            wtr,
            &[
                &format!("{commodity} subtotal"),
                "",
                "",
                "",
                &fmt_qty(s_qty),
                "",
                &fmt_money(s_value),
            ],
        )?;
    }
    Ok(())
}

/// Per-commodity holdings. Account-level breakdown isn't flattened — that's a
/// nested view, and CSV consumers usually want one row per asset for valuation.
pub fn balances_to_csv(report: &BalancesReport) -> Result<String, String> {
    let mut wtr = csv::Writer::from_writer(vec![]);
    wtr.write_record([
        "commodity",
        "quantity",
        "price",
        "price_date",
        "value",
        "portfolio_weight",
        "base_currency",
        "as_of_date",
    ])
    .map_err(|e| format!("csv: {e}"))?;
    for h in &report.holdings {
        wtr.write_record([
            &h.commodity,
            &fmt(h.quantity),
            &fmt(h.price),
            &h.price_date,
            &fmt(h.value),
            &fmt(h.portfolio_weight),
            &report.base_currency,
            &report.as_of_date,
        ])
        .map_err(|e| format!("csv: {e}"))?;
    }
    finish(wtr)
}

fn fmt(v: f64) -> String {
    // Plain decimal, no scientific notation. Six places is enough headroom for
    // crypto-quantity precision while keeping fiat columns readable.
    format!("{v:.6}")
}

fn fmt_money(v: f64) -> String {
    format!("{v:.2}")
}

fn fmt_qty(v: f64) -> String {
    // 8dp covers BTC-precision; trim trailing zeros for readability.
    let s = format!("{v:.8}");
    if s.contains('.') {
        let t = s.trim_end_matches('0').trim_end_matches('.');
        t.to_string()
    } else {
        s
    }
}

fn finish(wtr: csv::Writer<Vec<u8>>) -> Result<String, String> {
    let bytes = wtr
        .into_inner()
        .map_err(|e| format!("csv finalise: {e}"))?;
    String::from_utf8(bytes).map_err(|e| format!("csv utf8: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reports::{
        AccountBalance, BalancesReport, CgtEvent, CgtReport, CoinBalance, IncomeTaxReport,
    };

    #[test]
    fn cgt_csv_has_header_and_event_row() {
        let r = CgtReport {
            financial_year: "2024".into(),
            events: vec![CgtEvent {
                sell_date: "2024-05-01".into(),
                buy_date: "2023-01-15".into(),
                commodity: "BTC".into(),
                quantity: 0.5,
                cost_basis: 10_000.0,
                sale_proceeds: 25_000.0,
                capital_gain: 15_000.0,
                holding_days: 472,
                discount_eligible: true,
                discounted_gain: 7_500.0,
                trade_link_id: "tl1".into(),
                sell_txn_id: "tx-sell".into(),
                sell_account: "assets:exchange:kraken".into(),
            }],
            total_gains: 15_000.0,
            total_losses: 0.0,
            short_term_gains: 0.0,
            long_term_gains: 15_000.0,
            net_capital_gain: 15_000.0,
            total_discounted_gain: 7_500.0,
            warnings: vec![],
        };
        let csv = cgt_to_csv(&r).unwrap();
        assert!(csv.starts_with("Capital Gains Report"));
        assert!(csv.contains("Net Total Capital Gains,15000.00"));
        assert!(csv.contains("Long-Term"));
        assert!(csv.contains("\nBTC\n"));
        assert!(csv.contains(",2023-01-15,2024-05-01,0.5,10000.00,25000.00,15000.00,472"));
        assert!(csv.contains("BTC subtotal,,,0.5,10000.00,25000.00,15000.00"));
        assert!(csv.contains("Section Total,,,,10000.00,25000.00,15000.00"));
    }

    #[test]
    fn income_csv_has_cover_and_section_headers() {
        let r = IncomeTaxReport {
            financial_year: "2024".into(),
            income_categories: vec![],
            expense_categories: vec![],
            events: vec![],
            expense_events: vec![],
            total_income: 100.0,
            total_expenses: 5.0,
            net: 95.0,
            warnings: vec![],
        };
        let csv = income_to_csv(&r).unwrap();
        assert!(csv.starts_with("Income Tax Report"));
        assert!(csv.contains("Total Income,100.00"));
        assert!(csv.contains("Total Expenses,5.00"));
        assert!(csv.contains("Net,95.00"));
        // Both sections are emitted, even when empty.
        assert!(csv.contains("\nIncome\n"));
        assert!(csv.contains("\nExpenses\n"));
        assert!(csv.contains("(no events)"));
    }

    #[test]
    fn income_csv_groups_events_by_commodity_with_subtotals() {
        use crate::reports::IncomeEvent;
        let r = IncomeTaxReport {
            financial_year: "2026".into(),
            income_categories: vec![],
            expense_categories: vec![],
            events: vec![
                IncomeEvent {
                    date: "2025-08-15".into(),
                    account: "income:staking:eth".into(),
                    commodity: "ETH".into(),
                    quantity: 0.25,
                    price: 4000.0,
                    value: 1000.0,
                    base_currency: "AUD".into(),
                    txn_id: "txn:a".into(),
                    asset_account: "assets:crypto:wallet:eth".into(),
                },
                IncomeEvent {
                    date: "2025-09-10".into(),
                    account: "income:staking:eth".into(),
                    commodity: "ETH".into(),
                    quantity: 0.5,
                    price: 3000.0,
                    value: 1500.0,
                    base_currency: "AUD".into(),
                    txn_id: "txn:b".into(),
                    asset_account: "assets:crypto:wallet:eth".into(),
                },
                IncomeEvent {
                    date: "2025-08-20".into(),
                    account: "income:trading:fees".into(),
                    commodity: "USDC".into(),
                    quantity: -100.0,
                    price: 1.5,
                    value: -150.0,
                    base_currency: "AUD".into(),
                    txn_id: "txn:c".into(),
                    asset_account: "assets:crypto:exchange:hyperliquid:personal".into(),
                },
            ],
            expense_events: vec![],
            total_income: 2350.0,
            total_expenses: 0.0,
            net: 2350.0,
            warnings: vec![],
        };
        let csv = income_to_csv(&r).unwrap();
        assert!(csv.contains("\nIncome\n"));
        assert!(csv.contains("Section Total,,,,,,2350.00"));
        assert!(csv.contains("Commodity,Date,Where,Category,Quantity,Price,Value"));
        // Per-commodity block: header line, detail rows, subtotal.
        assert!(csv.contains("\nETH\n"));
        assert!(csv.contains(",2025-08-15,assets:crypto:wallet:eth,income:staking:eth,0.25,4000.00,1000.00"));
        assert!(csv.contains("ETH subtotal,,,,0.75,,2500.00"));
        // Negative-value (fee) row keeps its sign in the CSV.
        assert!(csv.contains("\nUSDC\n"));
        assert!(csv.contains(",2025-08-20,assets:crypto:exchange:hyperliquid:personal,income:trading:fees,-100,1.50,-150.00"));
        assert!(csv.contains("USDC subtotal,,,,-100,,-150.00"));
    }

    #[test]
    fn balances_csv_one_row_per_commodity() {
        let r = BalancesReport {
            as_of_date: "2024-06-30".into(),
            base_currency: "AUD".into(),
            base_account_scope: None,
            holdings: vec![CoinBalance {
                commodity: "ETH".into(),
                quantity: 2.0,
                price: 3000.0,
                price_date: "2024-06-30".into(),
                value: 6000.0,
                portfolio_weight: 1.0,
                accounts: vec![AccountBalance {
                    account: "assets:wallet".into(),
                    quantity: 2.0,
                    value: 6000.0,
                }],
            }],
            total_value: 6000.0,
            warnings: vec![],
        };
        let csv = balances_to_csv(&r).unwrap();
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[1].contains("ETH,2.000000,3000.000000"));
        assert!(lines[1].ends_with("AUD,2024-06-30"));
    }
}
