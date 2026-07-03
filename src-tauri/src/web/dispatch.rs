//! Maps a command name + JSON args to the same library calls the desktop
//! Tauri commands make, returning a `serde_json::Value`. Argument structs use
//! `rename_all = "camelCase"` because the frontend sends the same camelCase
//! keys it passes to `invoke()` (Tauri normally maps those to snake_case Rust
//! params; here we do it via serde).
//!
//! Only read-only commands are implemented. Unknown / mutating commands return
//! an error the frontend surfaces exactly as an `invoke()` rejection.

use std::path::Path;

use serde::Deserialize;
use serde_json::{json, Value};

use crate::ledger_parser::{ParseResult, PriceGraph};
use crate::processing_pipeline::{auto_link_equity_swaps, detect_account_gaps, PipelineMetadata};
use crate::reports::{self, TaxConfig};
use crate::{generated_store, issues, report_templates};

use super::context::WebCtx;

/// Dispatch a single read command. `args` is the JSON object the frontend
/// passed to `invoke()`.
pub fn dispatch(ctx: &WebCtx, command: &str, args: &Value) -> Result<Value, String> {
    match command {
        // ── Vault / root ────────────────────────────────────────────
        "has_root_dir" => Ok(json!(ctx.config.current_root.is_some())),
        "get_root_dir" => Ok(json!(ctx.config.current_root)),
        "get_known_roots" => Ok(json!(ctx.config.known_roots)),
        "list_account_sets" => list_account_sets(ctx),
        "get_update_prices_on_startup" => Ok(json!(ctx.config.update_prices_on_startup)),
        "get_extra_primary_account_prefixes" => {
            Ok(json!(ctx.config.extra_primary_account_prefixes))
        }

        // ── Config ──────────────────────────────────────────────────
        "get_display_config" => get_display_config(ctx, args),
        "get_tax_config" => to_value(get_tax_config(ctx, &account_set_arg(args)?)?),

        // ── Reports (financial-year) ────────────────────────────────
        "generate_cgt_report_cmd" => cgt_report(ctx, args),
        "generate_income_report_cmd" => income_report(ctx, args),
        "generate_balances_report_cmd" => balances_report(ctx, args),
        // ── Reports (date range) ────────────────────────────────────
        "generate_cgt_report_range_cmd" => cgt_report_range(ctx, args),
        "generate_income_report_range_cmd" => income_report_range(ctx, args),
        "generate_balances_report_range_cmd" => balances_report_range(ctx, args),
        "generate_performance_report_range_cmd" => performance_report_range(ctx, args),
        "generate_loss_harvest_report_cmd" => loss_harvest_report(ctx, args),
        "generate_loss_harvest_report_range_cmd" => loss_harvest_report_range(ctx, args),

        // ── Report metadata / markdown ──────────────────────────────
        "get_report_cmd" => get_report(ctx, args),
        "list_report_years_cmd" => list_report_years(ctx, args),
        "list_report_accounts_cmd" => list_report_accounts(ctx, args),

        // ── Ledger / query ──────────────────────────────────────────
        "query_search" => query_search(ctx, args),
        "query_global" => query_global(ctx, args),
        "load_account_tree" => load_account_tree(ctx, args),
        "load_generated_ledger" => load_generated_ledger(ctx, args),
        "load_pipeline_metadata" => PipelineMetadata::load(&ctx.generated_dir)
            .ok_or_else(|| "No pipeline metadata found — rebuild required".to_string())
            .and_then(to_value),
        // The desktop runs the pipeline on startup to populate `state.parse`
        // (ledger, balances, account maps). The web server can't rebuild from
        // sources, so this returns the already-generated ledger + metadata in
        // the same PipelineResponse shape — effectively a refresh.
        "rebuild_pipeline" => rebuild_pipeline_readonly(ctx, args),

        // ── Conversions ─────────────────────────────────────────────
        "convert_to_base_currency" => convert_to_base_currency(ctx, args),

        // ── Diagnostics ─────────────────────────────────────────────
        "get_account_gaps" => to_value(detect_account_gaps(&ctx.generated_dir)?),
        "collect_issues_cmd" => collect_issues(ctx, args),
        // StartupWarnings are populated by the desktop pipeline run; the web
        // server doesn't run the pipeline, so there are none to report.
        "get_pipeline_warnings" => Ok(json!([])),

        // ── Desktop-only in read mode (degrade to empty, don't error) ─
        // Trade links live in the in-memory Automerge store, which the web
        // server doesn't initialize. The plugins view is desktop-only.
        "get_trade_links" | "suggest_trade_links_cmd" | "list_plugins" => Ok(json!([])),
        // No-op acknowledgements so frontend startup doesn't hard-fail.
        "init_metadata"
        | "set_show_hidden"
        | "set_update_prices_on_startup"
        | "set_extra_primary_account_prefixes" => Ok(Value::Null),

        other => Err(format!("command not supported in web mode: {other}")),
    }
}

// ── helpers ─────────────────────────────────────────────────────────

fn to_value<T: serde::Serialize>(v: T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|e| e.to_string())
}

fn parse_args<T: serde::de::DeserializeOwned>(args: &Value) -> Result<T, String> {
    serde_json::from_value(args.clone()).map_err(|e| format!("invalid args: {e}"))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountSetArgs {
    #[serde(default)]
    account_set: String,
}

fn account_set_arg(args: &Value) -> Result<String, String> {
    let a: AccountSetArgs = parse_args(args)?;
    Ok(a.account_set)
}

/// `read_cached_report` equivalent — the on-disk report JSON is already the
/// serialized report, so we return it verbatim rather than round-tripping
/// through the concrete report type.
fn read_cached_report_value(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<Value>(&text).ok()
}

fn primary_accounts_allowlist(
    ctx: &WebCtx,
    generated_dir: &Path,
) -> Option<reports::AccountAllowlist> {
    PipelineMetadata::load(generated_dir).map(|m| {
        reports::AccountAllowlist::new(
            m.account_folders.into_keys().collect(),
            &ctx.config.extra_primary_account_prefixes,
        )
    })
}

/// The `(None, None, txn)` tagging dance the report commands run before
/// generation, so split equity-swap legs get auto-linked.
fn apply_auto_link(parse: &mut ParseResult, price_graph: &PriceGraph, base_currency: &str) {
    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(price_graph), Some(base_currency));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();
}

fn get_tax_config(ctx: &WebCtx, account_set: &str) -> Result<TaxConfig, String> {
    let config_path = ctx.set_dir(account_set).join("config.json");
    if config_path.exists() {
        let contents = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read config.json: {e}"))?;
        let json: Value = serde_json::from_str(&contents)
            .map_err(|e| format!("failed to parse config.json: {e}"))?;
        if let Some(tax) = json.get("tax") {
            return serde_json::from_value(tax.clone())
                .map_err(|e| format!("failed to parse tax config: {e}"));
        }
    }
    Ok(TaxConfig::default())
}

// ── command implementations ─────────────────────────────────────────

fn list_account_sets(ctx: &WebCtx) -> Result<Value, String> {
    if !ctx.sources_dir.exists() {
        return Ok(json!([]));
    }
    let entries = std::fs::read_dir(&ctx.sources_dir)
        .map_err(|e| format!("failed to read sources dir: {e}"))?;
    let mut sets = std::collections::BTreeSet::new();
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if name.starts_with('_') || name.starts_with('.') {
            continue;
        }
        if let Some(prefix) = name.split('-').next() {
            sets.insert(prefix.to_string());
        }
    }
    Ok(json!(sets.into_iter().collect::<Vec<_>>()))
}

fn get_display_config(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let config_path = ctx.set_dir(&account_set_arg(args)?).join("config.json");
    if config_path.exists() {
        let contents = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read config.json: {e}"))?;
        serde_json::from_str(&contents).map_err(|e| format!("failed to parse config.json: {e}"))
    } else {
        // Read-only: return defaults without writing (the desktop command
        // would create the file).
        Ok(json!({
            "commodities": { "AUD": { "decimals": 2 }, "USD": { "decimals": 2 } },
            "default_decimals": 2
        }))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FyArgs {
    #[serde(default)]
    account_set: String,
    financial_year: String,
    base_currency: String,
    #[serde(default)]
    base_account_scope: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RangeArgs {
    #[serde(default)]
    account_set: String,
    date_from: String,
    date_to: String,
    base_currency: String,
    #[serde(default)]
    base_account_scope: Option<String>,
}

fn cgt_report(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: FyArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    if a.base_account_scope.is_none() {
        let json_path = report_templates::cgt_json_path(&set_dir, &a.financial_year);
        if let Some(v) = read_cached_report_value(&json_path) {
            return Ok(v);
        }
    }
    let mut parse = generated_store::load_active_ledger(&set_dir)?;
    let price_graph = PriceGraph::load(&ctx.sources_dir);
    let tax_config = get_tax_config(ctx, &a.account_set)?;
    apply_auto_link(&mut parse, &price_graph, &a.base_currency);
    to_value(reports::generate_cgt_report(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &a.financial_year,
        &a.base_currency,
        a.base_account_scope.as_deref(),
    ))
}

fn cgt_report_range(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: RangeArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    let parse = generated_store::load_active_ledger(&set_dir)?;
    let price_graph = PriceGraph::load(&ctx.sources_dir);
    let tax_config = get_tax_config(ctx, &a.account_set)?;
    let label = format!("{} to {}", a.date_from, a.date_to);
    to_value(reports::generate_cgt_report_range(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &label,
        &a.date_from,
        &a.date_to,
        &a.base_currency,
        a.base_account_scope.as_deref(),
    ))
}

fn income_report(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: FyArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    if a.base_account_scope.is_none() {
        let json_path = report_templates::income_json_path(&set_dir, &a.financial_year);
        if let Some(v) = read_cached_report_value(&json_path) {
            return Ok(v);
        }
    }
    let mut parse = generated_store::load_active_ledger(&set_dir)?;
    let price_graph = PriceGraph::load(&ctx.sources_dir);
    let tax_config = get_tax_config(ctx, &a.account_set)?;
    apply_auto_link(&mut parse, &price_graph, &a.base_currency);
    to_value(reports::generate_income_report(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &a.financial_year,
        &a.base_currency,
        a.base_account_scope.as_deref(),
    ))
}

fn income_report_range(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: RangeArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    let parse = generated_store::load_active_ledger(&set_dir)?;
    let price_graph = PriceGraph::load(&ctx.sources_dir);
    let tax_config = get_tax_config(ctx, &a.account_set)?;
    let label = format!("{} to {}", a.date_from, a.date_to);
    to_value(reports::generate_income_report_range(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &label,
        &a.date_from,
        &a.date_to,
        &a.base_currency,
        a.base_account_scope.as_deref(),
    ))
}

fn balances_report(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: FyArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    if a.base_account_scope.is_none() {
        let json_path = report_templates::balances_json_path(&set_dir, &a.financial_year);
        if let Some(v) = read_cached_report_value(&json_path) {
            return Ok(v);
        }
    }
    let mut parse = generated_store::load_active_ledger(&set_dir)?;
    let hidden = generated_store::load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        generated_store::filter_hidden_accounts(&mut parse, &hidden);
    }
    let price_graph = PriceGraph::load(&ctx.sources_dir);
    let tax_config = get_tax_config(ctx, &a.account_set)?;
    apply_auto_link(&mut parse, &price_graph, &a.base_currency);
    let allowed = primary_accounts_allowlist(ctx, &ctx.generated_dir);
    to_value(reports::generate_balances_report(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &a.financial_year,
        &a.base_currency,
        a.base_account_scope.as_deref(),
        allowed.as_ref(),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BalancesRangeArgs {
    #[serde(default)]
    account_set: String,
    date_to: String,
    base_currency: String,
    #[serde(default)]
    base_account_scope: Option<String>,
}

fn balances_report_range(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: BalancesRangeArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    let mut parse = generated_store::load_active_ledger(&set_dir)?;
    let hidden = generated_store::load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        generated_store::filter_hidden_accounts(&mut parse, &hidden);
    }
    let price_graph = PriceGraph::load(&ctx.sources_dir);
    apply_auto_link(&mut parse, &price_graph, &a.base_currency);
    let allowed = primary_accounts_allowlist(ctx, &ctx.generated_dir);
    to_value(reports::generate_balances_report_range(
        &parse.transactions,
        &price_graph,
        &a.date_to,
        &a.base_currency,
        a.base_account_scope.as_deref(),
        allowed.as_ref(),
    ))
}

fn performance_report_range(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: RangeArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    let mut parse = generated_store::load_active_ledger(&set_dir)?;
    let hidden = generated_store::load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        generated_store::filter_hidden_accounts(&mut parse, &hidden);
    }
    let price_graph = PriceGraph::load(&ctx.sources_dir);
    let tax_config = get_tax_config(ctx, &a.account_set)?;
    apply_auto_link(&mut parse, &price_graph, &a.base_currency);
    let allowed = primary_accounts_allowlist(ctx, &ctx.generated_dir);
    let label = format!("{} to {}", a.date_from, a.date_to);
    to_value(reports::generate_performance_report_range(
        reports::PerformanceReportParams {
            transactions: &parse.transactions,
            price_graph: &price_graph,
            tax_config: &tax_config,
            label: &label,
            date_from: &a.date_from,
            date_to: &a.date_to,
            base_currency: &a.base_currency,
            base_account_scope: a.base_account_scope.as_deref(),
            allowed_accounts: allowed.as_ref(),
        },
    ))
}

fn loss_harvest_report(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: FyArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    let mut parse = generated_store::load_active_ledger(&set_dir)?;
    let hidden = generated_store::load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        generated_store::filter_hidden_accounts(&mut parse, &hidden);
    }
    let price_graph = PriceGraph::load(&ctx.sources_dir);
    let tax_config = get_tax_config(ctx, &a.account_set)?;
    apply_auto_link(&mut parse, &price_graph, &a.base_currency);
    let allowed = primary_accounts_allowlist(ctx, &ctx.generated_dir);
    to_value(reports::generate_loss_harvest_report(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &a.financial_year,
        &a.base_currency,
        a.base_account_scope.as_deref(),
        allowed.as_ref(),
    ))
}

fn loss_harvest_report_range(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: RangeArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    let mut parse = generated_store::load_active_ledger(&set_dir)?;
    let hidden = generated_store::load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        generated_store::filter_hidden_accounts(&mut parse, &hidden);
    }
    let price_graph = PriceGraph::load(&ctx.sources_dir);
    let tax_config = get_tax_config(ctx, &a.account_set)?;
    apply_auto_link(&mut parse, &price_graph, &a.base_currency);
    let allowed = primary_accounts_allowlist(ctx, &ctx.generated_dir);
    to_value(reports::generate_loss_harvest_report_range(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &a.date_from,
        &a.date_to,
        &a.base_currency,
        a.base_account_scope.as_deref(),
        allowed.as_ref(),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GetReportArgs {
    #[serde(default)]
    account_set: String,
    report_type: String,
    financial_year: String,
}

fn get_report(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: GetReportArgs = parse_args(args)?;
    let report_path = ctx
        .set_dir(&a.account_set)
        .join("reports")
        .join(format!("{}-{}.md", a.report_type, a.financial_year));
    if report_path.exists() {
        let md = std::fs::read_to_string(&report_path)
            .map_err(|e| format!("failed to read report: {e}"))?;
        Ok(json!(md))
    } else {
        Ok(json!(""))
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListYearsArgs {
    #[serde(default)]
    account_set: String,
    report_type: String,
}

fn list_report_years(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: ListYearsArgs = parse_args(args)?;
    let reports_dir = ctx.set_dir(&a.account_set).join("reports");
    Ok(json!(report_templates::list_report_years(
        &reports_dir,
        &a.report_type
    )))
}

fn list_report_accounts(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let set_dir = ctx.set_dir(&account_set_arg(args)?);
    let parse = generated_store::load_active_ledger(&set_dir)?;
    let (income, expenses) = reports::list_report_accounts(&parse.transactions);
    Ok(json!({ "income": income, "expenses": expenses }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueryArgs {
    #[serde(default)]
    account_set: String,
    search: String,
    #[serde(default)]
    sort_field: Option<String>,
    #[serde(default)]
    sort_order: Option<String>,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

fn query_search(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: QueryArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    let result = generated_store::scoped_query(
        &set_dir,
        &a.search,
        ctx.show_hidden,
        a.sort_field.as_deref(),
        a.sort_order.as_deref(),
        a.offset,
        a.limit,
    )?;
    to_value(result)
}

/// Union-of-all-ledgers query for the Categories page (non-folder-backed
/// accounts). Mirrors the desktop `query_global` command.
fn query_global(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: QueryArgs = parse_args(args)?;
    let set_dir = ctx.set_dir(&a.account_set);
    let result = generated_store::global_query(
        &set_dir,
        &a.search,
        ctx.show_hidden,
        a.sort_field.as_deref(),
        a.sort_order.as_deref(),
        a.offset,
        a.limit,
    )?;
    to_value(result)
}

fn load_account_tree(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let set_dir = ctx.set_dir(&account_set_arg(args)?);
    let mut balances = generated_store::load_account_tree(&set_dir)?;
    let prefixes = generated_store::load_hidden_accounts(&set_dir);
    if !ctx.show_hidden && !prefixes.is_empty() {
        balances.retain(|b| !prefixes.iter().any(|pfx| b.account.starts_with(pfx.as_str())));
    }
    to_value(balances)
}

fn load_generated_ledger(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let set_dir = ctx.set_dir(&account_set_arg(args)?);
    let mut parse = generated_store::load_active_ledger(&set_dir)?;
    if !ctx.show_hidden {
        let prefixes = generated_store::load_hidden_accounts(&set_dir);
        if !prefixes.is_empty() {
            generated_store::filter_hidden_accounts(&mut parse, &prefixes);
        }
    }
    // ParseResult serializes identically to the desktop ParseResponse.
    to_value(parse)
}

fn rebuild_pipeline_readonly(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let set_dir = ctx.set_dir(&account_set_arg(args)?);
    let mut parse = generated_store::load_active_ledger(&set_dir)?;
    if !ctx.show_hidden {
        let prefixes = generated_store::load_hidden_accounts(&set_dir);
        if !prefixes.is_empty() {
            generated_store::filter_hidden_accounts(&mut parse, &prefixes);
        }
    }
    // Account maps come from the persisted pipeline metadata (no rebuild).
    let meta = PipelineMetadata::load(&ctx.generated_dir);
    let (owner_accounts, account_folders, account_properties) = match meta {
        Some(m) => (m.owner_accounts, m.account_folders, m.account_properties),
        None => (
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        ),
    };
    Ok(json!({
        "result": {
            "csv_transformed": 0,
            "csv_cached": 0,
            "ofx_transformed": 0,
            "ofx_cached": 0,
            "manual_count": 0,
            "total_written": 0,
            "warnings": [],
            "owner_accounts": owner_accounts,
            "account_folders": account_folders,
            "account_properties": account_properties,
            "early_exit": true,
            "output_files_written": 0,
            "output_files_skipped": 0,
            "changed_folders": []
        },
        "parse": parse,
        "warnings": []
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConvertArgs {
    base_currency: String,
    requests: Vec<ConvReq>,
}

#[derive(Deserialize)]
struct ConvReq {
    commodity: String,
    amount: f64,
    datetime: String,
}

fn convert_to_base_currency(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: ConvertArgs = parse_args(args)?;
    let graph = PriceGraph::load(&ctx.sources_dir);
    let values: Vec<Option<f64>> = a
        .requests
        .iter()
        .map(|r| graph.convert_to_base(&r.commodity, r.amount, &r.datetime, &a.base_currency))
        .collect();
    Ok(json!({ "values": values }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssuesArgs {
    #[serde(default)]
    account: Option<String>,
}

fn collect_issues(ctx: &WebCtx, args: &Value) -> Result<Value, String> {
    let a: IssuesArgs = parse_args(args)?;
    let filter = issues::CollectFilter {
        categories: issues::Category::ALL.iter().copied().collect(),
        account: a.account,
    };
    to_value(issues::collect_all(
        &ctx.sources_dir,
        &ctx.generated_dir,
        &filter,
    )?)
}
