use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tera::{Context, Tera};

use crate::generated_store::{filter_hidden_accounts, load_active_ledger, load_hidden_accounts};
use crate::ledger_parser::{PriceGraph, Transaction};
use crate::processing_pipeline::auto_link_equity_swaps;
use crate::report_csv;
use crate::reports::{self, CgtReport, IncomeTaxReport, TaxConfig};

/// Output formats `regenerate_reports_for_set` can emit. Pipeline runs request
/// `ALL`; the standalone `arimalo-reports` CLI lets the user narrow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportFormat {
    Json,
    Md,
    Csv,
}

/// Default = every supported format. Used by the pipeline rebuild path.
pub const ALL_FORMATS: &[ReportFormat] =
    &[ReportFormat::Json, ReportFormat::Md, ReportFormat::Csv];

const CGT_TEMPLATE: &str = include_str!("../templates/cgt_report.md.tera");
const INCOME_TEMPLATE: &str = include_str!("../templates/income_report.md.tera");

// === Template context types (pre-formatted for display) ===

#[derive(Serialize)]
struct CgtTemplateEvent {
    sell_date: String,
    buy_date: String,
    commodity: String,
    quantity: String,
    cost_basis: String,
    sale_proceeds: String,
    capital_gain: String,
    holding_days: i64,
    discount: String,
    discounted_gain: String,
}

#[derive(Serialize)]
struct IncomeTemplateCategory {
    account: String,
    total: String,
}

// === Public API ===

/// Render a CGT report to markdown from parsed transactions.
pub fn render_cgt_report(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    tax_config: &TaxConfig,
    financial_year: &str,
    base_currency: &str,
) -> Result<String, String> {
    let report = reports::generate_cgt_report(
        transactions,
        price_graph,
        tax_config,
        financial_year,
        base_currency,
        None,
    );
    render_cgt_markdown(&report, tax_config)
}

/// Render an income report to markdown from parsed transactions.
pub fn render_income_report(
    transactions: &[Transaction],
    price_graph: &PriceGraph,
    tax_config: &TaxConfig,
    financial_year: &str,
    base_currency: &str,
) -> Result<String, String> {
    let report = reports::generate_income_report(
        transactions,
        price_graph,
        tax_config,
        financial_year,
        base_currency,
        None,
    );
    render_income_markdown(&report, tax_config, base_currency)
}

// === JSON artifact paths ===
//
// Per-FY JSON snapshots are written by `regenerate_reports_for_set` on every
// pipeline rebuild and consumed by the FY-form Tauri report commands. Keeping
// the path construction here means the on-disk layout has a single owner; if
// it changes, only the helpers below need to update.

pub fn reports_dir(set_dir: &Path) -> PathBuf {
    set_dir.join("reports")
}

pub fn cgt_json_path(set_dir: &Path, financial_year: &str) -> PathBuf {
    reports_dir(set_dir).join(format!("cgt-{financial_year}.json"))
}

pub fn income_json_path(set_dir: &Path, financial_year: &str) -> PathBuf {
    reports_dir(set_dir).join(format!("income-{financial_year}.json"))
}

pub fn balances_json_path(set_dir: &Path, financial_year: &str) -> PathBuf {
    reports_dir(set_dir).join(format!("balances-{financial_year}.json"))
}

pub fn cgt_csv_path(set_dir: &Path, financial_year: &str) -> PathBuf {
    reports_dir(set_dir).join(format!("cgt-{financial_year}.csv"))
}

pub fn income_csv_path(set_dir: &Path, financial_year: &str) -> PathBuf {
    reports_dir(set_dir).join(format!("income-{financial_year}.csv"))
}

pub fn balances_csv_path(set_dir: &Path, financial_year: &str) -> PathBuf {
    reports_dir(set_dir).join(format!("balances-{financial_year}.csv"))
}

/// Generate all reports for a single generated set directory.
/// Determines relevant FYs from transactions and renders CGT, income, and
/// balances reports — both as JSON artifacts (single source of truth for any
/// downstream renderer) and, where a markdown template exists, as `.md`.
///
/// `balances_primary_accounts` constrains the Balances report to the supplied
/// set of accounts (typically the source-folder primaries from
/// `pipeline-metadata.json`). Pass `None` to leave the Balances report
/// un-filtered (e.g. tests, or vaults without a metadata file).
pub fn regenerate_reports_for_set(
    set_dir: &Path,
    sources_dir: &Path,
    formats: &[ReportFormat],
    balances_primary_accounts: Option<&reports::AccountAllowlist>,
) -> Result<Vec<String>, String> {
    let want_json = formats.contains(&ReportFormat::Json);
    let want_md = formats.contains(&ReportFormat::Md);
    let want_csv = formats.contains(&ReportFormat::Csv);
    if !(want_json || want_md || want_csv) {
        return Ok(Vec::new());
    }
    let mut parse = load_active_ledger(set_dir)?;
    // Reports always exclude hidden accounts (config.json's `hidden_accounts`
    // prefixes — `ignore` by default). The Show Ignored runtime toggle only
    // affects the live UI; tax reports should never count txns the user has
    // explicitly hidden.
    let hidden = load_hidden_accounts(set_dir);
    if !hidden.is_empty() {
        filter_hidden_accounts(&mut parse, &hidden);
    }
    let price_graph = PriceGraph::load(sources_dir);
    let tax_config = load_tax_config(set_dir);
    let base_currency = load_base_currency(set_dir);

    // Match the live FY commands: ensure auto-linked swap price annotations are
    // on the in-memory postings before any price lookups, so the cached JSON
    // matches what an on-demand command would have produced.
    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(&price_graph), Some(&base_currency));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();

    let fys = relevant_financial_years(&parse.transactions, &tax_config);
    let dir = reports_dir(set_dir);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create reports dir: {e}"))?;

    let mut generated = Vec::new();

    for fy in &fys {
        let fy_str = fy.to_string();

        // CGT
        let cgt = reports::generate_cgt_report(
            &parse.transactions,
            &price_graph,
            &tax_config,
            &fy_str,
            &base_currency,
            None,
        );
        if want_json {
            write_json(&cgt_json_path(set_dir, &fy_str), &cgt)?;
            generated.push(format!("cgt-{fy}.json"));
        }
        if want_md {
            let cgt_md = render_cgt_markdown(&cgt, &tax_config)?;
            let cgt_md_path = dir.join(format!("cgt-{fy}.md"));
            std::fs::write(&cgt_md_path, &cgt_md)
                .map_err(|e| format!("failed to write {}: {e}", cgt_md_path.display()))?;
            generated.push(format!("cgt-{fy}.md"));
        }
        if want_csv {
            let csv = report_csv::cgt_to_csv(&cgt)?;
            let path = cgt_csv_path(set_dir, &fy_str);
            std::fs::write(&path, csv)
                .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
            generated.push(format!("cgt-{fy}.csv"));
        }

        // Income
        let income = reports::generate_income_report(
            &parse.transactions,
            &price_graph,
            &tax_config,
            &fy_str,
            &base_currency,
            None,
        );
        if want_json {
            write_json(&income_json_path(set_dir, &fy_str), &income)?;
            generated.push(format!("income-{fy}.json"));
        }
        if want_md {
            let income_md = render_income_markdown(&income, &tax_config, &base_currency)?;
            let income_md_path = dir.join(format!("income-{fy}.md"));
            std::fs::write(&income_md_path, &income_md)
                .map_err(|e| format!("failed to write {}: {e}", income_md_path.display()))?;
            generated.push(format!("income-{fy}.md"));
        }
        if want_csv {
            let csv = report_csv::income_to_csv(&income)?;
            let path = income_csv_path(set_dir, &fy_str);
            std::fs::write(&path, csv)
                .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
            generated.push(format!("income-{fy}.csv"));
        }

        // Balances — no markdown template yet.
        let balances = reports::generate_balances_report(
            &parse.transactions,
            &price_graph,
            &tax_config,
            &fy_str,
            &base_currency,
            None,
            balances_primary_accounts,
        );
        if want_json {
            write_json(&balances_json_path(set_dir, &fy_str), &balances)?;
            generated.push(format!("balances-{fy}.json"));
        }
        if want_csv {
            let csv = report_csv::balances_to_csv(&balances)?;
            let path = balances_csv_path(set_dir, &fy_str);
            std::fs::write(&path, csv)
                .map_err(|e| format!("failed to write {}: {e}", path.display()))?;
            generated.push(format!("balances-{fy}.csv"));
        }
    }

    Ok(generated)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value)
        .map_err(|e| format!("failed to serialize {}: {e}", path.display()))?;
    std::fs::write(path, text)
        .map_err(|e| format!("failed to write {}: {e}", path.display()))
}

/// Generate reports for all account sets after a pipeline rebuild.
pub fn generate_all_reports(
    sources_dir: &Path,
    generated_dir: &Path,
    account_sets: &[String],
    formats: &[ReportFormat],
    extra_prefixes: &[String],
) -> Result<(), String> {
    let primaries: Option<reports::AccountAllowlist> =
        crate::processing_pipeline::PipelineMetadata::load(generated_dir).map(|m| {
            reports::AccountAllowlist::new(m.account_folders.into_keys().collect(), extra_prefixes)
        });
    if account_sets.is_empty() {
        // Root directory (no multi-entity)
        regenerate_reports_for_set(generated_dir, sources_dir, formats, primaries.as_ref())?;
    } else {
        for set_name in account_sets {
            let set_dir = generated_dir.join(set_name);
            if set_dir.exists() {
                regenerate_reports_for_set(&set_dir, sources_dir, formats, primaries.as_ref())?;
            }
        }
    }
    Ok(())
}

/// List available report years for a given report type in a reports directory.
pub fn list_report_years(reports_dir: &Path, report_type: &str) -> Vec<i32> {
    let prefix = format!("{report_type}-");
    let mut years = Vec::new();
    if let Ok(entries) = std::fs::read_dir(reports_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&prefix) && name.ends_with(".md") {
                let year_str = &name[prefix.len()..name.len() - 3];
                if let Ok(year) = year_str.parse::<i32>() {
                    years.push(year);
                }
            }
        }
    }
    years.sort();
    years
}

// === Internal helpers ===

fn render_cgt_markdown(report: &CgtReport, tax_config: &TaxConfig) -> Result<String, String> {
    let mut tera = Tera::default();
    tera.add_raw_template("cgt", CGT_TEMPLATE)
        .map_err(|e| format!("CGT template error: {e}"))?;

    let (fy_start, fy_end) = fy_label(&report.financial_year, tax_config)?;

    let events: Vec<CgtTemplateEvent> = report
        .events
        .iter()
        .map(|e| CgtTemplateEvent {
            sell_date: e.sell_date.clone(),
            buy_date: e.buy_date.clone(),
            commodity: e.commodity.clone(),
            quantity: format!("{:.4}", e.quantity),
            cost_basis: format!("{:.2}", e.cost_basis),
            sale_proceeds: format!("{:.2}", e.sale_proceeds),
            capital_gain: format!("{:.2}", e.capital_gain),
            holding_days: e.holding_days,
            discount: if e.discount_eligible {
                "Yes".into()
            } else {
                "No".into()
            },
            discounted_gain: format!("{:.2}", e.discounted_gain),
        })
        .collect();

    let mut ctx = Context::new();
    ctx.insert("financial_year", &report.financial_year);
    ctx.insert("fy_start", &fy_start);
    ctx.insert("fy_end", &fy_end);
    ctx.insert("has_events", &!report.events.is_empty());
    ctx.insert("events", &events);
    ctx.insert("total_gains", &format!("{:.2}", report.total_gains));
    ctx.insert("total_losses", &format!("{:.2}", report.total_losses));
    ctx.insert(
        "net_capital_gain",
        &format!("{:.2}", report.net_capital_gain),
    );
    ctx.insert(
        "total_discounted_gain",
        &format!("{:.2}", report.total_discounted_gain),
    );
    ctx.insert("has_warnings", &!report.warnings.is_empty());
    ctx.insert("warnings", &report.warnings);

    tera.render("cgt", &ctx)
        .map_err(|e| format!("CGT render error: {e}"))
}

fn render_income_markdown(
    report: &IncomeTaxReport,
    tax_config: &TaxConfig,
    base_currency: &str,
) -> Result<String, String> {
    let mut tera = Tera::default();
    tera.add_raw_template("income", INCOME_TEMPLATE)
        .map_err(|e| format!("Income template error: {e}"))?;

    let (fy_start, fy_end) = fy_label(&report.financial_year, tax_config)?;

    let income_categories: Vec<IncomeTemplateCategory> = report
        .income_categories
        .iter()
        .map(|c| IncomeTemplateCategory {
            account: c.account.clone(),
            total: format!("{:.2}", c.total),
        })
        .collect();

    let expense_categories: Vec<IncomeTemplateCategory> = report
        .expense_categories
        .iter()
        .map(|c| IncomeTemplateCategory {
            account: c.account.clone(),
            total: format!("{:.2}", c.total),
        })
        .collect();

    let mut ctx = Context::new();
    ctx.insert("financial_year", &report.financial_year);
    ctx.insert("fy_start", &fy_start);
    ctx.insert("fy_end", &fy_end);
    ctx.insert("base_currency", base_currency);
    ctx.insert("has_income", &!report.income_categories.is_empty());
    ctx.insert("income_categories", &income_categories);
    ctx.insert("has_expenses", &!report.expense_categories.is_empty());
    ctx.insert("expense_categories", &expense_categories);
    ctx.insert("total_income", &format!("{:.2}", report.total_income));
    ctx.insert("total_expenses", &format!("{:.2}", report.total_expenses));
    ctx.insert("net", &format!("{:.2}", report.net));
    ctx.insert("warnings", &report.warnings);

    tera.render("income", &ctx)
        .map_err(|e| format!("Income render error: {e}"))
}

/// Compute FY start/end labels for display.
fn fy_label(fy: &str, tax_config: &TaxConfig) -> Result<(String, String), String> {
    let fy_year: i32 = fy
        .parse()
        .map_err(|_| format!("invalid financial year: '{fy}'"))?;
    let end_month = tax_config.financial_year_end_month;
    let end_day = tax_config.financial_year_end_day;
    let start_month = end_month + 1;
    let (start_year, start_month) = if start_month > 12 {
        (fy_year, 1u32) // calendar year: Jan–Dec of fy_year
    } else {
        (fy_year - 1, start_month) // split year: starts previous calendar year
    };
    let start = format!("{start_year}-{start_month:02}-01");
    let end = format!("{fy_year}-{end_month:02}-{end_day:02}");
    Ok((start, end))
}

/// Determine which financial years are relevant for a set of transactions.
fn relevant_financial_years(transactions: &[Transaction], tax_config: &TaxConfig) -> Vec<i32> {
    let end_m = tax_config.financial_year_end_month;
    let end_d = tax_config.financial_year_end_day;
    let mut fys: HashSet<i32> = HashSet::new();
    for txn in transactions {
        if let Some((y, m, d)) = parse_date(&txn.date) {
            // FY year = year the FY ends. Date after FY end → next FY.
            // E.g. 2025-08-20 with end_month=6 → FY2026 (Jul 2025–Jun 2026)
            let fy = if m > end_m || (m == end_m && d > end_d) {
                y + 1
            } else {
                y
            };
            fys.insert(fy);
        }
    }
    let mut sorted: Vec<i32> = fys.into_iter().collect();
    sorted.sort();
    sorted
}

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

/// Load TaxConfig from config.json, falling back to defaults.
pub fn load_tax_config(set_dir: &Path) -> TaxConfig {
    let config_path = set_dir.join("config.json");
    if let Ok(text) = std::fs::read_to_string(&config_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(tax) = json.get("tax") {
                if let Ok(tc) = serde_json::from_value::<TaxConfig>(tax.clone()) {
                    return tc;
                }
            }
        }
    }
    TaxConfig::default()
}

/// Persist TaxConfig into the "tax" key of config.json in `set_dir`.
/// Other keys in config.json are preserved. The file is created if absent.
pub fn persist_tax_config(set_dir: &Path, config: &TaxConfig) -> Result<(), String> {
    let config_path = set_dir.join("config.json");
    let mut json: serde_json::Value = if config_path.exists() {
        let text = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read config.json: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("failed to parse config.json: {e}"))?
    } else {
        serde_json::json!({})
    };
    json["tax"] = serde_json::to_value(config)
        .map_err(|e| format!("failed to serialize tax config: {e}"))?;
    std::fs::create_dir_all(set_dir).map_err(|e| format!("failed to create config dir: {e}"))?;
    let pretty = crate::to_sorted_json_pretty(&json)
        .map_err(|e| format!("failed to serialize config: {e}"))?;
    std::fs::write(&config_path, &pretty)
        .map_err(|e| format!("failed to write config.json: {e}"))
}

/// Load base currency from config.json, falling back to "AUD".
fn load_base_currency(set_dir: &Path) -> String {
    let config_path = set_dir.join("config.json");
    if let Ok(text) = std::fs::read_to_string(&config_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(bc) = json.get("base_currency").and_then(|v| v.as_str()) {
                return bc.to_string();
            }
        }
    }
    "AUD".to_string()
}
