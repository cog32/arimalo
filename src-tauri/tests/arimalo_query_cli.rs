//! Integration tests for the `arimalo-query` CLI binary.
//!
//! These run the compiled binary (via `CARGO_BIN_EXE_arimalo-query`) to exercise
//! argument parsing and the full load-query-format pipeline end-to-end.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "arimalo-query-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &Path, s: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, s).unwrap();
}

const HNT_LEDGER: &str = "\
2025-05-29T23:42:10 * \"Bybit\" \"Trade\"
    assets:crypto:exchange:bybit  13.904 HNT
    equity:trading:buy           -50.000 USDT

2025-11-19T18:31:02 * \"Bybit\" \"Trade\"
    assets:crypto:exchange:bybit  26.772 HNT
    equity:trading:buy           -54.557 USDT
";

fn run_query(generated: &Path, args: &[&str]) -> (String, String, i32) {
    let bin = env!("CARGO_BIN_EXE_arimalo-query");
    let output = Command::new(bin)
        .args(args)
        .env("ARIMALO_GENERATED_DIR", generated)
        .output()
        .expect("failed to run arimalo-query");
    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
    )
}

fn count_from_json(stdout: &str) -> usize {
    let v: serde_json::Value =
        serde_json::from_str(stdout).unwrap_or_else(|e| panic!("bad JSON: {e}\n{stdout}"));
    v.get("transaction_count")
        .and_then(|n| n.as_u64())
        .unwrap_or_else(|| panic!("no transaction_count in: {stdout}")) as usize
}

#[test]
fn finds_transactions_in_nested_layout_without_args() {
    // Real vault layout: ledgers live at generated/<user>/<category>/.../ledger.transactions.
    let root = tmp();
    let generated = root.join("generated");
    write(
        &generated
            .join("richard")
            .join("crypto")
            .join("exchange")
            .join("bybit")
            .join("personal")
            .join("ledger.transactions"),
        HNT_LEDGER,
    );

    let (stdout, stderr, code) = run_query(&generated, &["commodity:HNT", "--format", "json"]);
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");
    assert_eq!(count_from_json(&stdout), 2, "stdout:\n{stdout}");
}

#[test]
fn finds_transactions_at_explicit_dir() {
    // Passing the bybit personal dir directly should work the same — library
    // walks recursively from whatever base it's given.
    let root = tmp();
    let generated = root.join("generated");
    let bybit = generated
        .join("richard")
        .join("crypto")
        .join("exchange")
        .join("bybit")
        .join("personal");
    write(&bybit.join("ledger.transactions"), HNT_LEDGER);

    let (stdout, stderr, code) = run_query(
        &generated,
        &[bybit.to_str().unwrap(), "commodity:HNT", "--format", "json"],
    );
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");
    assert_eq!(count_from_json(&stdout), 2, "stdout:\n{stdout}");
}

#[test]
fn unions_multiple_sibling_sets() {
    // Vault has multiple top-level users; querying at the root should union
    // postings from all of them (no "first one wins" discovery).
    let root = tmp();
    let generated = root.join("generated");
    write(
        &generated
            .join("richard")
            .join("crypto")
            .join("exchange")
            .join("bybit")
            .join("ledger.transactions"),
        HNT_LEDGER,
    );
    write(
        &generated.join("suzi").join("ledger.transactions"),
        HNT_LEDGER,
    );

    let (stdout, stderr, code) = run_query(&generated, &["commodity:HNT", "--format", "json"]);
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");
    assert_eq!(count_from_json(&stdout), 4, "stdout:\n{stdout}");
}

// ---------------------------------------------------------------------------
// --balances mode
// ---------------------------------------------------------------------------

fn seed_hnt(generated: &Path) {
    write(
        &generated
            .join("richard")
            .join("crypto")
            .join("exchange")
            .join("bybit")
            .join("personal")
            .join("ledger.transactions"),
        HNT_LEDGER,
    );
}

#[test]
fn balances_json_shape() {
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);

    let (stdout, stderr, code) = run_query(&generated, &["--balances", "--format", "json"]);
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");

    let v: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("bad JSON: {e}\n{stdout}"));
    assert_eq!(v["transaction_count"].as_u64(), Some(2), "stdout:\n{stdout}");

    let balances = v["balances"]
        .as_array()
        .unwrap_or_else(|| panic!("balances is not an array: {stdout}"));
    assert_eq!(balances.len(), 2, "stdout:\n{stdout}");

    let mut by_commodity: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
    for entry in balances {
        let c = entry["commodity"]
            .as_str()
            .unwrap_or_else(|| panic!("commodity missing: {entry}"))
            .to_string();
        let q = entry["quantity"]
            .as_f64()
            .unwrap_or_else(|| panic!("quantity missing or not a number: {entry}"));
        by_commodity.insert(c, q);
    }
    assert!((by_commodity["HNT"] - 40.676).abs() < 1e-6, "{by_commodity:?}");
    assert!((by_commodity["USDT"] - -104.557).abs() < 1e-6, "{by_commodity:?}");
}

#[test]
fn balances_register_tabular() {
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);

    let (stdout, stderr, code) = run_query(&generated, &["--balances", "--format", "register"]);
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");
    assert!(stdout.contains("COMMODITY"), "stdout:\n{stdout}");
    assert!(stdout.contains("QUANTITY"), "stdout:\n{stdout}");
    assert!(stdout.contains("HNT"), "stdout:\n{stdout}");
    assert!(stdout.contains("USDT"), "stdout:\n{stdout}");
    assert!(stdout.contains("40.6760"), "stdout:\n{stdout}");
    assert!(stdout.contains("-104.5570"), "stdout:\n{stdout}");
    assert!(stdout.contains("2 commodities"), "stdout:\n{stdout}");
    // Must NOT print per-transaction rows
    assert!(!stdout.contains("Bybit"), "stdout:\n{stdout}");
}

#[test]
fn balances_summary_format() {
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);

    let (stdout, stderr, code) = run_query(&generated, &["--balances", "--format", "summary"]);
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");
    assert!(stdout.contains("Balances"), "stdout:\n{stdout}");
    assert!(stdout.contains("40.6760 HNT"), "stdout:\n{stdout}");
    assert!(stdout.contains("-104.5570 USDT"), "stdout:\n{stdout}");
    // Summary under --balances should not list accounts (that's the old --format summary)
    assert!(!stdout.contains("Accounts:"), "stdout:\n{stdout}");
}

#[test]
fn balances_account_filter_narrows_to_scope_postings() {
    // account: is the scope-style filter: when present, only postings matching
    // the account regex contribute to the balance. This is the primary use case
    // for the Balances report (e.g. `account:assets:crypto`).
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);

    let (stdout, stderr, code) = run_query(
        &generated,
        &[
            "--balances",
            "account:assets:crypto",
            "--format",
            "json",
        ],
    );
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");

    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let balances = v["balances"].as_array().unwrap();
    // Only the assets:crypto:exchange:bybit posting (HNT) — not the equity:trading USDT side.
    assert_eq!(balances.len(), 1, "stdout:\n{stdout}");
    assert_eq!(balances[0]["commodity"], "HNT");
    assert!((balances[0]["quantity"].as_f64().unwrap() - 40.676).abs() < 1e-6);
}

#[test]
fn balances_commodity_filter_includes_all_postings_of_matched_txns() {
    // commodity: matches at transaction level (find txns that touch this commodity).
    // Balance output includes all postings of those txns — including the counter-
    // commodity on the other side of a trade. Use account: if you want to isolate
    // one side.
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);

    let (stdout, stderr, code) = run_query(
        &generated,
        &["--balances", "commodity:HNT", "--format", "json"],
    );
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");

    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let balances = v["balances"].as_array().unwrap();
    assert_eq!(balances.len(), 2, "stdout:\n{stdout}");
}

#[test]
fn balances_honours_date_filter() {
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);

    // Only the first (May) transaction should be included.
    let (stdout, stderr, code) = run_query(
        &generated,
        &["--balances", "date:<=2025-06-01", "--format", "json"],
    );
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");

    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["transaction_count"].as_u64(), Some(1), "stdout:\n{stdout}");

    let balances = v["balances"].as_array().unwrap();
    let hnt = balances
        .iter()
        .find(|b| b["commodity"] == "HNT")
        .unwrap_or_else(|| panic!("no HNT in: {stdout}"));
    // Only the first trade → 13.904 HNT (not 40.676).
    assert!((hnt["quantity"].as_f64().unwrap() - 13.904).abs() < 1e-6);
}

#[test]
fn no_balances_flag_yields_original_json_shape() {
    // Regression guard: omitting --balances must preserve the existing QueryResult JSON
    // shape, which plugins like binance-prices depend on.
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);

    let (stdout, stderr, code) = run_query(&generated, &["--format", "json"]);
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");

    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // Full QueryResult: transactions, balances (AccountBalance objects), aggregated_balance,
    // accounts, transaction_count all present.
    assert_eq!(v["transactions"].as_array().unwrap().len(), 2, "stdout:\n{stdout}");
    assert!(v.get("aggregated_balance").is_some(), "stdout:\n{stdout}");
    assert!(v.get("accounts").is_some(), "stdout:\n{stdout}");
    assert_eq!(v["transaction_count"].as_u64(), Some(2));

    // The default `balances` field is an array of AccountBalance objects with an `account`
    // key — distinct from --balances mode's `{commodity, quantity}` shape.
    let default_balances = v["balances"].as_array().unwrap();
    assert!(
        default_balances.iter().all(|b| b.get("account").is_some()),
        "default balances should be AccountBalance shape with `account` key: {stdout}"
    );
}

// ---------------------------------------------------------------------------
// --min-value-usd
// ---------------------------------------------------------------------------

/// Write `_prices/<commodity>.txt` containing one P directive `<price> USD`
/// dated 2026-01-01 (latest-wins for `convert_to_base_latest`).
fn write_price_usd(sources: &Path, commodity: &str, price: f64) {
    write(
        &sources.join("_prices").join(format!("{commodity}.txt")),
        &format!("P 2026-01-01 {commodity} {price} USD\n"),
    );
}

#[test]
fn min_value_usd_errors_when_prices_dir_has_no_prices_subdir() {
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);
    let empty_sources = root.join("empty_sources");
    fs::create_dir_all(&empty_sources).unwrap();

    let (stdout, stderr, code) = run_query(
        &generated,
        &[
            "--balances",
            "--min-value-usd",
            "1",
            "--prices-dir",
            empty_sources.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 2, "should exit 2; stdout:\n{stdout}\nstderr:\n{stderr}");
    assert!(
        stderr.contains("--min-value-usd requires"),
        "stderr should explain the missing prices dir:\n{stderr}"
    );
    assert!(stdout.is_empty(), "stdout should be empty on error:\n{stdout}");
}

#[test]
fn min_value_usd_drops_unpriced_commodities() {
    // HNT ledger is seeded but neither HNT.txt nor USDT.txt exists → both balances
    // should be filtered out (treated as value 0).
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);
    let sources = root.join("sources");
    fs::create_dir_all(sources.join("_prices")).unwrap();

    let (stdout, stderr, code) = run_query(
        &generated,
        &[
            "--balances",
            "--min-value-usd",
            "1",
            "--prices-dir",
            sources.to_str().unwrap(),
            "--format",
            "json",
        ],
    );
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");

    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let balances = v["balances"].as_array().unwrap();
    assert!(
        balances.is_empty(),
        "all unpriced commodities should be dropped; got:\n{stdout}"
    );
    // Transaction count is independent of the filter — still 2 from the ledger.
    assert_eq!(v["transaction_count"].as_u64(), Some(2));
}

#[test]
fn min_value_usd_keeps_above_threshold_drops_below() {
    // HNT total = 40.676, USDT total = -104.557.
    // With HNT @ $1 USD and USDT @ $1 USD:
    //   HNT value = 40.676; USDT value = 104.557 (abs).
    // Threshold $50 → HNT dropped, USDT kept.
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);
    let sources = root.join("sources");
    write_price_usd(&sources, "HNT", 1.0);
    write_price_usd(&sources, "USDT", 1.0);

    let (stdout, stderr, code) = run_query(
        &generated,
        &[
            "--balances",
            "--min-value-usd",
            "50",
            "--prices-dir",
            sources.to_str().unwrap(),
            "--format",
            "json",
        ],
    );
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");

    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let balances = v["balances"].as_array().unwrap();
    assert_eq!(balances.len(), 1, "expected only USDT to survive:\n{stdout}");
    assert_eq!(balances[0]["commodity"], "USDT");
}

#[test]
fn min_value_usd_filters_default_mode_aggregated_balance() {
    // Regression guard: the filter must apply in default (non --balances) mode too.
    // The default mode's aggregated_balance is summed from posting amounts on
    // accounts matched by the search; seed two transactions on the same account.
    let root = tmp();
    let generated = root.join("generated");
    write(
        &generated
            .join("richard")
            .join("crypto")
            .join("exchange")
            .join("bybit")
            .join("personal")
            .join("ledger.transactions"),
        "\
2026-01-01 * \"Bybit\" \"Trade\"
    assets:crypto:exchange:bybit  100.000 HNT
    equity:trading:buy           -500.000 USDT

2026-01-02 * \"Spam Drop\" \"Random airdrop\"
    assets:crypto:exchange:bybit  3.000 SPAMTOKEN
    income:airdrops              -3.000 SPAMTOKEN
",
    );
    let sources = root.join("sources");
    write_price_usd(&sources, "HNT", 5.0); // 100 × 5 = $500 → keep

    let (stdout, stderr, code) = run_query(
        &generated,
        &[
            "account:assets:crypto:exchange:bybit",
            "--min-value-usd",
            "1",
            "--prices-dir",
            sources.to_str().unwrap(),
            "--format",
            "json",
        ],
    );
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");

    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let agg = v["aggregated_balance"].as_array().unwrap();
    assert_eq!(agg.len(), 1, "SPAMTOKEN should be filtered out:\n{stdout}");
    assert_eq!(agg[0]["commodity"], "HNT");
    // Per-account balances also filtered — no SPAMTOKEN entry.
    let balances = v["balances"].as_array().unwrap();
    for b in balances {
        let commodities: Vec<&str> = b["totals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["commodity"].as_str().unwrap())
            .collect();
        assert!(
            !commodities.contains(&"SPAMTOKEN"),
            "SPAMTOKEN leaked into per-account balances: {b}"
        );
    }
}

#[test]
fn min_value_usd_omitted_means_no_filter() {
    // Sanity: without --min-value-usd, --prices-dir is irrelevant and unpriced
    // commodities still appear. No regression for existing callers.
    let root = tmp();
    let generated = root.join("generated");
    seed_hnt(&generated);

    let (stdout, stderr, code) = run_query(&generated, &["--balances", "--format", "json"]);
    assert_eq!(code, 0, "exit {code}; stderr:\n{stderr}");

    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let balances = v["balances"].as_array().unwrap();
    assert_eq!(balances.len(), 2, "no filter → both commodities present:\n{stdout}");
}
