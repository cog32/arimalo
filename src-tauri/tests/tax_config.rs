//! Unit tests for tax-config persistence (issue #40).
//!
//! `get_tax_config` and `save_tax_config` Tauri commands delegate their
//! filesystem I/O to `load_tax_config` / `persist_tax_config` in
//! `report_templates`. These tests exercise those functions directly with a
//! temp directory so no Tauri AppHandle is needed.

use arimalo_covid::report_templates::{load_tax_config, persist_tax_config};
use arimalo_covid::reports::TaxConfig;
use tempfile::TempDir;

fn tmp() -> TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn custom_config() -> TaxConfig {
    TaxConfig {
        financial_year_end_month: 3,
        financial_year_end_day: 31,
        cgt_discount_percent: 33,
        cgt_discount_holding_months: 24,
        non_taxable_accounts: vec!["assets:super".to_string()],
        non_deductible_accounts: vec!["expenses:personal".to_string()],
        marginal_tax_rate_percent: 45,
    }
}

// === load_tax_config ===

#[test]
fn load_returns_default_when_no_file() {
    let dir = tmp();
    let cfg = load_tax_config(dir.path());
    let def = TaxConfig::default();
    assert_eq!(cfg.financial_year_end_month, def.financial_year_end_month);
    assert_eq!(cfg.cgt_discount_percent, def.cgt_discount_percent);
    assert_eq!(cfg.non_taxable_accounts, def.non_taxable_accounts);
}

#[test]
fn load_returns_default_when_tax_key_absent() {
    let dir = tmp();
    std::fs::write(dir.path().join("config.json"), r#"{"base_currency":"AUD"}"#).unwrap();
    let cfg = load_tax_config(dir.path());
    assert_eq!(cfg.cgt_discount_percent, TaxConfig::default().cgt_discount_percent);
}

#[test]
fn load_parses_tax_key() {
    let dir = tmp();
    let json = serde_json::json!({
        "tax": {
            "financial_year_end_month": 3,
            "financial_year_end_day": 31,
            "cgt_discount_percent": 33,
            "cgt_discount_holding_months": 24,
            "non_taxable_accounts": ["assets:super"],
            "non_deductible_accounts": []
        }
    });
    std::fs::write(dir.path().join("config.json"), json.to_string()).unwrap();
    let cfg = load_tax_config(dir.path());
    assert_eq!(cfg.financial_year_end_month, 3);
    assert_eq!(cfg.cgt_discount_percent, 33);
    assert_eq!(cfg.non_taxable_accounts, vec!["assets:super"]);
}

// === persist_tax_config ===

#[test]
fn persist_creates_file_with_tax_key() {
    let dir = tmp();
    persist_tax_config(dir.path(), &custom_config()).unwrap();
    let text = std::fs::read_to_string(dir.path().join("config.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["tax"]["cgt_discount_percent"], 33);
    assert_eq!(json["tax"]["financial_year_end_month"], 3);
}

#[test]
fn persist_preserves_other_keys() {
    let dir = tmp();
    std::fs::write(
        dir.path().join("config.json"),
        r#"{"base_currency":"USD","commodities":{"BTC":{"decimals":8}}}"#,
    )
    .unwrap();
    persist_tax_config(dir.path(), &TaxConfig::default()).unwrap();
    let text = std::fs::read_to_string(dir.path().join("config.json")).unwrap();
    let json: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(json["base_currency"], "USD");
    assert_eq!(json["commodities"]["BTC"]["decimals"], 8);
    assert!(json.get("tax").is_some());
}

#[test]
fn persist_then_load_roundtrips() {
    let dir = tmp();
    let original = custom_config();
    persist_tax_config(dir.path(), &original).unwrap();
    let loaded = load_tax_config(dir.path());
    assert_eq!(loaded.financial_year_end_month, original.financial_year_end_month);
    assert_eq!(loaded.financial_year_end_day, original.financial_year_end_day);
    assert_eq!(loaded.cgt_discount_percent, original.cgt_discount_percent);
    assert_eq!(loaded.cgt_discount_holding_months, original.cgt_discount_holding_months);
    assert_eq!(loaded.non_taxable_accounts, original.non_taxable_accounts);
    assert_eq!(loaded.non_deductible_accounts, original.non_deductible_accounts);
}

#[test]
fn persist_overwrites_existing_tax_key() {
    let dir = tmp();
    persist_tax_config(dir.path(), &TaxConfig::default()).unwrap();
    persist_tax_config(dir.path(), &custom_config()).unwrap();
    let loaded = load_tax_config(dir.path());
    assert_eq!(loaded.cgt_discount_percent, 33);
}

#[test]
fn persist_creates_parent_dir_if_missing() {
    let dir = tmp();
    let nested = dir.path().join("set-a");
    persist_tax_config(&nested, &TaxConfig::default()).unwrap();
    assert!(nested.join("config.json").exists());
}
