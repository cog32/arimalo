//! Fixture-driven tests for `reports.rs` covering the three review gaps
//! (issue #6):
//!   * CGT with fees on a sell.
//!   * Reports using a base currency other than AUD.
//!   * FX-conversion edge cases (price-graph miss, mid-year rate change).

use arimalo_covid::ledger_parser::{
    parse_transactions, PriceDirective, PriceGraph, Transaction,
};
use arimalo_covid::reports::{
    generate_balances_report_range, generate_cgt_report, generate_income_report,
    generate_loss_harvest_report, generate_performance_report_range, holdings_as_of,
    AccountAllowlist, PerformanceReportParams, TaxConfig,
};

fn aud_fy_config() -> TaxConfig {
    TaxConfig {
        financial_year_end_month: 6,
        financial_year_end_day: 30,
        cgt_discount_percent: 50,
        cgt_discount_holding_months: 12,
        non_taxable_accounts: vec![],
        non_deductible_accounts: vec![],
        marginal_tax_rate_percent: 47,
    }
}

fn parse_ok(src: &str) -> Vec<Transaction> {
    let r = parse_transactions(src);
    assert!(r.ok, "parse failed: {:?}", r.diagnostics);
    r.transactions
}

fn price(commodity: &str, datetime: &str, amount: f64, quote: &str) -> PriceDirective {
    PriceDirective {
        datetime: datetime.to_string(),
        commodity: commodity.to_string(),
        price_amount: amount,
        price_amount_text: format!("{amount}"),
        quote_commodity: quote.to_string(),
    }
}

// === 1. CGT with fees on a sell ===
//
// A fee posted to `expenses:fees:trading` reduces the cash receipt on the
// sell side, so FIFO proceeds reflect the net cash. This pins the current
// (correct) behaviour: fee-on-sell flows through naturally via the
// counterparty-cash branch of `resolve_sale_proceeds`.

#[test]
fn cgt_sell_with_fee_uses_net_cash_proceeds() {
    // Buy 1 BTC for 10,000 AUD on 2024-08-01.
    // Sell 1 BTC on 2025-03-01 for 20,050 AUD gross; 50 AUD fee taken
    // out → 20,000 AUD net cash receipt.
    let src = r#"2024-08-01 * "Buy BTC" ; txn:t-PLACEHOLDER
    assets:exchange:btc       1.00000000 BTC
    assets:cash:aud         -10000.00 AUD

2025-03-01 * "Sell BTC with fee" ; txn:t-PLACEHOLDER
    assets:exchange:btc      -1.00000000 BTC
    assets:cash:aud           20000.00 AUD
    expenses:fees:trading        50.00 AUD
    income:trading             -50.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![]);
    let report = generate_cgt_report(&txns, &pg, &aud_fy_config(), "2025", "AUD", None);

    assert_eq!(report.events.len(), 1, "{:?}", report);
    let e = &report.events[0];
    assert_eq!(e.commodity, "BTC");
    assert!((e.cost_basis - 10000.0).abs() < 1e-6, "cost_basis={}", e.cost_basis);
    assert!((e.sale_proceeds - 20000.0).abs() < 1e-6,
        "expected net-of-fee proceeds, got {}", e.sale_proceeds);
    assert!((e.capital_gain - 10000.0).abs() < 1e-6);
    // Buy 2024-08-01, sell 2025-03-01 → 7 months < 12, so not eligible.
    assert!(!e.discount_eligible);
}

#[test]
fn cgt_sell_long_term_gets_discount() {
    // Same shape, but held >12 months so the discount applies.
    let src = r#"2023-01-15 * "Buy BTC" ; txn:t-PLACEHOLDER
    assets:exchange:btc       1.00000000 BTC
    assets:cash:aud         -10000.00 AUD

2025-03-01 * "Sell BTC" ; txn:t-PLACEHOLDER
    assets:exchange:btc      -1.00000000 BTC
    assets:cash:aud           20000.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![]);
    let report = generate_cgt_report(&txns, &pg, &aud_fy_config(), "2025", "AUD", None);

    assert_eq!(report.events.len(), 1);
    let e = &report.events[0];
    assert!(e.discount_eligible);
    assert!((e.discounted_gain - 5000.0).abs() < 1e-6,
        "expected 50% discount of 10,000 gain, got {}", e.discounted_gain);
    assert!((report.total_discounted_gain - 5000.0).abs() < 1e-6);
}

// === 2. Non-AUD base currency ===

#[test]
fn cgt_report_with_usd_base_currency() {
    // Same trade shape but priced in USD. No AUD anywhere; the report must
    // honour the `base_currency` parameter.
    let src = r#"2024-02-01 * "Buy ETH" ; txn:t-PLACEHOLDER
    assets:exchange:eth       2.00000000 ETH
    assets:cash:usd          -5000.00 USD

2024-10-01 * "Sell ETH" ; txn:t-PLACEHOLDER
    assets:exchange:eth      -2.00000000 ETH
    assets:cash:usd           7000.00 USD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![]);
    // US FY end Dec 31, label "2024".
    let cfg = TaxConfig {
        financial_year_end_month: 12,
        financial_year_end_day: 31,
        cgt_discount_percent: 0,
        cgt_discount_holding_months: 12,
        non_taxable_accounts: vec![],
        non_deductible_accounts: vec![],
        marginal_tax_rate_percent: 47,
    };
    let report = generate_cgt_report(&txns, &pg, &cfg, "2024", "USD", None);

    assert_eq!(report.events.len(), 1, "{:?}", report);
    let e = &report.events[0];
    assert!((e.cost_basis - 5000.0).abs() < 1e-6);
    assert!((e.sale_proceeds - 7000.0).abs() < 1e-6);
    assert!((e.capital_gain - 2000.0).abs() < 1e-6);
    assert!((report.net_capital_gain - 2000.0).abs() < 1e-6);
}

#[test]
fn income_report_with_usd_base_currency() {
    // Income in USD, base USD — no FX conversion needed; report totals
    // should be in USD.
    let src = r#"2024-06-15 * "Staking reward" ; txn:t-PLACEHOLDER
    assets:exchange:eth       0.10000000 ETH @ 3000.00 USD
    income:staking          -300.00 USD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![]);
    let cfg = TaxConfig {
        financial_year_end_month: 12,
        financial_year_end_day: 31,
        cgt_discount_percent: 0,
        cgt_discount_holding_months: 12,
        non_taxable_accounts: vec![],
        non_deductible_accounts: vec![],
        marginal_tax_rate_percent: 47,
    };
    let report = generate_income_report(&txns, &pg, &cfg, "2024", "USD", None);

    assert!((report.total_income - 300.0).abs() < 1e-6,
        "expected 300 USD income, got {:?}", report);
    assert_eq!(report.income_categories.len(), 1);
    assert_eq!(report.income_categories[0].base_currency, "USD");
}

// === 3. FX edge cases ===

#[test]
fn balances_warns_on_price_graph_miss() {
    // Holds 1 XYZ but no price is known anywhere → must surface a warning
    // and skip the holding rather than fabricate a value.
    let src = r#"2024-05-01 * "Buy XYZ" ; txn:t-PLACEHOLDER
    assets:wallet:xyz         1.00000000 XYZ
    assets:cash:aud          -100.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![]);

    let report = generate_balances_report_range(
        &txns,
        &pg,
        "2025-06-30",
        "AUD",
        None,
        None,
    );

    assert!(
        report.warnings.iter().any(|w| w.contains("XYZ") && w.contains("AUD")),
        "expected a missing-price warning, got: {:?}",
        report.warnings,
    );
    assert!(
        report.holdings.iter().all(|h| h.commodity != "XYZ"),
        "XYZ should be skipped when no price is available, got: {:?}",
        report.holdings,
    );
}

#[test]
fn balances_mid_year_price_change_uses_nearest_prior() {
    // Price moves mid-FY. The balances report as-of 2025-06-30 must use
    // the most-recent-on-or-before price (the 2025-04 entry), not the
    // earlier one and not the later (in this fixture there is no later).
    let src = r#"2024-09-01 * "Buy ETH" ; txn:t-PLACEHOLDER
    assets:wallet:eth         1.00000000 ETH
    assets:cash:aud         -2000.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![
        price("ETH", "2024-09-01T00:00:00", 2000.0, "AUD"),
        price("ETH", "2025-04-01T00:00:00", 5000.0, "AUD"),
    ]);

    let report = generate_balances_report_range(
        &txns,
        &pg,
        "2025-06-30",
        "AUD",
        None,
        None,
    );

    let eth = report
        .holdings
        .iter()
        .find(|h| h.commodity == "ETH")
        .expect("ETH holding missing");
    assert!((eth.price - 5000.0).abs() < 1e-6, "expected nearest-prior price 5000, got {}", eth.price);
    assert!((eth.value - 5000.0).abs() < 1e-6);
    assert_eq!(eth.price_date, "2025-04-01T00:00:00");
}

#[test]
fn balances_one_hop_fx_conversion() {
    // ETH→USD direct, USD→AUD direct → balances in AUD via one-hop.
    let src = r#"2025-01-15 * "Buy ETH on US exchange" ; txn:t-PLACEHOLDER
    assets:wallet:eth         1.00000000 ETH
    assets:cash:usd         -3000.00 USD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![
        price("ETH", "2025-01-01T00:00:00", 3000.0, "USD"),
        price("USD", "2025-01-01T00:00:00", 1.5, "AUD"),
    ]);

    let report = generate_balances_report_range(
        &txns,
        &pg,
        "2025-06-30",
        "AUD",
        None,
        None,
    );

    let eth = report
        .holdings
        .iter()
        .find(|h| h.commodity == "ETH")
        .expect("ETH holding missing");
    assert!((eth.value - 4500.0).abs() < 1e-6,
        "expected 1 ETH × 3000 USD × 1.5 AUD/USD = 4500 AUD, got {}", eth.value);
}

#[test]
fn cgt_uses_mid_year_fx_for_disposal_in_non_base_currency() {
    // Buy and sell in USD; report in AUD. FX rate changes between buy
    // and sell, so cost basis (in AUD) and proceeds (in AUD) use
    // different rates — exactly the mid-year-rate-change edge case.
    let src = r#"2024-08-01 * "Buy BTC" ; txn:t-PLACEHOLDER
    assets:exchange:btc       1.00000000 BTC
    assets:cash:usd         -50000.00 USD

2025-03-01 * "Sell BTC" ; txn:t-PLACEHOLDER
    assets:exchange:btc      -1.00000000 BTC
    assets:cash:usd          60000.00 USD
"#;
    let txns = parse_ok(src);
    // 1 USD = 1.5 AUD on the buy date; 1 USD = 1.6 AUD on the sell date.
    let pg = PriceGraph::from_entries(vec![
        price("USD", "2024-08-01T00:00:00", 1.5, "AUD"),
        price("USD", "2025-03-01T00:00:00", 1.6, "AUD"),
    ]);
    let report = generate_cgt_report(&txns, &pg, &aud_fy_config(), "2025", "AUD", None);

    // USD itself is also treated as a non-base commodity (so the USD
    // disposal on the sell side shows up too); we focus on the BTC event.
    let e = report
        .events
        .iter()
        .find(|e| e.commodity == "BTC")
        .expect("BTC CGT event missing");
    // Cost basis uses counterparty USD posting converted at buy-date FX.
    assert!((e.cost_basis - 75000.0).abs() < 1e-3,
        "expected 50,000 USD × 1.5 = 75,000 AUD cost, got {}", e.cost_basis);
    // Proceeds use counterparty USD posting converted at sell-date FX.
    assert!((e.sale_proceeds - 96000.0).abs() < 1e-3,
        "expected 60,000 USD × 1.6 = 96,000 AUD proceeds, got {}", e.sale_proceeds);
    assert!((e.capital_gain - 21000.0).abs() < 1e-3,
        "expected 21,000 AUD gain, got {}", e.capital_gain);
}

// === Performance report: holdings_as_of + 12-month series ===

/// Buy 10 ETH @ 100 AUD (1000 total) on 2025-07-10, sell 4 ETH for 1000 AUD on
/// 2025-09-15 (FIFO cost 400 → realised 600), interest 50 AUD on 2025-11-20.
fn perf_fixture_txns() -> Vec<Transaction> {
    let src = r#"2025-07-10 * "Buy ETH" ; txn:perf-buy-01
    assets:exchange:eth      10.00000000 ETH {{ 1000.00 AUD }}
    assets:cash:aud       -1000.00 AUD

2025-09-15 * "Sell ETH" ; txn:perf-sell-01
    assets:exchange:eth      -4.00000000 ETH
    assets:cash:aud        1000.00 AUD

2025-11-20 * "Interest" ; txn:perf-income-01
    assets:cash:aud          50.00 AUD
    income:interest         -50.00 AUD
"#;
    parse_ok(src)
}

/// ETH/AUD month-end prices for the fixture window (one per month so each
/// snapshot moves).
fn perf_eth_prices() -> PriceGraph {
    PriceGraph::from_entries(vec![
        price("ETH", "2025-07-31", 120.0, "AUD"),
        price("ETH", "2025-08-31", 150.0, "AUD"),
        price("ETH", "2025-09-30", 250.0, "AUD"),
        price("ETH", "2025-10-31", 200.0, "AUD"),
        price("ETH", "2025-11-30", 220.0, "AUD"),
        price("ETH", "2025-12-31", 300.0, "AUD"),
        price("ETH", "2026-01-31", 260.0, "AUD"),
        price("ETH", "2026-02-28", 240.0, "AUD"),
        price("ETH", "2026-03-31", 270.0, "AUD"),
        price("ETH", "2026-04-30", 290.0, "AUD"),
        price("ETH", "2026-05-31", 310.0, "AUD"),
        price("ETH", "2026-06-30", 280.0, "AUD"),
    ])
}

#[test]
fn holdings_as_of_partial_disposal_reduces_qty_and_cost() {
    let txns = perf_fixture_txns();
    let pg = perf_eth_prices();
    // After the Sep sell, 6 ETH remain at 100 AUD cost. At 2025-12-31 (price
    // 300): value 1800, unrealised 1200.
    let snap = holdings_as_of(&txns, &pg, "2025-12-31", "AUD", None, None);
    assert_eq!(snap.holdings.len(), 1, "{:?}", snap);
    let h = &snap.holdings[0];
    assert_eq!(h.commodity, "ETH");
    assert!((h.quantity - 6.0).abs() < 1e-6, "qty={}", h.quantity);
    assert!((h.cost_basis - 600.0).abs() < 1e-6, "cost={}", h.cost_basis);
    assert!(h.has_price);
    assert!((h.value - 1800.0).abs() < 1e-6, "value={}", h.value);
    assert!((h.unrealised - 1200.0).abs() < 1e-6, "unreal={}", h.unrealised);
    assert!((snap.total_value - 1850.0).abs() < 1e-6); // ETH 1800 + cash 50 (Nov interest)
    assert!((snap.total_cost_basis - 650.0).abs() < 1e-6); // ETH cost 600 + cash 50
    assert!((snap.total_unrealised - 1200.0).abs() < 1e-6); // cash cancels (book = face)
}

#[test]
fn holdings_as_of_excludes_unpriced() {
    // An unpriced commodity is dropped from holdings (with a warning), matching
    // the Balances report — its value can't be resolved. Only base-currency cash
    // (the 50 interest) remains in the total here.
    let txns = perf_fixture_txns();
    let pg = PriceGraph::from_entries(vec![]); // no ETH price anywhere
    let snap = holdings_as_of(&txns, &pg, "2025-12-31", "AUD", None, None);
    assert!(
        snap.holdings.iter().all(|h| h.commodity != "ETH"),
        "unpriced ETH should be excluded, got {:?}",
        snap.holdings
    );
    assert!(
        (snap.total_value - 50.0).abs() < 1e-6,
        "total_value={}",
        snap.total_value
    );
    assert!(
        snap.warnings.iter().any(|w| w.contains("ETH")),
        "expected missing-price warning, got {:?}",
        snap.warnings
    );
}

#[test]
fn holdings_as_of_excludes_transfers() {
    // Buy 10 ETH to the exchange, then transfer all 10 to a wallet (two legs in
    // the same commodity netting to zero) — the transfer must not consume the
    // lot, so holdings stay 10 ETH at the original 1000 AUD cost.
    let src = r#"2025-07-10 * "Buy ETH" ; txn:t-buy
    assets:exchange:eth      10.00000000 ETH {{ 1000.00 AUD }}
    assets:cash:aud       -1000.00 AUD

2025-08-01 * "Transfer to wallet" ; txn:t-xfer
    assets:wallet:eth        10.00000000 ETH
    assets:exchange:eth     -10.00000000 ETH
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![price("ETH", "2025-08-31", 150.0, "AUD")]);
    let snap = holdings_as_of(&txns, &pg, "2025-08-31", "AUD", None, None);
    assert_eq!(snap.holdings.len(), 1, "{:?}", snap);
    let h = &snap.holdings[0];
    assert!((h.quantity - 10.0).abs() < 1e-6, "qty={}", h.quantity);
    assert!((h.cost_basis - 1000.0).abs() < 1e-6, "cost={}", h.cost_basis);
}

#[test]
fn holdings_as_of_skips_ignore_accounts() {
    // A scam airdrop offset to an `ignore:*` account must not appear as a holding.
    let src = r#"2025-07-10 * "Buy ETH" ; txn:t-buy
    assets:exchange:eth      10.00000000 ETH {{ 1000.00 AUD }}
    assets:cash:aud       -1000.00 AUD

2025-07-15 * "Scam airdrop" ; txn:t-scam
    assets:exchange:scam   100.00000000 SCAM
    ignore:airdrop        -100.00000000 SCAM
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![price("ETH", "2025-07-31", 120.0, "AUD")]);
    let snap = holdings_as_of(&txns, &pg, "2025-07-31", "AUD", None, None);
    assert_eq!(
        snap.holdings.len(),
        1,
        "only ETH expected, got {:?}",
        snap.holdings
    );
    assert_eq!(snap.holdings[0].commodity, "ETH");
}

#[test]
fn holdings_as_of_short_position_shows_negative_and_warns() {
    // Selling more than held leaves a NET SHORT (negative balance), valued like
    // the Balances report — NOT floored to zero — plus a "short" warning. A short
    // is carried at market (cost basis = value, unrealised 0): the engine can't
    // establish a FIFO cost basis for a short.
    let src = r#"2025-07-10 * "Buy ETH" ; txn:t-buy
    assets:exchange:eth      10.00000000 ETH {{ 1000.00 AUD }}
    assets:cash:aud       -1000.00 AUD

2025-08-10 * "Oversell" ; txn:t-sell
    assets:exchange:eth     -12.00000000 ETH
    assets:cash:aud        3000.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![price("ETH", "2025-08-31", 250.0, "AUD")]);
    let snap = holdings_as_of(&txns, &pg, "2025-08-31", "AUD", None, None);
    let eth = snap
        .holdings
        .iter()
        .find(|h| h.commodity == "ETH")
        .expect("net short ETH should appear as a negative holding");
    assert!((eth.quantity - (-2.0)).abs() < 1e-6, "qty={}", eth.quantity);
    assert!((eth.value - (-500.0)).abs() < 1e-6, "value={}", eth.value); // -2 × 250
    // Carried at market: cost basis = value, so unrealised is 0 (not floored to a
    // 0 cost that would book the whole short value as a fictitious loss).
    assert!((eth.cost_basis - (-500.0)).abs() < 1e-6, "cost={}", eth.cost_basis);
    assert!(eth.unrealised.abs() < 1e-6, "unrealised={}", eth.unrealised);
    assert!(
        snap.warnings.iter().any(|w| w.contains("short")),
        "expected short warning, got {:?}",
        snap.warnings
    );
}

#[test]
fn holdings_as_of_floored_over_disposal_does_not_inflate_cost_basis() {
    // Regression: an EARLY over-disposal (sending more than is held) floors the
    // unmatched units to zero, but later re-acquisitions push the position back
    // to a positive NET balance. FIFO-remaining lots then exceed the net
    // balance by the floored amount. Because a holding is VALUED on its net
    // balance but its cost basis is summed from FIFO-remaining lots, the stray
    // lots used to manufacture a fictitious unrealised loss (seen on USDC: a
    // stablecoin reported ~49% below cost). Cost basis must be measured over
    // the same units the position is valued on.
    //
    // 10 USDC bought @1.40, then 50 "sent" (floors 40), then 100 bought @1.40.
    // Net = 60 USDC; without the fix FIFO-remaining = 100 → cost 140 vs value 84.
    let src = r#"2025-07-01 * "Buy USDC" ; txn:t-buy1
    assets:exchange:usdc      10.00000000 USDC {{ 14.00 AUD }}
    assets:cash:aud         -14.00 AUD

2025-08-01 * "Send USDC (over-dispose)" ; txn:t-send
    assets:exchange:usdc     -50.00000000 USDC
    equity:trading            50.00000000 USDC

2025-09-01 * "Buy USDC" ; txn:t-buy2
    assets:exchange:usdc     100.00000000 USDC {{ 140.00 AUD }}
    assets:cash:aud        -140.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![price("USDC", "2025-12-31", 1.40, "AUD")]);
    let snap = holdings_as_of(&txns, &pg, "2025-12-31", "AUD", None, None);
    let h = snap
        .holdings
        .iter()
        .find(|h| h.commodity == "USDC")
        .expect("net-positive USDC holding should appear");

    assert!((h.quantity - 60.0).abs() < 1e-6, "qty={}", h.quantity);
    assert!((h.value - 84.0).abs() < 1e-6, "value={}", h.value); // 60 × 1.40
    // Cost basis reflects the 60 NET units, not the 100 FIFO-remaining units.
    assert!(
        (h.cost_basis - 84.0).abs() < 1e-6,
        "cost basis must track net holdings (84), got {}",
        h.cost_basis
    );
    assert!(h.unrealised.abs() < 1e-6, "stablecoin must not be underwater, got {}", h.unrealised);
    // Drill-down lots stay consistent with the parent: same unit count, and the
    // per-lot cost bases sum to the holding's cost basis.
    let lot_qty: f64 = h.lots.iter().map(|l| l.quantity).sum();
    let lot_cost: f64 = h.lots.iter().map(|l| l.cost_basis).sum();
    assert!((lot_qty - h.quantity).abs() < 1e-6, "lot qty {} != net {}", lot_qty, h.quantity);
    assert!((lot_cost - h.cost_basis).abs() < 1e-6, "lot cost {} != cost basis {}", lot_cost, h.cost_basis);
}

#[test]
fn holdings_as_of_net_short_with_orphan_lots_carried_at_market() {
    // Regression: a net-SHORT position with leftover orphan lots must be carried
    // at MARKET (cost basis = value, unrealised 0), NOT valued off the orphan
    // pile. This is the SOL/BNB case: coins routed out via an allowlist-excluded
    // transfer contra (or a lending contra) floor the over-disposal, later
    // receipts leave orphan acquisition lots, and the net is short. Summing those
    // lots booked a six-figure phantom cost on a tiny net → a fictitious
    // performance swing across snapshots.
    //
    // Send 50 with no holdings (floors 50, net -50), then buy 40 @ $10 → net -10
    // with 40 orphan lots. Cost basis must be the market value (-10 × $12 = -120),
    // NOT 0 and NOT the 40-lot $400 pile; unrealised 0; no phantom parcels.
    let src = r#"2025-07-01 * "Send SOL (over-dispose, no holdings)" ; txn:t-send
    assets:exchange:sol     -50.00000000 SOL
    equity:trading           50.00000000 SOL

2025-08-01 * "Buy SOL" ; txn:t-buy
    assets:exchange:sol      40.00000000 SOL {{ 400.00 AUD }}
    assets:cash:aud        -400.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![price("SOL", "2025-12-31", 12.0, "AUD")]);
    let snap = holdings_as_of(&txns, &pg, "2025-12-31", "AUD", None, None);
    let h = snap
        .holdings
        .iter()
        .find(|h| h.commodity == "SOL")
        .expect("net-short SOL holding should appear");

    assert!((h.quantity - (-10.0)).abs() < 1e-6, "qty={}", h.quantity);
    assert!((h.value - (-120.0)).abs() < 1e-6, "value={}", h.value); // -10 × 12
    // Carried at market: cost basis = value (-120), NOT 0, NOT the -400 orphan pile.
    assert!(
        (h.cost_basis - (-120.0)).abs() < 1e-6,
        "short must be carried at market (-120), got {}",
        h.cost_basis
    );
    // Unrealised 0 — no phantom gain/loss on a short the engine can't cost.
    assert!(h.unrealised.abs() < 1e-6, "unrealised={}", h.unrealised);
    // No phantom parcels for a short.
    assert!(h.lots.is_empty(), "short should expose no parcels, got {}", h.lots.len());
    assert!(
        snap.warnings.iter().any(|w| w.contains("short")),
        "expected over-disposal/short warning, got {:?}",
        snap.warnings
    );
}

#[test]
fn performance_report_basic_series() {
    let txns = perf_fixture_txns();
    let pg = perf_eth_prices();
    let report = generate_performance_report_range(PerformanceReportParams {
        transactions: &txns,
        price_graph: &pg,
        tax_config: &aud_fy_config(),
        label: "FY2026",
        date_from: "2025-07-01",
        date_to: "2026-06-30",
        base_currency: "AUD",
        base_account_scope: None,
        allowed_accounts: None,
    });

    assert_eq!(report.points.len(), 13, "expected opening baseline + 12 month-ends");

    let point = |label: &str| {
        report
            .points
            .iter()
            .find(|p| p.label == label)
            .unwrap_or_else(|| panic!("missing point {label}: {:?}", report.points))
    };

    let jul = point("Jul 2025");
    assert!((jul.portfolio_value - 200.0).abs() < 1e-6); // ETH 1200 − 1000 cash paid for the buy
    assert!((jul.unrealised_gain - 200.0).abs() < 1e-6);
    // Window opens empty (buy is 2025-07-10), so July's change == its stock.
    assert!((jul.unrealised_change - 200.0).abs() < 1e-6);
    assert!(jul.realised_gain.abs() < 1e-9 && jul.income.abs() < 1e-9);

    let sep = point("Sep 2025");
    assert!(
        (sep.realised_gain - 600.0).abs() < 1e-6,
        "realised={}",
        sep.realised_gain
    );
    assert!((sep.portfolio_value - 1500.0).abs() < 1e-6);
    assert!((sep.unrealised_gain - 900.0).abs() < 1e-6);

    let nov = point("Nov 2025");
    assert!((nov.income - 50.0).abs() < 1e-6, "income={}", nov.income);

    let jun = point("Jun 2026");
    assert!((jun.portfolio_value - 1730.0).abs() < 1e-6); // ETH 1680 + cash 50
    assert!((jun.unrealised_gain - 1080.0).abs() < 1e-6);

    assert!((report.total_realised_gain - 600.0).abs() < 1e-6);
    assert!((report.total_income - 50.0).abs() < 1e-6);
    // Buy is 2025-07-10, so the window opens (2025-06-30) with no holdings:
    // unrealised_change == closing unrealised, value_open == 0, pct undefined.
    assert!((report.unrealised_change - 1080.0).abs() < 1e-6);
    assert!((report.closing_value - 1730.0).abs() < 1e-6); // ETH 1680 + cash 50
    assert!((report.closing_cost_basis - 650.0).abs() < 1e-6); // ETH 600 + cash 50
    assert!(
        report.value_open.abs() < 1e-6,
        "value_open={}",
        report.value_open
    );
    assert!(
        (report.total_return - 1730.0).abs() < 1e-6,
        "total_return={}",
        report.total_return
    );
    assert!(
        report.total_return_pct.is_none(),
        "pct should be None when opening value is 0"
    );

    // Attribution: per-commodity realised + Δunrealised; Σ total + income ties
    // to total_return. ETH carries the whole window here (cash is base currency).
    let attr_sum: f64 = report.attribution.iter().map(|a| a.total).sum();
    assert!(
        (attr_sum + report.total_income - report.total_return).abs() < 1e-6,
        "attribution Σtotal ({attr_sum}) + income ({}) should equal total_return ({})",
        report.total_income,
        report.total_return
    );
    let eth = report
        .attribution
        .iter()
        .find(|a| a.commodity == "ETH")
        .expect("ETH present in attribution");
    assert!((eth.realised_gain - 600.0).abs() < 1e-6, "eth realised={}", eth.realised_gain);
    assert!(
        (eth.unrealised_change - 1080.0).abs() < 1e-6,
        "eth unrealised_change={}",
        eth.unrealised_change
    );
    assert!((eth.total - 1680.0).abs() < 1e-6, "eth total={}", eth.total);
    assert!((eth.closing_value - 1680.0).abs() < 1e-6, "eth value={}", eth.closing_value);
}

#[test]
fn performance_report_period_return_excludes_prewindow_gain() {
    // Buy 10 ETH @ 100 (cost 1000) on 2025-06-01, BEFORE the FY2026 window. It's
    // worth 110 at window open (2025-06-30) — that +100 is a PRIOR-period gain
    // and must NOT count toward the window return. By close it's worth 280, so
    // the in-window appreciation is 1800 − 100 = 1700.
    let src = r#"2025-06-01 * "Buy ETH" ; txn:pre-buy
    assets:exchange:eth      10.00000000 ETH {{ 1000.00 AUD }}
    equity:opening:aud    -1000.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![
        price("ETH", "2025-06-30", 110.0, "AUD"),
        price("ETH", "2026-06-30", 280.0, "AUD"),
    ]);
    let report = generate_performance_report_range(PerformanceReportParams {
        transactions: &txns,
        price_graph: &pg,
        tax_config: &aud_fy_config(),
        label: "FY2026",
        date_from: "2025-07-01",
        date_to: "2026-06-30",
        base_currency: "AUD",
        base_account_scope: None,
        allowed_accounts: None,
    });
    // Opening baseline (2025-06-30): value 1100, unrealised 100.
    assert!(
        (report.value_open - 1100.0).abs() < 1e-6,
        "value_open={}",
        report.value_open
    );
    // Only the in-window 1700 counts — the prior 100 is excluded.
    assert!(
        (report.unrealised_change - 1700.0).abs() < 1e-6,
        "unrealised_change={}",
        report.unrealised_change
    );
    assert!(report.total_realised_gain.abs() < 1e-9);
    assert!(report.total_income.abs() < 1e-9);
    assert!(
        (report.total_return - 1700.0).abs() < 1e-6,
        "total_return={}",
        report.total_return
    );
    let pct = report.total_return_pct.expect("pct present (opening value > 0)");
    assert!((pct - (1700.0 / 1100.0)).abs() < 1e-6, "pct={pct}");
}

#[test]
fn performance_report_realised_is_window_relative_for_prewindow_lots() {
    // Buy 10 ETH @ 100 (cost 1000) on 2025-06-01, BEFORE the FY2026 window; worth
    // 110 (1100) at window open. Sell ALL 10 in-window for 1500. Lifetime realised
    // is 500, but $100 of that accrued pre-window — a performance report must
    // measure from the window open, so realised = proceeds − open value = 1500 −
    // 1100 = 400, and the holding's unrealised contribution is 0 (sold, started at
    // open value). Total = 400. (The lifetime 500 lives in the tax/CGT report.)
    let src = r#"2025-06-01 * "Buy ETH" ; txn:pre-buy
    assets:exchange:eth      10.00000000 ETH {{ 1000.00 AUD }}
    equity:opening:aud    -1000.00 AUD

2025-09-15 * "Sell ETH" ; txn:sell
    assets:exchange:eth     -10.00000000 ETH
    assets:cash:aud         1500.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![
        price("ETH", "2025-06-30", 110.0, "AUD"),
        price("ETH", "2025-07-31", 120.0, "AUD"),
        price("ETH", "2025-08-31", 130.0, "AUD"),
    ]);
    let report = generate_performance_report_range(PerformanceReportParams {
        transactions: &txns,
        price_graph: &pg,
        tax_config: &aud_fy_config(),
        label: "FY2026",
        date_from: "2025-07-01",
        date_to: "2026-06-30",
        base_currency: "AUD",
        base_account_scope: None,
        allowed_accounts: None,
    });

    // Headline realised is window-relative (400), NOT the lifetime 500.
    assert!(
        (report.total_realised_gain - 400.0).abs() < 1e-6,
        "window realised should be 400, got {}",
        report.total_realised_gain
    );
    assert!(
        report.unrealised_change.abs() < 1e-6,
        "unrealised_change should be 0 (sold from open value), got {}",
        report.unrealised_change
    );
    assert!(
        (report.total_return - 400.0).abs() < 1e-6,
        "total_return={}",
        report.total_return
    );
    let eth = report
        .attribution
        .iter()
        .find(|a| a.commodity == "ETH")
        .expect("ETH in attribution");
    assert!((eth.realised_gain - 400.0).abs() < 1e-6, "eth realised={}", eth.realised_gain);
    assert!(eth.unrealised_change.abs() < 1e-6, "eth unrealised={}", eth.unrealised_change);
    assert!((eth.total - 400.0).abs() < 1e-6, "eth total={}", eth.total);
    assert!(eth.closing_value.abs() < 1e-6, "eth value={}", eth.closing_value);
}

#[test]
fn performance_report_midmonth_endpoint_plus_opening() {
    // A custom 12-month window ending mid-month: month-ends Jun30'25..May31'26
    // (12) plus the exact endpoint 2026-06-12 → 13 points.
    let txns = perf_fixture_txns();
    let pg = perf_eth_prices();
    let report = generate_performance_report_range(PerformanceReportParams {
        transactions: &txns,
        price_graph: &pg,
        tax_config: &aud_fy_config(),
        label: "custom",
        date_from: "2025-06-13",
        date_to: "2026-06-12",
        base_currency: "AUD",
        base_account_scope: None,
        allowed_accounts: None,
    });
    assert_eq!(
        report.points.len(),
        14,
        "{:?}",
        report.points.iter().map(|p| &p.date).collect::<Vec<_>>()
    );
    // First point is the opening baseline (the day before the window start).
    assert_eq!(report.points.first().unwrap().date, "2025-06-12");
    assert_eq!(report.points.last().unwrap().date, "2026-06-12");
}

#[test]
fn performance_report_month_end_dates_leap_year() {
    // Empty ledger still yields correctly-dated zero points; Feb 2024 must use
    // the leap-day month-end 2024-02-29.
    let txns: Vec<Transaction> = vec![];
    let pg = PriceGraph::from_entries(vec![]);
    let report = generate_performance_report_range(PerformanceReportParams {
        transactions: &txns,
        price_graph: &pg,
        tax_config: &aud_fy_config(),
        label: "custom",
        date_from: "2024-02-01",
        date_to: "2024-03-31",
        base_currency: "AUD",
        base_account_scope: None,
        allowed_accounts: None,
    });
    let dates: Vec<&str> = report.points.iter().map(|p| p.date.as_str()).collect();
    // Opening baseline (day before window start) + leap-day month-end + Mar end.
    assert_eq!(dates, vec!["2024-01-31", "2024-02-29", "2024-03-31"], "{dates:?}");
    assert_eq!(report.points[0].label, "Open");
    assert_eq!(report.points[1].label, "Feb 2024");
}

// === Performance report: per-child-group value breakdown (growth chart) ===

/// Two-account fixture for the per-child-group breakdown: cash deposited BEFORE
/// the window (so `assets:cash` opens non-zero) and ETH bought IN-window (so
/// `assets:crypto` opens at zero — exercises a mid-window appearance), plus an
/// income receipt (root must exclude `income`) and an opening-equity leg (root
/// shows a constant-negative `equity` line).
fn perf_breakdown_txns() -> Vec<Transaction> {
    let src = r#"2025-06-15 * "Deposit" ; txn:bd-deposit
    assets:cash:aud        5000.00 AUD
    equity:opening:aud    -5000.00 AUD

2025-07-10 * "Buy ETH" ; txn:bd-buy
    assets:crypto:eth      10.00000000 ETH {{ 1000.00 AUD }}
    assets:cash:aud       -1000.00 AUD

2025-11-20 * "Interest" ; txn:bd-income
    assets:cash:aud          50.00 AUD
    income:interest         -50.00 AUD
"#;
    parse_ok(src)
}

#[test]
fn performance_report_account_breakdown_scope_assets() {
    let txns = perf_breakdown_txns();
    let pg = perf_eth_prices();
    let report = generate_performance_report_range(PerformanceReportParams {
        transactions: &txns,
        price_graph: &pg,
        tax_config: &aud_fy_config(),
        label: "FY2026",
        date_from: "2025-07-01",
        date_to: "2026-06-30",
        base_currency: "AUD",
        base_account_scope: Some("assets"),
        allowed_accounts: None,
    });

    let accounts: Vec<&str> = report
        .account_breakdown
        .iter()
        .map(|s| s.account.as_str())
        .collect();
    // Direct children of `assets` only: cash + crypto. No equity (out of scope),
    // no income (a flow, not a holding).
    assert!(accounts.contains(&"assets:cash"), "{accounts:?}");
    assert!(accounts.contains(&"assets:crypto"), "{accounts:?}");
    assert!(
        accounts.iter().all(|a| a.starts_with("assets:")),
        "scope=assets must only yield assets:* children, got {accounts:?}"
    );

    let series = |account: &str| {
        report
            .account_breakdown
            .iter()
            .find(|s| s.account == account)
            .unwrap_or_else(|| panic!("missing series {account}: {accounts:?}"))
    };
    let cash = series("assets:cash");
    let crypto = series("assets:crypto");
    assert_eq!(cash.values.len(), report.points.len());
    assert_eq!(crypto.values.len(), report.points.len());

    // points: [0]=Open(06-30), [1]=Jul, [3]=Sep, [5]=Nov, [12]=Jun.
    assert!((cash.values[0] - 5000.0).abs() < 1e-6, "cash open={}", cash.values[0]);
    assert!((cash.values[1] - 4000.0).abs() < 1e-6, "cash Jul={}", cash.values[1]); // 5000 − 1000 buy
    assert!((cash.values[5] - 4050.0).abs() < 1e-6, "cash Nov={}", cash.values[5]); // + 50 interest
    assert!((cash.values.last().unwrap() - 4050.0).abs() < 1e-6);

    assert!(crypto.values[0].abs() < 1e-6, "crypto opens at 0, got {}", crypto.values[0]);
    assert!((crypto.values[1] - 1200.0).abs() < 1e-6, "crypto Jul={}", crypto.values[1]); // 10 × 120
    assert!((crypto.values[3] - 2500.0).abs() < 1e-6, "crypto Sep={}", crypto.values[3]); // 10 × 250
    assert!((crypto.values.last().unwrap() - 2800.0).abs() < 1e-6); // 10 × 280
}

#[test]
fn performance_report_account_breakdown_root() {
    let txns = perf_breakdown_txns();
    let pg = perf_eth_prices();
    let report = generate_performance_report_range(PerformanceReportParams {
        transactions: &txns,
        price_graph: &pg,
        tax_config: &aud_fy_config(),
        label: "FY2026",
        date_from: "2025-07-01",
        date_to: "2026-06-30",
        base_currency: "AUD",
        base_account_scope: None,
        allowed_accounts: None,
    });

    let accounts: Vec<&str> = report
        .account_breakdown
        .iter()
        .map(|s| s.account.as_str())
        .collect();
    // Root shows balance-sheet groups only — never income/expenses.
    assert!(
        accounts
            .iter()
            .all(|a| matches!(*a, "assets" | "equity" | "liabilities")),
        "root groups must be balance-sheet only, got {accounts:?}"
    );
    assert!(accounts.contains(&"assets"), "{accounts:?}");
    assert!(accounts.contains(&"equity"), "{accounts:?}");

    let series = |account: &str| {
        report
            .account_breakdown
            .iter()
            .find(|s| s.account == account)
            .unwrap_or_else(|| panic!("missing series {account}: {accounts:?}"))
    };
    // assets last = cash 4050 + crypto 2800 = 6850.
    let assets = series("assets");
    assert!(
        (assets.values.last().unwrap() - 6850.0).abs() < 1e-6,
        "assets last={}",
        assets.values.last().unwrap()
    );
    // equity is the constant opening balance −5000 (base currency at face).
    let equity = series("equity");
    assert!(
        equity.values.iter().all(|v| (v + 5000.0).abs() < 1e-6),
        "equity should be constant −5000, got {:?}",
        equity.values
    );
}

#[test]
fn performance_report_account_breakdown_hides_contra() {
    // A transfer contra under `assets` must not become its own growth line.
    let src = r#"2025-06-15 * "Deposit" ; txn:ct-deposit
    assets:cash:aud        5000.00 AUD
    equity:opening:aud    -5000.00 AUD

2025-07-10 * "Buy ETH" ; txn:ct-buy
    assets:crypto:eth      10.00000000 ETH {{ 1000.00 AUD }}
    assets:cash:aud       -1000.00 AUD

2025-08-05 * "Route via transfer contra" ; txn:ct-xfer
    assets:transfer:eth     3.00000000 ETH
    assets:crypto:eth      -3.00000000 ETH
"#;
    let txns = parse_ok(src);
    let pg = perf_eth_prices();
    let report = generate_performance_report_range(PerformanceReportParams {
        transactions: &txns,
        price_graph: &pg,
        tax_config: &aud_fy_config(),
        label: "FY2026",
        date_from: "2025-07-01",
        date_to: "2026-06-30",
        base_currency: "AUD",
        base_account_scope: Some("assets"),
        allowed_accounts: None,
    });
    let accounts: Vec<&str> = report
        .account_breakdown
        .iter()
        .map(|s| s.account.as_str())
        .collect();
    assert!(
        !accounts.contains(&"assets:transfer"),
        "transfer contra must be hidden, got {accounts:?}"
    );
    // The real categories are still present.
    assert!(accounts.contains(&"assets:cash"), "{accounts:?}");
    assert!(accounts.contains(&"assets:crypto"), "{accounts:?}");
}

// === Tax Savings (loss-harvesting) report ===
//
// Surfaces current holdings that are underwater (mark-to-market value below
// FIFO cost basis) and estimates the tax saved by realising those losses
// against the financial year's realised capital gains.

#[test]
fn loss_harvest_lists_only_underwater_and_offsets_short_term_gain() {
    // FY2026 (ends 2026-06-30).
    //   * ETH: bought 2025-08-01 @ 2,000, sold 2025-12-01 for 12,000 → a
    //     SHORT-term realised gain of 10,000 (held <12mo). Fully disposed, so
    //     it is NOT a current holding.
    //   * BTC: bought 2 @ 50,000 = 100,000 cost; spot 60,000 → value 120,000,
    //     unrealised +20,000 → a WINNER, must be excluded.
    //   * SOL: bought 100 for 30,000; spot 150 → value 15,000, unrealised
    //     -15,000 → underwater, the only harvestable position.
    let src = r#"2025-08-01 * "Buy ETH" ; txn:t-PLACEHOLDER
    assets:exchange:eth       1.00000000 ETH
    assets:cash:aud          -2000.00 AUD

2025-12-01 * "Sell ETH" ; txn:t-PLACEHOLDER
    assets:exchange:eth      -1.00000000 ETH
    assets:cash:aud          12000.00 AUD

2025-08-01 * "Buy BTC" ; txn:t-PLACEHOLDER
    assets:exchange:btc       2.00000000 BTC
    assets:cash:aud        -100000.00 AUD

2025-09-01 * "Buy SOL" ; txn:t-PLACEHOLDER
    assets:exchange:sol     100.00000000 SOL
    assets:cash:aud         -30000.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![
        price("BTC", "2026-06-30", 60000.0, "AUD"),
        price("SOL", "2026-06-30", 150.0, "AUD"),
    ]);
    let report =
        generate_loss_harvest_report(&txns, &pg, &aud_fy_config(), "2026", "AUD", None, None);

    // Only SOL is underwater; the BTC winner and the disposed ETH are absent.
    assert_eq!(report.positions.len(), 1, "{:?}", report.positions);
    let p = &report.positions[0];
    assert_eq!(p.commodity, "SOL");
    assert!((p.cost_basis - 30000.0).abs() < 1e-6, "cost_basis={}", p.cost_basis);
    assert!((p.value - 15000.0).abs() < 1e-6, "value={}", p.value);
    assert!((p.unrealised_loss - 15000.0).abs() < 1e-6, "loss={}", p.unrealised_loss);
    assert!((p.pct_below_cost - 0.5).abs() < 1e-6, "pct={}", p.pct_below_cost);
    // Single 100-unit buy → one FIFO lot, fully underwater.
    assert_eq!(p.lots.len(), 1);
    assert_eq!(p.lots[0].acquisition_date, "2025-09-01");
    assert!((p.lots[0].quantity - 100.0).abs() < 1e-6);
    assert!((p.lots[0].cost_per_unit - 300.0).abs() < 1e-6);
    assert!((p.lots[0].unrealised + 15000.0).abs() < 1e-6, "lot unrealised={}", p.lots[0].unrealised);
    assert!((report.total_realisable_loss - 15000.0).abs() < 1e-6);

    // Current-FY realised gains context: a single 10,000 short-term gain.
    assert!((report.realised_short_gains - 10000.0).abs() < 1e-6);
    assert!((report.realised_long_gains - 0.0).abs() < 1e-6);
    assert!((report.realised_net_gain - 10000.0).abs() < 1e-6);

    // 15,000 loss offsets the 10,000 short-term gain now; 5,000 carries forward.
    assert!((report.offset_now - 10000.0).abs() < 1e-6, "offset_now={}", report.offset_now);
    assert!((report.carry_forward - 5000.0).abs() < 1e-6, "carry={}", report.carry_forward);

    // Short-term gain is taxed in full, so the saving is 10,000 * 47%.
    assert_eq!(report.marginal_rate_percent, 47);
    assert!((report.estimated_tax_saved - 4700.0).abs() < 1e-6,
        "tax_saved={}", report.estimated_tax_saved);
}

#[test]
fn loss_harvest_discounts_offset_against_long_term_gain() {
    // Same SOL loss, but the FY's realised gain is LONG-term (held >12mo), so
    // offsetting it only reduces taxable gain by the discounted half: the
    // 10,000 offset saves tax on 5,000 → 5,000 * 47% = 2,350.
    let src = r#"2023-06-01 * "Buy ETH" ; txn:t-PLACEHOLDER
    assets:exchange:eth       1.00000000 ETH
    assets:cash:aud          -2000.00 AUD

2025-09-01 * "Sell ETH" ; txn:t-PLACEHOLDER
    assets:exchange:eth      -1.00000000 ETH
    assets:cash:aud          12000.00 AUD

2025-09-01 * "Buy SOL" ; txn:t-PLACEHOLDER
    assets:exchange:sol     100.00000000 SOL
    assets:cash:aud         -30000.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![price("SOL", "2026-06-30", 150.0, "AUD")]);
    let report =
        generate_loss_harvest_report(&txns, &pg, &aud_fy_config(), "2026", "AUD", None, None);

    assert_eq!(report.positions.len(), 1);
    assert_eq!(report.positions[0].commodity, "SOL");
    assert!((report.realised_long_gains - 10000.0).abs() < 1e-6);
    assert!((report.realised_short_gains - 0.0).abs() < 1e-6);
    assert!((report.offset_now - 10000.0).abs() < 1e-6);
    assert!((report.carry_forward - 5000.0).abs() < 1e-6);
    assert!((report.estimated_tax_saved - 2350.0).abs() < 1e-6,
        "tax_saved={}", report.estimated_tax_saved);
}

#[test]
fn loss_harvest_lot_drilldown_shows_gain_and_loss_parcels() {
    // A position that is NET underwater but whose OLDEST parcel is in gain —
    // the BTC case: buy 1 @ 50,000 (2024), buy 1 @ 150,000 (2025); spot 90,000.
    // Net cost 200,000 vs value 180,000 → -20,000 underwater. Lot 1 (50k) is
    // +40,000; lot 2 (150k) is -60,000. Under FIFO, selling 1 unit disposes the
    // GAIN parcel first — which is exactly why the drill-down matters.
    let src = r#"2024-03-01 * "Buy BTC cheap" ; txn:t-PLACEHOLDER
    assets:exchange:btc       1.00000000 BTC
    assets:cash:aud         -50000.00 AUD

2025-11-01 * "Buy BTC dear" ; txn:t-PLACEHOLDER
    assets:exchange:btc       1.00000000 BTC
    assets:cash:aud        -150000.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![price("BTC", "2026-06-30", 90000.0, "AUD")]);
    let report =
        generate_loss_harvest_report(&txns, &pg, &aud_fy_config(), "2026", "AUD", None, None);

    assert_eq!(report.positions.len(), 1);
    let p = &report.positions[0];
    assert_eq!(p.commodity, "BTC");
    assert!((p.unrealised_loss - 20000.0).abs() < 1e-6, "net loss={}", p.unrealised_loss);

    // Two parcels, oldest-first (FIFO disposal order).
    assert_eq!(p.lots.len(), 2);
    assert_eq!(p.lots[0].acquisition_date, "2024-03-01");
    assert_eq!(p.lots[1].acquisition_date, "2025-11-01");
    // Oldest parcel (50k cost, 90k value) is in GAIN; newest is underwater.
    assert!((p.lots[0].unrealised - 40000.0).abs() < 1e-6, "lot0={}", p.lots[0].unrealised);
    assert!((p.lots[1].unrealised + 60000.0).abs() < 1e-6, "lot1={}", p.lots[1].unrealised);
    // Parcels reconcile to the whole-position figures.
    let lot_qty: f64 = p.lots.iter().map(|l| l.quantity).sum();
    let lot_cost: f64 = p.lots.iter().map(|l| l.cost_basis).sum();
    assert!((lot_qty - p.quantity).abs() < 1e-6);
    assert!((lot_cost - p.cost_basis).abs() < 1e-6);
}

#[test]
fn loss_harvest_parcel_scan_finds_underwater_parcels_in_net_positive_holdings() {
    // BTC is NET in gain (+30k) but holds one underwater parcel; SOL is net
    // underwater. The parcel scan must surface BTC's underwater parcel even
    // though BTC is absent from the whole-position list.
    //   BTC: 1 @ 50,000 (2024) + 1 @ 100,000 (2025); spot 90,000 → net +30,000.
    //        cheap parcel +40,000 (gain), dear parcel -10,000 (underwater).
    //   SOL: 100 @ 300 = 30,000; spot 150 → -15,000 (net underwater).
    let src = r#"2024-03-01 * "Buy BTC cheap" ; txn:t-PLACEHOLDER
    assets:exchange:btc       1.00000000 BTC
    assets:cash:aud         -50000.00 AUD

2025-11-01 * "Buy BTC dear" ; txn:t-PLACEHOLDER
    assets:exchange:btc       1.00000000 BTC
    assets:cash:aud        -100000.00 AUD

2025-09-01 * "Buy SOL" ; txn:t-PLACEHOLDER
    assets:exchange:sol     100.00000000 SOL
    assets:cash:aud         -30000.00 AUD
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![
        price("BTC", "2026-06-30", 90000.0, "AUD"),
        price("SOL", "2026-06-30", 150.0, "AUD"),
    ]);
    let report =
        generate_loss_harvest_report(&txns, &pg, &aud_fy_config(), "2026", "AUD", None, None);

    // Whole-position view: BTC is net +30k → excluded; only SOL is underwater.
    assert_eq!(report.positions.len(), 1);
    assert_eq!(report.positions[0].commodity, "SOL");

    // Parcel scan: SOL's parcel (-15k) AND BTC's dear parcel (-10k); BTC's cheap
    // parcel (+40k gain) is excluded. Sorted largest loss first.
    assert_eq!(report.underwater_parcels.len(), 2, "{:?}", report.underwater_parcels);
    assert_eq!(report.underwater_parcels[0].commodity, "SOL");
    assert!((report.underwater_parcels[0].unrealised_loss - 15000.0).abs() < 1e-6);
    let btc = &report.underwater_parcels[1];
    assert_eq!(btc.commodity, "BTC");
    assert_eq!(btc.acquisition_date, "2025-11-01");
    assert!((btc.unrealised_loss - 10000.0).abs() < 1e-6, "btc parcel loss={}", btc.unrealised_loss);
    assert!((report.total_parcel_loss - 25000.0).abs() < 1e-6);
    // No gain parcel leaked into the underwater list.
    assert!(report.underwater_parcels.iter().all(|p| p.unrealised_loss > 0.0));
}

// === AccountAllowlist: extra primary-account prefixes ===
//
// The allowlist that gates Balances / Performance / Tax-Savings holdings is the
// UNION of auto-derived source-folder accounts (`exact`) and user-configured
// `extra_primary_account_prefixes`. Prefixes let nominal accounts (e.g.
// `assets:staking`) count toward holdings; an empty prefix list reproduces the
// folder-accounts-only behaviour. Until now nothing exercised a non-`None`
// allowlist — these are the first.

fn allowlist(exact: &[&str], prefixes: &[&str]) -> AccountAllowlist {
    let exact: std::collections::HashSet<String> = exact.iter().map(|s| s.to_string()).collect();
    let prefixes: Vec<String> = prefixes.iter().map(|s| s.to_string()).collect();
    AccountAllowlist::new(exact, &prefixes)
}

#[test]
fn allowlist_exact_membership() {
    let a = allowlist(&["assets:cash:bank:ubank"], &[]);
    assert!(a.allows("assets:cash:bank:ubank"));
    assert!(!a.allows("assets:cash:bank:cba"));
}

#[test]
fn allowlist_prefix_matches_self_and_children() {
    let a = allowlist(&[], &["assets:staking"]);
    assert!(a.allows("assets:staking")); // the prefix itself
    assert!(a.allows("assets:staking:lido:steth")); // a sub-account
}

#[test]
fn allowlist_prefix_respects_segment_boundary() {
    // A sibling that merely shares leading text must NOT match.
    let a = allowlist(&[], &["assets:staking"]);
    assert!(!a.allows("assets:stakingpool"));
}

#[test]
fn allowlist_unions_exact_and_prefixes() {
    let a = allowlist(&["assets:crypto:wallet:eth"], &["assets:staking"]);
    assert!(a.allows("assets:crypto:wallet:eth")); // exact member
    assert!(a.allows("assets:staking:sol")); // prefix child
    assert!(!a.allows("assets:transfer")); // neither
}

#[test]
fn allowlist_drops_blank_prefixes() {
    // A blank/whitespace entry must not match every account via starts_with("").
    let a = allowlist(&[], &["", "  "]);
    assert!(!a.allows("assets:staking"));
    assert!(a.prefixes.is_empty());
}

/// Buy 10 SOL into a real wallet folder account, then "stake" it by moving the
/// 10 SOL to the nominal `assets:staking:sol` account (a same-commodity transfer
/// out of the wallet). The staked SOL therefore lives OUTSIDE any source folder.
fn staking_fixture_txns() -> Vec<Transaction> {
    let src = r#"2025-07-10 * "Buy SOL" ; txn:t-buy
    assets:crypto:wallet:sol      10.00000000 SOL {{ 1000.00 AUD }}
    assets:cash:aud           -1000.00 AUD

2025-08-01 * "Stake SOL" ; txn:t-stake
    assets:staking:sol            10.00000000 SOL
    assets:crypto:wallet:sol     -10.00000000 SOL
"#;
    parse_ok(src)
}

#[test]
fn holdings_as_of_includes_staking_only_when_prefix_configured() {
    let txns = staking_fixture_txns();
    let pg = PriceGraph::from_entries(vec![price("SOL", "2025-08-31", 150.0, "AUD")]);

    // Folder-only allowlist (the wallet): the wallet nets to 0 and the staked
    // SOL sits outside it, so nothing is held.
    let folder_only = allowlist(&["assets:crypto:wallet:sol"], &[]);
    let snap = holdings_as_of(&txns, &pg, "2025-08-31", "AUD", None, Some(&folder_only));
    assert!(
        snap.holdings.iter().all(|h| h.commodity != "SOL"),
        "staked SOL must be excluded by the folder-only allowlist, got {:?}",
        snap.holdings
    );

    // Add the `assets:staking` prefix → the staked SOL now counts.
    let with_prefix = allowlist(&["assets:crypto:wallet:sol"], &["assets:staking"]);
    let snap = holdings_as_of(&txns, &pg, "2025-08-31", "AUD", None, Some(&with_prefix));
    let sol = snap
        .holdings
        .iter()
        .find(|h| h.commodity == "SOL")
        .expect("SOL must be held once the staking prefix is allowed");
    assert!((sol.quantity - 10.0).abs() < 1e-6, "qty={}", sol.quantity);
    assert!((sol.value - 1500.0).abs() < 1e-6, "value={}", sol.value);
}

#[test]
fn balances_report_includes_staking_only_when_prefix_configured() {
    let txns = staking_fixture_txns();
    let pg = PriceGraph::from_entries(vec![price("SOL", "2025-08-31", 150.0, "AUD")]);

    let folder_only = allowlist(&["assets:crypto:wallet:sol"], &[]);
    let report =
        generate_balances_report_range(&txns, &pg, "2025-08-31", "AUD", None, Some(&folder_only));
    assert!(
        report.holdings.iter().all(|h| h.commodity != "SOL"),
        "staked SOL must be excluded by the folder-only allowlist, got {:?}",
        report.holdings
    );

    let with_prefix = allowlist(&["assets:crypto:wallet:sol"], &["assets:staking"]);
    let report =
        generate_balances_report_range(&txns, &pg, "2025-08-31", "AUD", None, Some(&with_prefix));
    let sol = report
        .holdings
        .iter()
        .find(|h| h.commodity == "SOL")
        .expect("staked SOL must count once its prefix is allowed");
    assert!((sol.quantity - 10.0).abs() < 1e-6, "qty={}", sol.quantity);
}

/// REGRESSION (Kraken earn-autoallocation): a same-account round-trip routed
/// through the `assets:transfer` contra, modelled as TWO separate transactions
/// (out-leg and in-leg). The asset never leaves the tracked account in net
/// terms — it is the SAME 1 BTC swept spot→earn then earn→spot. Cost basis and
/// acquisition date must be preserved.
///
/// Because the holdings allowlist excludes the `assets:transfer` contra, each
/// transaction presents a single primary leg, so the in-transaction transfer
/// netting in `holdings_as_of` never fires: the out-leg looks like a real
/// disposal (consuming the cheap 2016 lot) and the in-leg like a real
/// acquisition (re-costed at the transfer-date spot price). Result: the BTC
/// cost basis is overwritten with recent spot and the acquisition date is reset.
#[test]
fn holdings_as_of_round_trip_transfer_via_excluded_contra_preserves_cost() {
    let src = r#"2016-01-20 * "Buy BTC" ; txn:t-buy
    assets:crypto:exchange:kraken:personal      1.00000000 BTC {{ 1000.00 AUD }}
    assets:cash:aud       -1000.00 AUD

2025-08-13 13:41:28 * "earn autoallocation spot / main" ; txn:t-out1
    assets:crypto:exchange:kraken:personal     -1.00000000 BTC
    assets:transfer      1.00000000 BTC

2025-08-13 13:41:28 * "earn autoallocation earn / liquid" ; txn:t-in1
    assets:crypto:exchange:kraken:personal      1.00000000 BTC
    assets:transfer     -1.00000000 BTC

2025-11-17 10:59:40 * "earn autoallocation spot / main" ; txn:t-in2
    assets:crypto:exchange:kraken:personal      1.00000000 BTC
    assets:transfer     -1.00000000 BTC

2025-11-17 10:59:40 * "earn autoallocation earn / liquid" ; txn:t-out2
    assets:crypto:exchange:kraken:personal     -1.00000000 BTC
    assets:transfer      1.00000000 BTC
"#;
    let txns = parse_ok(src);
    let pg = PriceGraph::from_entries(vec![
        price("BTC", "2025-08-13", 183_579.21, "AUD"),
        price("BTC", "2025-11-17", 146_844.49, "AUD"),
        price("BTC", "2025-12-31", 84_776.00, "AUD"),
    ]);
    // Folder-only allowlist: the kraken account counts, `assets:transfer` does not.
    let folder_only = allowlist(&["assets:crypto:exchange:kraken:personal"], &[]);
    let snap = holdings_as_of(&txns, &pg, "2025-12-31", "AUD", None, Some(&folder_only));

    let btc = snap
        .holdings
        .iter()
        .find(|h| h.commodity == "BTC")
        .expect("BTC must be held");
    assert!((btc.quantity - 1.0).abs() < 1e-6, "qty={}", btc.quantity);
    // The position is the original 1 BTC bought for 1000 AUD in 2016. The
    // round-trip transfers must not touch its cost basis or acquisition date.
    assert!(
        (btc.cost_basis - 1000.0).abs() < 1e-6,
        "cost basis corrupted by transfers: got {} (expected 1000), lots={:?}",
        btc.cost_basis,
        btc.lots
    );
    assert!(
        btc.lots.iter().all(|l| l.acquisition_date == "2016-01-20"),
        "acquisition date reset by transfers, lots={:?}",
        btc.lots
    );
}
