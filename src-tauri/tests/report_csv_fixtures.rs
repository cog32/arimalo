//! Fixture-driven tests for `report_csv.rs` covering edge cases (issue #80):
//!   * Embedded commas in strings (commodity names, account paths, narrations).
//!   * Very large and very small numbers (no scientific notation).
//!   * Negative values (fees, losses).
//!   * Locale-safety: decimal separator is always `.`, never `,`.

use arimalo_covid::report_csv::{balances_to_csv, cgt_to_csv, income_to_csv};
use arimalo_covid::reports::{
    AccountBalance, BalancesReport, CgtEvent, CgtReport, CoinBalance, IncomeEvent, IncomeTaxReport,
};

// === Helpers ===

fn base_cgt(events: Vec<CgtEvent>) -> CgtReport {
    let total_gains: f64 = events.iter().filter(|e| e.capital_gain > 0.0).map(|e| e.capital_gain).sum();
    let total_losses: f64 = events.iter().filter(|e| e.capital_gain < 0.0).map(|e| e.capital_gain.abs()).sum();
    CgtReport {
        financial_year: "2025".into(),
        short_term_gains: total_gains,
        long_term_gains: 0.0,
        total_gains,
        total_losses,
        net_capital_gain: total_gains - total_losses,
        total_discounted_gain: 0.0,
        events,
        warnings: vec![],
    }
}

fn base_income(events: Vec<IncomeEvent>) -> IncomeTaxReport {
    let total_income: f64 = events.iter().map(|e| e.value).sum();
    IncomeTaxReport {
        financial_year: "2025".into(),
        income_categories: vec![],
        expense_categories: vec![],
        events,
        expense_events: vec![],
        total_income,
        total_expenses: 0.0,
        net: total_income,
        warnings: vec![],
    }
}

fn cgt_event(commodity: &str, gain: f64) -> CgtEvent {
    CgtEvent {
        sell_date: "2025-01-01".into(),
        buy_date: "2024-01-01".into(),
        commodity: commodity.into(),
        quantity: 1.0,
        cost_basis: 1000.0,
        sale_proceeds: 1000.0 + gain,
        capital_gain: gain,
        holding_days: 366,
        discount_eligible: false,
        discounted_gain: 0.0,
        trade_link_id: "tl1".into(),
        sell_txn_id: "tx1".into(),
        sell_account: "assets:exchange:kraken".into(),
    }
}

fn income_event(commodity: &str, account: &str, value: f64) -> IncomeEvent {
    IncomeEvent {
        date: "2025-01-01".into(),
        account: account.into(),
        commodity: commodity.into(),
        quantity: 1.0,
        price: value,
        value,
        base_currency: "AUD".into(),
        txn_id: "tx1".into(),
        asset_account: "assets:crypto:wallet:eth".into(),
    }
}

// === 1. Embedded commas in commodity names ===
//
// CSV consumers must not split a commodity like "ETH,USD" into two cells.
// The `csv` crate handles this by quoting — we assert the string is in the
// output without being split.

#[test]
fn cgt_csv_quotes_commodity_with_embedded_comma() {
    let report = base_cgt(vec![cgt_event("ETH,USD", 500.0)]);
    let csv = cgt_to_csv(&report).unwrap();
    // The commodity cell must appear quoted and intact, not split across fields.
    assert!(csv.contains("\"ETH,USD\""), "embedded comma not quoted: {csv}");
}

#[test]
fn income_csv_quotes_commodity_with_embedded_comma() {
    let report = base_income(vec![income_event("ETH,USD", "income:staking", 100.0)]);
    let csv = income_to_csv(&report).unwrap();
    assert!(csv.contains("\"ETH,USD\""), "embedded comma not quoted: {csv}");
}

#[test]
fn balances_csv_quotes_commodity_with_embedded_comma() {
    let report = BalancesReport {
        as_of_date: "2025-06-30".into(),
        base_currency: "AUD".into(),
        base_account_scope: None,
        holdings: vec![CoinBalance {
            commodity: "ETH,PERP".into(),
            quantity: 10.0,
            price: 3000.0,
            price_date: "2025-06-30".into(),
            value: 30_000.0,
            portfolio_weight: 1.0,
            accounts: vec![AccountBalance {
                account: "assets:exchange".into(),
                quantity: 10.0,
                value: 30_000.0,
            }],
        }],
        total_value: 30_000.0,
        warnings: vec![],
    };
    let csv = balances_to_csv(&report).unwrap();
    assert!(csv.contains("\"ETH,PERP\""), "embedded comma not quoted: {csv}");
}

// === 2. Embedded commas in account paths ===
//
// Account names don't normally contain commas, but defensive quoting is
// tested to ensure the CSV library's quoting is active end-to-end.

#[test]
fn income_csv_quotes_account_with_embedded_comma() {
    let report = base_income(vec![income_event("ETH", "income:staking,rewards", 200.0)]);
    let csv = income_to_csv(&report).unwrap();
    assert!(
        csv.contains("\"income:staking,rewards\""),
        "account with comma not quoted: {csv}"
    );
}

// === 3. Very large numbers — no scientific notation ===
//
// Rust's default f64 Display switches to scientific notation for very large
// or very small magnitudes. The formatters in report_csv.rs must always emit
// plain decimal so CSV consumers don't receive "1.23e10".
//
// We check for the `e±` exponent pattern in numeric positions, not the
// letter 'e' itself (which appears in column headers like "price", "value").

fn has_scientific_notation(csv: &str) -> bool {
    // Match patterns like 1.23e+10, 9e-5, 1E10 that appear in numeric fields.
    // We look for a digit followed by e/E followed by an optional sign and digits.
    let bytes = csv.as_bytes();
    for i in 1..bytes.len().saturating_sub(1) {
        if (bytes[i] == b'e' || bytes[i] == b'E')
            && bytes[i - 1].is_ascii_digit()
            && (bytes[i + 1].is_ascii_digit()
                || bytes[i + 1] == b'+'
                || bytes[i + 1] == b'-')
        {
            return true;
        }
    }
    false
}

#[test]
fn cgt_csv_no_scientific_notation_for_large_gain() {
    let mut e = cgt_event("BTC", 0.0);
    e.cost_basis = 1.0;
    e.sale_proceeds = 1_000_000_000.0; // 1 billion AUD
    e.capital_gain = 999_999_999.0;
    let report = base_cgt(vec![e]);
    let csv = cgt_to_csv(&report).unwrap();
    assert!(!has_scientific_notation(&csv), "scientific notation in CGT CSV: {csv}");
    assert!(csv.contains("999999999.00"), "large gain not formatted correctly: {csv}");
}

#[test]
fn income_csv_no_scientific_notation_for_large_value() {
    let mut e = income_event("BTC", "income:mining", 0.0);
    e.value = 1_234_567_890.5;
    e.price = 1_234_567_890.5;
    let report = base_income(vec![e]);
    let csv = income_to_csv(&report).unwrap();
    assert!(!has_scientific_notation(&csv), "scientific notation in income CSV: {csv}");
    assert!(csv.contains("1234567890.50"), "large value not formatted correctly: {csv}");
}

#[test]
fn balances_csv_no_scientific_notation_for_large_quantity() {
    let report = BalancesReport {
        as_of_date: "2025-06-30".into(),
        base_currency: "AUD".into(),
        base_account_scope: None,
        holdings: vec![CoinBalance {
            commodity: "SHIB".into(),
            quantity: 1_000_000_000_000.0,
            price: 0.000042,
            price_date: "2025-06-30".into(),
            value: 42_000.0,
            portfolio_weight: 1.0,
            accounts: vec![AccountBalance {
                account: "assets:wallet".into(),
                quantity: 1_000_000_000_000.0,
                value: 42_000.0,
            }],
        }],
        total_value: 42_000.0,
        warnings: vec![],
    };
    let csv = balances_to_csv(&report).unwrap();
    assert!(!has_scientific_notation(&csv), "scientific notation in balances CSV: {csv}");
    assert!(
        csv.contains("1000000000000.000000"),
        "large quantity not formatted correctly: {csv}"
    );
}

// === 4. Locale-safety: decimal separator must be `.` ===
//
// Rust's formatting always produces `.` regardless of locale, but this test
// pins that invariant so a future platform-specific libc change or localised
// display wrapper would surface immediately.

#[test]
fn cgt_csv_decimal_separator_is_dot() {
    let report = base_cgt(vec![cgt_event("ETH", 1234.56)]);
    let csv = cgt_to_csv(&report).unwrap();
    // The gain 1234.56 must appear with a `.` separator, not a `,`.
    assert!(csv.contains("1234.56"), "decimal separator wrong in CGT CSV: {csv}");
    // If comma-as-decimal appeared it would look like "1234,56".
    assert!(!csv.contains("1234,56"), "locale comma as decimal in CGT CSV: {csv}");
}

#[test]
fn income_csv_decimal_separator_is_dot() {
    let mut e = income_event("ETH", "income:staking", 0.0);
    e.value = 9876.54;
    e.price = 9876.54;
    let report = base_income(vec![e]);
    let csv = income_to_csv(&report).unwrap();
    assert!(csv.contains("9876.54"), "decimal separator wrong in income CSV: {csv}");
    assert!(!csv.contains("9876,54"), "locale comma as decimal in income CSV: {csv}");
}

#[test]
fn balances_csv_decimal_separator_is_dot() {
    let report = BalancesReport {
        as_of_date: "2025-06-30".into(),
        base_currency: "AUD".into(),
        base_account_scope: None,
        holdings: vec![CoinBalance {
            commodity: "BTC".into(),
            quantity: 0.12345678,
            price: 98765.43,
            price_date: "2025-06-30".into(),
            value: 12195.97,
            portfolio_weight: 1.0,
            accounts: vec![AccountBalance {
                account: "assets:wallet".into(),
                quantity: 0.12345678,
                value: 12195.97,
            }],
        }],
        total_value: 12195.97,
        warnings: vec![],
    };
    let csv = balances_to_csv(&report).unwrap();
    assert!(csv.contains("0.123457"), "quantity not formatted correctly: {csv}");
    assert!(!csv.contains("0,123457"), "locale comma as decimal in balances CSV: {csv}");
}

// === 5. Negative values stay negative ===
//
// Losses (negative gains) and negative income (fees) must not have their
// sign dropped or inverted by the formatters.

#[test]
fn cgt_csv_negative_gain_keeps_sign() {
    let report = base_cgt(vec![cgt_event("ETH", -500.0)]);
    let csv = cgt_to_csv(&report).unwrap();
    assert!(csv.contains("-500.00"), "negative gain sign dropped: {csv}");
}

#[test]
fn income_csv_negative_value_keeps_sign() {
    let mut e = income_event("USDC", "income:trading:fees", 0.0);
    e.value = -75.25;
    e.price = 75.25;
    let report = base_income(vec![e]);
    let csv = income_to_csv(&report).unwrap();
    assert!(csv.contains("-75.25"), "negative income value sign dropped: {csv}");
}
