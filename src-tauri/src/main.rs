#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![deny(warnings)]

use serde::Serialize;
use arimalo_covid::automerge_store::{
    suggest_trade_links, DeviceInfo, MetadataStore, SyncEvent, TradeLink, TradeSuggestion,
};
use arimalo_covid::content_store::ContentStore;
use arimalo_covid::generated_store::{
    filter_hidden_accounts, load_active_ledger, load_hidden_accounts, ManualTransactionInput,
};
use arimalo_covid::ledger_parser::{
    parse_transactions, AccountBalance, AccountProperties, Diagnostic, ParseResult, PriceGraph,
    Transaction,
};
use arimalo_covid::plugins;
use arimalo_covid::processing_pipeline::{
    append_account_and_rebuild, append_hide_rule, append_manual_and_rebuild,
    auto_link_equity_swaps,
    delete_account_folder_and_rebuild, delete_manual_transaction_and_rebuild, detect_account_gaps,
    import_prices_file, process_all_imports, process_imports, rename_account_folder_and_rebuild,
    set_price_directive,
    run_pipeline, save_transform_and_rebuild, suggest_transform_for_import, update_account_name,
    update_opening_balance, AccountGap, ImportResult, PipelineConfig, PipelineMetadata,
    PipelineResult,
};
use arimalo_covid::query::QueryResult;
use arimalo_covid::relay_client;
use arimalo_covid::reports::{
    self, BalancesReport, CgtReport, IncomeTaxReport, LossHarvestReport, PerformanceReport,
    TaxConfig,
};
use arimalo_covid::root_config::{self, RootConfig};
use arimalo_covid::rules::{
    generate_rule_id, remove_trade_link_rules, AccountConfig, LabelsFile,
    Rule,
    RulesFile,
};
use arimalo_covid::sync::full_sync;
use arimalo_covid::trade_link_repair::{load_per_folder_ledgers, plan_trade_link_rules};
use std::env;
use std::path::PathBuf;
use tauri::{Emitter, Manager};

/// Placeholder yyyymm value for commands that don't need time-based rotation.
const YYYYMM_UNUSED: &str = "000000";
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug, Serialize)]
struct ParseResponse {
    ok: bool,
    diagnostics: Vec<Diagnostic>,
    transactions: Vec<Transaction>,
    balances: Vec<AccountBalance>,
    accounts_with_opening: Vec<String>,
    account_properties: std::collections::HashMap<String, AccountProperties>,
}

impl From<ParseResult> for ParseResponse {
    fn from(result: ParseResult) -> Self {
        Self {
            ok: result.ok,
            diagnostics: result.diagnostics,
            transactions: result.transactions,
            balances: result.balances,
            accounts_with_opening: result.accounts_with_opening,
            account_properties: result.account_properties,
        }
    }
}

#[derive(Debug, Serialize)]
struct PipelineResponse {
    result: PipelineResult,
    parse: ParseResponse,
    warnings: Vec<String>,
}

/// Slim alternative to PipelineResponse for mutations that don't need the
/// full ledger snapshot to come back over IPC. The frontend keeps its
/// existing state.parse and refreshes only the visible search window
/// (TX_WINDOW=500 rows) via the paged query API. On a 40K-txn vault this
/// drops the IPC payload from ~8MB JSON to a few hundred bytes — and
/// avoids the multi-second main-thread JSON deserialize that was producing
/// the macOS beach ball on click.
#[derive(Debug, Serialize)]
struct MutationResponse {
    ok: bool,
    warnings: Vec<String>,
    output_files_written: usize,
}

fn resolve_generated_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let env_override = env::var("ARIMALO_GENERATED_DIR").ok();
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    let config = app.state::<RootConfigState>().0.lock().clone();
    Ok(root_config::resolve_generated(
        env_override.as_deref(),
        &config,
        &app_dir,
    ))
}

fn resolve_sources_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let env_override = env::var("ARIMALO_SOURCES_DIR").ok();
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    let config = app.state::<RootConfigState>().0.lock().clone();
    Ok(root_config::resolve_sources(
        env_override.as_deref(),
        &config,
        &app_dir,
    ))
}

fn resolve_set_dir(generated_dir: &std::path::Path, account_set: &str) -> PathBuf {
    if account_set.is_empty() {
        generated_dir.to_path_buf()
    } else {
        generated_dir.join(account_set)
    }
}

/// Build the allowlist of accounts that count toward the Balances / Performance
/// / Tax-Savings reports: the auto-derived source-folder accounts from
/// `pipeline-metadata.json`, unioned with the user's
/// `extra_primary_account_prefixes` (read from the global config). Returns
/// `None` when no `pipeline-metadata.json` is present (brand-new vault that
/// hasn't been built yet) — callers must treat that as "no filtering" rather
/// than "empty set". An empty prefix list reproduces the folder-only behaviour.
fn primary_accounts_allowlist(
    app: &tauri::AppHandle,
    generated_dir: &std::path::Path,
) -> Option<reports::AccountAllowlist> {
    let extra_prefixes = app
        .state::<RootConfigState>()
        .0
        .lock()
        .extra_primary_account_prefixes
        .clone();
    PipelineMetadata::load(generated_dir).map(|m| {
        reports::AccountAllowlist::new(m.account_folders.into_keys().collect(), &extra_prefixes)
    })
}

/// Extract account prefix from a search string like "account:assets:crypto AND date:>=2025".
/// Returns the account value (e.g. "assets:crypto") or empty string if no account filter.
fn build_pipeline_response(
    app: &tauri::AppHandle,
    result: PipelineResult,
    set_dir: &std::path::Path,
    account_set: &str,
) -> Result<PipelineResponse, String> {
    // Invalidate ledger cache — pipeline output changed
    app.state::<LedgerCache>().invalidate();
    let mut parse = match result.parse_result_for_set(account_set) {
        Some(p) => p,
        None => load_active_ledger(set_dir)?,
    };
    maybe_filter_hidden(app, set_dir, &mut parse);
    let warnings = result.warnings.clone();
    Ok(PipelineResponse {
        result,
        parse: parse.into(),
        warnings,
    })
}

/// Build a slim mutation response for a balance-changing rebuild. Unlike
/// `build_pipeline_response` it does NOT load and serialise the full active
/// ledger (~40MB on a large vault) — the frontend refreshes the sidebar via
/// `load_account_tree` and the visible window via `query_search`, both of which
/// read fresh from disk. The `LedgerCache` is invalidated here to match
/// `build_pipeline_response`'s contract (the on-disk ledger just changed).
fn build_mutation_response(app: &tauri::AppHandle, result: &PipelineResult) -> MutationResponse {
    app.state::<LedgerCache>().invalidate();
    MutationResponse {
        ok: true,
        warnings: result.warnings.clone(),
        output_files_written: result.output_files_written,
    }
}

fn make_pipeline_config(
    app: &tauri::AppHandle,
    now_yyyymm: &str,
) -> Result<PipelineConfig, String> {
    let root_config = app.state::<RootConfigState>().0.lock().clone();
    Ok(PipelineConfig {
        sources_dir: resolve_sources_dir(app)?,
        generated_dir: resolve_generated_dir(app)?,
        now_yyyymm: now_yyyymm.to_string(),
        force: false,
        default_expense_account: root_config.default_expense_account.clone(),
        changed_folder_hint: None,
    })
}

#[tauri::command]
async fn load_generated_ledger(
    app: tauri::AppHandle,
    account_set: String,
) -> Result<ParseResponse, String> {
    let cache = app.state::<LedgerCache>();
    let result = cache.get_or_load(&app, &account_set)?;
    Ok(result.into())
}

/// Load account tree from per-folder summaries (no transaction parsing).
#[tauri::command]
async fn load_account_tree(
    app: tauri::AppHandle,
    account_set: String,
) -> Result<Vec<AccountBalance>, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let mut balances = arimalo_covid::generated_store::load_account_tree(&set_dir)?;
    // Filter hidden accounts
    let prefixes = load_hidden_accounts(&set_dir);
    if !app.state::<ShowHiddenState>().0.load(Ordering::Relaxed) && !prefixes.is_empty() {
        balances.retain(|b| {
            !prefixes
                .iter()
                .any(|pfx| b.account.starts_with(pfx.as_str()))
        });
    }
    Ok(balances)
}

#[tauri::command]
async fn query_search(
    app: tauri::AppHandle,
    account_set: String,
    search: String,
    sort_field: Option<String>,
    sort_order: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<QueryResult, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let show_hidden = app.state::<ShowHiddenState>().0.load(Ordering::Relaxed);
    arimalo_covid::generated_store::scoped_query(
        &set_dir,
        &search,
        show_hidden,
        sort_field.as_deref(),
        sort_order.as_deref(),
        offset,
        limit,
    )
}

/// Like [`query_search`] but queries the UNION of every per-folder ledger in the
/// set instead of folder-pruning. Used by the Categories page to view
/// non-folder-backed accounts (income/expenses/equity/liabilities and `assets`
/// contras), which `query_search` returns zero rows for.
#[tauri::command]
fn query_global(
    app: tauri::AppHandle,
    account_set: String,
    search: String,
    sort_field: Option<String>,
    sort_order: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<QueryResult, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let show_hidden = app.state::<ShowHiddenState>().0.load(Ordering::Relaxed);
    arimalo_covid::generated_store::global_query(
        &set_dir,
        &search,
        show_hidden,
        sort_field.as_deref(),
        sort_order.as_deref(),
        offset,
        limit,
    )
}

#[tauri::command]
fn load_pipeline_metadata(app: tauri::AppHandle) -> Result<PipelineMetadata, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    PipelineMetadata::load(&generated_dir)
        .ok_or_else(|| "No pipeline metadata found — rebuild required".to_string())
}

#[tauri::command]
async fn rebuild_pipeline(
    app: tauri::AppHandle,
    now_yyyymm: String,
    account_set: String,
) -> Result<PipelineResponse, String> {
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[tauri::command]
async fn add_manual_transaction(
    app: tauri::AppHandle,
    now_yyyymm: String,
    input: ManualTransactionInput,
    account_folder: String,
    _account_set: String,
) -> Result<MutationResponse, String> {
    let mut config = make_pipeline_config(&app, &now_yyyymm)?;
    // Scope the rebuild to the changed folder's account-set (skips global cache
    // load + scopes auto_link_equity_swaps to that set) — see save_trade_link.
    config.changed_folder_hint = Some(vec![account_folder.clone()]);
    // Mark before the write so the file-watcher skips its redundant rebuild.
    app.state::<LastCommandRebuild>().mark();
    let result = append_manual_and_rebuild(&config, &input, &account_folder)?;
    spawn_report_generation(&app, &config, &result);
    Ok(build_mutation_response(&app, &result))
}

#[tauri::command]
async fn add_account_declaration(
    app: tauri::AppHandle,
    now_yyyymm: String,
    account_name: String,
    currency: Option<String>,
    opening_balance: Option<String>,
    account_set: String,
    account_folder: Option<String>,
) -> Result<PipelineResponse, String> {
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let result = append_account_and_rebuild(
        &config,
        &account_name,
        currency.as_deref(),
        opening_balance.as_deref(),
        &account_set,
        account_folder.as_deref(),
    )?;
    spawn_report_generation(&app, &config, &result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[tauri::command]
async fn rename_account_folder(
    app: tauri::AppHandle,
    now_yyyymm: String,
    old_folder: String,
    new_folder: String,
    account_set: String,
) -> Result<PipelineResponse, String> {
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let result = rename_account_folder_and_rebuild(&config, &old_folder, &new_folder)?;
    spawn_report_generation(&app, &config, &result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[tauri::command]
async fn delete_account_folder(
    app: tauri::AppHandle,
    now_yyyymm: String,
    folder: String,
    account_set: String,
) -> Result<PipelineResponse, String> {
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let result = delete_account_folder_and_rebuild(&config, &folder)?;
    spawn_report_generation(&app, &config, &result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[tauri::command]
async fn import_csv_to_account(
    app: tauri::AppHandle,
    now_yyyymm: String,
    source_path: String,
    account_folder: String,
    _account_set: String,
) -> Result<MutationResponse, String> {
    let mut config = make_pipeline_config(&app, &now_yyyymm)?;
    config.changed_folder_hint = Some(vec![account_folder.clone()]);
    app.state::<LastCommandRebuild>().mark();
    let source = std::path::Path::new(&source_path);

    // Copy file into imports/ subdirectory first, then process
    let imports_dir = config.sources_dir.join(&account_folder).join("imports");
    std::fs::create_dir_all(&imports_dir)
        .map_err(|e| format!("failed to create imports dir: {e}"))?;
    let filename = source.file_name().ok_or("source has no filename")?;
    let imports_dest = imports_dir.join(filename);
    std::fs::copy(source, &imports_dest).map_err(|e| format!("failed to copy to imports: {e}"))?;

    let (_import_result, result) = process_imports(&config, &account_folder)?;
    spawn_report_generation(&app, &config, &result);
    Ok(build_mutation_response(&app, &result))
}

/// Import several CSVs into one account in a single pipeline run. Stages every
/// file into the account's `imports/` dir, then calls `process_imports` once —
/// it drains the whole dir, so N files cost one rebuild instead of N.
#[tauri::command]
async fn import_csv_files_to_account(
    app: tauri::AppHandle,
    now_yyyymm: String,
    source_paths: Vec<String>,
    account_folder: String,
    _account_set: String,
) -> Result<MutationResponse, String> {
    let mut config = make_pipeline_config(&app, &now_yyyymm)?;
    config.changed_folder_hint = Some(vec![account_folder.clone()]);
    app.state::<LastCommandRebuild>().mark();

    let imports_dir = config.sources_dir.join(&account_folder).join("imports");
    std::fs::create_dir_all(&imports_dir)
        .map_err(|e| format!("failed to create imports dir: {e}"))?;
    for source_path in &source_paths {
        let source = std::path::Path::new(source_path);
        let filename = source.file_name().ok_or("source has no filename")?;
        let imports_dest = imports_dir.join(filename);
        std::fs::copy(source, &imports_dest)
            .map_err(|e| format!("failed to copy {source_path} to imports: {e}"))?;
    }

    let (_import_result, result) = process_imports(&config, &account_folder)?;
    spawn_report_generation(&app, &config, &result);
    Ok(build_mutation_response(&app, &result))
}

#[derive(Debug, Serialize)]
struct ProcessImportsResponse {
    import_result: ImportResult,
    pipeline: PipelineResponse,
}

#[tauri::command]
async fn process_imports_cmd(
    app: tauri::AppHandle,
    now_yyyymm: String,
    account_set: String,
) -> Result<ProcessImportsResponse, String> {
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let (import_result, pipeline_result) = process_all_imports(&config)?;
    spawn_report_generation(&app, &config, &pipeline_result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    Ok(ProcessImportsResponse {
        import_result,
        pipeline: build_pipeline_response(&app, pipeline_result, &set_dir, &account_set)?,
    })
}

/// Hide a transaction by writing a meta-pattern rule into the txn's source
/// folder. The rule re-categorises the txn to `ignore:hidden`, which the
/// existing hidden-accounts filter (driven by `config.json`'s
/// `hidden_accounts`) elides from queries when "Show Ignored" is off.
///
/// Returns a slim MutationResponse rather than a full PipelineResponse —
/// the frontend keeps its existing state.parse and refreshes only the
/// visible search window.
///
/// CRITICAL: this command is `async` so Tauri runs it on the runtime's
/// thread pool instead of the WebView main thread. Sync Tauri commands
/// block the WebView UI thread for their entire duration — a 7-second
/// pipeline rebuild on a 40K-txn vault was triggering the macOS beach
/// ball even though the JS thread was idle waiting on the invoke.
#[tauri::command]
async fn hide_transaction(
    app: tauri::AppHandle,
    now_yyyymm: String,
    txn_id: String,
    account_folder: String,
    _account_set: String,
) -> Result<MutationResponse, String> {
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    append_hide_rule(&config.sources_dir, &account_folder, &txn_id)?;
    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    Ok(MutationResponse {
        ok: true,
        warnings: result.warnings.clone(),
        output_files_written: result.output_files_written,
    })
}

#[tauri::command]
async fn delete_manual_transaction(
    app: tauri::AppHandle,
    now_yyyymm: String,
    datetime: String,
    payee: String,
    narration: String,
    account_folder: String,
    account_set: String,
) -> Result<PipelineResponse, String> {
    let mut config = make_pipeline_config(&app, &now_yyyymm)?;
    config.changed_folder_hint = Some(vec![account_folder.clone()]);
    app.state::<LastCommandRebuild>().mark();
    let result = delete_manual_transaction_and_rebuild(
        &config,
        &datetime,
        &payee,
        &narration,
        &account_folder,
    )?;
    spawn_report_generation(&app, &config, &result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[derive(Debug, Serialize)]
struct SuggestTransformResponse {
    needs_transform: bool,
    suggestion: Option<String>,
    csv_filename: String,
    headers: Vec<String>,
}

#[tauri::command]
fn suggest_transform(
    app: tauri::AppHandle,
    source_path: String,
    account_folder: String,
    _account_name: String,
    currency: Option<String>,
) -> Result<SuggestTransformResponse, String> {
    let config = make_pipeline_config(&app, YYYYMM_UNUSED)?;
    let path = std::path::Path::new(&source_path);
    let csv_filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();

    let suggestion =
        suggest_transform_for_import(&config, path, &account_folder, currency.as_deref())?;

    let headers = csv::Reader::from_path(path)
        .ok()
        .and_then(|mut r| {
            r.headers()
                .ok()
                .map(|h| h.iter().map(|s| s.to_string()).collect())
        })
        .unwrap_or_default();

    Ok(SuggestTransformResponse {
        needs_transform: suggestion.is_some(),
        suggestion,
        csv_filename,
        headers,
    })
}

#[tauri::command]
fn read_transform(app: tauri::AppHandle, account_folder: String) -> Result<Option<String>, String> {
    let config = make_pipeline_config(&app, YYYYMM_UNUSED)?;
    let transform_path = config
        .sources_dir
        .join(&account_folder)
        .join("_transform.rhai");
    if transform_path.exists() {
        std::fs::read_to_string(&transform_path)
            .map(Some)
            .map_err(|e| format!("failed to read transform: {e}"))
    } else {
        Ok(None)
    }
}

#[tauri::command]
fn save_transform(
    app: tauri::AppHandle,
    account_folder: String,
    script: String,
) -> Result<(), String> {
    // Validate Rhai syntax before saving
    let engine = rhai::Engine::new();
    engine
        .compile(&script)
        .map_err(|e| format!("Rhai syntax error: {e}"))?;

    let config = make_pipeline_config(&app, YYYYMM_UNUSED)?;
    let dest_dir = config.sources_dir.join(&account_folder);
    std::fs::create_dir_all(&dest_dir).map_err(|e| format!("failed to create dir: {e}"))?;
    let transform_path = dest_dir.join("_transform.rhai");
    std::fs::write(&transform_path, &script).map_err(|e| format!("failed to write transform: {e}"))
}

#[tauri::command]
async fn save_transform_and_rebuild_cmd(
    app: tauri::AppHandle,
    now_yyyymm: String,
    source_path: String,
    account_folder: String,
    script: String,
    _account_set: String,
) -> Result<MutationResponse, String> {
    let mut config = make_pipeline_config(&app, &now_yyyymm)?;
    config.changed_folder_hint = Some(vec![account_folder.clone()]);
    app.state::<LastCommandRebuild>().mark();
    let result = save_transform_and_rebuild(
        &config,
        std::path::Path::new(&source_path),
        &account_folder,
        &script,
    )?;
    spawn_report_generation(&app, &config, &result);
    Ok(build_mutation_response(&app, &result))
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn save_rule(
    app: tauri::AppHandle,
    now_yyyymm: String,
    account_folder: String,
    pattern: String,
    payee: Option<String>,
    commodity: Option<String>,
    match_field: Option<String>,
    amount_condition: Option<String>,
    fee_condition: Option<String>,
    payee_condition: Option<String>,
    narration_condition: Option<String>,
    commodity_condition: Option<String>,
    meta_condition: Option<String>,
    amount_account: Option<String>,
    fee_account: Option<String>,
    comment: Option<String>,
    _account_set: String,
) -> Result<MutationResponse, String> {
    if pattern.trim().is_empty() {
        return Err("Cannot save a rule with an empty pattern".into());
    }
    if pattern.trim() == "*" && match_field.is_none() {
        return Err(
            "Catch-all rules are not allowed — the fallback account is applied automatically"
                .into(),
        );
    }
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let folder = config.sources_dir.join(&account_folder);

    let mut rules = RulesFile::load(&folder);

    // Dedup: only match if all condition fields are identical
    let existing = rules.rules.iter_mut().find(|r| {
        r.pattern == pattern
            && r.match_field == match_field
            && r.amount_condition == amount_condition
            && r.fee_condition == fee_condition
            && r.payee_condition == payee_condition
            && r.narration_condition == narration_condition
            && r.commodity_condition == commodity_condition
            && r.meta_condition == meta_condition
            && r.amount_account == amount_account
            && r.fee_account == fee_account
    });
    if let Some(existing) = existing {
        existing.payee = payee;
        existing.commodity = commodity;
        existing.comment = comment;
    } else {
        let rule = Rule {
            id: generate_rule_id(&pattern),
            pattern,
            match_field,
            payee,
            commodity,
            comment,
            amount_condition,
            fee_condition,
            payee_condition,
            narration_condition,
            commodity_condition,
            meta_condition,
            amount_account,
            fee_account,
            postings: vec![],
        };
        rules.insert_rule(rule);
    }
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    rules.save(&folder)?;

    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    Ok(MutationResponse {
        ok: true,
        warnings: result.warnings.clone(),
        output_files_written: result.output_files_written,
    })
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn save_label(
    app: tauri::AppHandle,
    now_yyyymm: String,
    account_folder: String,
    pattern: String,
    payee: Option<String>,
    commodity: Option<String>,
    match_field: Option<String>,
    _account_set: String,
) -> Result<MutationResponse, String> {
    if pattern.trim().is_empty() {
        return Err("Cannot save a label with an empty pattern".into());
    }
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let folder = config.sources_dir.join(&account_folder);

    let mut labels = LabelsFile::load(&folder);

    // Dedup: match on pattern + match_field
    let existing = labels
        .labels
        .iter_mut()
        .find(|r| r.pattern == pattern && r.match_field == match_field);
    if let Some(existing) = existing {
        existing.payee = payee;
        existing.commodity = commodity;
    } else {
        let label = Rule {
            id: generate_rule_id(&pattern),
            pattern,
            match_field,
            payee,
            commodity,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            amount_account: None,
            fee_account: None,
            postings: vec![],
        };
        labels.labels.push(label);
    }
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    labels.save(&folder)?;

    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    Ok(MutationResponse {
        ok: true,
        warnings: result.warnings.clone(),
        output_files_written: result.output_files_written,
    })
}

#[derive(Debug, Serialize, serde::Deserialize)]
struct AiRuleSuggestion {
    pattern: String,
    amount_account: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    payee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    match_field: Option<String>,
    #[serde(default)]
    explanation: String,
}

#[derive(Debug, Serialize)]
struct AiSuggestResponse {
    suggestions: Vec<AiRuleSuggestion>,
    raw_output: String,
    success: bool,
    error: Option<String>,
}

/// Default model + effort for the app's interactive AI suggestions
/// (transform-script generation + categorisation). Opus at max effort: these
/// generate Rhai transforms / rules that need to be correct, so quality beats
/// latency. Centralised here so both call sites stay in sync.
const AI_CLAUDE_MODEL: &str = "opus";
const AI_CLAUDE_EFFORT: &str = "max";

#[tauri::command]
fn ai_suggest_categorisation(
    app: tauri::AppHandle,
    account_name: String,
    account_folder: String,
    account_set: String,
    uncategorised_json: String,
) -> Result<(), String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let root_dir = sources_dir
        .parent()
        .ok_or("sources dir has no parent")?
        .to_path_buf();
    let folder_path = sources_dir.join(&account_folder);

    // --- Build context: rules, matching transactions, nearby transactions ---

    let _ = app.emit("ai-suggest-step", "Loading rules\u{2026}");

    // Load existing rules
    let rules_file = RulesFile::load(&folder_path);
    let rules_json =
        serde_json::to_string_pretty(&rules_file.rules).unwrap_or_else(|_| "[]".to_string());
    let rule_count = rules_file.rules.len();
    let _ = app.emit(
        "ai-suggest-step",
        format!("Loaded {} existing rules", rule_count),
    );

    // Parse the target transaction fields for matching
    let target: serde_json::Value = serde_json::from_str(&uncategorised_json).unwrap_or_default();
    let target_payee = target["payee"].as_str().unwrap_or("").to_lowercase();
    let target_narration = target["narration"].as_str().unwrap_or("").to_lowercase();
    let target_datetime = target["datetime"].as_str().unwrap_or("");

    // Load the active ledger to find context transactions
    let _ = app.emit(
        "ai-suggest-step",
        "Searching for context transactions\u{2026}",
    );
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let mut matching_txns = Vec::new();
    let mut nearby_txns = Vec::new();
    if let Ok(parse) = load_active_ledger(&set_dir) {
        // Parse target datetime for 1-minute window comparison
        let target_ts =
            chrono::NaiveDateTime::parse_from_str(target_datetime, "%Y-%m-%d %H:%M:%S").ok();

        for txn in &parse.transactions {
            // Skip the target transaction itself
            if txn.datetime == target_datetime
                && txn.narration.as_deref().map(|n| n.to_lowercase())
                    == Some(target_narration.clone())
                && (txn.display_payee.as_deref().or(txn.payee.as_deref())).map(|p| p.to_lowercase())
                    == Some(target_payee.clone())
            {
                continue;
            }

            let txn_payee = txn
                .display_payee
                .as_deref()
                .or(txn.payee.as_deref())
                .unwrap_or("")
                .to_lowercase();
            let txn_narration = txn.narration.as_deref().unwrap_or("").to_lowercase();

            // Check for matching payee or narration
            let payee_match = !target_payee.is_empty() && txn_payee == target_payee;
            let narration_match = !target_narration.is_empty() && txn_narration == target_narration;
            if payee_match || narration_match {
                matching_txns.push(txn.clone());
            }

            // Check for nearby transactions (within 1 minute)
            if let Some(ref t_ts) = target_ts {
                if let Ok(txn_ts) =
                    chrono::NaiveDateTime::parse_from_str(&txn.datetime, "%Y-%m-%d %H:%M:%S")
                {
                    let diff = (txn_ts - *t_ts).num_seconds().unsigned_abs();
                    if diff <= 60 && diff > 0 {
                        // Avoid duplicates if already in matching
                        if !(payee_match || narration_match) {
                            nearby_txns.push(txn.clone());
                        }
                    }
                }
            }
        }
    }

    // Cap context to avoid overly long prompts
    matching_txns.truncate(20);
    nearby_txns.truncate(10);
    let _ = app.emit(
        "ai-suggest-step",
        format!(
            "Found {} matching, {} nearby transactions",
            matching_txns.len(),
            nearby_txns.len()
        ),
    );

    // Format context sections
    let fmt_txn = |t: &Transaction| -> String {
        let payee = t
            .display_payee
            .as_deref()
            .or(t.payee.as_deref())
            .unwrap_or("—");
        let narration = t.narration.as_deref().unwrap_or("—");
        let category = t
            .postings
            .iter()
            .find(|p| !p.account.starts_with("assets:") && !p.account.starts_with("liabilities:"))
            .map(|p| p.account.as_str())
            .unwrap_or("—");
        format!(
            "  {} | {} | {} | {} {} → {}",
            t.date, payee, narration, t.amount, t.amount_commodity, category
        )
    };

    let matching_section = if matching_txns.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = matching_txns.iter().map(fmt_txn).collect();
        format!("\n\nTransactions with the same payee or narration (showing how they were categorised):\n{}", lines.join("\n"))
    };

    let nearby_section = if nearby_txns.is_empty() {
        String::new()
    } else {
        let lines: Vec<String> = nearby_txns.iter().map(fmt_txn).collect();
        format!("\n\nOther transactions within 1 minute of this one (may be part of the same action):\n{}", lines.join("\n"))
    };

    let prompt = format!(
        r#"You are analyzing a single financial transaction for the account "{account_name}" in a personal finance app called Arimalo.

This transaction is currently uncategorised (expenses:unknown):

{uncategorised_json}

Existing rules for this account:
{rules_json}{matching_section}{nearby_section}

The app uses a rules system where wildcard patterns match transaction narration/payee text and assign categories (accounts like "expenses:groceries", "expenses:subscriptions", "income:salary", "assets:transfer", etc.).

If this is a crypto wallet, particularly look at transactions within a minute of each other on the same account as they are often a group of transactions for a single action.

Please:
1. Use the existing rules and similar transactions above for context
2. Identify what this transaction likely represents based on the payee/narration
3. Suggest 1-2 categorisation rules. Return ONLY a JSON array, no other text. Each rule object must have:
   - "pattern": wildcard pattern to match (e.g. "*UBER*", "*Netflix*")
   - "amount_account": the category account (e.g. "expenses:transport", "expenses:subscriptions")
   - "payee": optional human-readable payee name (e.g. "Uber", "Netflix")
   - "explanation": brief reason for the categorization

Return the JSON array directly with no markdown formatting or code fences."#
    );

    // Resolve claude binary — macOS GUI apps don't inherit shell PATH
    let claude_bin = ["/opt/homebrew/bin/claude", "/usr/local/bin/claude"]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "claude".to_string());

    let _ = app.emit("ai-suggest-step", format!("Prompt:\n{prompt}"));

    // Spawn on background thread so the invoke returns immediately
    let app_handle = app.clone();
    std::thread::spawn(move || {
        let app = app_handle;
        let _ = app.emit("ai-suggest-step", "Calling Claude\u{2026}");
        let result = match std::process::Command::new(&claude_bin)
            .arg("-p")
            .arg("--dangerously-skip-permissions")
            .arg("--model")
            .arg(AI_CLAUDE_MODEL)
            .arg("--effort")
            .arg(AI_CLAUDE_EFFORT)
            .arg(&prompt)
            .current_dir(&root_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
        {
            Err(e) => AiSuggestResponse {
                suggestions: vec![],
                raw_output: String::new(),
                success: false,
                error: Some(format!(
                    "Failed to run claude CLI ({claude_bin}): {e}. Is claude installed?"
                )),
            },
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();

                if !output.status.success() {
                    AiSuggestResponse {
                        suggestions: vec![],
                        raw_output: if stderr.is_empty() { stdout } else { stderr },
                        success: false,
                        error: Some("Claude CLI returned an error".to_string()),
                    }
                } else {
                    let _ = app.emit("ai-suggest-step", "Parsing response\u{2026}");
                    let json_str = extract_json_array(&stdout).unwrap_or(&stdout);
                    match serde_json::from_str::<Vec<AiRuleSuggestion>>(json_str) {
                        Ok(suggestions) => AiSuggestResponse {
                            suggestions,
                            raw_output: stdout,
                            success: true,
                            error: None,
                        },
                        Err(e) => AiSuggestResponse {
                            suggestions: vec![],
                            raw_output: stdout,
                            success: true,
                            error: Some(format!("Could not parse suggestions: {e}")),
                        },
                    }
                }
            }
        };
        eprintln!(
            "[ai-suggest] Claude finished. success={}, suggestions={}, error={:?}, raw_len={}",
            result.success,
            result.suggestions.len(),
            result.error,
            result.raw_output.len()
        );
        let json = serde_json::to_string(&result).unwrap_or_else(|e| {
            eprintln!("[ai-suggest] Failed to serialize result: {e}");
            String::from("{}")
        });
        eprintln!("[ai-suggest] Emitting event, json_len={}", json.len());
        if let Err(e) = app.emit("ai-suggest-result", &result) {
            eprintln!("[ai-suggest] Failed to emit event: {e}");
        } else {
            eprintln!("[ai-suggest] Event emitted successfully");
        }
    });

    Ok(())
}

/// Extract a JSON array from text that may contain markdown code fences.
fn extract_json_array(text: &str) -> Option<&str> {
    // Try to find ```json ... ``` or ``` ... ```
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim());
        }
    }
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        // Skip optional language tag on same line
        let after = if let Some(nl) = after.find('\n') {
            &after[nl + 1..]
        } else {
            after
        };
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim());
        }
    }
    // Try to find bare [ ... ] anywhere in the text
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            if end > start {
                return Some(text[start..=end].trim());
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// AI-assisted transform generation
// ---------------------------------------------------------------------------

fn build_ai_transform_prompt(
    currency: &str,
    branching_hint: &str,
    csv_sections: &str,
    examples: &str,
    compile_error: Option<&str>,
) -> String {
    let retry_section = match compile_error {
        Some(err) => format!(
            "\n\nIMPORTANT: Your previous script failed to compile with this error:\n  {err}\n\
             Fix the error and return a corrected script.\n"
        ),
        None => String::new(),
    };

    let examples_section = if examples.is_empty() {
        String::new()
    } else {
        format!(
            "\n\nHere are working examples of Rhai transform scripts from this project. \
                 Study these carefully — they show correct Rhai syntax and patterns:\n{examples}\n"
        )
    };

    format!(
        r#"You are generating a Rhai transform script for a personal finance app called Arimalo.

The script maps CSV columns to ledger transaction fields. It receives each CSV row as `row["ColumnName"]` and must return a Rhai map literal (#{{ ... }}).

CRITICAL Rhai language rules — trim() and replace() MUTATE in place and return ():
- NEVER chain trim(): `row["x"].trim().split(" ")` FAILS because trim() returns ().
  Correct: `let s = row["x"]; s.trim(); let parts = s.split(" ");`
- NEVER chain replace(): same issue. `s.replace(",","")` mutates s, returns ().
- split(delimiter) returns an array. Index with `parts[0]`, `parts[1]`.
- sub_string(start, len) extracts a substring (does NOT mutate).
- String concatenation: `"a" + "b"`.
- `if` is an expression: `let x = if cond {{ a }} else {{ b }};`
- Map literal: `#{{ key: value, key2: value2 }}`
- `contains(substring)` checks if a string contains another.
- Define a clean() helper to strip currency symbols:
  `fn clean(s) {{ s.replace("$", ""); s.replace("+", ""); s.replace(",", ""); s }}`
  Note: each replace() mutates s in place. The final bare `s` returns the cleaned string.
- For non-ISO dates, define a parse helper in the script. Do NOT use external libraries.
- SELF-CONTAINED: define every helper you call (write `fn clean(s) {{ ... }}` before using clean()). NEVER reference a variable or call a function you have not defined — a bare word like `narr` or `contra` compiles but crashes at runtime with "Variable not found".

Required output fields:
- date: ISO date string (YYYY-MM-DD)
- payee: who the transaction is with (or "" if not available)
- narration: transaction description
- amount: numeric amount (use clean() to strip $, +, commas)
- commodity: "{currency}"
- status: "*"

Optional fields (include a line ONLY if you assign a concrete string literal or a variable you defined — otherwise omit it entirely; never write `contra: contra`):
- txn_id: "" (leave empty for auto-generation)
- contra: contra account (e.g. "expenses:unknown", "income:sales"). Default is expenses:unknown.
- fee: fee amount if the CSV has a fee column
- datetime: full ISO datetime if available (e.g. "2024-01-15 10:30:00")

Available injected variables:
- row["_source_path"]: the filename of the current CSV (for branching on CSV type)

The account is auto-derived from the folder path — do NOT include an account field.
{examples_section}{branching_hint}{retry_section}
CSV data to analyze:
{csv_sections}

Return the script as a single ```rhai code block — any helper functions (clean, date parsers, `let` bindings) FIRST, then the #{{ ... }} map that uses them. Put nothing outside the code block."#
    )
}

/// Collect up to `max` existing `_transform.rhai` files from `sources_dir`,
/// formatted as numbered examples for the AI prompt.
fn collect_transform_examples(sources_dir: &std::path::Path, max: usize) -> String {
    let mut examples = Vec::new();
    let mut walker = vec![sources_dir.to_path_buf()];
    while let Some(dir) = walker.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                walker.push(path);
            } else if path
                .file_name()
                .map(|n| n == "_transform.rhai")
                .unwrap_or(false)
            {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    // Skip very large scripts to avoid prompt bloat
                    if content.lines().count() <= 50 {
                        let rel = path
                            .strip_prefix(sources_dir)
                            .map(|p| p.to_string_lossy().to_string())
                            .unwrap_or_default();
                        examples.push((rel, content));
                    }
                }
            }
        }
        if examples.len() >= max * 2 {
            break; // enough candidates
        }
    }
    // Take up to max, preferring shorter ones first (simpler examples)
    examples.sort_by_key(|(_, c)| c.len());
    examples.truncate(max);
    examples
        .iter()
        .enumerate()
        .map(|(i, (path, content))| format!("\n--- Example {} ({}) ---\n{}", i + 1, path, content))
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Debug, Serialize, Clone)]
struct CsvTypeInfo {
    pattern: String,
    headers: Vec<String>,
    file_count: usize,
    sample_rows: Vec<Vec<String>>,
}

#[derive(Debug, Serialize, Clone)]
struct AiTransformResponse {
    script: String,
    raw_output: String,
    success: bool,
    error: Option<String>,
    csv_types: Vec<CsvTypeInfo>,
}

#[tauri::command]
fn ai_suggest_transform(
    app: tauri::AppHandle,
    account_folder: String,
    extra_csv_path: Option<String>,
    currency: Option<String>,
) -> Result<(), String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let root_dir = sources_dir
        .parent()
        .ok_or("sources dir has no parent")?
        .to_path_buf();
    let folder_path = sources_dir.join(&account_folder);
    let currency = currency.unwrap_or_else(|| "USD".to_string());

    let _ = app.emit("ai-transform-step", "Scanning CSV files…");

    // Collect all CSVs in the account folder + the extra one being imported
    let mut csv_paths: Vec<std::path::PathBuf> = Vec::new();
    if folder_path.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&folder_path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().map(|e| e == "csv").unwrap_or(false)
                    && !p
                        .file_name()
                        .map(|n| n.to_string_lossy().starts_with('_'))
                        .unwrap_or(false)
                {
                    csv_paths.push(p);
                }
            }
        }
    }
    if let Some(ref extra) = extra_csv_path {
        let extra_path = std::path::PathBuf::from(extra);
        if !csv_paths.iter().any(|p| p == &extra_path) {
            csv_paths.push(extra_path);
        }
    }

    if csv_paths.is_empty() {
        return Err("No CSV files found to analyze".to_string());
    }

    // Group CSVs by their header signature
    #[allow(clippy::type_complexity)]
    let mut type_map: std::collections::HashMap<Vec<String>, Vec<(String, Vec<Vec<String>>)>> =
        std::collections::HashMap::new();
    for csv_path in &csv_paths {
        match arimalo_covid::transform_suggest::read_csv_sample(csv_path, 3) {
            Ok((headers, rows)) => {
                let filename = csv_path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                type_map.entry(headers).or_default().push((filename, rows));
            }
            Err(e) => {
                let _ = app.emit(
                    "ai-transform-step",
                    format!("Skipping {}: {e}", csv_path.display()),
                );
            }
        }
    }

    let csv_types: Vec<CsvTypeInfo> = type_map
        .into_iter()
        .map(|(headers, files)| {
            let filenames: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
            // Find common pattern in filenames for the branch condition
            let pattern = if filenames.len() == 1 {
                filenames[0].to_string()
            } else {
                // Find longest common substring or use first filename as hint
                filenames[0].to_string()
            };
            let sample_rows = files.first().map(|(_, r)| r.clone()).unwrap_or_default();
            CsvTypeInfo {
                pattern,
                headers,
                file_count: files.len(),
                sample_rows,
            }
        })
        .collect();

    let _ = app.emit(
        "ai-transform-step",
        format!(
            "Found {} CSV type{}: {}",
            csv_types.len(),
            if csv_types.len() == 1 { "" } else { "s" },
            csv_types
                .iter()
                .map(|t| format!(
                    "{} ({} file{})",
                    t.pattern,
                    t.file_count,
                    if t.file_count == 1 { "" } else { "s" }
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    );

    // Build the prompt
    let mut csv_sections = String::new();
    for (i, ct) in csv_types.iter().enumerate() {
        csv_sections.push_str(&format!(
            "\n--- CSV Type {} (e.g. \"{}\", {} file{}) ---\nHeaders: {}\nSample rows:\n",
            i + 1,
            ct.pattern,
            ct.file_count,
            if ct.file_count == 1 { "" } else { "s" },
            ct.headers.join(", "),
        ));
        for row in &ct.sample_rows {
            csv_sections.push_str(&format!("  {}\n", row.join(", ")));
        }
    }

    let branching_hint = if csv_types.len() > 1 {
        format!(
            "\n\nIMPORTANT: There are {} different CSV formats in this account folder. \
             Generate a SINGLE script that branches on `row[\"_source_path\"]` using `.contains(\"keyword\")` \
             to detect which CSV type is being processed. Each branch should return the appropriate field mapping.\n",
            csv_types.len()
        )
    } else {
        String::new()
    };

    let _ = app.emit("ai-transform-step", "Collecting working examples…");
    let examples = collect_transform_examples(&sources_dir, 4);
    let example_count = if examples.is_empty() {
        0
    } else {
        examples.matches("--- Example").count()
    };
    if example_count > 0 {
        let _ = app.emit(
            "ai-transform-step",
            format!(
                "Found {example_count} existing transform{} as reference",
                if example_count == 1 { "" } else { "s" }
            ),
        );
    }

    // Resolve claude binary
    let claude_bin = ["/opt/homebrew/bin/claude", "/usr/local/bin/claude"]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "claude".to_string());

    let _ = app.emit("ai-transform-step", "Building prompt…");

    // Spawn background thread
    let app_handle = app.clone();
    let csv_types_clone = csv_types.clone();
    std::thread::spawn(move || {
        let app = app_handle;

        // Helper: call Claude CLI with a prompt and return (stdout, error)
        let call_claude = |prompt: &str, step_msg: &str| -> Result<String, AiTransformResponse> {
            let _ = app.emit("ai-transform-step", step_msg.to_string());
            match std::process::Command::new(&claude_bin)
                .arg("-p")
                .arg("--dangerously-skip-permissions")
                .arg("--model")
                .arg(AI_CLAUDE_MODEL)
                .arg("--effort")
                .arg(AI_CLAUDE_EFFORT)
                .arg(prompt)
                .current_dir(&root_dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
            {
                Err(e) => Err(AiTransformResponse {
                    script: String::new(),
                    raw_output: String::new(),
                    success: false,
                    error: Some(format!(
                        "Failed to run claude CLI ({claude_bin}): {e}. Is claude installed?"
                    )),
                    csv_types: csv_types_clone.clone(),
                }),
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    if !output.status.success() {
                        Err(AiTransformResponse {
                            script: String::new(),
                            raw_output: if stderr.is_empty() { stdout } else { stderr },
                            success: false,
                            error: Some("Claude CLI returned an error".to_string()),
                            csv_types: csv_types_clone.clone(),
                        })
                    } else {
                        Ok(stdout)
                    }
                }
            }
        };

        // Validate a generated script: it must compile AND run against each CSV
        // type's first sample row. Rhai resolves variables/functions at runtime,
        // so a compile-only check lets undefined-variable / undefined-function
        // bugs (e.g. `narration: narr`, an undefined `clean()`) reach the
        // pipeline. Checking every type also exercises each branch of a
        // multi-format script.
        let samples: Vec<(Vec<String>, Vec<String>, String)> = csv_types_clone
            .iter()
            .filter_map(|ct| {
                ct.sample_rows
                    .first()
                    .map(|r| (ct.headers.clone(), r.clone(), ct.pattern.clone()))
            })
            .collect();
        let validate = |script: &str| -> Result<(), String> {
            arimalo_covid::transform_suggest::rhai_compile_check(script)?;
            for (headers, row, src) in &samples {
                arimalo_covid::transform_suggest::rhai_run_check(script, headers, row, src)?;
            }
            Ok(())
        };

        // Generate → validate → retry. The model fixes its mistakes when handed
        // the exact error (it dropped `narration: narr` once told), so feed each
        // failure back and retry a few times before giving up. A CLI failure
        // (process error, not a script error) aborts immediately.
        const MAX_ATTEMPTS: usize = 4;
        let mut last_err = String::new();
        let mut last_script = String::new();
        let mut last_raw = String::new();
        let mut result: Option<AiTransformResponse> = None;

        for attempt in 0..MAX_ATTEMPTS {
            let prompt = build_ai_transform_prompt(
                &currency,
                &branching_hint,
                &csv_sections,
                &examples,
                if attempt == 0 { None } else { Some(last_err.as_str()) },
            );
            let step = if attempt == 0 {
                "Calling Claude…".to_string()
            } else {
                format!("Retry {}/{} — fixing: {last_err}", attempt, MAX_ATTEMPTS - 1)
            };

            let stdout = match call_claude(&prompt, &step) {
                Ok(s) => s,
                Err(err_resp) => {
                    result = Some(err_resp);
                    break;
                }
            };

            let _ = app.emit("ai-transform-step", "Parsing response…");
            let script = arimalo_covid::transform_suggest::extract_rhai_block(&stdout);
            match validate(&script) {
                Ok(()) => {
                    let _ = app.emit("ai-transform-step", "Script compiles & runs ✓");
                    result = Some(AiTransformResponse {
                        script,
                        raw_output: stdout,
                        success: true,
                        error: None,
                        csv_types: csv_types_clone.clone(),
                    });
                    break;
                }
                Err(err) => {
                    let _ = app.emit(
                        "ai-transform-step",
                        format!("Attempt {} failed: {err}", attempt + 1),
                    );
                    last_err = err;
                    last_script = script;
                    last_raw = stdout;
                }
            }
        }

        // No clean script after all attempts — return the last one (with the
        // error) so the user can fix it manually.
        let result = result.unwrap_or_else(|| AiTransformResponse {
            script: last_script,
            raw_output: last_raw,
            success: true,
            error: Some(format!(
                "Script still has errors after {MAX_ATTEMPTS} attempts: {last_err}"
            )),
            csv_types: csv_types_clone,
        });

        eprintln!(
            "[ai-transform] Claude finished. success={}, error={:?}, script_len={}",
            result.success,
            result.error,
            result.script.len()
        );
        if let Err(e) = app.emit("ai-transform-result", &result) {
            eprintln!("[ai-transform] Failed to emit event: {e}");
        }
    });

    Ok(())
}

/// Walk up from `start` to `sources_dir` looking for a `_rules.json` that
/// contains a rule with the given `rule_id`.  Returns the directory where
/// the rule lives, or `None` if not found anywhere.
fn find_rule_folder(
    start: &std::path::Path,
    sources_dir: &std::path::Path,
    rule_id: &str,
) -> Option<std::path::PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        let rules = RulesFile::load(&current);
        if rules.rules.iter().any(|r| r.id == rule_id) {
            return Some(current);
        }
        if current == sources_dir {
            break;
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }
    None
}

#[tauri::command]
fn get_rules(app: tauri::AppHandle, account_folder: String) -> Result<Vec<Rule>, String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let folder = sources_dir.join(&account_folder);
    // Collect rules from this folder and all parents up to sources_dir
    let mut all_rules = Vec::new();
    let mut current = folder.as_path().to_path_buf();
    loop {
        let rules = RulesFile::load(&current);
        all_rules.extend(rules.rules);
        if current == sources_dir {
            break;
        }
        match current.parent() {
            Some(p) => current = p.to_path_buf(),
            None => break,
        }
    }
    Ok(all_rules)
}

/// Update an existing rule in place, or move it to a different folder when
/// `target_folder_rel` is `Some` and differs from the folder the rule currently
/// lives in. `target_folder_rel` is relative to `sources_dir` ("" = root).
fn apply_rule_update(
    sources_dir: &std::path::Path,
    start_folder: &std::path::Path,
    target_folder_rel: Option<&str>,
    rule: Rule,
) -> Result<(), String> {
    let current_folder = find_rule_folder(start_folder, sources_dir, &rule.id)
        .ok_or_else(|| format!("Rule '{}' not found", rule.id))?;
    let target_folder = target_folder_rel.map(|rel| sources_dir.join(rel));

    match target_folder {
        Some(ref target) if target != &current_folder => {
            let mut src = RulesFile::load(&current_folder);
            src.rules.retain(|r| r.id != rule.id);
            src.save(&current_folder)?;

            let mut dst = RulesFile::load(target);
            if dst.rules.iter().any(|r| r.id == rule.id) {
                return Err(format!(
                    "Rule '{}' already exists in target folder",
                    rule.id
                ));
            }
            dst.insert_rule(rule);
            dst.save(target)?;
        }
        _ => {
            let mut rules = RulesFile::load(&current_folder);
            if let Some(existing) = rules.rules.iter_mut().find(|r| r.id == rule.id) {
                *existing = rule;
            }
            rules.save(&current_folder)?;
        }
    }
    Ok(())
}

#[tauri::command]
async fn update_rule(
    app: tauri::AppHandle,
    now_yyyymm: String,
    account_folder: String,
    rule: Rule,
    target_folder: Option<String>,
    _account_set: String,
) -> Result<MutationResponse, String> {
    if rule.pattern.trim().is_empty() && !rule.is_transform() {
        return Err("Cannot save a rule with an empty pattern".into());
    }
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let start_folder = config.sources_dir.join(&account_folder);

    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    apply_rule_update(
        &config.sources_dir,
        &start_folder,
        target_folder.as_deref(),
        rule,
    )?;

    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    Ok(MutationResponse {
        ok: true,
        warnings: result.warnings.clone(),
        output_files_written: result.output_files_written,
    })
}

#[tauri::command]
async fn delete_rule(
    app: tauri::AppHandle,
    now_yyyymm: String,
    account_folder: String,
    rule_id: String,
    _account_set: String,
) -> Result<MutationResponse, String> {
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let start_folder = config.sources_dir.join(&account_folder);

    let rule_folder =
        find_rule_folder(&start_folder, &config.sources_dir, &rule_id).unwrap_or(start_folder);

    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    let mut rules = RulesFile::load(&rule_folder);
    rules.rules.retain(|r| r.id != rule_id);
    rules.save(&rule_folder)?;

    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    Ok(MutationResponse {
        ok: true,
        warnings: result.warnings.clone(),
        output_files_written: result.output_files_written,
    })
}

#[tauri::command]
async fn import_rules_csv(
    app: tauri::AppHandle,
    now_yyyymm: String,
    source_path: String,
    account_folder: String,
    account_set: String,
) -> Result<PipelineResponse, String> {
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let folder = config.sources_dir.join(&account_folder);

    // Read CSV (pattern, payee, posting, comment columns)
    let mut reader = csv::Reader::from_path(&source_path)
        .map_err(|e| format!("failed to open rules CSV: {e}"))?;
    let mut new_rules: Vec<Rule> = Vec::new();
    for result in reader.records() {
        let record = result.map_err(|e| format!("failed to read CSV record: {e}"))?;
        let pattern = record.get(0).unwrap_or("").to_string();
        let payee_str = record.get(1).unwrap_or("").trim().to_string();
        let posting_str = record.get(2).unwrap_or("").trim().to_string();
        let comment_str = record.get(3).unwrap_or("").trim().to_string();
        if pattern.is_empty() {
            continue;
        }
        new_rules.push(Rule {
            id: generate_rule_id(&pattern),
            pattern,
            match_field: None,
            payee: if payee_str.is_empty() {
                None
            } else {
                Some(payee_str)
            },
            commodity: None,
            comment: if comment_str.is_empty() {
                None
            } else {
                Some(comment_str)
            },
            amount_condition: None,
            fee_condition: None,
            amount_account: if posting_str.is_empty() {
                None
            } else {
                Some(posting_str)
            },
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        });
    }

    // Append to existing rules; txn-anchored entries promote to top.
    let mut rules_file = RulesFile::load(&folder);
    rules_file.insert_rules(new_rules);
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    rules_file.save(&folder)?;

    // Rebuild pipeline once
    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[derive(Debug, Serialize)]
struct PriceImportResponse {
    commodities: Vec<String>,
    total_count: usize,
}

#[tauri::command]
async fn import_prices(
    app: tauri::AppHandle,
    source_path: String,
    merge: bool,
) -> Result<PriceImportResponse, String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let file = std::path::Path::new(&source_path);
    let result = import_prices_file(&sources_dir, file, merge)?;
    Ok(PriceImportResponse {
        commodities: result.commodities,
        total_count: result.total_count,
    })
}

#[tauri::command]
async fn set_price(
    app: tauri::AppHandle,
    commodity: String,
    datetime: String,
    price_amount: String,
    quote_currency: String,
) -> Result<(), String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    set_price_directive(&sources_dir, &commodity, &datetime, &price_amount, &quote_currency)
}

#[tauri::command]
fn parse_transactions_file(path: String) -> Result<ParseResponse, String> {
    let contents =
        std::fs::read_to_string(&path).map_err(|e| format!("failed to read file: {e}"))?;
    let result: ParseResult = parse_transactions(&contents);
    Ok(result.into())
}

struct StartupWarnings(Mutex<Vec<String>>);

struct RootConfigState(Mutex<RootConfig>);

struct ReportGeneration(AtomicU64);

struct ShowHiddenState(AtomicBool);

/// Cache parsed ledger to avoid re-reading from disk on every query.
struct LedgerCache {
    entries: Mutex<std::collections::HashMap<String, ParseResult>>,
}

impl LedgerCache {
    fn new() -> Self {
        Self {
            entries: Mutex::new(std::collections::HashMap::new()),
        }
    }
    fn get_or_load(
        &self,
        app: &tauri::AppHandle,
        account_set: &str,
    ) -> Result<ParseResult, String> {
        let mut cache = self.entries.lock();
        if let Some(cached) = cache.get(account_set) {
            let mut result = cached.clone();
            let generated_dir = resolve_generated_dir(app)?;
            let set_dir = resolve_set_dir(&generated_dir, account_set);
            maybe_filter_hidden(app, &set_dir, &mut result);
            return Ok(result);
        }
        let generated_dir = resolve_generated_dir(app)?;
        let set_dir = resolve_set_dir(&generated_dir, account_set);
        let result = load_active_ledger(&set_dir)?;
        cache.insert(account_set.to_string(), result.clone());
        let mut filtered = result;
        maybe_filter_hidden(app, &set_dir, &mut filtered);
        Ok(filtered)
    }
    fn invalidate(&self) {
        self.entries.lock().clear();
    }
    /// Pre-populate cache with in-memory pipeline data to avoid disk re-reads.
    fn populate(&self, account_set: &str, result: ParseResult) {
        self.entries.lock().insert(account_set.to_string(), result);
    }
}

/// Apply hidden-account filtering if the toggle is off.
fn maybe_filter_hidden(app: &tauri::AppHandle, set_dir: &std::path::Path, parse: &mut ParseResult) {
    let show = app.state::<ShowHiddenState>().0.load(Ordering::Relaxed);
    if show {
        return;
    }
    let prefixes = load_hidden_accounts(set_dir);
    if !prefixes.is_empty() {
        filter_hidden_accounts(parse, &prefixes);
    }
}

fn spawn_report_generation(
    app: &tauri::AppHandle,
    config: &PipelineConfig,
    result: &PipelineResult,
) {
    if result.output_files_written == 0 {
        return;
    }
    let gen_state = app.state::<ReportGeneration>();
    let generation = gen_state.0.fetch_add(1, Ordering::SeqCst) + 1;

    let sources_dir = config.sources_dir.clone();
    let generated_dir = config.generated_dir.clone();
    let mut account_sets: Vec<String> = result.owner_accounts.keys().cloned().collect();
    account_sets.sort();
    let handle = app.clone();
    let extra_prefixes = app
        .state::<RootConfigState>()
        .0
        .lock()
        .extra_primary_account_prefixes
        .clone();

    std::thread::spawn(move || {
        if let Err(e) = arimalo_covid::report_templates::generate_all_reports(
            &sources_dir,
            &generated_dir,
            &account_sets,
            arimalo_covid::report_templates::ALL_FORMATS,
            &extra_prefixes,
        ) {
            eprintln!("Report generation error: {e}");
        }
        // Only notify frontend if this is still the latest generation
        let current = handle.state::<ReportGeneration>().0.load(Ordering::SeqCst);
        if current == generation {
            let _ = handle.emit("reports-rebuilt", ());
        }
    });
}

#[tauri::command]
fn get_pipeline_warnings(state: tauri::State<StartupWarnings>) -> Vec<String> {
    state.0.lock().clone()
}

// WatcherStopHandle is defined near start_file_watcher()

struct MetadataState {
    store: Mutex<Option<MetadataStore>>,
}

#[tauri::command]
fn init_metadata(app: tauri::AppHandle, state: tauri::State<MetadataState>) -> Result<(), String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let metadata_path = sources_dir.join("arimalo-metadata.automerge");
    let already_exists = metadata_path.exists();
    let mut store = MetadataStore::new(metadata_path)?;
    if !already_exists {
        store.build_from_sources(&sources_dir)?;
        store.save()?;
    }
    *state.store.lock() = Some(store);
    Ok(())
}

#[tauri::command]
fn get_sync_log(state: tauri::State<MetadataState>) -> Result<Vec<SyncEvent>, String> {
    let lock = state.store.lock();
    let store = lock.as_ref().ok_or("Metadata not initialized")?;
    let meta = store.get_metadata()?;
    Ok(meta.sync_log)
}

#[tauri::command]
fn merge_metadata(remote_path: String, state: tauri::State<MetadataState>) -> Result<(), String> {
    let mut lock = state.store.lock();
    let store = lock.as_mut().ok_or("Metadata not initialized")?;
    store.merge_from_file(std::path::Path::new(&remote_path))?;
    store.save()?;
    Ok(())
}

#[tauri::command]
fn list_devices(state: tauri::State<MetadataState>) -> Result<Vec<DeviceInfo>, String> {
    let lock = state.store.lock();
    let store = lock.as_ref().ok_or("Metadata not initialized")?;
    let meta = store.get_metadata()?;
    Ok(meta.devices.values().cloned().collect())
}

#[derive(Debug, Serialize)]
struct SyncResponse {
    files_transferred: usize,
    metadata_merged: bool,
}

#[tauri::command]
fn sync_with_remote(
    app: tauri::AppHandle,
    remote_metadata_path: String,
    remote_cas_path: String,
    state: tauri::State<MetadataState>,
) -> Result<SyncResponse, String> {
    let mut lock = state.store.lock();
    let store = lock.as_mut().ok_or("Metadata not initialized")?;

    let sources_dir = resolve_sources_dir(&app)?;
    let local_cas = ContentStore::new(sources_dir.join("cas"));
    let remote_cas = ContentStore::new(PathBuf::from(&remote_cas_path));

    let result = full_sync(
        store,
        &local_cas,
        std::path::Path::new(&remote_metadata_path),
        &remote_cas,
    )?;

    store.save()?;

    Ok(SyncResponse {
        files_transferred: result.files_transferred,
        metadata_merged: result.metadata_merged,
    })
}

#[derive(Debug, Serialize)]
struct PairInitiateResult {
    group_id: String,
    pairing_code: String,
    expires_in: u64,
}

#[tauri::command]
fn pair_initiate(relay_url: String) -> Result<PairInitiateResult, String> {
    let result = relay_client::pair_initiate(&relay_url)?;
    Ok(PairInitiateResult {
        group_id: result.group_id,
        pairing_code: result.pairing_code,
        expires_in: result.expires_in,
    })
}

#[tauri::command]
fn pair_join(relay_url: String, pairing_code: String) -> Result<String, String> {
    relay_client::pair_join(&relay_url, &pairing_code)
}

#[derive(Debug, Serialize)]
struct RelaySyncResponse {
    metadata_merged: bool,
    blobs_uploaded: usize,
    blobs_downloaded: usize,
}

#[tauri::command]
fn sync_with_relay_cmd(
    app: tauri::AppHandle,
    state: tauri::State<MetadataState>,
) -> Result<RelaySyncResponse, String> {
    let mut lock = state.store.lock();
    let store = lock.as_mut().ok_or("Metadata not initialized")?;

    let sources_dir = resolve_sources_dir(&app)?;
    let local_cas = ContentStore::new(sources_dir.join("cas"));

    let config = relay_client::load_relay_config(&sources_dir)?
        .ok_or("Relay not configured. Set relay URL and pair first.")?;

    let result = relay_client::sync_with_relay(store, &local_cas, &config)?;

    Ok(RelaySyncResponse {
        metadata_merged: result.metadata_merged,
        blobs_uploaded: result.blobs_uploaded,
        blobs_downloaded: result.blobs_downloaded,
    })
}

#[tauri::command]
fn save_relay_config(
    app: tauri::AppHandle,
    relay_url: String,
    group_id: String,
) -> Result<(), String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let config = relay_client::RelayConfig {
        relay_url,
        group_id,
    };
    relay_client::save_relay_config(&sources_dir, &config)
}

#[tauri::command]
fn get_relay_config(app: tauri::AppHandle) -> Result<Option<relay_client::RelayConfig>, String> {
    let sources_dir = resolve_sources_dir(&app)?;
    relay_client::load_relay_config(&sources_dir)
}

#[tauri::command]
fn get_account_gaps(app: tauri::AppHandle) -> Result<Vec<AccountGap>, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    detect_account_gaps(&generated_dir)
}

#[tauri::command]
fn collect_issues_cmd(
    app: tauri::AppHandle,
    account: Option<String>,
) -> Result<arimalo_covid::issues::CollectedIssues, String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let generated_dir = resolve_generated_dir(&app)?;
    let filter = arimalo_covid::issues::CollectFilter {
        categories: arimalo_covid::issues::Category::ALL.iter().copied().collect(),
        account,
    };
    arimalo_covid::issues::collect_all(&sources_dir, &generated_dir, &filter)
}

#[tauri::command]
fn reveal_in_finder(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    if !p.exists() {
        return Err(format!("Path does not exist: {path}"));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to reveal in Finder: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        let parent = p.parent().unwrap_or(p);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| format!("Failed to open file manager: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path))
            .spawn()
            .map_err(|e| format!("Failed to reveal in Explorer: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
fn get_source_folder_path(app: tauri::AppHandle, folder_name: String) -> Result<String, String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let folder_path = sources_dir.join(&folder_name);
    if folder_path.exists() {
        Ok(folder_path.to_string_lossy().to_string())
    } else {
        Err(format!("Source folder not found: {folder_name}"))
    }
}

#[tauri::command]
fn list_account_sets(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let sources_dir = resolve_sources_dir(&app)?;
    if !sources_dir.exists() {
        return Ok(Vec::new());
    }
    let entries =
        std::fs::read_dir(&sources_dir).map_err(|e| format!("failed to read sources dir: {e}"))?;
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
    Ok(sets.into_iter().collect())
}

#[tauri::command]
async fn update_account_properties(
    app: tauri::AppHandle,
    now_yyyymm: String,
    account_name: String,
    friendly_name: String,
    account_set: String,
) -> Result<PipelineResponse, String> {
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let result = update_account_name(&config, &account_name, &friendly_name, &account_set)?;
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[tauri::command]
fn get_display_config(
    app: tauri::AppHandle,
    account_set: String,
) -> Result<serde_json::Value, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let config_path = set_dir.join("config.json");
    if config_path.exists() {
        let contents = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read config.json: {e}"))?;
        serde_json::from_str(&contents).map_err(|e| format!("failed to parse config.json: {e}"))
    } else {
        let default_config = serde_json::json!({
          "commodities": {
            "AUD": { "decimals": 2 },
            "USD": { "decimals": 2 }
          },
          "default_decimals": 2
        });
        std::fs::create_dir_all(&set_dir)
            .map_err(|e| format!("failed to create config dir: {e}"))?;
        let pretty = arimalo_covid::to_sorted_json_pretty(&default_config)
            .map_err(|e| format!("failed to serialize config: {e}"))?;
        std::fs::write(&config_path, &pretty)
            .map_err(|e| format!("failed to write config.json: {e}"))?;
        Ok(default_config)
    }
}

#[tauri::command]
fn open_display_config(app: tauri::AppHandle, account_set: String) -> Result<(), String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let config_path = set_dir.join("config.json");
    // Ensure file exists with defaults
    if !config_path.exists() {
        let _ = get_display_config(app.clone(), account_set)?;
    }
    let path_str = config_path.to_string_lossy().to_string();
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| format!("Failed to open config: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path_str)
            .spawn()
            .map_err(|e| format!("Failed to open config: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &path_str])
            .spawn()
            .map_err(|e| format!("Failed to open config: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
fn get_account_config(
    app: tauri::AppHandle,
    account_folder: String,
) -> Result<AccountConfig, String> {
    let sources_dir = resolve_sources_dir(&app)?;
    Ok(AccountConfig::resolve(
        std::path::Path::new(&account_folder),
        &sources_dir,
    ))
}

#[tauri::command]
fn set_show_hidden(app: tauri::AppHandle, show: bool) {
    app.state::<ShowHiddenState>()
        .0
        .store(show, Ordering::Relaxed);
}

#[tauri::command]
fn get_show_hidden(app: tauri::AppHandle) -> bool {
    app.state::<ShowHiddenState>().0.load(Ordering::Relaxed)
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    // Only allow https:// URLs
    if !url.starts_with("https://") {
        return Err("Only https:// URLs are allowed".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {e}"))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| format!("Failed to open URL: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .spawn()
            .map_err(|e| format!("Failed to open URL: {e}"))?;
    }
    Ok(())
}

#[tauri::command]
async fn set_opening_balance(
    app: tauri::AppHandle,
    now_yyyymm: String,
    account_name: String,
    amount: String,
    commodity: String,
    account_set: String,
) -> Result<PipelineResponse, String> {
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    let result = update_opening_balance(&config, &account_name, &amount, &commodity, &account_set)?;
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[derive(Debug, serde::Deserialize)]
struct ConversionRequest {
    commodity: String,
    amount: f64,
    datetime: String,
}

#[derive(Debug, Serialize)]
struct ConversionResult {
    values: Vec<Option<f64>>,
}

#[tauri::command]
async fn convert_to_base_currency(
    app: tauri::AppHandle,
    base_currency: String,
    requests: Vec<ConversionRequest>,
) -> Result<ConversionResult, String> {
    let sources_dir = resolve_sources_dir(&app)?;
    let graph = PriceGraph::load(&sources_dir);
    let values: Vec<Option<f64>> = requests
        .iter()
        .map(|r| graph.convert_to_base(&r.commodity, r.amount, &r.datetime, &base_currency))
        .collect();
    Ok(ConversionResult { values })
}

#[allow(clippy::too_many_arguments)]
#[tauri::command]
async fn save_trade_link(
    app: tauri::AppHandle,
    state: tauri::State<'_, MetadataState>,
    now_yyyymm: String,
    txn_id_a: String,
    txn_id_b: String,
    account_folder: String,
    is_a_sell: bool,
    account_set: String,
) -> Result<PipelineResponse, String> {
    let mut lock = state.store.lock();
    let store = lock.as_mut().ok_or("Metadata not initialized")?;
    let id = store.save_trade_link(&txn_id_a, &txn_id_b)?;
    store.save()?;

    // Generate rules in _rules.json. For an on-chain swap (both legs share a
    // txn id) the rule must be leg-anchored and co-located in the leaf folder
    // that owns the legs, or a pre-existing per-leg categorisation would
    // outrank it; plan_trade_link_rules resolves that from the built ledgers.
    let sources_dir = resolve_sources_dir(&app)?;
    let generated_dir = resolve_generated_dir(&app)?;
    let by_folder = load_per_folder_ledgers(&generated_dir, &sources_dir);
    let (sell_txn, buy_txn) = if is_a_sell {
        (txn_id_a.as_str(), txn_id_b.as_str())
    } else {
        (txn_id_b.as_str(), txn_id_a.as_str())
    };
    let (target_folder, link_rules) =
        plan_trade_link_rules(&id, sell_txn, buy_txn, &account_folder, &by_folder);
    let folder = sources_dir.join(&target_folder);
    let mut rules = RulesFile::load(&folder);
    let rule_count_before = rules.rules.len();
    rules.insert_rules(link_rules);
    // Suppress watcher while we modify source files and run pipeline ourselves
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    rules.save(&folder)?;
    eprintln!(
        "[trade-link] wrote {rule_count_before} -> {} rules to {}",
        rules.rules.len(),
        folder.join("_rules.json").display()
    );

    // Rebuild pipeline for just the affected folder
    let mut config = make_pipeline_config(&app, &now_yyyymm)?;
    config.changed_folder_hint = Some(vec![target_folder.clone()]);
    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[derive(Debug, serde::Deserialize)]
struct BulkTradeLinkItem {
    txn_id_a: String,
    txn_id_b: String,
    account_folder: String,
    is_a_sell: bool,
}

#[tauri::command]
async fn save_trade_links_bulk(
    app: tauri::AppHandle,
    state: tauri::State<'_, MetadataState>,
    now_yyyymm: String,
    links: Vec<BulkTradeLinkItem>,
    account_set: String,
) -> Result<PipelineResponse, String> {
    if links.is_empty() {
        return Err("No links to save".to_string());
    }
    let mut lock = state.store.lock();
    let store = lock.as_mut().ok_or("Metadata not initialized")?;

    let sources_dir = resolve_sources_dir(&app)?;
    let generated_dir = resolve_generated_dir(&app)?;
    let by_folder = load_per_folder_ledgers(&generated_dir, &sources_dir);
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();

    // Group links by the resolved target folder so we only load/save each rules
    // file once.
    let mut folder_rules: std::collections::HashMap<String, RulesFile> =
        std::collections::HashMap::new();
    let mut changed_folders: Vec<String> = Vec::new();

    for item in &links {
        let id = store.save_trade_link(&item.txn_id_a, &item.txn_id_b)?;
        let (sell_txn, buy_txn) = if item.is_a_sell {
            (item.txn_id_a.as_str(), item.txn_id_b.as_str())
        } else {
            (item.txn_id_b.as_str(), item.txn_id_a.as_str())
        };
        let (target_folder, link_rules) =
            plan_trade_link_rules(&id, sell_txn, buy_txn, &item.account_folder, &by_folder);
        let folder_path = sources_dir.join(&target_folder);
        let rules = folder_rules.entry(target_folder.clone()).or_insert_with(|| {
            changed_folders.push(target_folder.clone());
            RulesFile::load(&folder_path)
        });
        rules.insert_rules(link_rules);
    }
    store.save()?;

    // Save all modified rules files
    for (folder_name, rules) in &folder_rules {
        let folder_path = sources_dir.join(folder_name);
        rules.save(&folder_path)?;
    }

    eprintln!(
        "[trade-link-bulk] linked {} pairs across {} folder(s)",
        links.len(),
        changed_folders.len()
    );

    // Single pipeline rebuild
    let mut config = make_pipeline_config(&app, &now_yyyymm)?;
    config.changed_folder_hint = Some(changed_folders);
    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[tauri::command]
async fn delete_trade_link(
    app: tauri::AppHandle,
    state: tauri::State<'_, MetadataState>,
    now_yyyymm: String,
    link_id: String,
    account_folder: String,
    account_set: String,
) -> Result<PipelineResponse, String> {
    let mut lock = state.store.lock();
    let store = lock.as_mut().ok_or("Metadata not initialized")?;
    store.delete_trade_link(&link_id)?;
    store.save()?;

    // Suppress watcher while we modify source files and run pipeline ourselves
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();

    // Remove trade link rules from _rules.json
    let sources_dir = resolve_sources_dir(&app)?;
    let folder = sources_dir.join(&account_folder);
    let mut rules = RulesFile::load(&folder);
    remove_trade_link_rules(&mut rules, &link_id);
    rules.save(&folder)?;

    // Rebuild pipeline for just the affected folder
    let mut config = make_pipeline_config(&app, &now_yyyymm)?;
    config.changed_folder_hint = Some(vec![account_folder.clone()]);
    let result = run_pipeline(&config)?;
    spawn_report_generation(&app, &config, &result);
    let set_dir = resolve_set_dir(&config.generated_dir, &account_set);
    build_pipeline_response(&app, result, &set_dir, &account_set)
}

#[tauri::command]
fn get_trade_links(state: tauri::State<MetadataState>) -> Result<Vec<TradeLink>, String> {
    let lock = state.store.lock();
    let store = lock.as_ref().ok_or("Metadata not initialized")?;
    store.get_trade_links()
}

#[tauri::command]
async fn suggest_trade_links_cmd(
    app: tauri::AppHandle,
    state: tauri::State<'_, MetadataState>,
    account_set: String,
    base_currency: Option<String>,
) -> Result<Vec<TradeSuggestion>, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let parse = load_active_ledger(&set_dir)?;

    let lock = state.store.lock();
    let store = lock.as_ref().ok_or("Metadata not initialized")?;
    let existing = store.get_trade_links()?;

    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);

    Ok(suggest_trade_links(
        &parse.transactions,
        &existing,
        base_currency.as_ref().map(|_| &price_graph),
        base_currency.as_deref(),
    ))
}

#[tauri::command]
fn get_tax_config(app: tauri::AppHandle, account_set: String) -> Result<TaxConfig, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let config_path = set_dir.join("config.json");
    if config_path.exists() {
        let contents = std::fs::read_to_string(&config_path)
            .map_err(|e| format!("failed to read config.json: {e}"))?;
        let json: serde_json::Value = serde_json::from_str(&contents)
            .map_err(|e| format!("failed to parse config.json: {e}"))?;
        if let Some(tax) = json.get("tax") {
            return serde_json::from_value(tax.clone())
                .map_err(|e| format!("failed to parse tax config: {e}"));
        }
    }
    Ok(TaxConfig::default())
}

#[tauri::command]
async fn save_tax_config(
    app: tauri::AppHandle,
    account_set: String,
    config: TaxConfig,
) -> Result<(), String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    arimalo_covid::report_templates::persist_tax_config(&set_dir, &config)?;

    // Regenerate reports with new tax config
    let sources_dir = resolve_sources_dir(&app)?;
    let generated_dir = resolve_generated_dir(&app)?;
    let primaries = primary_accounts_allowlist(&app, &generated_dir);
    let _ = arimalo_covid::report_templates::regenerate_reports_for_set(
        &set_dir,
        &sources_dir,
        arimalo_covid::report_templates::ALL_FORMATS,
        primaries.as_ref(),
    );

    Ok(())
}

// FY-form report commands first try the cached JSON artifact written by
// `report_templates::regenerate_reports_for_set` on every pipeline rebuild.
// Cache hit → instant return. Cache miss (fresh vault, struct shape changed,
// scope filter requested) → fall through to the live generator. Range
// commands below stay live-only since their date windows aren't precomputed.
//
// `base_account_scope.is_some()` always falls through: cached snapshots are
// the unscoped form.
fn read_cached_report<T: serde::de::DeserializeOwned>(path: &std::path::Path) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str::<T>(&text).ok()
}

#[tauri::command]
async fn generate_cgt_report_cmd(
    app: tauri::AppHandle,
    account_set: String,
    financial_year: String,
    base_currency: String,
    base_account_scope: Option<String>,
) -> Result<CgtReport, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    if base_account_scope.is_none() {
        let json_path =
            arimalo_covid::report_templates::cgt_json_path(&set_dir, &financial_year);
        if let Some(report) = read_cached_report::<CgtReport>(&json_path) {
            return Ok(report);
        }
    }
    let mut parse = load_active_ledger(&set_dir)?;
    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);
    let tax_config = get_tax_config(app.clone(), account_set.clone())?;

    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(&price_graph), Some(&base_currency));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();

    Ok(reports::generate_cgt_report(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &financial_year,
        &base_currency,
        base_account_scope.as_deref(),
    ))
}

#[tauri::command]
async fn generate_income_report_cmd(
    app: tauri::AppHandle,
    account_set: String,
    financial_year: String,
    base_currency: String,
    base_account_scope: Option<String>,
) -> Result<IncomeTaxReport, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    if base_account_scope.is_none() {
        let json_path =
            arimalo_covid::report_templates::income_json_path(&set_dir, &financial_year);
        if let Some(report) = read_cached_report::<IncomeTaxReport>(&json_path) {
            return Ok(report);
        }
    }
    let mut parse = load_active_ledger(&set_dir)?;
    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);
    let tax_config = get_tax_config(app.clone(), account_set.clone())?;

    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(&price_graph), Some(&base_currency));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();

    Ok(reports::generate_income_report(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &financial_year,
        &base_currency,
        base_account_scope.as_deref(),
    ))
}

#[tauri::command]
async fn generate_cgt_report_range_cmd(
    app: tauri::AppHandle,
    account_set: String,
    date_from: String,
    date_to: String,
    base_currency: String,
    base_account_scope: Option<String>,
) -> Result<CgtReport, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let parse = load_active_ledger(&set_dir)?;
    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);
    let tax_config = get_tax_config(app.clone(), account_set)?;
    let label = format!("{} to {}", date_from, date_to);
    Ok(reports::generate_cgt_report_range(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &label,
        &date_from,
        &date_to,
        &base_currency,
        base_account_scope.as_deref(),
    ))
}

#[tauri::command]
async fn generate_income_report_range_cmd(
    app: tauri::AppHandle,
    account_set: String,
    date_from: String,
    date_to: String,
    base_currency: String,
    base_account_scope: Option<String>,
) -> Result<IncomeTaxReport, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let parse = load_active_ledger(&set_dir)?;
    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);
    let tax_config = get_tax_config(app.clone(), account_set)?;
    let label = format!("{} to {}", date_from, date_to);
    Ok(reports::generate_income_report_range(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &label,
        &date_from,
        &date_to,
        &base_currency,
        base_account_scope.as_deref(),
    ))
}

#[tauri::command]
async fn generate_balances_report_cmd(
    app: tauri::AppHandle,
    account_set: String,
    financial_year: String,
    base_currency: String,
    base_account_scope: Option<String>,
) -> Result<BalancesReport, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    // Skip the cached FY-balances snapshot when extra primary-account prefixes
    // are configured: the cache is keyed only by FY, so a non-empty prefix list
    // must recompute live (mirrors the `base_account_scope` bypass).
    if base_account_scope.is_none()
        && app
            .state::<RootConfigState>()
            .0
            .lock()
            .extra_primary_account_prefixes
            .is_empty()
    {
        let json_path =
            arimalo_covid::report_templates::balances_json_path(&set_dir, &financial_year);
        if let Some(report) = read_cached_report::<BalancesReport>(&json_path) {
            return Ok(report);
        }
    }
    let mut parse = load_active_ledger(&set_dir)?;
    let hidden = load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        filter_hidden_accounts(&mut parse, &hidden);
    }
    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);
    let tax_config = get_tax_config(app.clone(), account_set)?;

    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(&price_graph), Some(&base_currency));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();

    let allowed = primary_accounts_allowlist(&app, &generated_dir);
    Ok(reports::generate_balances_report(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &financial_year,
        &base_currency,
        base_account_scope.as_deref(),
        allowed.as_ref(),
    ))
}

#[tauri::command]
async fn generate_balances_report_range_cmd(
    app: tauri::AppHandle,
    account_set: String,
    date_to: String,
    base_currency: String,
    base_account_scope: Option<String>,
) -> Result<BalancesReport, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let mut parse = load_active_ledger(&set_dir)?;
    let hidden = load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        filter_hidden_accounts(&mut parse, &hidden);
    }
    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);

    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(&price_graph), Some(&base_currency));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();

    let allowed = primary_accounts_allowlist(&app, &generated_dir);
    Ok(reports::generate_balances_report_range(
        &parse.transactions,
        &price_graph,
        &date_to,
        &base_currency,
        base_account_scope.as_deref(),
        allowed.as_ref(),
    ))
}

/// Live performance report over `[date_from, date_to]` (default 12-month
/// window chosen by the frontend). Computed on demand — no cache — like the
/// other `*_range_cmd`s. Mirrors the balances command's loading (hidden-account
/// filter, equity-swap auto-link, primary-accounts allowlist) plus the tax
/// config needed by the internal CGT/income range calls.
#[tauri::command]
async fn generate_performance_report_range_cmd(
    app: tauri::AppHandle,
    account_set: String,
    date_from: String,
    date_to: String,
    base_currency: String,
    base_account_scope: Option<String>,
) -> Result<PerformanceReport, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let mut parse = load_active_ledger(&set_dir)?;
    let hidden = load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        filter_hidden_accounts(&mut parse, &hidden);
    }
    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);
    let tax_config = get_tax_config(app.clone(), account_set)?;

    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(&price_graph), Some(&base_currency));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();

    let allowed = primary_accounts_allowlist(&app, &generated_dir);
    let label = format!("{} to {}", date_from, date_to);
    Ok(reports::generate_performance_report_range(
        reports::PerformanceReportParams {
            transactions: &parse.transactions,
            price_graph: &price_graph,
            tax_config: &tax_config,
            label: &label,
            date_from: &date_from,
            date_to: &date_to,
            base_currency: &base_currency,
            base_account_scope: base_account_scope.as_deref(),
            allowed_accounts: allowed.as_ref(),
        },
    ))
}

/// Live Tax Savings (loss-harvesting) report for a financial year. Mirrors the
/// balances command's loading (hidden-account filter, equity-swap auto-link,
/// primary-accounts allowlist) plus the tax config for the marginal-rate
/// estimate and the internal CGT gains split. Computed live — no cache.
#[tauri::command]
async fn generate_loss_harvest_report_cmd(
    app: tauri::AppHandle,
    account_set: String,
    financial_year: String,
    base_currency: String,
    base_account_scope: Option<String>,
) -> Result<LossHarvestReport, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let mut parse = load_active_ledger(&set_dir)?;
    let hidden = load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        filter_hidden_accounts(&mut parse, &hidden);
    }
    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);
    let tax_config = get_tax_config(app.clone(), account_set)?;

    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(&price_graph), Some(&base_currency));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();

    let allowed = primary_accounts_allowlist(&app, &generated_dir);
    Ok(reports::generate_loss_harvest_report(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &financial_year,
        &base_currency,
        base_account_scope.as_deref(),
        allowed.as_ref(),
    ))
}

/// Range variant of the Tax Savings report: as of `date_to`, offsetting gains
/// realised in `[date_from, date_to]`.
#[tauri::command]
async fn generate_loss_harvest_report_range_cmd(
    app: tauri::AppHandle,
    account_set: String,
    date_from: String,
    date_to: String,
    base_currency: String,
    base_account_scope: Option<String>,
) -> Result<LossHarvestReport, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let mut parse = load_active_ledger(&set_dir)?;
    let hidden = load_hidden_accounts(&set_dir);
    if !hidden.is_empty() {
        filter_hidden_accounts(&mut parse, &hidden);
    }
    let sources_dir = resolve_sources_dir(&app)?;
    let price_graph = PriceGraph::load(&sources_dir);
    let tax_config = get_tax_config(app.clone(), account_set)?;

    let mut tagged: Vec<(Option<String>, Option<String>, _)> =
        parse.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, Some(&price_graph), Some(&base_currency));
    parse.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();

    let allowed = primary_accounts_allowlist(&app, &generated_dir);
    Ok(reports::generate_loss_harvest_report_range(
        &parse.transactions,
        &price_graph,
        &tax_config,
        &date_from,
        &date_to,
        &base_currency,
        base_account_scope.as_deref(),
        allowed.as_ref(),
    ))
}

/// Render a report as CSV at `dest_path`. When `base_account_scope` is
/// supplied and non-empty, the report is regenerated live from the ledger
/// (matching whatever the user is seeing in the UI); otherwise the cached
/// JSON snapshot at `<set>/reports/<type>-<fy>.json` is used.
///
/// Caller (frontend) is responsible for picking the path via the OS save
/// dialog.
#[tauri::command]
async fn export_report_csv_cmd(
    app: tauri::AppHandle,
    account_set: String,
    report_type: String,
    financial_year: String,
    dest_path: String,
    base_currency: Option<String>,
    base_account_scope: Option<String>,
) -> Result<(), String> {
    let scope = base_account_scope.filter(|s| !s.is_empty());
    // Default matches the unscoped cache path's invariant — pipeline-time
    // reports are always written in AUD. A live regeneration accepts whatever
    // the UI is asking for.
    let base = base_currency.unwrap_or_else(|| "AUD".into());

    let csv = match report_type.as_str() {
        "cgt" => {
            let report = generate_cgt_report_cmd(
                app.clone(),
                account_set.clone(),
                financial_year.clone(),
                base.clone(),
                scope.clone(),
            ).await?;
            arimalo_covid::report_csv::cgt_to_csv(&report)?
        }
        "income" => {
            let report = generate_income_report_cmd(
                app.clone(),
                account_set.clone(),
                financial_year.clone(),
                base.clone(),
                scope.clone(),
            ).await?;
            arimalo_covid::report_csv::income_to_csv(&report)?
        }
        "balances" => {
            let report = generate_balances_report_cmd(
                app.clone(),
                account_set.clone(),
                financial_year.clone(),
                base.clone(),
                scope.clone(),
            ).await?;
            arimalo_covid::report_csv::balances_to_csv(&report)?
        }
        other => return Err(format!("unknown report type '{other}'")),
    };

    std::fs::write(&dest_path, csv)
        .map_err(|e| format!("failed to write {dest_path}: {e}"))
}

#[tauri::command]
async fn list_report_accounts_cmd(
    app: tauri::AppHandle,
    account_set: String,
) -> Result<ReportAccounts, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let parse = load_active_ledger(&set_dir)?;
    let (income, expenses) = reports::list_report_accounts(&parse.transactions);
    Ok(ReportAccounts { income, expenses })
}

#[derive(serde::Serialize)]
struct ReportAccounts {
    income: Vec<String>,
    expenses: Vec<String>,
}

#[tauri::command]
async fn get_report_cmd(
    app: tauri::AppHandle,
    account_set: String,
    report_type: String,
    financial_year: String,
) -> Result<String, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let report_path = set_dir
        .join("reports")
        .join(format!("{report_type}-{financial_year}.md"));
    if report_path.exists() {
        std::fs::read_to_string(&report_path).map_err(|e| format!("failed to read report: {e}"))
    } else {
        Ok(String::new())
    }
}

#[tauri::command]
async fn list_report_years_cmd(
    app: tauri::AppHandle,
    account_set: String,
    report_type: String,
) -> Result<Vec<i32>, String> {
    let generated_dir = resolve_generated_dir(&app)?;
    let set_dir = resolve_set_dir(&generated_dir, &account_set);
    let reports_dir = set_dir.join("reports");
    Ok(arimalo_covid::report_templates::list_report_years(
        &reports_dir,
        &report_type,
    ))
}

#[tauri::command]
fn has_root_dir(state: tauri::State<RootConfigState>) -> bool {
    state.0.lock().current_root.is_some()
}

#[tauri::command]
fn get_root_dir(state: tauri::State<RootConfigState>) -> Option<String> {
    state.0.lock().current_root.clone()
}

#[tauri::command]
fn get_known_roots(state: tauri::State<RootConfigState>) -> Vec<String> {
    state.0.lock().known_roots.clone()
}

#[tauri::command]
async fn set_root_dir(
    app: tauri::AppHandle,
    root_config_state: tauri::State<'_, RootConfigState>,
    path: String,
) -> Result<(), String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    let root_path = PathBuf::from(&path);
    let new_config = root_config::set_root(&app_dir, &root_path)?;
    *root_config_state.0.lock() = new_config;

    // Stop existing watcher if any
    if let Some(stop) = app.try_state::<WatcherStopHandle>() {
        if let Some(old_stop) = stop.0.lock().take() {
            old_stop.store(true, Ordering::Relaxed);
        }
    }

    // Start new watcher on the new sources dir
    let sources_dir = root_path.join("sources");
    let generated_dir = root_path.join("generated");
    if sources_dir.exists() {
        let new_stop = start_file_watcher(app.clone(), sources_dir, generated_dir);
        if let Some(handle) = app.try_state::<WatcherStopHandle>() {
            *handle.0.lock() = Some(new_stop);
        }
    }

    // Run pipeline on the new root
    let now_yyyymm = current_yyyymm();
    let config = make_pipeline_config(&app, &now_yyyymm)?;
    match run_pipeline(&config) {
        Ok(result) => {
            if !result.early_exit {
                app.state::<LedgerCache>().invalidate();
                for set_name in result.owner_accounts.keys() {
                    if let Some(parse) = result.parse_result_for_set(set_name) {
                        app.state::<LedgerCache>().populate(set_name, parse);
                    }
                }
            }
            spawn_report_generation(&app, &config, &result);
            let _ = app.emit("pipeline-rebuilt", &result);
        }
        Err(e) => eprintln!("Pipeline run after root change failed: {e}"),
    }

    Ok(())
}

fn current_yyyymm() -> String {
    env::var("ARIMALO_E2E_NOW_YYYYMM").unwrap_or_else(|_| {
        let now = chrono::Local::now();
        format!("{}{:02}", now.format("%Y"), now.format("%m"))
    })
}

struct WatcherStopHandle(Mutex<Option<Arc<AtomicBool>>>);

/// Flag to suppress watcher rebuilds when the app itself is modifying source files.
/// Commands that write to sources/ and then run the pipeline directly should set this
/// to prevent the watcher from triggering a duplicate pipeline run.
///
/// Refcounted so concurrent commands compose; resume happens via the `SuppressGuard`'s
/// `Drop`, so an early `?` return cannot leave the watcher disabled until restart.
struct WatcherSuppressFlag(Arc<AtomicUsize>);

impl WatcherSuppressFlag {
    fn new() -> Self {
        Self(Arc::new(AtomicUsize::new(0)))
    }
    #[must_use = "watcher stays suppressed only while the guard is alive"]
    fn suppress(&self) -> SuppressGuard {
        self.0.fetch_add(1, Ordering::SeqCst);
        SuppressGuard(self.0.clone())
    }
    fn is_suppressed(&self) -> bool {
        self.0.load(Ordering::SeqCst) > 0
    }
}

struct SuppressGuard(Arc<AtomicUsize>);

impl Drop for SuppressGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Wall-clock millis since the Unix epoch (0 on clock error).
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Timestamp (epoch millis) of the most recent command-driven pipeline rebuild
/// — manual add, import, transform-save, delete. The file-watcher consults this
/// to skip the redundant rebuild its ~500ms debounce would otherwise fire right
/// after a command wrote to `sources/` and already rebuilt that change. A scoped
/// rebuild finishes before the debounce elapses, so the RAII WatcherSuppressFlag
/// guard alone can't cover the window; the command marks the time *before* its
/// write so the watcher (waking ~500ms after the write) sees it as recent.
struct LastCommandRebuild(AtomicU64);

impl LastCommandRebuild {
    fn new() -> Self {
        Self(AtomicU64::new(0))
    }
    fn mark(&self) {
        self.0.store(now_millis(), Ordering::SeqCst);
    }
    /// True if a command rebuild was marked within `window` of now.
    fn within(&self, window: std::time::Duration) -> bool {
        let last = self.0.load(Ordering::SeqCst);
        last != 0 && now_millis().saturating_sub(last) < window.as_millis() as u64
    }
}

fn start_file_watcher(
    handle: tauri::AppHandle,
    sources_dir: PathBuf,
    generated_dir: PathBuf,
) -> Arc<AtomicBool> {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_clone = stop.clone();

    std::thread::spawn(move || {
        use notify::{RecursiveMode, Watcher};
        use std::sync::mpsc;
        use std::time::Duration;

        let (tx, rx) = mpsc::channel();
        let mut watcher =
            match notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    use notify::EventKind;
                    match event.kind {
                        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) => {
                            let _ = tx.send(());
                        }
                        _ => {}
                    }
                }
            }) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("Failed to create file watcher: {e}");
                    return;
                }
            };

        if let Err(e) = watcher.watch(&sources_dir, RecursiveMode::Recursive) {
            eprintln!("Failed to watch sources dir: {e}");
            return;
        }

        eprintln!("File watcher started on {}", sources_dir.display());

        while !stop_clone.load(Ordering::Relaxed) {
            match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(()) => {
                    std::thread::sleep(Duration::from_millis(500));
                    while rx.try_recv().is_ok() {}

                    // Skip if a command is running the pipeline directly, or just
                    // did (a scoped command rebuild finishes before this debounce,
                    // so a recent mark — not just the live guard — must suppress us).
                    if handle.state::<WatcherSuppressFlag>().is_suppressed()
                        || handle
                            .state::<LastCommandRebuild>()
                            .within(std::time::Duration::from_millis(800))
                    {
                        eprintln!("File watcher: suppressed (command pipeline recent)");
                        continue;
                    }

                    let now_yyyymm = current_yyyymm();
                    let root_config = handle.state::<RootConfigState>().0.lock().clone();
                    let config = PipelineConfig {
                        sources_dir: sources_dir.clone(),
                        generated_dir: generated_dir.clone(),
                        now_yyyymm,
                        force: false,
                        default_expense_account: root_config.default_expense_account.clone(),
                        changed_folder_hint: None,
                    };

                    match run_pipeline(&config) {
                        Ok(result) => {
                            // Drain any events generated during pipeline run (e.g. strip_noop_rules)
                            while rx.try_recv().is_ok() {}

                            if result.early_exit {
                                eprintln!("Auto-rebuild: early exit (no changes)");
                                continue;
                            }
                            eprintln!(
                                "Auto-rebuild: {} transformed, {} cached, {} total (output: {} written, {} skipped)",
                                result.csv_transformed, result.csv_cached, result.total_written,
                                result.output_files_written, result.output_files_skipped
                            );
                            // Pre-populate LedgerCache from in-memory pipeline data
                            // so frontend doesn't re-read from disk.
                            handle.state::<LedgerCache>().invalidate();
                            for set_name in result.owner_accounts.keys() {
                                if let Some(parse) = result.parse_result_for_set(set_name) {
                                    handle.state::<LedgerCache>().populate(set_name, parse);
                                }
                            }
                            spawn_report_generation(&handle, &config, &result);
                            let _ = handle.emit("pipeline-rebuilt", &result);
                        }
                        Err(e) => {
                            eprintln!("Auto-rebuild failed: {e}");
                            // Drain events even on failure
                            while rx.try_recv().is_ok() {}
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    });

    stop
}

// === Plugin commands ===

fn resolve_plugins_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let sources = resolve_sources_dir(app)?;
    let root = sources.parent().ok_or("sources dir has no parent")?;
    Ok(root.join("plugins"))
}

#[tauri::command]
fn list_plugins(app: tauri::AppHandle) -> Result<Vec<plugins::PluginInfo>, String> {
    let sources = resolve_sources_dir(&app)?;
    let root = sources.parent().ok_or("sources dir has no parent")?;
    Ok(plugins::discover_plugins(root))
}

#[tauri::command]
async fn run_plugin_cmd(
    app: tauri::AppHandle,
    plugin_name: String,
) -> Result<plugins::PluginRunResult, String> {
    let plugins_dir = resolve_plugins_dir(&app)?;
    let plugin_dir = plugins_dir.join(&plugin_name);
    if !plugin_dir.join("plugin.toml").exists() {
        return Err(format!("Plugin {plugin_name:?} not found"));
    }
    let sources_dir = resolve_sources_dir(&app)?;
    let config = plugins::load_plugin_config(&plugin_dir);
    let secrets = plugins::load_plugin_secrets(&plugin_dir);
    // Suppress the file watcher across the plugin run so its writes
    // don't fire a redundant rebuild — we'll run the pipeline ourselves
    // once the plugin completes so the UI gets a deterministic
    // `pipeline-rebuilt` event.
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    // Offload the blocking subprocess wait off the async runtime so
    // long-running plugins (e.g. do-my-crypto-taxes) don't freeze the UI.
    // Emit each line of stdout/stderr as a `plugin-log` event so the UI
    // can render progress live.
    let app_for_log = app.clone();
    let plugin_for_log = plugin_name.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        plugins::run_plugin_with_logger(
            &plugin_dir,
            &sources_dir,
            &config,
            &secrets,
            move |stream, line| {
                let _ = app_for_log.emit(
                    "plugin-log",
                    serde_json::json!({
                        "plugin": plugin_for_log,
                        "stream": stream,
                        "line": line,
                    }),
                );
            },
        )
    })
    .await
    .map_err(|e| format!("plugin task join error: {e}"))?;
    let _ = app.emit(
        "plugin-finished",
        serde_json::json!({
            "plugin": plugin_name,
            "success": result.success,
        }),
    );
    // Plugins that wrote into `sources/` (rules, labels, CSVs, …) need a
    // pipeline rebuild for the UI to reflect the change.
    rebuild_pipeline_after_plugins(&app);
    drop(_suppress);
    Ok(result)
}

/// Rebuild the pipeline after one or more plugins wrote into `sources/`.
/// Invalidates the ledger cache, repopulates each set, regenerates reports,
/// and emits `pipeline-rebuilt`. Mirrors save_rule / hide_transaction —
/// explicit run, not relying on the OS watcher to pick the writes up.
fn rebuild_pipeline_after_plugins(app: &tauri::AppHandle) {
    let now_yyyymm = current_yyyymm();
    if let Ok(pipeline_config) = make_pipeline_config(app, &now_yyyymm) {
        match run_pipeline(&pipeline_config) {
            Ok(pipeline_result) => {
                app.state::<LedgerCache>().invalidate();
                for set_name in pipeline_result.owner_accounts.keys() {
                    if let Some(parse) = pipeline_result.parse_result_for_set(set_name) {
                        app.state::<LedgerCache>().populate(set_name, parse);
                    }
                }
                spawn_report_generation(app, &pipeline_config, &pipeline_result);
                let _ = app.emit("pipeline-rebuilt", &pipeline_result);
            }
            Err(e) => {
                eprintln!("post-plugin pipeline rebuild failed: {e}");
            }
        }
    }
}

/// Run all daily-flagged plugins (the "update prices on startup" batch) and
/// rebuild the pipeline once at the end. `skip_if_succeeded_today` skips
/// plugins that already succeeded today so repeated launches don't re-fetch.
#[tauri::command]
async fn run_daily_plugins_cmd(
    app: tauri::AppHandle,
    skip_if_succeeded_today: bool,
) -> Result<plugins::DailyRunSummary, String> {
    let plugins_dir = resolve_plugins_dir(&app)?;
    let sources_dir = resolve_sources_dir(&app)?;
    // Suppress the watcher across the batch so plugin writes don't fire
    // redundant rebuilds — we rebuild once ourselves at the end.
    let _suppress = app.state::<WatcherSuppressFlag>().suppress();
    let app_for_log = app.clone();
    let summary = tauri::async_runtime::spawn_blocking(move || {
        plugins::run_daily_plugins(
            &plugins_dir,
            &sources_dir,
            skip_if_succeeded_today,
            move |dir_name, stream, line| {
                let _ = app_for_log.emit(
                    "plugin-log",
                    serde_json::json!({
                        "plugin": dir_name,
                        "stream": stream,
                        "line": line,
                    }),
                );
            },
        )
    })
    .await
    .map_err(|e| format!("daily plugin task join error: {e}"))?;

    // Only rebuild if at least one plugin actually ran (not all skipped).
    if summary.any_ran() {
        rebuild_pipeline_after_plugins(&app);
    }
    drop(_suppress);
    Ok(summary)
}

#[tauri::command]
fn get_update_prices_on_startup(state: tauri::State<RootConfigState>) -> bool {
    state.0.lock().update_prices_on_startup
}

#[tauri::command]
fn set_update_prices_on_startup(
    app: tauri::AppHandle,
    state: tauri::State<RootConfigState>,
    enabled: bool,
) -> Result<(), String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    let config = {
        let mut guard = state.0.lock();
        guard.update_prices_on_startup = enabled;
        guard.clone()
    };
    root_config::save_root_config(&app_dir, &config)
}

#[tauri::command]
fn get_extra_primary_account_prefixes(state: tauri::State<RootConfigState>) -> Vec<String> {
    state.0.lock().extra_primary_account_prefixes.clone()
}

#[tauri::command]
fn set_extra_primary_account_prefixes(
    app: tauri::AppHandle,
    state: tauri::State<RootConfigState>,
    prefixes: Vec<String>,
) -> Result<(), String> {
    let app_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
    let config = {
        let mut guard = state.0.lock();
        guard.extra_primary_account_prefixes = prefixes
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        guard.clone()
    };
    root_config::save_root_config(&app_dir, &config)?;
    // The Balances FY cache is keyed only by FY, so regenerate cached reports for
    // every set with the new prefix list (mirrors `save_tax_config`). Spawned so
    // the setter returns promptly; the frontend refreshes on `reports-rebuilt`.
    regenerate_reports_after_prefix_change(&app, config.extra_primary_account_prefixes);
    Ok(())
}

/// Regenerate every set's cached reports off-thread after the primary-account
/// prefixes change, then notify the UI via `reports-rebuilt`. Best-effort:
/// dir-resolution failures are swallowed (nothing to regenerate).
fn regenerate_reports_after_prefix_change(app: &tauri::AppHandle, prefixes: Vec<String>) {
    let (Ok(sources_dir), Ok(generated_dir)) =
        (resolve_sources_dir(app), resolve_generated_dir(app))
    else {
        return;
    };
    let mut account_sets: Vec<String> = PipelineMetadata::load(&generated_dir)
        .map(|m| m.owner_accounts.keys().cloned().collect())
        .unwrap_or_default();
    account_sets.sort();
    let handle = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = arimalo_covid::report_templates::generate_all_reports(
            &sources_dir,
            &generated_dir,
            &account_sets,
            arimalo_covid::report_templates::ALL_FORMATS,
            &prefixes,
        ) {
            eprintln!("Report regeneration after prefix change failed: {e}");
        }
        let _ = handle.emit("reports-rebuilt", ());
    });
}

#[tauri::command]
fn get_plugin_config(
    app: tauri::AppHandle,
    plugin_name: String,
) -> Result<serde_json::Value, String> {
    let plugins_dir = resolve_plugins_dir(&app)?;
    Ok(plugins::load_plugin_config(&plugins_dir.join(&plugin_name)))
}

#[tauri::command]
fn save_plugin_config_cmd(
    app: tauri::AppHandle,
    plugin_name: String,
    config: serde_json::Value,
) -> Result<(), String> {
    let plugins_dir = resolve_plugins_dir(&app)?;
    plugins::save_plugin_config(&plugins_dir.join(&plugin_name), &config)
}

#[tauri::command]
fn save_plugin_secrets_cmd(
    app: tauri::AppHandle,
    plugin_name: String,
    secrets: serde_json::Value,
) -> Result<(), String> {
    let plugins_dir = resolve_plugins_dir(&app)?;
    plugins::save_plugin_secrets(&plugins_dir.join(&plugin_name), &secrets)
}

#[tauri::command]
fn get_plugin_secrets(
    app: tauri::AppHandle,
    plugin_name: String,
) -> Result<serde_json::Value, String> {
    let plugins_dir = resolve_plugins_dir(&app)?;
    Ok(plugins::load_plugin_secrets(&plugins_dir.join(&plugin_name)))
}

fn main() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default().plugin(tauri_plugin_dialog::init());

    #[cfg(feature = "webdriver")]
    {
        builder = builder.plugin(tauri_plugin_webdriver_automation::init());
    }

    builder
        .manage(MetadataState {
            store: Mutex::new(None),
        })
        .manage(StartupWarnings(Mutex::new(Vec::new())))
        .manage(ReportGeneration(AtomicU64::new(0)))
        .manage(ShowHiddenState(AtomicBool::new(false)))
        .manage(LedgerCache::new())
        .manage(LastCommandRebuild::new())
        .setup(|app| {
            // Window title: when running from a git worktree, suffix with the
            // worktree name so multiple instances (each on a different branch)
            // are distinguishable in the OS window list. Detected from the
            // build path (`CARGO_MANIFEST_DIR` set at compile time): a path
            // segment of `worktrees/<repo>/<name>/...` extracts <name>. The
            // `ARIMALO_INSTANCE` env var overrides, e.g. for ad-hoc runs.
            let instance: Option<String> = env::var("ARIMALO_INSTANCE").ok().or_else(|| {
                let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
                let mut iter = manifest_dir.iter();
                while let Some(seg) = iter.next() {
                    if seg == "worktrees" {
                        iter.next(); // skip <repo>
                        return iter.next().and_then(|s| s.to_str().map(String::from));
                    }
                }
                None
            });
            if let Some(name) = instance {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.set_title(&format!("Arimalo COVID — {name}"));
                }
            }

            // Load root config from app_data_dir/config.json
            let app_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("failed to resolve app data dir: {e}"))?;
            let config = root_config::load_root_config(&app_dir);
            let has_root = config.current_root.is_some();
            app.manage(RootConfigState(Mutex::new(config)));
            app.manage(WatcherStopHandle(Mutex::new(None)));
            app.manage(WatcherSuppressFlag::new());

            // Skip pipeline and watcher when no root is configured yet —
            // the frontend vault picker will call set_root_dir to bootstrap.
            if has_root || env::var("ARIMALO_SOURCES_DIR").is_ok() {
                // Run pipeline on startup — the build cache makes this a no-op when
                // nothing has changed (inputs_hash match skips the output rewrite).
                {
                    let now_yyyymm = current_yyyymm();
                    let config = make_pipeline_config(app.handle(), &now_yyyymm)?;
                    match run_pipeline(&config) {
                        Ok(result) => {
                            if !result.warnings.is_empty() {
                                for w in &result.warnings {
                                    eprintln!("Pipeline warning: {w}");
                                }
                                *app.state::<StartupWarnings>().0.lock() = result.warnings.clone();
                            }
                            spawn_report_generation(app.handle(), &config, &result);
                            if result.total_written > 0 {
                                eprintln!(
                                    "Startup rebuild: {} transformed, {} cached, {} written",
                                    result.csv_transformed, result.csv_cached, result.total_written
                                );
                            } else {
                                eprintln!("Startup: sources unchanged, skipped rewrite");
                            }
                        }
                        Err(e) => eprintln!("Startup rebuild failed: {e}"),
                    }
                }

                // File watcher: watch sources/ for changes and auto-rebuild pipeline
                if let (Ok(sources_dir), Ok(generated_dir)) = (
                    resolve_sources_dir(app.handle()),
                    resolve_generated_dir(app.handle()),
                ) {
                    if sources_dir.exists() {
                        let stop =
                            start_file_watcher(app.handle().clone(), sources_dir, generated_dir);
                        *app.state::<WatcherStopHandle>().0.lock() = Some(stop);
                    }
                }
            } else {
                eprintln!("No root configured — waiting for user to pick a data folder");
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            parse_transactions_file,
            load_generated_ledger,
            load_account_tree,
            query_search,
            query_global,
            load_pipeline_metadata,
            rebuild_pipeline,
            get_pipeline_warnings,
            add_manual_transaction,
            add_account_declaration,
            import_csv_to_account,
            import_csv_files_to_account,
            import_rules_csv,
            import_prices,
            set_price,
            process_imports_cmd,
            suggest_transform,
            read_transform,
            save_transform,
            save_transform_and_rebuild_cmd,
            save_rule,
            save_label,
            get_rules,
            update_rule,
            delete_rule,
            init_metadata,
            get_sync_log,
            merge_metadata,
            list_devices,
            sync_with_remote,
            pair_initiate,
            pair_join,
            sync_with_relay_cmd,
            save_relay_config,
            get_relay_config,
            hide_transaction,
            delete_manual_transaction,
            list_account_sets,
            get_account_gaps,
            collect_issues_cmd,
            reveal_in_finder,
            get_source_folder_path,
            update_account_properties,
            rename_account_folder,
            delete_account_folder,
            set_opening_balance,
            get_display_config,
            open_display_config,
            convert_to_base_currency,
            save_trade_link,
            save_trade_links_bulk,
            delete_trade_link,
            get_trade_links,
            suggest_trade_links_cmd,
            get_tax_config,
            save_tax_config,
            generate_cgt_report_cmd,
            generate_income_report_cmd,
            generate_cgt_report_range_cmd,
            generate_income_report_range_cmd,
            generate_balances_report_cmd,
            generate_balances_report_range_cmd,
            generate_performance_report_range_cmd,
            generate_loss_harvest_report_cmd,
            generate_loss_harvest_report_range_cmd,
            export_report_csv_cmd,
            list_report_accounts_cmd,
            get_report_cmd,
            list_report_years_cmd,
            has_root_dir,
            get_root_dir,
            get_known_roots,
            set_root_dir,
            list_plugins,
            run_plugin_cmd,
            run_daily_plugins_cmd,
            get_update_prices_on_startup,
            set_update_prices_on_startup,
            get_extra_primary_account_prefixes,
            set_extra_primary_account_prefixes,
            get_plugin_config,
            get_plugin_secrets,
            save_plugin_config_cmd,
            save_plugin_secrets_cmd,
            ai_suggest_categorisation,
            ai_suggest_transform,
            get_account_config,
            set_show_hidden,
            get_show_hidden,
            open_url
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn watcher_suppress_guard_resumes_on_drop() {
        let flag = WatcherSuppressFlag::new();
        assert!(!flag.is_suppressed());
        {
            let _g = flag.suppress();
            assert!(flag.is_suppressed());
        }
        assert!(!flag.is_suppressed(), "guard drop must clear suppression");
    }

    #[test]
    fn watcher_suppress_guard_refcounts() {
        let flag = WatcherSuppressFlag::new();
        let g1 = flag.suppress();
        let g2 = flag.suppress();
        assert!(flag.is_suppressed());
        drop(g1);
        assert!(flag.is_suppressed(), "outer guard still active");
        drop(g2);
        assert!(!flag.is_suppressed());
    }

    #[test]
    fn watcher_suppress_guard_clears_on_panic_unwind() {
        // Simulates the early-`?`-return bug: previously, an error between
        // suppress()/resume() left the watcher disabled until restart. With a
        // RAII guard, even a panic mid-section must clear suppression.
        let flag = WatcherSuppressFlag::new();
        let arc = flag.0.clone();
        let result = std::panic::catch_unwind(move || {
            let flag = WatcherSuppressFlag(arc);
            let _g = flag.suppress();
            panic!("simulated mid-command failure");
        });
        assert!(result.is_err());
        assert!(!flag.is_suppressed(), "panic must not leave watcher disabled");
    }

    fn mk_rule(id: &str, pattern: &str) -> Rule {
        Rule {
            id: id.to_string(),
            pattern: pattern.to_string(),
            match_field: None,
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            amount_account: Some("income:crypto:airdrop".to_string()),
            fee_account: None,
            postings: vec![],
        }
    }

    #[test]
    fn apply_rule_update_moves_rule_to_parent_folder_when_scope_broadens() {
        let tmp = TempDir::new().unwrap();
        let sources = tmp.path();
        let sub = sources.join("solana").join("walletA");
        let parent = sources.join("solana");
        std::fs::create_dir_all(&sub).unwrap();

        // Seed: rule lives in sub-folder
        let mut seed = RulesFile::default();
        seed.rules.push(mk_rule("rule-e64e2ebe", "*token_transfer*"));
        seed.save(&sub).unwrap();

        // Act: broaden scope to parent folder ("solana")
        let updated = mk_rule("rule-e64e2ebe", "*token_transfer*");
        apply_rule_update(sources, &sub, Some("solana"), updated)
            .expect("apply_rule_update should succeed");

        // Assert: removed from sub, present in parent
        let sub_rules = RulesFile::load(&sub);
        assert!(
            !sub_rules.rules.iter().any(|r| r.id == "rule-e64e2ebe"),
            "rule should be removed from sub folder"
        );
        let parent_rules = RulesFile::load(&parent);
        assert!(
            parent_rules.rules.iter().any(|r| r.id == "rule-e64e2ebe"),
            "rule should now live in parent folder"
        );
    }

    #[test]
    fn apply_rule_update_edits_in_place_when_target_matches_current() {
        let tmp = TempDir::new().unwrap();
        let sources = tmp.path();
        let sub = sources.join("solana").join("walletA");
        std::fs::create_dir_all(&sub).unwrap();

        let mut seed = RulesFile::default();
        seed.rules.push(mk_rule("rule-1", "old-pattern"));
        seed.save(&sub).unwrap();

        let updated = mk_rule("rule-1", "new-pattern");
        apply_rule_update(sources, &sub, Some("solana/walletA"), updated).unwrap();

        let rules = RulesFile::load(&sub);
        let r = rules.rules.iter().find(|r| r.id == "rule-1").unwrap();
        assert_eq!(r.pattern, "new-pattern");
    }

    #[test]
    fn apply_rule_update_to_root_when_target_is_empty_string() {
        let tmp = TempDir::new().unwrap();
        let sources = tmp.path();
        let sub = sources.join("solana").join("walletA");
        std::fs::create_dir_all(&sub).unwrap();

        let mut seed = RulesFile::default();
        seed.rules.push(mk_rule("rule-2", "p"));
        seed.save(&sub).unwrap();

        apply_rule_update(sources, &sub, Some(""), mk_rule("rule-2", "p")).unwrap();

        let sub_rules = RulesFile::load(&sub);
        assert!(!sub_rules.rules.iter().any(|r| r.id == "rule-2"));
        let root_rules = RulesFile::load(sources);
        assert!(root_rules.rules.iter().any(|r| r.id == "rule-2"));
    }
}
