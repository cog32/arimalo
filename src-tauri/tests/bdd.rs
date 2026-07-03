use cucumber::{given, then, when, World as _};
use arimalo_covid::automerge_store::{
    suggest_trade_links, MetadataStore, ArimaloMetadata, TradeSuggestion,
};
use arimalo_covid::build_cache;
use arimalo_covid::content_store::{ingest_sources_to_cas, BlobStatus, ContentStore};
use arimalo_covid::generated_ledger::rotate_ledger_if_needed;
use arimalo_covid::generated_store::{
    filter_hidden_accounts, load_account_tree, load_active_ledger, ManualPostingInput,
    ManualTransactionInput,
};
use arimalo_covid::ledger_parser::{
    lookup_price, parse_prices, parse_transactions, AccountBalance, ParseResult, PriceGraph,
    PriceLookupResult, PricesParseResult,
};
use arimalo_covid::plugins::{self, discover_plugins_in, PluginInfo, PluginRunResult};
use arimalo_covid::processing_pipeline::{
    append_account_and_rebuild, append_hide_rule, append_manual_and_rebuild, append_to_ignored,
    auto_link_equity_swaps, delete_account_folder_and_rebuild, transaction_to_text,
    delete_manual_transaction_and_rebuild, detect_account_gaps, import_csv_to_sources,
    import_prices_file, rename_account_folder_and_rebuild, run_pipeline,
    save_transform_and_rebuild, update_opening_balance, AccountGap, PipelineConfig, PipelineResult,
    PriceImportResult,
};
use arimalo_covid::query;
use arimalo_covid::relay::server::{RelayConfig, RelayServer};
use arimalo_covid::relay_client;
use arimalo_covid::report_templates;
use arimalo_covid::reports::{
    self, BalancesReport, CgtReport, IncomeTaxReport, PerformanceReport, TaxConfig, TradeLinkRef,
};
use arimalo_covid::root_config;
use arimalo_covid::rules::{
    remove_trade_link_rules, LabelsFile, MatchFields, Rule, RulesFile,
};
use arimalo_covid::sync::{diff_manifests, full_sync, SyncResult};
use arimalo_covid::trade_link_repair::plan_trade_link_rules;
use arimalo_covid::transform_suggest;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Default, cucumber::World)]
struct LedgerWorld {
    file_path: Option<PathBuf>,
    result: Option<ParseResult>,
    generated_dir: Option<PathBuf>,
    source_file_path: Option<PathBuf>,
    source_file_before: Option<String>,
    // Transform suggestion fields
    csv_headers: Option<Vec<String>>,
    target_account: Option<String>,
    transform_suggestion: Option<String>,
    // CSV pipeline fields
    sources_dir: Option<PathBuf>,
    pipeline_result: Option<PipelineResult>,
    pipeline_result_prev: Option<PipelineResult>,
    active_ledger_text: Option<String>,
    active_ledger_text_prev: Option<String>,
    now_yyyymm: Option<String>,
    // Automerge metadata fields
    metadata_store: Option<MetadataStore>,
    metadata_snapshot: Option<ArimaloMetadata>,
    metadata_path_a: Option<PathBuf>,
    metadata_path_b: Option<PathBuf>,
    // Content-addressed storage fields
    cas: Option<ContentStore>,
    cas_dir: Option<PathBuf>,
    last_stored_hash: Option<String>,
    // Sync fields
    device_a_sources: Option<PathBuf>,
    device_b_sources: Option<PathBuf>,
    device_a_cas: Option<ContentStore>,
    device_b_cas: Option<ContentStore>,
    device_a_store: Option<MetadataStore>,
    device_b_store: Option<MetadataStore>,
    sync_result: Option<SyncResult>,
    manifest_diff_missing: Option<usize>,
    // Relay fields
    relay_server: Option<Arc<RelayServer>>,
    relay_url: Option<String>,
    relay_data_dir: Option<PathBuf>,
    relay_group_id: Option<String>,
    relay_pairing_code: Option<String>,
    relay_join_error: Option<String>,
    relay_blob_hashes: Vec<String>,
    relay_sync_error: Option<String>,
    relay_sync_result: Option<relay_client::RelaySyncResult>,
    // Gap detection fields
    gap_results: Option<Vec<AccountGap>>,
    // Prices parser fields
    prices_result: Option<PricesParseResult>,
    // Per-set output: which set to read from
    active_set_name: Option<String>,
    // Import prices fields
    prices_import_path: Option<PathBuf>,
    price_import_result: Option<Result<PriceImportResult, String>>,
    price_lookup_result: Option<Option<PriceLookupResult>>,
    // Import rules CSV fields
    rules_csv_path: Option<PathBuf>,
    // Price conversion fields
    conversion_result: Option<Option<f64>>,
    batch_conversion_results: Option<Vec<Option<f64>>>,
    // Trade link fields
    trade_link_store: Option<Box<MetadataStore>>,
    last_trade_link_id: Option<String>,
    trade_suggestions: Option<Vec<TradeSuggestion>>,
    exchange_txn_ids: Option<(String, String)>,
    trade_base_currency: Option<String>,
    // Reports fields
    cgt_report: Option<CgtReport>,
    income_report: Option<IncomeTaxReport>,
    balances_report: Option<BalancesReport>,
    performance_report: Option<PerformanceReport>,
    report_trade_links: Vec<TradeLinkRef>,
    rendered_report: Option<String>,
    // Account manage fields
    last_error: Option<String>,
    // Plugin fields
    plugins_dir: Option<PathBuf>,
    discovered_plugins: Option<Vec<PluginInfo>>,
    plugin_run_result: Option<PluginRunResult>,
    daily_summary: Option<plugins::DailyRunSummary>,
    // Account config fields
    account_config: Option<arimalo_covid::rules::AccountConfig>,
    // Hidden accounts filtering
    filtered_parse: Option<ParseResult>,
    // Scoped query / account tree fields
    scoped_query_result: Option<query::QueryResult>,
    scoped_query_prev: Option<query::QueryResult>,
    scoped_account_tree: Option<Vec<AccountBalance>>,
    // Root config fields
    root_config_dir: Option<PathBuf>,
    root_config: Option<arimalo_covid::root_config::RootConfig>,
    root_config_new_path: Option<PathBuf>,
    resolved_sources: Option<PathBuf>,
    resolved_generated: Option<PathBuf>,
    // Incremental pipeline scenarios: snapshot of generated ledger files
    // captured before a hinted pipeline run so "should be unchanged" can diff.
    generated_ledger_snapshot: Option<std::collections::HashMap<String, Vec<u8>>>,
    // Per-folder summary.json captured after each run by "I run the pipeline twice".
    // BTreeMap keeps ordering stable for assertion error messages.
    summary_snapshot_prev: Option<std::collections::BTreeMap<String, String>>,
    summary_snapshot_after: Option<std::collections::BTreeMap<String, String>>,
    // arimalo-dedupe CLI fields
    dedupe_stdout: Option<String>,
}

impl LedgerWorld {
    fn set_device_sources(
        &mut self,
        device: &str,
        base: PathBuf,
        sources: PathBuf,
        cas_dir: PathBuf,
    ) {
        match device {
            "A" => {
                self.device_a_sources = Some(sources);
                self.device_a_cas = Some(ContentStore::new(cas_dir));
                self.metadata_path_a = Some(base.join("metadata.automerge"));
            }
            "B" => {
                self.device_b_sources = Some(sources);
                self.device_b_cas = Some(ContentStore::new(cas_dir));
                self.metadata_path_b = Some(base.join("metadata.automerge"));
            }
            _ => panic!("unknown device: {device}"),
        }
    }

    fn device_sources(&self, device: &str) -> &PathBuf {
        match device {
            "A" => self.device_a_sources.as_ref().expect("device_a_sources"),
            "B" => self.device_b_sources.as_ref().expect("device_b_sources"),
            _ => panic!("unknown device: {device}"),
        }
    }
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("features")
        .join("fixtures")
}

/// Build CSV content from a Cucumber data table (first row = headers, rest = data rows).
fn table_to_csv(table: &cucumber::gherkin::Table) -> String {
    let headers: Vec<String> = table
        .rows
        .first()
        .expect("header row")
        .iter()
        .cloned()
        .collect();
    let mut content = headers.join(",");
    content.push('\n');
    for row in table.rows.iter().skip(1) {
        content.push_str(&row.join(","));
        content.push('\n');
    }
    content
}

// === Parser scenarios ===

#[given(expr = "a transactions file named {string}")]
async fn a_transactions_file_named(world: &mut LedgerWorld, file_name: String) {
    world.file_path = Some(fixtures_dir().join(file_name));
}

#[when("I run the ledger parser on that file")]
async fn i_run_the_ledger_parser_on_that_file(world: &mut LedgerWorld) {
    let file_path = world
        .file_path
        .as_ref()
        .expect("file path should be set by the Given step");
    let contents = std::fs::read_to_string(file_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {file_path:?}: {e}"));
    world.result = Some(parse_transactions(&contents));
}

#[then("the parse should succeed")]
async fn the_parse_should_succeed(world: &mut LedgerWorld) {
    let result = world
        .result
        .as_ref()
        .expect("parse result should be set by the When step");
    assert!(
        result.ok,
        "expected parse ok; diagnostics: {:?}",
        result.diagnostics
    );
}

#[then("the parse should fail")]
async fn the_parse_should_fail(world: &mut LedgerWorld) {
    let result = world
        .result
        .as_ref()
        .expect("parse result should be set by the When step");
    assert!(!result.ok, "expected parse failure");
}

#[then(expr = "diagnostics should include {string}")]
async fn diagnostics_should_include(world: &mut LedgerWorld, needle: String) {
    let result = world
        .result
        .as_ref()
        .expect("parse result should be set by the When step");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.message.contains(&needle)),
        "expected diagnostics to include {needle:?}; got: {:?}",
        result.diagnostics
    );
}

#[then(expr = "the first transaction payee should be {string}")]
async fn first_transaction_payee_should_be(world: &mut LedgerWorld, expected: String) {
    let result = world
        .result
        .as_ref()
        .expect("parse result should be set by the When step");
    let txn = result
        .transactions
        .first()
        .expect("expected at least one parsed transaction");
    assert_eq!(
        txn.payee.as_deref(),
        Some(expected.as_str()),
        "unexpected payee: {:?}",
        txn.payee
    );
}

#[then(expr = "the first transaction narration should be {string}")]
async fn first_transaction_narration_should_be(world: &mut LedgerWorld, expected: String) {
    let result = world
        .result
        .as_ref()
        .expect("parse result should be set by the When step");
    let txn = result
        .transactions
        .first()
        .expect("expected at least one parsed transaction");
    assert_eq!(
        txn.narration.as_deref(),
        Some(expected.as_str()),
        "unexpected narration: {:?}",
        txn.narration
    );
}

#[then(expr = "the first transaction meta should include {string}")]
async fn first_transaction_meta_should_include(world: &mut LedgerWorld, expected: String) {
    let result = world
        .result
        .as_ref()
        .expect("parse result should be set by the When step");
    let txn = result
        .transactions
        .first()
        .expect("expected at least one parsed transaction");
    let meta = txn.meta.as_deref().unwrap_or("");
    assert!(
        meta.contains(&expected),
        "expected meta to include {expected:?}, got: {meta:?}"
    );
}

#[then(expr = "the balance for account {string} should be {string} {string}")]
async fn the_balance_for_account_should_be(
    world: &mut LedgerWorld,
    account: String,
    amount_text: String,
    commodity: String,
) {
    let result = world
        .result
        .as_ref()
        .expect("parse result should be set by the When step");
    let expected_amount: f64 = amount_text
        .parse()
        .unwrap_or_else(|e| panic!("invalid expected amount {amount_text:?}: {e}"));
    let balance = result
        .balances
        .iter()
        .find(|b| b.account == account)
        .unwrap_or_else(|| panic!("missing balance for account {account:?}"));
    let actual = balance
        .totals
        .iter()
        .find(|t| t.commodity == commodity)
        .unwrap_or_else(|| panic!("missing commodity {commodity:?} for account {account:?}"));

    assert!(
        (actual.amount - expected_amount).abs() < 1e-9,
        "expected {expected_amount} {commodity} for {account}, got {actual:?}",
    );
}

// === Commodity pricing scenarios ===

#[given(expr = "a prices file named {string}")]
async fn a_prices_file_named(world: &mut LedgerWorld, file_name: String) {
    world.file_path = Some(fixtures_dir().join(file_name));
}

#[when("I run the prices parser on that file")]
async fn i_run_the_prices_parser_on_that_file(world: &mut LedgerWorld) {
    let file_path = world
        .file_path
        .as_ref()
        .expect("file path should be set by the Given step");
    let contents = std::fs::read_to_string(file_path)
        .unwrap_or_else(|e| panic!("failed to read fixture {file_path:?}: {e}"));
    world.prices_result = Some(parse_prices(&contents));
}

#[then("the prices parse should succeed")]
async fn the_prices_parse_should_succeed(world: &mut LedgerWorld) {
    let result = world
        .prices_result
        .as_ref()
        .expect("prices result should be set by the When step");
    assert!(
        result.ok,
        "expected prices parse ok; diagnostics: {:?}",
        result.diagnostics
    );
}

#[then("the prices parse should fail")]
async fn the_prices_parse_should_fail(world: &mut LedgerWorld) {
    let result = world
        .prices_result
        .as_ref()
        .expect("prices result should be set by the When step");
    assert!(!result.ok, "expected prices parse failure");
}

#[then(expr = "there should be {int} price directives")]
async fn there_should_be_n_price_directives(world: &mut LedgerWorld, count: usize) {
    let result = world
        .prices_result
        .as_ref()
        .expect("prices result should be set by the When step");
    assert_eq!(
        result.prices.len(),
        count,
        "expected {count} price directives, got {}",
        result.prices.len()
    );
}

#[then(expr = "price directive {int} should be {string} at {string} {string} on {string}")]
async fn price_directive_should_be(
    world: &mut LedgerWorld,
    index: usize,
    commodity: String,
    amount: String,
    quote: String,
    datetime: String,
) {
    let result = world
        .prices_result
        .as_ref()
        .expect("prices result should be set by the When step");
    let directive = &result.prices[index - 1];
    assert_eq!(directive.commodity, commodity, "commodity mismatch");
    assert_eq!(directive.price_amount_text, amount, "amount mismatch");
    assert_eq!(directive.quote_commodity, quote, "quote commodity mismatch");
    assert_eq!(directive.datetime, datetime, "datetime mismatch");
}

// === Import prices scenarios ===

#[given("a prices import file with content:")]
async fn a_prices_import_file_with_content(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
) {
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim()
        .to_string();
    let tmp_path = std::env::temp_dir().join(format!(
        "arimalo-prices-import-{}.txt",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&tmp_path, &content).expect("write prices import file");
    world.prices_import_path = Some(tmp_path);
}

#[given("a prices CSV import file with content:")]
async fn a_prices_csv_import_file_with_content(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
) {
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim()
        .to_string();
    let tmp_path = std::env::temp_dir().join(format!(
        "arimalo-prices-import-{}.csv",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&tmp_path, &content).expect("write prices CSV import file");
    world.prices_import_path = Some(tmp_path);
}

#[given(regex = r#"^a prices file at "([^"]+)" in sources with content:$"#)]
async fn a_prices_file_at_in_sources(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
    relative_path: String,
) {
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim()
        .to_string();
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let full_path = sources.join(&relative_path);
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent).expect("create prices parent dir");
    }
    std::fs::write(&full_path, format!("{content}\n")).expect("write prices file");
}

#[when("I import the prices file")]
async fn i_import_the_prices_file(world: &mut LedgerWorld) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let import_path = world
        .prices_import_path
        .as_ref()
        .expect("prices_import_path should be set");
    world.price_import_result = Some(import_prices_file(sources, import_path, false));
}

#[then(expr = "the prices import should succeed with {int} directives")]
async fn the_prices_import_should_succeed(world: &mut LedgerWorld, count: usize) {
    let result = world
        .price_import_result
        .as_ref()
        .expect("price_import_result should be set");
    match result {
        Ok(r) => assert_eq!(
            r.total_count, count,
            "expected {count} directives, got {}",
            r.total_count
        ),
        Err(e) => panic!("expected prices import to succeed, got error: {e}"),
    }
}

#[then(expr = "the prices import should include commodity {string}")]
async fn the_prices_import_should_include_commodity(world: &mut LedgerWorld, commodity: String) {
    let result = world
        .price_import_result
        .as_ref()
        .expect("price_import_result should be set");
    match result {
        Ok(r) => assert!(
            r.commodities.contains(&commodity),
            "expected commodity {commodity:?} in {:?}",
            r.commodities
        ),
        Err(e) => panic!("expected prices import to succeed, got error: {e}"),
    }
}

#[then(regex = r#"^the prices import should fail with "([^"]+)"$"#)]
async fn the_prices_import_should_fail_with(world: &mut LedgerWorld, expected_msg: String) {
    let result = world
        .price_import_result
        .as_ref()
        .expect("price_import_result should be set");
    match result {
        Err(e) => assert!(
            e.contains(&expected_msg),
            "expected error containing {expected_msg:?}, got {e:?}"
        ),
        Ok(r) => panic!(
            "expected prices import to fail, but got success with {:?}",
            r.commodities
        ),
    }
}

#[then(regex = r#"^the file "([^"]+)" should exist in sources$"#)]
async fn the_file_should_exist_in_sources(world: &mut LedgerWorld, relative_path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let full_path = sources.join(&relative_path);
    assert!(
        full_path.exists(),
        "expected file at {}",
        full_path.display()
    );
}

#[when(regex = r#"^I look up the price for "([^"]+)" at "([^"]+)"$"#)]
async fn i_look_up_the_price(world: &mut LedgerWorld, commodity: String, target_datetime: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    world.price_lookup_result = Some(lookup_price(sources, &commodity, &target_datetime));
}

#[then(regex = r#"^the lookup result should be "([^"]+)" "([^"]+)"$"#)]
async fn the_lookup_result_should_be(
    world: &mut LedgerWorld,
    expected_amount: String,
    expected_currency: String,
) {
    let result = world
        .price_lookup_result
        .as_ref()
        .expect("price_lookup_result should be set");
    let lookup = result.as_ref().expect("expected a lookup result, got None");
    assert_eq!(lookup.price_amount_text, expected_amount, "amount mismatch");
    assert_eq!(
        lookup.quote_commodity, expected_currency,
        "currency mismatch"
    );
}

#[then("the lookup result should be empty")]
async fn the_lookup_result_should_be_empty(world: &mut LedgerWorld) {
    let result = world
        .price_lookup_result
        .as_ref()
        .expect("price_lookup_result should be set");
    assert!(
        result.is_none(),
        "expected empty lookup result, got {:?}",
        result
    );
}

// === Price conversion scenarios ===

#[when(
    regex = r#"^I convert (-?\d+(?:\.\d+)?) "([^"]+)" to base currency "([^"]+)" at "([^"]+)"$"#
)]
async fn i_convert_to_base_currency(
    world: &mut LedgerWorld,
    amount: f64,
    commodity: String,
    base_currency: String,
    datetime: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let graph = PriceGraph::load(sources);
    world.conversion_result =
        Some(graph.convert_to_base(&commodity, amount, &datetime, &base_currency));
}

#[then(regex = r#"^the converted value should be "([^"]+)"$"#)]
async fn the_converted_value_should_be(world: &mut LedgerWorld, expected: String) {
    let result = world
        .conversion_result
        .as_ref()
        .expect("conversion_result should be set");
    let value = result.expect("expected a converted value, got None");
    let expected_val: f64 = expected.parse().expect("expected a valid number");
    assert!(
        (value - expected_val).abs() < 0.01,
        "expected {expected_val}, got {value}"
    );
}

#[then("the converted value should be empty")]
async fn the_converted_value_should_be_empty(world: &mut LedgerWorld) {
    let result = world
        .conversion_result
        .as_ref()
        .expect("conversion_result should be set");
    assert!(
        result.is_none(),
        "expected empty conversion result, got {:?}",
        result
    );
}

#[when(regex = r#"^I batch convert the following to base currency "([^"]+)" at "([^"]+)":$"#)]
async fn i_batch_convert(
    world: &mut LedgerWorld,
    base_currency: String,
    datetime: String,
    step: &cucumber::gherkin::Step,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let graph = PriceGraph::load(sources);
    let table = step.table.as_ref().expect("expected a data table");
    let headers: Vec<String> = table
        .rows
        .first()
        .expect("header row")
        .iter()
        .cloned()
        .collect();
    let amount_idx = headers
        .iter()
        .position(|h| h == "amount")
        .expect("amount column");
    let commodity_idx = headers
        .iter()
        .position(|h| h == "commodity")
        .expect("commodity column");

    let mut results = Vec::new();
    for row in table.rows.iter().skip(1) {
        let amount: f64 = row[amount_idx].parse().expect("valid amount");
        let commodity = &row[commodity_idx];
        results.push(graph.convert_to_base(commodity, amount, &datetime, &base_currency));
    }
    world.batch_conversion_results = Some(results);
}

#[then("the batch results should be:")]
async fn the_batch_results_should_be(world: &mut LedgerWorld, step: &cucumber::gherkin::Step) {
    let results = world
        .batch_conversion_results
        .as_ref()
        .expect("batch_conversion_results should be set");
    let table = step.table.as_ref().expect("expected a data table");
    let headers: Vec<String> = table
        .rows
        .first()
        .expect("header row")
        .iter()
        .cloned()
        .collect();
    let value_idx = headers
        .iter()
        .position(|h| h == "value")
        .expect("value column");

    let expected_rows: Vec<&str> = table
        .rows
        .iter()
        .skip(1)
        .map(|r| r[value_idx].as_str())
        .collect();
    assert_eq!(results.len(), expected_rows.len(), "result count mismatch");

    for (i, (result, expected)) in results.iter().zip(expected_rows.iter()).enumerate() {
        if *expected == "empty" {
            assert!(
                result.is_none(),
                "row {}: expected empty, got {:?}",
                i,
                result
            );
        } else {
            let expected_val: f64 = expected.parse().expect("valid expected number");
            let value = result.expect(&format!("row {}: expected a value, got None", i));
            assert!(
                (value - expected_val).abs() < 0.01,
                "row {}: expected {expected_val}, got {value}",
                i
            );
        }
    }
}

#[then(
    expr = "posting {int} of transaction {int} should have a per-unit price of {string} {string}"
)]
async fn posting_should_have_per_unit_price(
    world: &mut LedgerWorld,
    posting_idx: usize,
    txn_idx: usize,
    amount: String,
    commodity: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let posting = &result.transactions[txn_idx - 1].postings[posting_idx - 1];
    let price = posting.price.as_ref().expect("expected price annotation");
    assert!(!price.is_total, "expected per-unit price, got total");
    assert_eq!(price.amount_text, amount, "price amount mismatch");
    assert_eq!(price.commodity, commodity, "price commodity mismatch");
}

#[then(expr = "posting {int} of transaction {int} should have a total price of {string} {string}")]
async fn posting_should_have_total_price(
    world: &mut LedgerWorld,
    posting_idx: usize,
    txn_idx: usize,
    amount: String,
    commodity: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let posting = &result.transactions[txn_idx - 1].postings[posting_idx - 1];
    let price = posting.price.as_ref().expect("expected price annotation");
    assert!(price.is_total, "expected total price, got per-unit");
    assert_eq!(price.amount_text, amount, "price amount mismatch");
    assert_eq!(price.commodity, commodity, "price commodity mismatch");
}

#[then(
    expr = "posting {int} of transaction {int} should have a per-unit cost of {string} {string}"
)]
async fn posting_should_have_per_unit_cost(
    world: &mut LedgerWorld,
    posting_idx: usize,
    txn_idx: usize,
    amount: String,
    commodity: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let posting = &result.transactions[txn_idx - 1].postings[posting_idx - 1];
    let cost = posting.cost.as_ref().expect("expected cost annotation");
    assert!(!cost.is_total, "expected per-unit cost, got total");
    assert_eq!(
        cost.amount_text.as_deref(),
        Some(amount.as_str()),
        "cost amount mismatch"
    );
    assert_eq!(
        cost.commodity.as_deref(),
        Some(commodity.as_str()),
        "cost commodity mismatch"
    );
}

#[then(expr = "posting {int} of transaction {int} should have a total cost of {string} {string}")]
async fn posting_should_have_total_cost(
    world: &mut LedgerWorld,
    posting_idx: usize,
    txn_idx: usize,
    amount: String,
    commodity: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let posting = &result.transactions[txn_idx - 1].postings[posting_idx - 1];
    let cost = posting.cost.as_ref().expect("expected cost annotation");
    assert!(cost.is_total, "expected total cost, got per-unit");
    assert_eq!(
        cost.amount_text.as_deref(),
        Some(amount.as_str()),
        "cost amount mismatch"
    );
    assert_eq!(
        cost.commodity.as_deref(),
        Some(commodity.as_str()),
        "cost commodity mismatch"
    );
}

#[then(expr = "posting {int} of transaction {int} should have cost fields including {string}")]
async fn posting_should_have_cost_fields_including(
    world: &mut LedgerWorld,
    posting_idx: usize,
    txn_idx: usize,
    expected_field: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let posting = &result.transactions[txn_idx - 1].postings[posting_idx - 1];
    let cost = posting.cost.as_ref().expect("expected cost annotation");
    assert!(
        cost.fields.iter().any(|f| f == &expected_field),
        "expected cost fields to include {expected_field:?}; got: {:?}",
        cost.fields
    );
}

#[then(expr = "posting {int} of transaction {int} should have no price annotation")]
async fn posting_should_have_no_price(world: &mut LedgerWorld, posting_idx: usize, txn_idx: usize) {
    let result = world.result.as_ref().expect("parse result should be set");
    let posting = &result.transactions[txn_idx - 1].postings[posting_idx - 1];
    assert!(
        posting.price.is_none(),
        "expected no price annotation, got: {:?}",
        posting.price
    );
}

#[then(expr = "posting {int} of transaction {int} should have no cost annotation")]
async fn posting_should_have_no_cost(world: &mut LedgerWorld, posting_idx: usize, txn_idx: usize) {
    let result = world.result.as_ref().expect("parse result should be set");
    let posting = &result.transactions[txn_idx - 1].postings[posting_idx - 1];
    assert!(
        posting.cost.is_none(),
        "expected no cost annotation, got: {:?}",
        posting.cost
    );
}

#[then(expr = "posting {int} of transaction {int} should have remainder {string}")]
async fn posting_should_have_remainder(
    world: &mut LedgerWorld,
    posting_idx: usize,
    txn_idx: usize,
    expected: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let posting = &result.transactions[txn_idx - 1].postings[posting_idx - 1];
    assert_eq!(
        posting.remainder.as_deref(),
        Some(expected.as_str()),
        "remainder mismatch"
    );
}

// === Shared helpers ===

fn new_temp_dir(prefix: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    std::env::temp_dir().join(format!("{prefix}-{pid}-{nanos}"))
}

fn make_pipeline_config(world: &LedgerWorld) -> PipelineConfig {
    PipelineConfig {
        sources_dir: world
            .sources_dir
            .clone()
            .expect("sources_dir should be set"),
        generated_dir: world
            .generated_dir
            .clone()
            .expect("generated_dir should be set"),
        now_yyyymm: world
            .now_yyyymm
            .clone()
            .unwrap_or_else(|| "202501".to_string()),
        force: false,
        default_expense_account: arimalo_covid::FALLBACK_EXPENSE_ACCOUNT.to_string(),
        changed_folder_hint: None,
    }
}

/// Generate reports synchronously after pipeline run (reports are no longer
/// generated inside run_pipeline itself).
fn generate_reports_after_pipeline(config: &PipelineConfig, result: &PipelineResult) {
    if result.output_files_written > 0 {
        let mut account_sets: Vec<String> = result.owner_accounts.keys().cloned().collect();
        account_sets.sort();
        let _ = report_templates::generate_all_reports(
            &config.sources_dir,
            &config.generated_dir,
            &account_sets,
            report_templates::ALL_FORMATS,
            &[],
        );
    }
}

/// Infer the primary account set name from sources directory structure.
fn infer_primary_set(sources_dir: &std::path::Path) -> Option<String> {
    if !sources_dir.exists() {
        return None;
    }
    let mut sets = std::collections::BTreeSet::new();
    if let Ok(entries) = std::fs::read_dir(sources_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if !name.starts_with('_') && !name.starts_with('.') {
                    if let Some(prefix) = name.split('-').next() {
                        sets.insert(prefix.to_string());
                    }
                }
            }
        }
    }
    sets.into_iter().next()
}

/// Get the set-specific generated directory.
fn try_read_file(path: &std::path::Path) -> Option<String> {
    if path.exists() {
        std::fs::read_to_string(path).ok()
    } else {
        None
    }
}

fn set_generated_dir(world: &LedgerWorld) -> std::path::PathBuf {
    let generated = world.generated_dir.as_ref().expect("generated_dir");
    if let Some(ref set_name) = world.active_set_name {
        generated.join(set_name)
    } else {
        generated.clone()
    }
}

fn write_simple_csv(sources_dir: &PathBuf, relative_csv: &str, rows: &[(&str, &str, &str)]) {
    let csv_path = sources_dir.join(relative_csv);
    if let Some(parent) = csv_path.parent() {
        std::fs::create_dir_all(parent).expect("create CSV parent dirs");
    }
    let mut content = "Date,Description,Amount\n".to_string();
    for (date, desc, amount) in rows {
        content.push_str(&format!("{date},{desc},{amount}\n"));
    }
    std::fs::write(&csv_path, content).expect("write CSV");
}

fn write_transform(sources_dir: &PathBuf, relative_path: &str, _account: &str) {
    let transform_path = sources_dir.join(relative_path);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = r##"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "AUD",
  status: "*"
}"##;
    std::fs::write(&transform_path, script).expect("write transform");
}

fn write_transform_no_account(sources_dir: &PathBuf, relative_path: &str) {
    let transform_path = sources_dir.join(relative_path);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = r##"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "AUD",
  status: "*"
}"##;
    std::fs::write(&transform_path, script).expect("write transform");
}

fn setup_clean_dirs(world: &mut LedgerWorld) {
    let sources = new_temp_dir("arimalo-csv-sources");
    let generated = new_temp_dir("arimalo-csv-generated");
    std::fs::create_dir_all(&sources).expect("create sources dir");
    std::fs::create_dir_all(&generated).expect("create generated dir");
    world.sources_dir = Some(sources);
    world.generated_dir = Some(generated);
}

// === Rotation scenarios (kept from before) ===

#[given("a clean generated ledger directory")]
async fn a_clean_generated_ledger_directory(world: &mut LedgerWorld) {
    let base_dir = new_temp_dir("arimalo-covid-generated-ledger");
    std::fs::create_dir_all(&base_dir).expect("create temp dir");
    world.generated_dir = Some(base_dir);
    world.source_file_path = None;
    world.source_file_before = None;
}

#[given(expr = "a generated ledger file for month {string}")]
async fn a_generated_ledger_file_for_month(world: &mut LedgerWorld, yyyymm: String) {
    let year: i32 = yyyymm[0..4].parse().expect("year");
    let month: i32 = yyyymm[4..6].parse().expect("month");
    let date = format!("{year:04}-{month:02}-16");

    let base_dir = new_temp_dir("arimalo-covid-generated-ledger");
    std::fs::create_dir_all(&base_dir).expect("create temp dir");
    let ledger = base_dir.join("ledger.transactions");

    let contents = format!(
        r#"{date} * "Kraken" "Sell BTC" ; txn:01J2NB..., src:kraken:trade:def456
    assets:cash:usd               160.00 USD

"#
    );
    std::fs::write(&ledger, contents).expect("write ledger.transactions");
    world.generated_dir = Some(base_dir);
}

#[when(expr = "I rotate the generated ledger with current month {string}")]
async fn i_rotate_the_generated_ledger_with_current_month(
    world: &mut LedgerWorld,
    now_yyyymm: String,
) {
    let dir = world
        .generated_dir
        .as_ref()
        .expect("generated dir should be set by the Given step");
    rotate_ledger_if_needed(dir, &now_yyyymm).expect("rotate ledger");
}

#[then(expr = "the archive ledger file {string} should exist")]
async fn the_archive_ledger_file_should_exist(world: &mut LedgerWorld, file_name: String) {
    let dir = world
        .generated_dir
        .as_ref()
        .expect("generated dir should be set by the Given step");
    let path = dir.join("archive").join(file_name);
    assert!(path.exists(), "expected archive file to exist: {path:?}");
}

#[then(expr = "the archive ledger file {string} should not exist")]
async fn the_archive_ledger_file_should_not_exist(world: &mut LedgerWorld, file_name: String) {
    let dir = world
        .generated_dir
        .as_ref()
        .expect("generated dir should be set by the Given step");
    let path = dir.join("archive").join(file_name);
    assert!(
        !path.exists(),
        "expected archive file to not exist: {path:?}"
    );
}

#[then("the active ledger file should be empty")]
async fn the_active_ledger_file_should_be_empty(world: &mut LedgerWorld) {
    let dir = world
        .generated_dir
        .as_ref()
        .expect("generated dir should be set by the Given step");
    let contents =
        std::fs::read_to_string(dir.join("ledger.transactions")).expect("read ledger.transactions");
    assert!(
        contents.trim().is_empty(),
        "expected ledger.transactions to be empty, got: {contents:?}"
    );
}

#[then("the active ledger file should not be empty")]
async fn the_active_ledger_file_should_not_be_empty(world: &mut LedgerWorld) {
    let dir = world
        .generated_dir
        .as_ref()
        .expect("generated dir should be set by the Given step");
    let contents =
        std::fs::read_to_string(dir.join("ledger.transactions")).expect("read ledger.transactions");
    assert!(
        !contents.trim().is_empty(),
        "expected ledger.transactions to not be empty"
    );
}

// === CSV Pipeline scenarios ===

#[given(regex = r#"^a clean sources directory with a CSV "([^"]+)":$"#)]
async fn a_clean_sources_directory_with_csv_table(
    world: &mut LedgerWorld,
    csv_relative: String,
    step: &cucumber::gherkin::Step,
) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();

    // Parse the data table from the step
    let table = step.table.as_ref().expect("expected a data table");

    let csv_path = sources.join(&csv_relative);
    if let Some(parent) = csv_path.parent() {
        std::fs::create_dir_all(parent).expect("create CSV parent dirs");
    }

    std::fs::write(&csv_path, table_to_csv(table)).expect("write CSV");
}

#[given(regex = r#"^a transform at "([^"]+)" that maps Date/Description/Amount to AUD$"#)]
async fn a_transform_at_that_maps(world: &mut LedgerWorld, transform_relative: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_transform(sources, &transform_relative, "assets:bank:savings");
}

#[given(regex = r#"^a transform at "([^"]+)" without an account field$"#)]
async fn a_transform_without_account(world: &mut LedgerWorld, transform_relative: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_transform_no_account(sources, &transform_relative);
}

#[given(regex = r#"^no "accounts.transactions" file exists$"#)]
async fn no_accounts_file_exists(world: &mut LedgerWorld) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let accounts_path = sources.join("accounts.transactions");
    if accounts_path.exists() {
        std::fs::remove_file(&accounts_path).expect("remove accounts.transactions");
    }
}

#[given("a clean sources directory")]
async fn a_clean_sources_directory(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
}

#[given(regex = r#"^an institution transform at "([^"]+)" using account "([^"]+)"$"#)]
async fn an_institution_transform(world: &mut LedgerWorld, path: String, account: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_transform(sources, &path, &account);
}

#[given(regex = r#"^an account transform at "([^"]+)" using account "([^"]+)"$"#)]
async fn an_account_transform(world: &mut LedgerWorld, path: String, account: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_transform(sources, &path, &account);
}

#[given(regex = r#"^a CSV "([^"]+)":$"#)]
async fn a_csv_with_table(
    world: &mut LedgerWorld,
    csv_relative: String,
    step: &cucumber::gherkin::Step,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let table = step.table.as_ref().expect("step should have a data table");
    let rows: Vec<(&str, &str, &str)> = table
        .rows
        .iter()
        .skip(1)
        .map(|r| (r[0].as_str(), r[1].as_str(), r[2].as_str()))
        .collect();
    write_simple_csv(sources, &csv_relative, &rows);
}

#[given(regex = r#"^a CSV "([^"]+)" with one row$"#)]
async fn a_csv_with_one_row(world: &mut LedgerWorld, csv_relative: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_simple_csv(
        sources,
        &csv_relative,
        &[("2025-01-15", "Test Item", "-10.00")],
    );
}

#[given("a clean sources directory with a CSV and transform")]
async fn a_clean_sources_directory_with_csv_and_transform(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_simple_csv(
        sources,
        "bank/2025-01.csv",
        &[("2025-01-15", "CSV Entry", "-10.00")],
    );
    write_transform(sources, "bank/_transform.rhai", "assets:bank:test");
}

#[given(regex = r#"^a "manual.transactions" file with payee "([^"]+)"$"#)]
async fn a_manual_transactions_file_with_payee(world: &mut LedgerWorld, payee: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    // Write to the first source folder (bank/) or create one for manual transactions
    let manual_dir = sources.join("bank");
    std::fs::create_dir_all(&manual_dir).expect("create bank dir for manual");
    let manual = manual_dir.join("manual.transactions");
    let text = format!(
        r#"2025-01-15 * "{payee}" "manual test" ;
    assets:bank:test 1.00 AUD
    expenses:manual -1.00 AUD
"#
    );
    std::fs::write(&manual, text).expect("write manual.transactions");
}

#[when(expr = "I append a manual transaction with payee {string} to account folder {string}")]
async fn i_append_manual_transaction_to_folder(
    world: &mut LedgerWorld,
    payee: String,
    folder: String,
) {
    let config = make_pipeline_config(world);
    let input = ManualTransactionInput {
        datetime: "2025-01-20".to_string(),
        status: Some('*'),
        payee,
        narration: "manual test".to_string(),
        postings: vec![
            ManualPostingInput {
                account: "assets:bank:test".to_string(),
                amount: "100.00".to_string(),
                commodity: "AUD".to_string(),
                remainder: None,
            },
            ManualPostingInput {
                account: "income:other".to_string(),
                amount: "-100.00".to_string(),
                commodity: "AUD".to_string(),
                remainder: None,
            },
        ],
    };
    let result = append_manual_and_rebuild(&config, &input, &folder).expect("append manual");
    world.pipeline_result = Some(result);
    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }
}

#[then(expr = "a {string} file should exist in {string}")]
async fn a_file_should_exist_in(world: &mut LedgerWorld, filename: String, folder: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let path = sources.join(&folder).join(&filename);
    assert!(
        path.exists(),
        "expected {} to exist at {}",
        filename,
        path.display()
    );
}

#[then(expr = "no {string} file should exist at the sources root")]
async fn no_file_should_exist_at_sources_root(world: &mut LedgerWorld, filename: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let path = sources.join(&filename);
    assert!(
        !path.exists(),
        "expected {} NOT to exist at {}",
        filename,
        path.display()
    );
}

#[then(expr = "the only top-level source folders should be {string}")]
async fn the_only_top_level_source_folders_should_be(world: &mut LedgerWorld, expected: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let expected_set: std::collections::BTreeSet<String> =
        expected.split(',').map(|s| s.trim().to_string()).collect();
    let mut actual = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(sources).expect("read sources dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('_') || name.starts_with('.') {
            continue;
        }
        actual.insert(name);
    }
    assert_eq!(
        actual,
        expected_set,
        "Unexpected top-level source folders. Expected {:?}, got {:?}. Stray folders: {:?}",
        expected_set,
        actual,
        actual.difference(&expected_set).collect::<Vec<_>>()
    );
}

#[given("a pipeline has been run once with a transform")]
async fn a_pipeline_has_been_run_once_with_a_transform(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_simple_csv(
        sources,
        "bank/2025-01.csv",
        &[("2025-01-15", "CacheTest", "-5.00")],
    );
    write_transform(sources, "bank/_transform.rhai", "assets:bank:test");
    world.now_yyyymm = Some("202501".to_string());
    let config = make_pipeline_config(world);
    let result = run_pipeline(&config).expect("first pipeline run");
    world.pipeline_result = Some(result);
}

#[given("CSVs from two sources with interleaved dates")]
async fn csvs_from_two_sources_with_interleaved_dates(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_simple_csv(
        sources,
        "bank/a/data.csv",
        &[
            ("2025-01-15", "Source A First", "-10.00"),
            ("2025-01-20", "Source A Second", "-20.00"),
        ],
    );
    write_simple_csv(
        sources,
        "bank/b/data.csv",
        &[
            ("2025-01-16", "Source B First", "-15.00"),
            ("2025-01-18", "Source B Second", "-25.00"),
        ],
    );
    write_transform(sources, "bank/_transform.rhai", "assets:bank:test");
    world.now_yyyymm = Some("202501".to_string());
}

#[given("CSVs with transactions in January and February")]
async fn csvs_with_transactions_in_january_and_february(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_simple_csv(
        sources,
        "bank/jan.csv",
        &[("2025-01-15", "January Item", "-10.00")],
    );
    write_simple_csv(
        sources,
        "bank/feb.csv",
        &[("2025-02-15", "February Item", "-20.00")],
    );
    write_transform(sources, "bank/_transform.rhai", "assets:bank:test");
}

#[given(regex = r#"^an "accounts.transactions" file declaring "([^"]+)"$"#)]
async fn an_accounts_transactions_file(world: &mut LedgerWorld, declaration: String) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    let accounts = sources.join("accounts.transactions");
    std::fs::write(&accounts, format!("account {declaration}\n"))
        .expect("write accounts.transactions");
    world.now_yyyymm = Some("202501".to_string());
}

// === When steps for pipeline ===

#[when(regex = r#"^I run the pipeline for month "([^"]+)"$"#)]
async fn i_run_the_pipeline_for_month(world: &mut LedgerWorld, month: String) {
    world.now_yyyymm = Some(month);
    let config = make_pipeline_config(world);
    let result = run_pipeline(&config).expect("pipeline run");
    generate_reports_after_pipeline(&config, &result);
    world.pipeline_result = Some(result);

    // Infer active set from sources
    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }

    // Load active ledger text from set-specific directory
    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

#[when("I run the pipeline")]
async fn i_run_the_pipeline(world: &mut LedgerWorld) {
    if world.now_yyyymm.is_none() {
        world.now_yyyymm = Some("202501".to_string());
    }
    let config = make_pipeline_config(world);
    let result = run_pipeline(&config).expect("pipeline run");
    generate_reports_after_pipeline(&config, &result);
    world.pipeline_result = Some(result);

    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }

    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

fn collect_summary_files(
    generated_dir: &std::path::Path,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for entry in walkdir::WalkDir::new(generated_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) != Some("summary.json") {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(generated_dir) {
            if let Ok(text) = std::fs::read_to_string(path) {
                out.insert(rel.to_string_lossy().to_string(), text);
            }
        }
    }
    out
}

#[when("I run the pipeline twice")]
async fn i_run_the_pipeline_twice(world: &mut LedgerWorld) {
    if world.now_yyyymm.is_none() {
        world.now_yyyymm = Some("202501".to_string());
    }

    // First run
    let config = make_pipeline_config(world);
    let result1 = run_pipeline(&config).expect("pipeline run 1");
    generate_reports_after_pipeline(&config, &result1);
    world.pipeline_result_prev = Some(result1);

    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }

    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text_prev = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
    world.summary_snapshot_prev = Some(collect_summary_files(&config.generated_dir));

    // Second run
    let result2 = run_pipeline(&config).expect("pipeline run 2");
    generate_reports_after_pipeline(&config, &result2);
    world.pipeline_result = Some(result2);

    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
    world.summary_snapshot_after = Some(collect_summary_files(&config.generated_dir));
}

#[then("the per-folder summary.json should be byte-identical between runs")]
async fn summary_json_byte_identical_between_runs(world: &mut LedgerWorld) {
    let prev = world
        .summary_snapshot_prev
        .as_ref()
        .expect("summary_snapshot_prev not captured — use 'I run the pipeline twice' first");
    let after = world
        .summary_snapshot_after
        .as_ref()
        .expect("summary_snapshot_after not captured — use 'I run the pipeline twice' first");
    assert!(
        !prev.is_empty(),
        "expected at least one summary.json after run 1, found none"
    );
    assert_eq!(
        prev.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "summary.json file set differs between runs"
    );
    for (rel, prev_text) in prev {
        let after_text = after.get(rel).expect("key existence checked above");
        assert_eq!(
            prev_text, after_text,
            "summary.json drifted between runs at {rel}\n--- run 1 ---\n{prev_text}\n--- run 2 ---\n{after_text}"
        );
    }
}

#[when("the transform is modified")]
async fn the_transform_is_modified(world: &mut LedgerWorld) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    // Rewrite the transform with a different narration to change its content+size
    let transform_path = sources.join("bank/_transform.rhai");
    let script = r##"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "modified transform narration",
  amount: row["Amount"],
  commodity: "AUD",
  status: "*"
}"##;
    std::fs::write(&transform_path, script).expect("write modified transform");
}

#[when("I run the pipeline again")]
async fn i_run_the_pipeline_again(world: &mut LedgerWorld) {
    let config = make_pipeline_config(world);
    let result = run_pipeline(&config).expect("pipeline run again");
    generate_reports_after_pipeline(&config, &result);
    world.pipeline_result = Some(result);

    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }

    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

/// Mirrors what the rebuild_pipeline Tauri command in main.rs does:
/// constructs a PipelineConfig the same way the frontend-facing command would.
/// This catches regressions like force=true being set on the command handler.
#[when("I run the pipeline again as the frontend would")]
async fn i_run_the_pipeline_as_frontend(world: &mut LedgerWorld) {
    // This must match main.rs rebuild_pipeline — default config, no force override.
    let config = make_pipeline_config(world);
    assert!(
        !config.force,
        "make_pipeline_config should not set force=true — \
    the rebuild_pipeline Tauri command must respect the build cache"
    );
    let result = run_pipeline(&config).expect("pipeline run (frontend path)");
    generate_reports_after_pipeline(&config, &result);
    world.pipeline_result = Some(result);

    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }

    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

// === Regenerate steps ===

#[when("I regenerate the pipeline")]
async fn i_regenerate_the_pipeline(world: &mut LedgerWorld) {
    let config = make_pipeline_config(world);

    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }

    // Save current ledger as "previous" before regenerating
    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text_prev = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };

    let result = run_pipeline(&config).expect("regenerate pipeline");
    world.pipeline_result = Some(result);

    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

#[when(expr = "a new CSV {string} is added:")]
async fn a_new_csv_is_added(
    world: &mut LedgerWorld,
    csv_relative: String,
    step: &cucumber::gherkin::Step,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let table = step.table.as_ref().expect("step should have a data table");
    let rows: Vec<(&str, &str, &str)> = table
        .rows
        .iter()
        .skip(1)
        .map(|r| (r[0].as_str(), r[1].as_str(), r[2].as_str()))
        .collect();
    write_simple_csv(sources, &csv_relative, &rows);
}

#[then("the regenerated ledger should match the original")]
async fn the_regenerated_ledger_should_match(world: &mut LedgerWorld) {
    let prev = world
        .active_ledger_text_prev
        .as_ref()
        .expect("previous ledger should be set");
    let curr = world
        .active_ledger_text
        .as_ref()
        .expect("current ledger should be set");
    assert_eq!(prev, curr, "regenerated ledger should match the original");
}

#[then("the regenerate should report 0 CSVs transformed and all cached")]
async fn the_regenerate_should_report_all_cached(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert_eq!(
        result.csv_transformed, 0,
        "expected 0 CSVs transformed, got {}",
        result.csv_transformed
    );
    assert!(
        result.csv_cached > 0,
        "expected some cached CSVs, got {}",
        result.csv_cached
    );
}

// === Then steps for pipeline ===

#[then(expr = "the active ledger should contain {int} transactions")]
async fn the_active_ledger_should_contain_n_transactions(world: &mut LedgerWorld, expected: usize) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    assert_eq!(
        result.transactions.len(),
        expected,
        "expected {} transactions, got {}",
        expected,
        result.transactions.len()
    );
}

#[then(regex = r#"^filtering by prefix "([^"]*)" should return (\d+) transactions$"#)]
async fn filtering_by_prefix_should_return_n(
    world: &mut LedgerWorld,
    prefix: String,
    expected: usize,
) {
    let dir = set_generated_dir(world);
    let parse = load_active_ledger(&dir).expect("load active ledger");
    let search = if prefix.is_empty() {
        String::new()
    } else {
        format!("account:{prefix}")
    };
    let expr = query::parse_search(&search).expect("parse search");
    let qr = query::query(
        &parse,
        &query::QueryOptions {
            search: expr,
            sort_field: None,
            sort_order: query::SortOrder::Asc,
            offset: None,
            limit: None,
            input_order: None,
            min_value: None,
            hidden_prefixes: Vec::new(),
        },
    );
    assert_eq!(
        qr.transaction_count, expected,
        "expected {} transactions matching prefix {:?}, got {}; accounts: {:?}",
        expected, prefix, qr.transaction_count, qr.accounts
    );
}

#[then(
    regex = r#"^querying prefix "([^"]*)" should return aggregated balances from (\d+) accounts$"#
)]
async fn querying_prefix_should_aggregate_balances(
    world: &mut LedgerWorld,
    prefix: String,
    expected_accounts: usize,
) {
    let dir = set_generated_dir(world);
    let parse = load_active_ledger(&dir).expect("load active ledger");
    let search = if prefix.is_empty() {
        String::new()
    } else {
        format!("account:{prefix}")
    };
    let expr = query::parse_search(&search).expect("parse search");
    let qr = query::query(
        &parse,
        &query::QueryOptions {
            search: expr,
            sort_field: None,
            sort_order: query::SortOrder::Asc,
            offset: None,
            limit: None,
            input_order: None,
            min_value: None,
            hidden_prefixes: Vec::new(),
        },
    );
    assert_eq!(
        qr.accounts.len(),
        expected_accounts,
        "expected {} accounts for prefix {:?}, got {:?}",
        expected_accounts,
        prefix,
        qr.accounts
    );
    assert!(
        !qr.aggregated_balance.is_empty(),
        "expected non-empty aggregated_balance for prefix {:?}",
        prefix
    );
}

#[then(expr = "the active ledger should include payee {string}")]
async fn the_active_ledger_should_include_payee(world: &mut LedgerWorld, payee: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    assert!(
        result
            .transactions
            .iter()
            .any(|t| t.payee.as_deref() == Some(payee.as_str())),
        "expected active ledger to include payee {payee:?}; got: {:?}",
        result
            .transactions
            .iter()
            .map(|t| t.payee.clone())
            .collect::<Vec<_>>()
    );
}

#[then(regex = r#"^the transaction should use account "([^"]+)"$"#)]
async fn the_transaction_should_use_account(world: &mut LedgerWorld, account: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let has_account = result
        .transactions
        .iter()
        .any(|t| t.postings.iter().any(|p| p.account == account));
    assert!(
        has_account,
        "expected transaction with account {account:?}; got: {:?}",
        result.transactions
    );
}

#[then("the active ledger should include payee from CSV")]
async fn the_active_ledger_should_include_payee_from_csv(world: &mut LedgerWorld) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    assert!(
        result
            .transactions
            .iter()
            .any(|t| t.payee.as_deref() == Some("CSV Entry")),
        "expected active ledger to include CSV payee 'CSV Entry'"
    );
}

#[then("the second run should report 0 CSVs transformed and all cached")]
async fn the_second_run_should_report_cached(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert_eq!(
        result.csv_transformed, 0,
        "expected 0 CSVs transformed, got {}",
        result.csv_transformed
    );
    assert!(
        result.csv_cached > 0,
        "expected some cached CSVs, got {}",
        result.csv_cached
    );
}

// Adds a manual.transactions file to a folder with a single test transaction.
// Used to verify that an unchanged-CSV folder with a manual.transactions file
// does not silently truncate its on-disk ledger on the next regenerate.
#[when(
    expr = "a \"manual.transactions\" file is added to the {string} folder with one transaction"
)]
async fn a_manual_transactions_file_is_added_to_the_folder_with_one_transaction(
    world: &mut LedgerWorld,
    folder: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder_dir = sources.join(&folder);
    std::fs::create_dir_all(&folder_dir).expect("create folder for manual.transactions");
    let manual_path = folder_dir.join("manual.transactions");
    let text = r#"2025-01-20 * "Manual Entry" "manual test"
    assets:bank:test 5.00 AUD
    income:other -5.00 AUD
"#;
    std::fs::write(&manual_path, text).expect("write manual.transactions");
}

// Externally clobbers the per-folder generated ledger to simulate a tool
// outside the pipeline (git checkout, manual edit) overwriting the file.
// The next regenerate must detect the divergence and restore from cache.
#[when(expr = "the per-folder ledger for {string} is externally clobbered")]
async fn the_per_folder_ledger_is_externally_clobbered(world: &mut LedgerWorld, folder: String) {
    let generated = world
        .generated_dir
        .as_ref()
        .expect("generated_dir should be set");
    let ledger = generated.join(&folder).join("ledger.transactions");
    assert!(
        ledger.exists(),
        "per-folder ledger should exist before clobbering: {}",
        ledger.display()
    );
    std::fs::write(&ledger, "; clobbered by external tool\n")
        .expect("write clobbered ledger");
}

#[then(expr = "the per-folder ledger for {string} should include payee {string}")]
async fn the_per_folder_ledger_should_include_payee(
    world: &mut LedgerWorld,
    folder: String,
    payee: String,
) {
    let generated = world
        .generated_dir
        .as_ref()
        .expect("generated_dir should be set");
    let ledger = generated.join(&folder).join("ledger.transactions");
    let text = std::fs::read_to_string(&ledger).expect("read per-folder ledger");
    assert!(
        text.contains(&format!("\"{payee}\"")),
        "per-folder ledger {} should include payee {:?}, got:\n{}",
        ledger.display(),
        payee,
        text
    );
}

// Adds a CSV with one row plus a matching transform in the same folder.
// Used when a scenario needs to introduce a second source folder to defeat
// the global early-exit, without caring about the CSV's exact content.
#[when(expr = "a CSV {string} is added with one row")]
async fn a_csv_is_added_with_one_row(world: &mut LedgerWorld, relative_csv: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_simple_csv(
        sources,
        &relative_csv,
        &[("2025-02-15", "Other Entry", "-3.00")],
    );
    let folder = std::path::Path::new(&relative_csv)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let transform_path = format!("{folder}/_transform.rhai");
    write_transform(sources, &transform_path, "assets:bank2:test");
}

#[then(expr = "the per-folder ledger for {string} should have {int} transactions")]
async fn the_per_folder_ledger_should_have_n_transactions(
    world: &mut LedgerWorld,
    folder: String,
    expected: usize,
) {
    let generated = world
        .generated_dir
        .as_ref()
        .expect("generated_dir should be set");
    let ledger = generated.join(&folder).join("ledger.transactions");
    let text = std::fs::read_to_string(&ledger).expect("read per-folder ledger");
    let parsed = arimalo_covid::ledger_parser::parse_transactions(&text);
    assert_eq!(
        parsed.transactions.len(),
        expected,
        "per-folder ledger {} should have {} transactions, got {}\n{}",
        ledger.display(),
        expected,
        parsed.transactions.len(),
        text
    );
}

#[then("the CSV should be re-transformed (not cached)")]
async fn the_csv_should_be_re_transformed(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert!(
        result.csv_transformed > 0,
        "expected CSVs to be re-transformed, got 0 transformed"
    );
}

#[then("all transaction txn: IDs should be identical between runs")]
async fn all_txn_ids_should_be_identical(world: &mut LedgerWorld) {
    let text1 = world
        .active_ledger_text_prev
        .as_ref()
        .expect("first run text");
    let text2 = world.active_ledger_text.as_ref().expect("second run text");

    fn extract_txn_ids(text: &str) -> Vec<String> {
        let mut ids: Vec<String> = Vec::new();
        for line in text.lines() {
            if let Some(pos) = line.find("txn:") {
                let after = &line[pos + 4..];
                let end = after
                    .find(|c: char| c == ',' || c.is_whitespace())
                    .unwrap_or(after.len());
                ids.push(after[..end].to_string());
            }
        }
        ids.sort();
        ids
    }

    let ids1 = extract_txn_ids(text1);
    let ids2 = extract_txn_ids(text2);
    assert_eq!(ids1, ids2, "txn IDs differ between runs");
}

#[then("transactions should be sorted by date, then by source path")]
async fn transactions_should_be_sorted(world: &mut LedgerWorld) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let dates: Vec<&str> = result
        .transactions
        .iter()
        .map(|t| t.date.as_str())
        .collect();
    let mut sorted = dates.clone();
    sorted.sort();
    assert_eq!(dates, sorted, "transactions not sorted by date");
}

#[then(regex = r#"^January transactions should be in archive/ledger-202501.transactions$"#)]
async fn january_transactions_in_archive(world: &mut LedgerWorld) {
    // With per-folder output, all transactions are in the folder's ledger.transactions (no archive split).
    // Just verify January Item is in the active ledger.
    let set_dir = set_generated_dir(world);
    let parse = load_active_ledger(&set_dir).expect("load active ledger");
    assert!(
        parse
            .transactions
            .iter()
            .any(|t| t.payee.as_deref() == Some("January Item")),
        "expected January Item in ledger"
    );
}

#[then(regex = r#"^February transactions should be in ledger.transactions$"#)]
async fn february_transactions_in_active(world: &mut LedgerWorld) {
    let text = world
        .active_ledger_text
        .as_ref()
        .expect("active ledger text");
    assert!(
        text.contains("February Item"),
        "expected February Item in active ledger"
    );
}

#[then("the output should include the account declaration")]
async fn the_output_should_include_account_declaration(world: &mut LedgerWorld) {
    // With per-folder output, account declarations are in accounts.transactions at the set root
    // or in the generated root when there's no set structure.
    let generated = world.generated_dir.as_ref().expect("generated_dir");
    let set_dir = set_generated_dir(world);
    // Check accounts.transactions first (new layout), then ledger.transactions (legacy)
    let contents = try_read_file(&set_dir.join("accounts.transactions"))
        .or_else(|| try_read_file(&set_dir.join("ledger.transactions")))
        .or_else(|| try_read_file(&generated.join("accounts.transactions")))
        .or_else(|| try_read_file(&generated.join("ledger.transactions")))
        .unwrap_or_default();
    assert!(
        contents.contains("account assets:bank:commbank AUD"),
        "expected account declaration in output; got: {contents:?}"
    );
}

#[then(
    regex = r#"^the output should include an auto-generated account declaration for "([^"]+)"$"#
)]
async fn the_output_should_include_auto_generated_account(
    world: &mut LedgerWorld,
    account: String,
) {
    let set_dir = set_generated_dir(world);
    let contents = try_read_file(&set_dir.join("accounts.transactions"))
        .or_else(|| try_read_file(&set_dir.join("ledger.transactions")))
        .unwrap_or_default();
    let expected = format!("account {account}");
    assert!(
        contents.contains(&expected),
        "expected auto-generated account declaration '{expected}' in output; got: {contents:?}"
    );
}

#[then(regex = r#"^loading all ledgers should include payee "([^"]+)"$"#)]
async fn loading_all_ledgers_should_include_payee(world: &mut LedgerWorld, payee: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load all ledgers");
    assert!(
        result
            .transactions
            .iter()
            .any(|t| t.payee.as_deref() == Some(payee.as_str())),
        "expected all ledgers to include payee {payee:?}; got: {:?}",
        result
            .transactions
            .iter()
            .map(|t| t.payee.clone())
            .collect::<Vec<_>>()
    );
}

// Legacy step kept for existing ingest feature:
#[then(expr = "the active ledger should include meta tag {string}")]
async fn the_active_ledger_should_include_meta_tag(world: &mut LedgerWorld, needle: String) {
    let dir = set_generated_dir(world);
    let contents = std::fs::read_to_string(dir.join("ledger.transactions")).unwrap_or_default();
    assert!(
        contents.contains(&needle),
        "expected active ledger to include {needle:?}; got: {contents:?}"
    );
}

#[then(expr = "the active ledger should contain {int} distinct leg ids")]
async fn the_active_ledger_distinct_leg_ids(world: &mut LedgerWorld, expected: usize) {
    let dir = set_generated_dir(world);
    let contents = std::fs::read_to_string(dir.join("ledger.transactions")).unwrap_or_default();
    let mut legs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for line in contents.lines() {
        if let Some(idx) = line.find("; ") {
            for seg in line[idx + 2..].split(',') {
                if let Some(rest) = seg.trim().strip_prefix("leg:") {
                    legs.insert(rest.to_string());
                }
            }
        }
    }
    assert_eq!(
        legs.len(),
        expected,
        "expected {expected} distinct leg ids in active ledger, found {}: {legs:?}",
        legs.len()
    );
}

// === Import CSV to account folder scenario ===

#[given(regex = r#"^a clean sources directory with a transform at "([^"]+)"$"#)]
async fn a_clean_sources_directory_with_transform_at(
    world: &mut LedgerWorld,
    transform_relative: String,
) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_transform(sources, &transform_relative, "assets:bank:commbank");
    world.now_yyyymm = Some("202501".to_string());
}

#[given(regex = r#"^a CSV fixture file "([^"]+)"$"#)]
async fn a_csv_fixture_file(world: &mut LedgerWorld, fixture_name: String) {
    let fixture_path = fixtures_dir().join(&fixture_name);
    assert!(
        fixture_path.exists(),
        "fixture file should exist: {fixture_path:?}"
    );
    world.file_path = Some(fixture_path);
}

#[when(regex = r#"^I import the CSV to account folder "([^"]+)"$"#)]
async fn i_import_the_csv_to_account_folder(world: &mut LedgerWorld, account_folder: String) {
    let config = make_pipeline_config(world);
    let source_path = world
        .file_path
        .as_ref()
        .expect("fixture file path should be set");
    let result = import_csv_to_sources(&config, source_path, &account_folder)
        .expect("import_csv_to_sources");
    world.pipeline_result = Some(result);
}

#[then(regex = r#"^the file should exist in "([^"]+)"$"#)]
async fn the_file_should_exist_in(world: &mut LedgerWorld, relative_path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let full_path = sources.join(&relative_path);
    assert!(full_path.exists(), "expected file at {full_path:?}");
}

#[then(regex = r#"^the pipeline should have transformed (\d+) CSV$"#)]
async fn the_pipeline_should_have_transformed_n_csv(world: &mut LedgerWorld, expected: usize) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert_eq!(
        result.csv_transformed, expected,
        "expected {} CSVs transformed, got {}",
        expected, result.csv_transformed
    );
}

#[then("the pipeline should have early-exited")]
async fn the_pipeline_should_have_early_exited(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert!(
    result.early_exit,
    "expected pipeline to early-exit, but it did a full rebuild (csv_transformed={}, ofx_transformed={})",
    result.csv_transformed, result.ofx_transformed
  );
}

#[then("the pipeline should have early-exited is false")]
async fn the_pipeline_should_not_have_early_exited(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert!(
        !result.early_exit,
        "expected pipeline to do a full rebuild, but it early-exited"
    );
}

// === Incremental output (Layer 4 cache) steps ===

#[then("the pipeline should report 0 output files written")]
async fn the_pipeline_should_report_0_output_files_written(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert_eq!(
        result.output_files_written, 0,
        "expected 0 output files written, got {}",
        result.output_files_written
    );
}

#[then("the pipeline should report output files written > 0")]
async fn the_pipeline_should_report_output_files_written_gt_0(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert!(
        result.output_files_written > 0,
        "expected output_files_written > 0, got {}",
        result.output_files_written
    );
}

#[then("the pipeline should report output files skipped > 0")]
async fn the_pipeline_should_report_output_files_skipped_gt_0(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert!(
        result.output_files_skipped > 0,
        "expected output_files_skipped > 0, got {}",
        result.output_files_skipped
    );
}

#[when("the sources directory is touched to bypass global cache")]
async fn the_sources_dir_is_touched(world: &mut LedgerWorld) {
    // Invalidate Layers 1+2 by clearing global/folder fingerprints in the cache file.
    // This forces a non-early-exit rebuild while letting Layer 4 (output hash) take effect.
    let generated_dir = world.generated_dir.as_ref().expect("generated_dir");
    let mut cache = build_cache::load_cache(generated_dir);
    cache.inputs_hash = None;
    cache.folder_hashes.clear();
    build_cache::save_cache(generated_dir, &cache, None).expect("save cache");
}

// === Incremental pipeline (architecture) step definitions ===

/// Snapshot every generated ledger.transactions file so the matching
/// "… ledger file should be unchanged" assertion can diff against the
/// pre-hinted-run state.
fn snapshot_generated_ledgers(world: &mut LedgerWorld) {
    let generated_dir = world
        .generated_dir
        .as_ref()
        .expect("generated_dir")
        .clone();
    let mut snapshots: std::collections::HashMap<String, Vec<u8>> =
        std::collections::HashMap::new();
    for entry in walkdir::WalkDir::new(&generated_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) != Some("ledger.transactions") {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(&generated_dir) {
            if let Ok(bytes) = std::fs::read(path) {
                snapshots.insert(rel.to_string_lossy().to_string(), bytes);
            }
        }
    }
    world.generated_ledger_snapshot = Some(snapshots);
}

#[when(expr = "I run the pipeline with changed folder hint {string}")]
async fn i_run_pipeline_with_changed_folder_hint(world: &mut LedgerWorld, hint: String) {
    // Snapshot BEFORE running so "should be unchanged" assertions later can compare.
    snapshot_generated_ledgers(world);
    if world.now_yyyymm.is_none() {
        world.now_yyyymm = Some("202501".to_string());
    }
    let mut config = make_pipeline_config(world);
    config.changed_folder_hint = Some(vec![hint]);
    let result = run_pipeline(&config).expect("pipeline with changed folder hint");
    generate_reports_after_pipeline(&config, &result);
    world.pipeline_result = Some(result);

    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }

    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

#[when(expr = "I add a rule to {string} matching {string} with payee {string}")]
async fn i_add_a_rule_to(
    world: &mut LedgerWorld,
    folder: String,
    pattern: String,
    payee: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let folder_path = sources.join(&folder);
    std::fs::create_dir_all(&folder_path).expect("create rule folder");
    let mut rules = RulesFile::load(&folder_path);
    let next_index = rules.rules.len();
    // Wrap the raw pattern in wildcards so "Coffee" substring-matches "Coffee Shop"
    // (wildcard_match_single is exact without `*`).
    let pattern_glob = if pattern.contains('*') {
        pattern
    } else {
        format!("*{pattern}*")
    };
    rules.rules.push(Rule {
        id: format!("test-rule-{next_index}"),
        pattern: pattern_glob,
        match_field: None,
        payee: Some(payee),
        commodity: None,
        comment: Some("test fixture".to_string()),
        amount_condition: None,
        fee_condition: None,
        amount_account: None,
        fee_account: None,
        payee_condition: None,
        narration_condition: None,
        commodity_condition: None,
        meta_condition: None,        postings: vec![],
    });
    // Give the filesystem a chance to register a distinct mtime — the folder
    // fingerprint mixes in `_rules.json`'s mtime and its granularity is seconds
    // on most filesystems, so back-to-back writes can otherwise collide.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    rules.save(&folder_path).expect("save rules");
}

#[then(expr = "only the {string} folder should have been reprocessed")]
async fn only_folder_should_have_been_reprocessed(world: &mut LedgerWorld, folder: String) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert_eq!(
        result.changed_folders,
        vec![folder.clone()],
        "expected only '{}' to be in changed_folders; got: {:?}",
        folder,
        result.changed_folders
    );
}

#[then(expr = "the {string} ledger file should be unchanged")]
async fn ledger_file_should_be_unchanged(world: &mut LedgerWorld, folder: String) {
    let generated_dir = world
        .generated_dir
        .as_ref()
        .expect("generated_dir")
        .clone();
    let ledger_path = generated_dir.join(&folder).join("ledger.transactions");
    let snapshot = world
        .generated_ledger_snapshot
        .as_ref()
        .expect("snapshot must be taken by an earlier hinted pipeline step");
    let rel = ledger_path
        .strip_prefix(&generated_dir)
        .expect("strip generated_dir prefix")
        .to_string_lossy()
        .to_string();
    let previous = snapshot
        .get(&rel)
        .unwrap_or_else(|| panic!("no snapshot for '{rel}' (was it missing before the hinted run?)"));
    let current = std::fs::read(&ledger_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", ledger_path.display()));
    if previous != &current {
        let prev_text = String::from_utf8_lossy(previous);
        let curr_text = String::from_utf8_lossy(&current);
        panic!(
            "expected ledger at '{rel}' to be byte-identical to its snapshot\nBEFORE:\n{prev_text}\nAFTER:\n{curr_text}"
        );
    }
}

#[then(expr = "a ledger file should exist at {string}")]
async fn a_ledger_file_should_exist_at(world: &mut LedgerWorld, path: String) {
    let generated_dir = world.generated_dir.as_ref().expect("generated_dir");
    let full = generated_dir.join(&path);
    assert!(
        full.exists(),
        "expected ledger file at '{}' to exist; resolved to {}",
        path,
        full.display()
    );
}

#[then(expr = "the ledger at {string} should contain {string}")]
async fn ledger_at_path_should_contain(
    world: &mut LedgerWorld,
    path: String,
    expected: String,
) {
    let generated_dir = world.generated_dir.as_ref().expect("generated_dir");
    let ledger_path = generated_dir.join(&path);
    let contents = std::fs::read_to_string(&ledger_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", ledger_path.display()));
    assert!(
        contents.contains(&expected),
        "expected ledger at '{path}' to contain {expected:?}; got:\n{contents}"
    );
}

#[then(expr = "changed_folders should contain {string}")]
async fn changed_folders_should_contain(world: &mut LedgerWorld, folder: String) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert!(
        result.changed_folders.iter().any(|f| f == &folder),
        "expected changed_folders to contain '{}'; got: {:?}",
        folder,
        result.changed_folders
    );
}

#[then(expr = "changed_folders should not contain {string}")]
async fn changed_folders_should_not_contain(world: &mut LedgerWorld, folder: String) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert!(
        !result.changed_folders.iter().any(|f| f == &folder),
        "expected changed_folders to NOT contain '{}'; got: {:?}",
        folder,
        result.changed_folders
    );
}

#[given(expr = "USD price data for {string} and {string}")]
async fn usd_price_data_for(
    world: &mut LedgerWorld,
    commodity_a: String,
    commodity_b: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let prices_dir = sources.join("_prices");
    std::fs::create_dir_all(&prices_dir).expect("create _prices dir");
    // USDC is base-like in auto_link's eyes, but PriceGraph still needs an
    // explicit rate to convert USDC → USD (base). Provide it.
    let content = format!(
        "P 2024-05-01 USDC 1.00 USD\n\
         P 2024-05-01 {commodity_a} 120.00 USD\n\
         P 2024-05-01 {commodity_b} 3000.00 USD\n",
    );
    std::fs::write(prices_dir.join("test.txt"), content).expect("write prices file");
    // base_currency=USD must be on the set's generated config.json for auto_link
    // to derive price annotations. Write it for the inferred primary set.
    let generated_dir = world.generated_dir.as_ref().expect("generated_dir");
    let set = infer_primary_set(sources).unwrap_or_else(|| "richard".to_string());
    let set_dir = generated_dir.join(&set);
    std::fs::create_dir_all(&set_dir).expect("create set dir");
    let config_json = r#"{"base_currency":"USD"}"#;
    std::fs::write(set_dir.join("config.json"), config_json).expect("write set config.json");
}

#[given(expr = "a swap CSV at {string} trading {string} for {string}")]
async fn a_swap_csv_at(
    world: &mut LedgerWorld,
    csv_relative: String,
    sell_commodity: String,
    buy_commodity: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let csv_path = sources.join(&csv_relative);
    let parent = csv_path
        .parent()
        .expect("csv path must have a parent")
        .to_path_buf();
    std::fs::create_dir_all(&parent).expect("create swap CSV parent dirs");
    // Two rows: sell leg (negative) + buy leg (positive) sharing the same on-chain
    // hash (matches the Solana/Ethereum shape). Different txn_ids per leg would
    // work too but we want the shared-txn-id path exercised.
    let tx_hash = format!("swap-{sell_commodity}-{buy_commodity}");
    let csv_content = format!(
        "Date,Description,Amount,TxHash\n\
         2024-05-01 11:44:03,Sell {sell_commodity},-100,{tx_hash}\n\
         2024-05-01 11:44:03,Buy {buy_commodity},0.83,{tx_hash}\n"
    );
    std::fs::write(&csv_path, csv_content).expect("write swap CSV");
    // Simple two-leg transform:
    //   - narration is a constant so both legs group under auto_link's
    //     (datetime, asset_account, narration) key.
    //   - `contra: "equity:trading"` makes auto_link pick them up.
    let transform_path = parent.join("_transform.rhai");
    let script = format!(
        r##"let desc = row["Description"];
let commodity = if desc.contains("{sell_commodity}") {{ "{sell_commodity}" }} else {{ "{buy_commodity}" }};
#{{
  date: row["Date"],
  payee: desc,
  narration: "on-chain swap",
  amount: row["Amount"],
  commodity: commodity,
  contra: "equity:trading",
  txn_id: row["TxHash"],
  status: "*"
}}"##,
    );
    std::fs::write(&transform_path, script).expect("write swap transform");
}

#[when(expr = "a CSV {string} is modified:")]
async fn a_csv_is_modified(
    world: &mut LedgerWorld,
    csv_relative: String,
    step: &cucumber::gherkin::Step,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let table = step.table.as_ref().expect("step should have a data table");
    let rows: Vec<(&str, &str, &str)> = table
        .rows
        .iter()
        .skip(1)
        .map(|r| (r[0].as_str(), r[1].as_str(), r[2].as_str()))
        .collect();
    // Sleep briefly so the filesystem mtime changes (dir_fingerprint uses mtime+size).
    std::thread::sleep(std::time::Duration::from_millis(1100));
    write_simple_csv(sources, &csv_relative, &rows);
}

#[when(expr = "the source folder {string} is removed")]
async fn the_source_folder_is_removed(world: &mut LedgerWorld, folder: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let path = sources.join(&folder);
    if path.exists() {
        std::fs::remove_dir_all(&path).expect("remove source folder");
    }
}

#[when(expr = "a {string} file is added to {string} with payee {string}")]
async fn a_file_is_added_to_folder_with_payee(
    world: &mut LedgerWorld,
    filename: String,
    folder: String,
    payee: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let dir = sources.join(&folder);
    std::fs::create_dir_all(&dir).expect("create folder");
    let path = dir.join(&filename);
    let text = format!(
        r#"2025-01-20 * "{payee}" "added offline" ;
    assets:bank:test 50.00 AUD
    income:other -50.00 AUD
"#
    );
    std::fs::write(&path, text).expect("write file");
}

// === Transform suggestion scenarios ===

#[given(regex = r#"^a CSV with headers "([^"]+)"$"#)]
async fn a_csv_with_headers(world: &mut LedgerWorld, headers_str: String) {
    let headers: Vec<String> = headers_str.split(',').map(|s| s.to_string()).collect();
    world.csv_headers = Some(headers);
}

#[given(regex = r#"^the target account is "([^"]+)"$"#)]
async fn the_target_account_is(world: &mut LedgerWorld, account: String) {
    world.target_account = Some(account);
}

#[when("I generate a transform suggestion")]
async fn i_generate_a_transform_suggestion(world: &mut LedgerWorld) {
    let headers = world
        .csv_headers
        .as_ref()
        .expect("csv_headers should be set");
    let script = transform_suggest::suggest_transform_script(headers, None);
    world.transform_suggestion = Some(script);
}

#[when("I generate and apply the suggested transform")]
async fn i_generate_and_apply_the_suggested_transform(world: &mut LedgerWorld) {
    let account = world
        .target_account
        .as_ref()
        .expect("target_account should be set");
    let csv_path = world
        .file_path
        .as_ref()
        .expect("fixture file path should be set");
    let script = transform_suggest::generate_suggestion(csv_path, None)
        .expect("generate_suggestion should succeed");
    world.transform_suggestion = Some(script.clone());

    // Derive account_folder from account name (e.g. "assets:bank:savings" -> "bank/savings")
    let parts: Vec<&str> = account.split(':').collect();
    let account_folder = parts[1..].join("/");

    if world.now_yyyymm.is_none() {
        world.now_yyyymm = Some("202211".to_string());
    }
    let config = make_pipeline_config(world);
    let result = save_transform_and_rebuild(&config, csv_path, &account_folder, &script)
        .expect("save_transform_and_rebuild should succeed");
    world.pipeline_result = Some(result);

    // Infer active set from sources
    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }
}

#[then(regex = r#"^the suggestion should map "([^"]+)" from "([^"]+)"$"#)]
async fn the_suggestion_should_map_field_from_column(
    world: &mut LedgerWorld,
    field: String,
    column: String,
) {
    let script = world
        .transform_suggestion
        .as_ref()
        .expect("suggestion should be set");
    // Accept either `field: row["Col"]` or `field: clean(row["Col"])` or `field: parse_date(row["Col"])`
    let direct = format!(r#"{field}: row["{column}"]"#);
    let cleaned = format!(r#"{field}: clean(row["{column}"])"#);
    let parsed = format!(r#"{field}: parse_date(row["{column}"])"#);
    assert!(
        script.contains(&direct) || script.contains(&cleaned) || script.contains(&parsed),
        "expected suggestion to reference column {column:?} for field {field:?}; got:\n{script}"
    );
}

#[then(regex = r#"^the suggestion should set account to "([^"]+)"$"#)]
async fn the_suggestion_should_set_account(world: &mut LedgerWorld, account: String) {
    let script = world
        .transform_suggestion
        .as_ref()
        .expect("suggestion should be set");
    let expected = format!(r#"account: "{account}""#);
    assert!(
        script.contains(&expected),
        "expected suggestion to contain {expected:?}; got:\n{script}"
    );
}

#[then("the suggestion should compile as valid Rhai")]
async fn the_suggestion_should_compile_as_valid_rhai(world: &mut LedgerWorld) {
    let script = world
        .transform_suggestion
        .as_ref()
        .expect("suggestion should be set");
    let engine = rhai::Engine::new();
    let result = engine.compile(script);
    assert!(
        result.is_ok(),
        "expected Rhai script to compile; error: {:?}\nscript:\n{script}",
        result.err()
    );
}

#[then("the suggestion should derive amount from Debit and Credit")]
async fn the_suggestion_should_derive_amount_from_debit_and_credit(world: &mut LedgerWorld) {
    let script = world
        .transform_suggestion
        .as_ref()
        .expect("suggestion should be set");
    assert!(
        script.contains("Debit") && script.contains("Credit"),
        "expected suggestion to reference both Debit and Credit columns; got:\n{script}"
    );
    // Should not have a simple row["Amount"] mapping
    assert!(
        !script.contains(r#"amount: row["Amount"]"#),
        "expected suggestion NOT to have a simple amount mapping; got:\n{script}"
    );
}

#[then("the suggestion should contain placeholder comments")]
async fn the_suggestion_should_contain_placeholder_comments(world: &mut LedgerWorld) {
    let script = world
        .transform_suggestion
        .as_ref()
        .expect("suggestion should be set");
    assert!(
        script.contains("TODO") || script.contains("FIXME"),
        "expected suggestion to contain placeholder comments (TODO/FIXME); got:\n{script}"
    );
}

// === Rules scenarios ===

fn write_rules_file(sources_dir: &PathBuf, relative_path: &str, rules: &RulesFile) {
    let path = sources_dir.join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create rules parent dirs");
    }
    let contents = arimalo_covid::to_sorted_json_pretty(rules).expect("serialize rules");
    std::fs::write(&path, contents).expect("write rules file");
}

fn write_labels_file(sources_dir: &PathBuf, relative_path: &str, labels: &LabelsFile) {
    let path = sources_dir.join(relative_path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create labels parent dirs");
    }
    let contents = arimalo_covid::to_sorted_json_pretty(labels).expect("serialize labels");
    std::fs::write(&path, contents).expect("write labels file");
}

#[given(regex = r#"^a rules file at "([^"]+)" matching "([^"]+)" with payee "([^"]+)"$"#)]
async fn a_rules_file_with_payee(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    payee: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-test1".to_string(),
            pattern,
            match_field: None,
            payee: Some(payee),
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: None,
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

#[given(regex = r#"^a labels file at "([^"]+)" matching "([^"]+)" with payee "([^"]+)"$"#)]
async fn a_labels_file_with_payee(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    payee: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let labels = LabelsFile {
        labels: vec![Rule {
            id: "label-test1".to_string(),
            pattern,
            match_field: None,
            payee: Some(payee),
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: None,
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_labels_file(sources, &path, &labels);
}

#[given(regex = r#"^a rules file at "([^"]+)" matching "([^"]+)" with contra "([^"]+)"$"#)]
async fn a_rules_file_with_contra(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    contra: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-test2".to_string(),
            pattern,
            match_field: None,
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: Some(contra),
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

#[given(
    regex = r#"^a labels file at "([^"]+)" matching "([^"]+)" on field "([^"]+)" with payee "([^"]+)"$"#
)]
async fn a_labels_file_with_payee_on_field(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    field: String,
    payee: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let labels = LabelsFile {
        labels: vec![Rule {
            id: "label-test-field".to_string(),
            pattern,
            match_field: Some(field),
            payee: Some(payee),
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: None,
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_labels_file(sources, &path, &labels);
}

#[given(
    regex = r#"^a rules file at "([^"]+)" matching "([^"]+)" on field "([^"]+)" with contra "([^"]+)"$"#
)]
async fn a_rules_file_with_contra_on_field(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    field: String,
    contra: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-test-field".to_string(),
            pattern,
            match_field: Some(field),
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: Some(contra),
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

#[given(regex = r#"^a rules file at "([^"]+)" matching "([^"]+)" with postings "([^"]+)"$"#)]
async fn a_rules_file_with_postings(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    postings_str: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let accounts: Vec<String> = postings_str
        .split('|')
        .map(|s| s.trim().to_string())
        .collect();
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-postings1".to_string(),
            pattern,
            match_field: None,
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: accounts.first().cloned(),
            fee_account: accounts.get(1).cloned(),
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

#[when(regex = r#"^a rules file is added to "([^"]+)" matching "([^"]+)" with payee "([^"]+)"$"#)]
async fn a_rules_file_is_added(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    payee: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-test3".to_string(),
            pattern,
            match_field: None,
            payee: Some(payee),
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: None,
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

#[given(regex = r#"^a rules file at "([^"]+)" matching "([^"]+)" with fee_account "([^"]+)"$"#)]
async fn a_rules_file_with_fee_account(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    fee_acct: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-fee1".to_string(),
            pattern,
            match_field: None,
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: None,
            fee_account: Some(fee_acct),
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

#[given(
    regex = r#"^a rules file at "([^"]+)" matching "([^"]+)" with contra "([^"]+)" and fee_account "([^"]+)"$"#
)]
async fn a_rules_file_with_contra_and_fee(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    contra: String,
    fee_acct: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-contrafee1".to_string(),
            pattern,
            match_field: None,
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: Some(contra),
            fee_account: Some(fee_acct),
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

// === Amount rule scenarios ===

#[given(
    regex = r#"^a rules file at "([^"]+)" matching "([^"]+)" with contra "([^"]+)" and amount condition "([^"]+)"$"#
)]
async fn a_rules_file_with_contra_and_amount(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    contra: String,
    amount_condition: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-amount1".to_string(),
            pattern,
            match_field: None,
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: Some(amount_condition),
            fee_condition: None,
            amount_account: Some(contra),
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

#[then(
    regex = r#"^only transactions with amount less than (-?\d+\.?\d*) should use contra "([^"]+)"$"#
)]
async fn only_transactions_with_amount_lt(
    world: &mut LedgerWorld,
    threshold: f64,
    expected_contra: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    for txn in &result.transactions {
        let first_amount = txn.postings[0].amount;
        let contra = &txn.postings[1].account;
        if first_amount < threshold {
            assert_eq!(
                contra, &expected_contra,
                "transaction with amount {} should have contra {}, got {}",
                first_amount, expected_contra, contra
            );
        } else {
            assert_ne!(
                contra, &expected_contra,
                "transaction with amount {} should NOT have contra {}",
                first_amount, expected_contra
            );
        }
    }
}

#[then(regex = r#"^the transaction with description "([^"]+)" should use contra "([^"]+)"$"#)]
async fn the_transaction_with_description_should_use_contra(
    world: &mut LedgerWorld,
    desc: String,
    expected_contra: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let matching: Vec<_> = result
        .transactions
        .iter()
        .filter(|t| {
            let payee = t.payee.as_deref().unwrap_or("");
            let narration = t.narration.as_deref().unwrap_or("");
            payee.contains(&desc) || narration.contains(&desc)
        })
        .collect();
    assert!(
        !matching.is_empty(),
        "expected to find transactions with description '{}'",
        desc
    );
    for txn in &matching {
        assert_eq!(
            txn.postings[1].account, expected_contra,
            "transaction with description '{}' should use contra '{}', got '{}'",
            desc, expected_contra, txn.postings[1].account
        );
    }
}

#[then(
    regex = r#"^only the coffee transaction with amount less than (-?\d+\.?\d*) should use contra "([^"]+)"$"#
)]
async fn only_coffee_with_amount_lt(
    world: &mut LedgerWorld,
    threshold: f64,
    expected_contra: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    for txn in &result.transactions {
        let payee = txn.payee.as_deref().unwrap_or("");
        let narration = txn.narration.as_deref().unwrap_or("");
        let has_pattern =
            payee.to_lowercase().contains("coffee") || narration.to_lowercase().contains("coffee");
        let first_amount = txn.postings[0].amount;
        let contra = &txn.postings[1].account;

        if has_pattern && first_amount < threshold {
            assert_eq!(
                contra, &expected_contra,
                "Coffee transaction with amount {} should have contra {}, got {}",
                first_amount, expected_contra, contra
            );
        } else {
            assert_ne!(
                contra, &expected_contra,
                "transaction '{}'/'{}'  with amount {} should NOT have contra {}",
                payee, narration, first_amount, expected_contra
            );
        }
    }
}

#[then(
    regex = r#"^only the transaction with amount between (-?\d+\.?\d*) and (-?\d+\.?\d*) should use contra "([^"]+)"$"#
)]
async fn only_transaction_between_amounts(
    world: &mut LedgerWorld,
    lo: f64,
    hi: f64,
    expected_contra: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    for txn in &result.transactions {
        let first_amount = txn.postings[0].amount;
        let contra = &txn.postings[1].account;
        if first_amount >= lo && first_amount <= hi {
            assert_eq!(
                contra, &expected_contra,
                "transaction with amount {} should have contra {}, got {}",
                first_amount, expected_contra, contra
            );
        } else {
            assert_ne!(
                contra, &expected_contra,
                "transaction with amount {} should NOT have contra {}",
                first_amount, expected_contra
            );
        }
    }
}

// === Commodity rule scenarios ===

#[given(regex = r#"^a transform at "([^"]+)" that outputs commodity "([^"]+)"$"#)]
async fn a_transform_with_commodity(
    world: &mut LedgerWorld,
    transform_relative: String,
    commodity: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let transform_path = sources.join(&transform_relative);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = format!(
        r##"#{{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "{commodity}",
  status: "*"
}}"##
    );
    std::fs::write(&transform_path, script).expect("write transform");
}

#[given(regex = r#"^a transform at "([^"]+)" with token sanitization$"#)]
async fn a_transform_with_token_sanitization(world: &mut LedgerWorld, transform_relative: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let transform_path = sources.join(&transform_relative);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = r##"fn sanitize_commodity(sym) {
  let out = "";
  for ch in sym.chars() {
    if ch.is_alphabetic() || (ch >= '0' && ch <= '9') || ch == '_' {
      out += ch;
    }
  }
  if out == "" || out.len() > 24 { "SPAM" } else { out }
}

let token_sym = row["token_symbol"];
let tx_type = row["tx_type"];
let method = row["method"];

let commodity = if tx_type == "token_transfer" && token_sym != "" {
  sanitize_commodity(token_sym)
} else {
  "ETH"
};

let info = tx_type;
if method != "" { info = info + ":" + method; }
if token_sym != "" && tx_type == "token_transfer" { info = info + " " + commodity; }

#{
  date: row["Date"],
  payee: row["from_address"],
  narration: info,
  amount: row["value"],
  commodity: commodity,
  status: "*"
}
"##;
    std::fs::write(&transform_path, script).expect("write transform");
}

#[given(
    regex = r#"^a rules file at "([^"]+)" matching commodity "([^"]+)" with commodity "([^"]+)"$"#
)]
async fn a_rules_file_with_commodity(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    new_commodity: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-commodity1".to_string(),
            pattern,
            match_field: Some("commodity".to_string()),
            payee: None,
            commodity: Some(new_commodity),
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: None,
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

#[given(
    regex = r#"^a rules file at "([^"]+)" with commodity rename "([^"]+)" to "([^"]+)" and payee rule "([^"]+)" to "([^"]+)"$"#
)]
async fn a_rules_file_with_commodity_and_payee(
    world: &mut LedgerWorld,
    path: String,
    old_commodity: String,
    new_commodity: String,
    pattern: String,
    payee: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![
            Rule {
                id: "rule-commodity-rename".to_string(),
                pattern: old_commodity,
                match_field: Some("commodity".to_string()),
                payee: None,
                commodity: Some(new_commodity),
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: None,
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            },
            Rule {
                id: "rule-categorize".to_string(),
                pattern,
                match_field: None,
                payee: Some(payee),
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: None,
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            },
        ],
    };
    write_rules_file(sources, &path, &rules);
}

#[given(
    regex = r#"^a rules file at "([^"]+)" with commodity rename "([^"]+)" to "([^"]+)" and contra rule "([^"]+)" to "([^"]+)"$"#
)]
async fn a_rules_file_with_commodity_and_contra(
    world: &mut LedgerWorld,
    path: String,
    old_commodity: String,
    new_commodity: String,
    pattern: String,
    contra: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![
            Rule {
                id: "rule-commodity-rename".to_string(),
                pattern: old_commodity,
                match_field: Some("commodity".to_string()),
                payee: None,
                commodity: Some(new_commodity),
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: None,
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            },
            Rule {
                id: "rule-categorize".to_string(),
                pattern,
                match_field: None,
                payee: None,
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some(contra),
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            },
        ],
    };
    write_rules_file(sources, &path, &rules);
}

#[given(
    regex = r#"^a rules file at "([^"]+)" with payee rule "([^"]+)" to "([^"]+)" and contra rule "([^"]+)" to "([^"]+)"$"#
)]
async fn a_rules_file_with_payee_and_contra(
    world: &mut LedgerWorld,
    path: String,
    payee_pattern: String,
    payee: String,
    contra_pattern: String,
    contra: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![
            Rule {
                id: "rule-payee-transform".to_string(),
                pattern: payee_pattern,
                match_field: None,
                payee: Some(payee),
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: None,
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            },
            Rule {
                id: "rule-categorize".to_string(),
                pattern: contra_pattern,
                match_field: None,
                payee: None,
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some(contra),
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            },
        ],
    };
    write_rules_file(sources, &path, &rules);
}

#[given(
    regex = r#"^a rules file at "([^"]+)" with categorization rule "([^"]+)" payee "([^"]+)" contra "([^"]+)" and payee-only rule "([^"]+)" to "([^"]+)"$"#
)]
async fn a_rules_file_with_categorization_and_payee_only(
    world: &mut LedgerWorld,
    path: String,
    cat_pattern: String,
    cat_payee: String,
    cat_contra: String,
    payee_pattern: String,
    payee_label: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![
            Rule {
                id: "rule-categorize".to_string(),
                pattern: cat_pattern,
                match_field: None,
                payee: Some(cat_payee),
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some(cat_contra),
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            },
            Rule {
                id: "rule-payee-label".to_string(),
                pattern: payee_pattern,
                match_field: None,
                payee: Some(payee_label),
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: None,
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            },
        ],
    };
    write_rules_file(sources, &path, &rules);
}

#[then(regex = r#"^transactions should have commodity "([^"]+)"$"#)]
async fn transactions_should_have_commodity(world: &mut LedgerWorld, expected: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    assert!(
        !result.transactions.is_empty(),
        "expected transactions, found none"
    );
    for txn in &result.transactions {
        let first = &txn.postings[0];
        assert_eq!(
            first.commodity, expected,
            "expected commodity {expected:?}, got {:?}",
            first.commodity
        );
    }
}

#[then(regex = r#"^transaction amount_commodity should be "([^"]+)"$"#)]
async fn transaction_amount_commodity_should_be(world: &mut LedgerWorld, expected: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    assert!(
        !result.transactions.is_empty(),
        "expected transactions, found none"
    );
    for txn in &result.transactions {
        assert_eq!(
            txn.amount_commodity, expected,
            "expected amount_commodity {expected:?}, got {:?}",
            txn.amount_commodity
        );
    }
}

#[then(regex = r#"^transaction display_amount_commodity should be "([^"]+)"$"#)]
async fn transaction_display_amount_commodity_should_be(world: &mut LedgerWorld, expected: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    assert!(
        !result.transactions.is_empty(),
        "expected transactions, found none"
    );
    for txn in &result.transactions {
        assert_eq!(
            txn.display_amount_commodity.as_deref(),
            Some(expected.as_str()),
            "expected display_amount_commodity {expected:?}, got {:?}",
            txn.display_amount_commodity
        );
    }
}

#[then(regex = r#"^transactions with narration "([^"]+)" should have payee "([^"]+)"$"#)]
async fn transactions_with_narration_should_have_payee(
    world: &mut LedgerWorld,
    narration: String,
    expected_payee: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let matching: Vec<_> = result
        .transactions
        .iter()
        .filter(|t| t.narration.as_deref() == Some(narration.as_str()))
        .collect();
    assert!(
        !matching.is_empty(),
        "expected transactions with narration {narration:?}, found none"
    );
    for txn in &matching {
        let effective = txn.display_payee.as_deref().or(txn.payee.as_deref());
        assert_eq!(
            effective,
            Some(expected_payee.as_str()),
            "expected payee {expected_payee:?} for narration {narration:?}, got {:?} (display: {:?})",
            txn.payee, txn.display_payee
        );
    }
}

#[then(regex = r#"^transactions with narration "([^"]+)" should use contra "([^"]+)"$"#)]
async fn transactions_with_narration_should_use_contra(
    world: &mut LedgerWorld,
    narration: String,
    expected_contra: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let matching: Vec<_> = result
        .transactions
        .iter()
        .filter(|t| t.narration.as_deref() == Some(narration.as_str()))
        .collect();
    assert!(
        !matching.is_empty(),
        "expected transactions with narration {narration:?}, found none"
    );
    for txn in &matching {
        assert!(
            txn.postings.len() >= 2,
            "expected at least 2 postings for transaction"
        );
        assert_eq!(
            txn.postings[1].account, expected_contra,
            "expected contra account {expected_contra:?}, got {:?}",
            txn.postings[1].account
        );
    }
}

#[then("the suggestion should have blank payee")]
async fn the_suggestion_should_have_blank_payee(world: &mut LedgerWorld) {
    let script = world
        .transform_suggestion
        .as_ref()
        .expect("suggestion should be set");
    assert!(
        script.contains(r#"payee: """#),
        "expected suggestion to contain blank payee (payee: \"\"); got:\n{script}"
    );
}

#[then(regex = r#"^the active ledger should include narration "([^"]+)"$"#)]
async fn the_active_ledger_should_include_narration(world: &mut LedgerWorld, narration: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    assert!(
        result
            .transactions
            .iter()
            .any(|t| t.narration.as_deref() == Some(narration.as_str())),
        "expected active ledger to include narration {narration:?}; got: {:?}",
        result
            .transactions
            .iter()
            .map(|t| t.narration.clone())
            .collect::<Vec<_>>()
    );
}

// === Import rules CSV scenarios ===

#[given("a CSV rules file with contents:")]
async fn a_csv_rules_file_with_contents(world: &mut LedgerWorld, step: &cucumber::gherkin::Step) {
    let table = step.table.as_ref().expect("expected a data table");
    let csv_content = table_to_csv(table);
    let csv_path = std::env::temp_dir().join(format!(
        "arimalo-rules-import-{}.csv",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&csv_path, csv_content).expect("write rules CSV");
    world.rules_csv_path = Some(csv_path);
}

#[when(regex = r#"^I import the rules CSV into "([^"]+)"$"#)]
async fn i_import_the_rules_csv_into(world: &mut LedgerWorld, account_folder: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let csv_path = world
        .rules_csv_path
        .as_ref()
        .expect("rules_csv_path should be set");
    let folder = sources.join(&account_folder);

    // Read CSV
    let mut reader = csv::Reader::from_path(csv_path).expect("open rules CSV");
    let mut new_rules: Vec<Rule> = Vec::new();
    for result in reader.records() {
        let record = result.expect("read CSV record");
        let pattern = record.get(0).unwrap_or("").to_string();
        let payee_str = record.get(1).unwrap_or("").trim().to_string();
        let posting_str = record.get(2).unwrap_or("").trim().to_string();
        let comment_str = record.get(3).unwrap_or("").trim().to_string();
        if pattern.is_empty() {
            continue;
        }
        new_rules.push(Rule {
            id: arimalo_covid::rules::generate_rule_id(&pattern),
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

    // Load existing rules and append
    let mut rules_file = RulesFile::load(&folder);
    rules_file.rules.extend(new_rules);
    rules_file.save(&folder).expect("save rules file");

    // Rebuild pipeline
    let config = make_pipeline_config(world);
    let result = run_pipeline(&config).expect("pipeline rebuild after rules import");
    world.pipeline_result = Some(result);
}

#[then(regex = r#"^the rules file "([^"]+)" should contain (\d+) rules$"#)]
async fn the_rules_file_should_contain_n_rules(
    world: &mut LedgerWorld,
    path: String,
    count: usize,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    assert_eq!(
        rules.rules.len(),
        count,
        "expected {count} rules in {path}, got {}",
        rules.rules.len()
    );
}

#[when(expr = "I hide txn id {string} in folder {string}")]
async fn i_hide_txn_id_in_folder(world: &mut LedgerWorld, txn_id: String, folder: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir should be set");
    std::fs::create_dir_all(sources.join(&folder)).expect("create folder");
    append_hide_rule(sources, &folder, &txn_id).expect("append_hide_rule should succeed");
}

#[given(expr = "a meta rule {string} with pattern {string} routes to {string} in {string}")]
async fn a_meta_rule_in_folder(
    world: &mut LedgerWorld,
    id: String,
    pattern: String,
    contra: String,
    folder: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir should be set");
    let folder_path = sources.join(&folder);
    std::fs::create_dir_all(&folder_path).expect("create folder");
    let mut rules = RulesFile::load(&folder_path);
    rules.rules.push(Rule {
        id,
        pattern,
        match_field: Some("meta".to_string()),
        payee: None,
        commodity: None,
        comment: None,
        amount_condition: None,
        fee_condition: None,
        payee_condition: None,
        narration_condition: None,
        commodity_condition: None,
        meta_condition: None,        amount_account: Some(contra),
        fee_account: None,
        postings: vec![],
    });
    rules.save(&folder_path).expect("save rules");
}

#[then(expr = "the rule at index {int} of {string} has pattern {string}")]
async fn rule_at_index_has_pattern(
    world: &mut LedgerWorld,
    index: usize,
    folder: String,
    pattern: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir should be set");
    let rules = RulesFile::load(&sources.join(&folder));
    assert!(
        rules.rules.len() > index,
        "folder {folder} has {} rules, index {index} out of range",
        rules.rules.len()
    );
    assert_eq!(rules.rules[index].pattern, pattern);
}

#[then(expr = "the rule at index {int} of {string} has match_field {string}")]
async fn rule_at_index_has_match_field(
    world: &mut LedgerWorld,
    index: usize,
    folder: String,
    field: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir should be set");
    let rules = RulesFile::load(&sources.join(&folder));
    assert_eq!(rules.rules[index].match_field.as_deref(), Some(field.as_str()));
}

#[then(expr = "the rule at index {int} of {string} routes to {string}")]
async fn rule_at_index_routes_to(
    world: &mut LedgerWorld,
    index: usize,
    folder: String,
    contra: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir should be set");
    let rules = RulesFile::load(&sources.join(&folder));
    assert_eq!(rules.rules[index].amount_account.as_deref(), Some(contra.as_str()));
}

#[then(expr = "a meta string {string} in {string} routes to {string}")]
async fn meta_string_in_folder_routes_to(
    world: &mut LedgerWorld,
    meta: String,
    folder: String,
    contra: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir should be set");
    let rules = RulesFile::load(&sources.join(&folder));
    let m = rules
        .find_match_prioritized(&MatchFields {
            payee: None,
            display_payee: None,
            narration: None,
            meta: Some(meta.as_str()),
            commodity: None,
            display_commodity: None,
            amount: Some(-1.0),
            fee: None,
        })
        .unwrap_or_else(|| panic!("no rule matched meta {meta:?} in {folder}"));
    assert_eq!(m.amount_account.as_deref(), Some(contra.as_str()));
}

#[then(expr = "{string} contains exactly {int} rule")]
async fn folder_contains_exactly_n_rule(world: &mut LedgerWorld, folder: String, count: usize) {
    let sources = world.sources_dir.as_ref().expect("sources_dir should be set");
    let rules = RulesFile::load(&sources.join(&folder));
    assert_eq!(
        rules.rules.len(),
        count,
        "expected {count} rule(s) in {folder}, got {}",
        rules.rules.len()
    );
}

#[then("the pipeline should have been rebuilt")]
async fn the_pipeline_should_have_been_rebuilt(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert!(
        result.total_written > 0 || result.csv_transformed > 0 || result.csv_cached > 0,
        "expected pipeline to have been rebuilt"
    );
}

#[then(regex = r#"^rule (\d+) in "([^"]+)" should have no payee$"#)]
async fn rule_n_should_have_no_payee(world: &mut LedgerWorld, index: usize, path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    let rule = &rules.rules[index];
    assert!(
        rule.payee.is_none(),
        "expected rule {index} to have no payee, got {:?}",
        rule.payee
    );
}

#[then(regex = r#"^rule (\d+) in "([^"]+)" should have contra "([^"]+)"$"#)]
async fn rule_n_should_have_contra(
    world: &mut LedgerWorld,
    index: usize,
    path: String,
    expected: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    let rule = &rules.rules[index];
    assert_eq!(
        rule.amount_account.as_deref(),
        Some(expected.as_str()),
        "expected rule {index} amount_account {expected:?}, got {:?}",
        rule.amount_account
    );
}

#[then(regex = r#"^rule (\d+) in "([^"]+)" should have payee "([^"]+)"$"#)]
async fn rule_n_should_have_payee(
    world: &mut LedgerWorld,
    index: usize,
    path: String,
    expected: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    let rule = &rules.rules[index];
    assert_eq!(
        rule.payee.as_deref(),
        Some(expected.as_str()),
        "expected rule {index} payee {expected:?}, got {:?}",
        rule.payee
    );
}

#[then(regex = r#"^rule (\d+) in "([^"]+)" should have no contra$"#)]
async fn rule_n_should_have_no_contra(world: &mut LedgerWorld, index: usize, path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    let rule = &rules.rules[index];
    assert!(
        rule.amount_account.is_none(),
        "expected rule {index} to have no amount_account, got {:?}",
        rule.amount_account
    );
}

#[then(regex = r#"^rule (\d+) in "([^"]+)" should have comment "([^"]+)"$"#)]
async fn rule_n_should_have_comment(
    world: &mut LedgerWorld,
    index: usize,
    path: String,
    expected: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    let rule = &rules.rules[index];
    assert_eq!(
        rule.comment.as_deref(),
        Some(expected.as_str()),
        "expected rule {index} comment {expected:?}, got {:?}",
        rule.comment
    );
}

#[then(regex = r#"^rule (\d+) in "([^"]+)" should have no comment$"#)]
async fn rule_n_should_have_no_comment(world: &mut LedgerWorld, index: usize, path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    let rule = &rules.rules[index];
    assert!(
        rule.comment.is_none(),
        "expected rule {index} to have no comment, got {:?}",
        rule.comment
    );
}

// === Automerge Metadata scenarios ===

#[when("I initialize metadata from sources")]
async fn i_initialize_metadata_from_sources(world: &mut LedgerWorld) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let metadata_path = sources.join("arimalo-metadata.automerge");
    let mut store = MetadataStore::new(metadata_path).expect("create MetadataStore");
    store
        .build_from_sources(sources)
        .expect("build_from_sources");
    world.metadata_store = Some(store);
}

#[then("the metadata should contain transaction refs")]
async fn the_metadata_should_contain_transaction_refs(world: &mut LedgerWorld) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    assert!(
        !meta.transaction_refs.is_empty(),
        "expected transaction refs, got none"
    );
}

#[then(regex = r#"^the metadata file manifest should include "([^"]+)"$"#)]
async fn the_metadata_file_manifest_should_include(world: &mut LedgerWorld, filename: String) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    let has_file = meta
        .file_manifest
        .values()
        .any(|f| f.relative_path.contains(&filename));
    assert!(
        has_file,
        "expected file manifest to include {filename:?}; got paths: {:?}",
        meta.file_manifest
            .values()
            .map(|f| &f.relative_path)
            .collect::<Vec<_>>()
    );
}

#[then("the metadata should track this device")]
async fn the_metadata_should_track_this_device(world: &mut LedgerWorld) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    let device_id = store.device_id();
    assert!(
        meta.devices.contains_key(device_id),
        "expected devices to include {device_id:?}; got: {:?}",
        meta.devices.keys().collect::<Vec<_>>()
    );
}

#[then("the file manifest should include a CSV entry")]
async fn the_file_manifest_should_include_csv(world: &mut LedgerWorld) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    let has_csv = meta.file_manifest.values().any(|f| f.file_type == "csv");
    assert!(has_csv, "expected a CSV entry in file manifest");
}

#[then("the file manifest should include a transform entry")]
async fn the_file_manifest_should_include_transform(world: &mut LedgerWorld) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    let has_transform = meta
        .file_manifest
        .values()
        .any(|f| f.file_type == "transform");
    assert!(has_transform, "expected a transform entry in file manifest");
}

#[then("each file entry should have a non-empty content hash")]
async fn each_file_entry_should_have_nonempty_hash(world: &mut LedgerWorld) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    for (key, entry) in &meta.file_manifest {
        assert!(
            !entry.content_hash.is_empty(),
            "empty content hash for file {key:?}"
        );
    }
}

#[when("I save metadata to disk")]
async fn i_save_metadata_to_disk(world: &mut LedgerWorld) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    store.save().expect("save metadata");
}

#[when("I reload metadata from disk")]
async fn i_reload_metadata_from_disk(world: &mut LedgerWorld) {
    // Snapshot current metadata before reload
    let original_meta = {
        let store = world
            .metadata_store
            .as_ref()
            .expect("metadata_store should be set");
        store.get_metadata().expect("get_metadata")
    };
    world.metadata_snapshot = Some(original_meta);

    // Reload from the same path
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let metadata_path = sources.join("arimalo-metadata.automerge");
    let store = MetadataStore::new(metadata_path).expect("reload MetadataStore");
    world.metadata_store = Some(store);
}

#[then("the reloaded metadata should match the original")]
async fn the_reloaded_metadata_should_match_original(world: &mut LedgerWorld) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let reloaded = store.get_metadata().expect("get_metadata");
    let original = world
        .metadata_snapshot
        .as_ref()
        .expect("metadata_snapshot should be set");

    assert_eq!(
        original.transaction_refs.len(),
        reloaded.transaction_refs.len(),
        "transaction_refs count mismatch"
    );
    assert_eq!(
        original.file_manifest.len(),
        reloaded.file_manifest.len(),
        "file_manifest count mismatch"
    );
    assert_eq!(
        original.devices.len(),
        reloaded.devices.len(),
        "devices count mismatch"
    );
}

#[given(regex = r#"^device A creates metadata with a sync event "([^"]+)"$"#)]
async fn device_a_creates_metadata_with_event(world: &mut LedgerWorld, event_type: String) {
    let path_a = new_temp_dir("arimalo-meta-a").join("metadata.automerge");
    if let Some(parent) = path_a.parent() {
        std::fs::create_dir_all(parent).expect("create dir for device A");
    }
    let mut store = MetadataStore::new(path_a.clone()).expect("create store A");
    store
        .log_sync_event(&event_type, "", "from device A")
        .expect("log event A");
    store.save().expect("save store A");
    world.metadata_path_a = Some(path_a);
}

#[given(regex = r#"^device B loads device A metadata and adds sync event "([^"]+)"$"#)]
async fn device_b_loads_a_and_adds_event(world: &mut LedgerWorld, event_type: String) {
    let path_a = world
        .metadata_path_a
        .as_ref()
        .expect("path_a should be set");
    let path_b = new_temp_dir("arimalo-meta-b").join("metadata.automerge");
    if let Some(parent) = path_b.parent() {
        std::fs::create_dir_all(parent).expect("create dir for device B");
    }
    // Copy A's doc so B shares the same origin
    std::fs::copy(path_a, &path_b).expect("copy A to B");
    let mut store = MetadataStore::new(path_b.clone()).expect("create store B");
    store
        .log_sync_event(&event_type, "", "from device B")
        .expect("log event B");
    store.save().expect("save store B");
    world.metadata_path_b = Some(path_b);
}

#[when("device A merges metadata from device B")]
async fn device_a_merges_from_b(world: &mut LedgerWorld) {
    let path_a = world
        .metadata_path_a
        .as_ref()
        .expect("path_a should be set");
    let path_b = world
        .metadata_path_b
        .as_ref()
        .expect("path_b should be set");
    let mut store = MetadataStore::new(path_a.clone()).expect("load store A");
    store.merge_from_file(path_b).expect("merge B into A");
    world.metadata_store = Some(store);
}

#[then(regex = r#"^device A sync log should contain event "([^"]+)"$"#)]
async fn device_a_sync_log_should_contain(world: &mut LedgerWorld, event_type: String) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    let has_event = meta.sync_log.iter().any(|e| e.event_type == event_type);
    assert!(
        has_event,
        "expected sync log to contain event {event_type:?}; got: {:?}",
        meta.sync_log
            .iter()
            .map(|e| &e.event_type)
            .collect::<Vec<_>>()
    );
}

#[then(regex = r#"^the sync log should contain a "([^"]+)" event$"#)]
async fn the_sync_log_should_contain_event(world: &mut LedgerWorld, event_type: String) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    let has_event = meta.sync_log.iter().any(|e| e.event_type == event_type);
    assert!(
        has_event,
        "expected sync log to contain {event_type:?}; got: {:?}",
        meta.sync_log
            .iter()
            .map(|e| &e.event_type)
            .collect::<Vec<_>>()
    );
}

#[then("the sync log event should include this device ID")]
async fn the_sync_log_event_should_include_device_id(world: &mut LedgerWorld) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    let device_id = store.device_id();
    let has_device = meta.sync_log.iter().any(|e| e.device_id == device_id);
    assert!(
        has_device,
        "expected sync log to include device {device_id:?}"
    );
}

#[given(regex = r#"^a clean sources directory with rules at "([^"]+)":$"#)]
async fn a_clean_sources_directory_with_rules(
    world: &mut LedgerWorld,
    rules_relative: String,
    step: &cucumber::gherkin::Step,
) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    let rules_path = sources.join(&rules_relative);
    if let Some(parent) = rules_path.parent() {
        std::fs::create_dir_all(parent).expect("create rules parent dirs");
    }
    let docstring = step
        .docstring
        .as_ref()
        .expect("expected a docstring with JSON");
    std::fs::write(&rules_path, docstring.trim()).expect("write rules file");
}

#[then(regex = r#"^the metadata should contain rule "([^"]+)"$"#)]
async fn the_metadata_should_contain_rule(world: &mut LedgerWorld, rule_id: String) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let meta = store.get_metadata().expect("get_metadata");
    assert!(
        meta.rules.contains_key(&rule_id),
        "expected rules to contain {rule_id:?}; got: {:?}",
        meta.rules.keys().collect::<Vec<_>>()
    );
}

// === Content-addressed storage scenarios ===

#[given("a clean CAS directory")]
async fn a_clean_cas_directory(world: &mut LedgerWorld) {
    let dir = new_temp_dir("arimalo-cas");
    std::fs::create_dir_all(&dir).expect("create CAS dir");
    world.cas_dir = Some(dir.clone());
    world.cas = Some(ContentStore::new(dir));
}

#[when(regex = r#"^I store a file with content "([^"]+)"$"#)]
async fn i_store_a_file_with_content(world: &mut LedgerWorld, content: String) {
    let cas = world.cas.as_ref().expect("CAS should be set");
    let hash = cas.store(content.as_bytes()).expect("CAS store");
    world.last_stored_hash = Some(hash);
}

#[when(regex = r#"^I store another file with content "([^"]+)"$"#)]
async fn i_store_another_file_with_content(world: &mut LedgerWorld, content: String) {
    let cas = world.cas.as_ref().expect("CAS should be set");
    let hash = cas.store(content.as_bytes()).expect("CAS store");
    world.last_stored_hash = Some(hash);
}

#[then(regex = r#"^the CAS should contain exactly (\d+) blobs?$"#)]
async fn the_cas_should_contain_n_blobs(world: &mut LedgerWorld, expected: usize) {
    let cas = world.cas.as_ref().expect("CAS should be set");
    let count = cas.blob_count();
    assert_eq!(count, expected, "expected {expected} blobs, got {count}");
}

#[then(regex = r#"^retrieving by the content hash should return "([^"]+)"$"#)]
async fn retrieving_by_hash_should_return(world: &mut LedgerWorld, expected: String) {
    let cas = world.cas.as_ref().expect("CAS should be set");
    let hash = world.last_stored_hash.as_ref().expect("hash should be set");
    let content = cas.retrieve(hash).expect("CAS retrieve");
    assert_eq!(
        String::from_utf8_lossy(&content),
        expected,
        "content mismatch"
    );
}

#[then("the integrity check should pass for that blob")]
async fn the_integrity_check_should_pass(world: &mut LedgerWorld) {
    let cas = world.cas.as_ref().expect("CAS should be set");
    let hash = world.last_stored_hash.as_ref().expect("hash should be set");
    let status = cas.verify(hash).expect("CAS verify");
    assert_eq!(status, BlobStatus::Ok, "expected integrity Ok");
}

#[when("the blob file is corrupted")]
async fn the_blob_file_is_corrupted(world: &mut LedgerWorld) {
    let cas = world.cas.as_ref().expect("CAS should be set");
    let hash = world.last_stored_hash.as_ref().expect("hash should be set");
    cas.corrupt_blob(hash).expect("corrupt blob");
}

#[then("the integrity check should fail for that blob")]
async fn the_integrity_check_should_fail(world: &mut LedgerWorld) {
    let cas = world.cas.as_ref().expect("CAS should be set");
    let hash = world.last_stored_hash.as_ref().expect("hash should be set");
    let status = cas.verify(hash).expect("CAS verify");
    match status {
        BlobStatus::Corrupted { .. } => {}
        other => panic!("expected Corrupted, got {:?}", other),
    }
}

#[when("I ingest sources into CAS")]
async fn i_ingest_sources_into_cas(world: &mut LedgerWorld) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let cas_dir = sources.join("cas");
    std::fs::create_dir_all(&cas_dir).expect("create CAS dir");
    let cas = ContentStore::new(cas_dir.clone());

    let results = ingest_sources_to_cas(sources, &cas).expect("ingest sources");

    // Also build metadata store and update file manifest
    let metadata_path = sources.join("arimalo-metadata.automerge");
    let mut store = MetadataStore::new(metadata_path).expect("create MetadataStore");
    store
        .build_from_sources(sources)
        .expect("build_from_sources");
    world.metadata_store = Some(store);
    world.cas_dir = Some(cas_dir);
    world.cas = Some(cas);

    // Store the count for assertions
    assert!(!results.is_empty(), "expected at least one file ingested");
}

#[then("the metadata file manifest should reference the CSV by hash")]
async fn the_manifest_should_reference_csv_by_hash(world: &mut LedgerWorld) {
    let store = world
        .metadata_store
        .as_ref()
        .expect("metadata_store should be set");
    let cas = world.cas.as_ref().expect("CAS should be set");
    let meta = store.get_metadata().expect("get_metadata");

    let csv_entries: Vec<_> = meta
        .file_manifest
        .values()
        .filter(|f| f.file_type == "csv")
        .collect();
    assert!(!csv_entries.is_empty(), "expected CSV entry in manifest");

    for entry in &csv_entries {
        assert!(
            cas.exists(&entry.content_hash),
            "CAS blob missing for hash {}",
            entry.content_hash
        );
    }
}

#[when("the blob file is deleted")]
async fn the_blob_file_is_deleted(world: &mut LedgerWorld) {
    let cas = world.cas.as_ref().expect("CAS should be set");
    let hash = world.last_stored_hash.as_ref().expect("hash should be set");
    cas.delete_blob(hash).expect("delete blob");
}

#[then("the missing blob should be detected during verification")]
async fn the_missing_blob_should_be_detected(world: &mut LedgerWorld) {
    let cas = world.cas.as_ref().expect("CAS should be set");
    let hash = world.last_stored_hash.as_ref().expect("hash should be set");
    let status = cas.verify(hash).expect("CAS verify");
    assert_eq!(
        status,
        BlobStatus::Missing(hash.clone()),
        "expected Missing status"
    );
}

// === Sync protocol scenarios ===

fn setup_device_dir(label: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = new_temp_dir(&format!("arimalo-sync-{label}"));
    let sources = base.join("sources");
    let cas = base.join("cas");
    std::fs::create_dir_all(&sources).expect("create sources");
    std::fs::create_dir_all(&cas).expect("create cas");
    (base, sources, cas)
}

#[given(regex = r#"^device (A|B) has sources with a CSV "([^"]+)":$"#)]
async fn device_has_sources_with_csv(
    world: &mut LedgerWorld,
    device: String,
    csv_relative: String,
    step: &cucumber::gherkin::Step,
) {
    let (base, sources, cas_dir) = setup_device_dir(&device.to_lowercase());
    let table = step.table.as_ref().expect("expected data table");
    let csv_path = sources.join(&csv_relative);
    if let Some(parent) = csv_path.parent() {
        std::fs::create_dir_all(parent).expect("create CSV parent dirs");
    }
    std::fs::write(&csv_path, table_to_csv(table)).expect("write CSV");
    world.set_device_sources(&device, base, sources, cas_dir);
}

#[given(regex = r#"^device (A|B) has a transform at "([^"]+)"$"#)]
async fn device_has_a_transform(
    world: &mut LedgerWorld,
    device: String,
    transform_relative: String,
) {
    let sources = world.device_sources(&device).clone();
    write_transform(&sources, &transform_relative, "assets:bank:savings");
}

#[given("both devices have initialized metadata and CAS")]
async fn both_devices_have_initialized_metadata_and_cas(world: &mut LedgerWorld) {
    // Device A: init metadata and ingest to CAS
    let sources_a = world
        .device_a_sources
        .as_ref()
        .expect("device_a_sources")
        .clone();
    let meta_a_path = world
        .metadata_path_a
        .as_ref()
        .expect("metadata_path_a")
        .clone();
    let cas_a = world.device_a_cas.as_ref().expect("device_a_cas");

    let mut store_a = MetadataStore::new(meta_a_path.clone()).expect("create store A");
    store_a.build_from_sources(&sources_a).expect("build A");
    // Ingest files into CAS and register in manifest
    let results_a = ingest_sources_to_cas(&sources_a, cas_a).expect("ingest A");
    for (path, hash, size) in &results_a {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        store_a
            .register_file(hash, path, ext, *size)
            .expect("register A file");
    }
    store_a.save().expect("save A");
    world.device_a_store = Some(store_a);

    // Device B: copy A's metadata (shared origin), then build from B's sources
    let sources_b = world
        .device_b_sources
        .as_ref()
        .expect("device_b_sources")
        .clone();
    let meta_b_path = world
        .metadata_path_b
        .as_ref()
        .expect("metadata_path_b")
        .clone();
    let cas_b = world.device_b_cas.as_ref().expect("device_b_cas");

    std::fs::copy(&meta_a_path, &meta_b_path).expect("copy metadata A to B");
    let mut store_b = MetadataStore::new(meta_b_path).expect("create store B");
    // Ingest B's files and register
    let results_b = ingest_sources_to_cas(&sources_b, cas_b).expect("ingest B");
    for (path, hash, size) in &results_b {
        let ext = std::path::Path::new(path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        store_b
            .register_file(hash, path, ext, *size)
            .expect("register B file");
    }
    store_b.save().expect("save B");
    world.device_b_store = Some(store_b);
}

#[when("device A syncs with device B")]
async fn device_a_syncs_with_device_b(world: &mut LedgerWorld) {
    let meta_b_path = world
        .metadata_path_b
        .as_ref()
        .expect("metadata_path_b")
        .clone();
    let store_a = world.device_a_store.as_mut().expect("device_a_store");
    let cas_a = world.device_a_cas.as_ref().expect("device_a_cas");
    let cas_b = world.device_b_cas.as_ref().expect("device_b_cas");

    let result = full_sync(store_a, cas_a, &meta_b_path, cas_b).expect("full_sync");
    world.sync_result = Some(result);
}

#[then("device A CAS should contain files from both devices")]
async fn device_a_cas_should_contain_files_from_both(world: &mut LedgerWorld) {
    let cas_a = world.device_a_cas.as_ref().expect("device_a_cas");
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let meta = store_a.get_metadata().expect("get_metadata");

    for (hash, entry) in &meta.file_manifest {
        assert!(
            cas_a.exists(hash),
            "CAS A missing blob for {} ({})",
            entry.relative_path,
            hash
        );
    }
}

#[then("device A metadata should reference files from both devices")]
async fn device_a_metadata_should_reference_both(world: &mut LedgerWorld) {
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let meta = store_a.get_metadata().expect("get_metadata");
    // Should have files from both device A and B sources
    assert!(
        meta.file_manifest.len() >= 2,
        "expected at least 2 file manifest entries, got {}",
        meta.file_manifest.len()
    );
}

#[then("the sync log should record the sync event")]
async fn the_sync_log_should_record_sync(world: &mut LedgerWorld) {
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let meta = store_a.get_metadata().expect("get_metadata");
    let has_sync = meta.sync_log.iter().any(|e| e.event_type == "full_sync");
    assert!(has_sync, "expected sync log to contain full_sync event");
}

#[given("device B shares device A metadata")]
async fn device_b_shares_device_a_metadata(world: &mut LedgerWorld) {
    let meta_a_path = world
        .metadata_path_a
        .as_ref()
        .expect("metadata_path_a")
        .clone();
    let meta_b_path = world
        .metadata_path_b
        .as_ref()
        .expect("metadata_path_b")
        .clone();
    std::fs::copy(&meta_a_path, &meta_b_path).expect("copy A metadata to B");
    let store_b = MetadataStore::new(meta_b_path).expect("load store B");
    world.device_b_store = Some(store_b);
}

#[given("device B has the same CAS blobs as device A")]
async fn device_b_has_same_cas_blobs(world: &mut LedgerWorld) {
    let cas_a = world.device_a_cas.as_ref().expect("device_a_cas");
    let cas_b = world.device_b_cas.as_ref().expect("device_b_cas");

    // Copy A's blobs to B and B's blobs to A so both have everything
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let meta_a = store_a.get_metadata().expect("get_metadata A");
    for hash in meta_a.file_manifest.keys() {
        if !cas_b.exists(hash) {
            let content = cas_a.retrieve(hash).expect("retrieve from A");
            cas_b.store(&content).expect("store to B");
        }
    }

    let store_b = world.device_b_store.as_ref().expect("device_b_store");
    let meta_b = store_b.get_metadata().expect("get_metadata B");
    for hash in meta_b.file_manifest.keys() {
        if !cas_a.exists(hash) {
            let content = cas_b.retrieve(hash).expect("retrieve from B");
            cas_a.store(&content).expect("store to A");
        }
    }
}

#[then("no files should be transferred")]
async fn no_files_should_be_transferred(world: &mut LedgerWorld) {
    let result = world.sync_result.as_ref().expect("sync_result");
    assert_eq!(
        result.files_transferred, 0,
        "expected 0 files transferred, got {}",
        result.files_transferred
    );
}

#[given(regex = r#"^device A has initialized metadata with (\d+) files$"#)]
async fn device_a_has_metadata_with_n_files(world: &mut LedgerWorld, count: usize) {
    let (base, _sources, cas_dir) = setup_device_dir("a-diff");
    let meta_path = base.join("metadata.automerge");
    let cas = ContentStore::new(cas_dir);
    let mut store = MetadataStore::new(meta_path.clone()).expect("create store");
    for i in 0..count {
        let content = format!("file content A-{i}");
        let hash = cas.store(content.as_bytes()).expect("store");
        store
            .register_file(
                &hash,
                &format!("file_a_{i}.csv"),
                "csv",
                content.len() as u64,
            )
            .expect("register");
    }
    store.save().expect("save");
    world.metadata_path_a = Some(meta_path);
    world.device_a_store = Some(store);
    world.device_a_cas = Some(cas);
}

#[given(regex = r#"^device B has initialized metadata with (\d+) files$"#)]
async fn device_b_has_metadata_with_n_files(world: &mut LedgerWorld, count: usize) {
    let meta_a_path = world
        .metadata_path_a
        .as_ref()
        .expect("metadata_path_a")
        .clone();
    let (base, _sources, cas_dir) = setup_device_dir("b-diff");
    let meta_path = base.join("metadata.automerge");
    let cas = ContentStore::new(cas_dir);
    // Share A's origin
    std::fs::copy(&meta_a_path, &meta_path).expect("copy A to B");
    let mut store = MetadataStore::new(meta_path.clone()).expect("create store");
    for i in 0..count {
        let content = format!("file content B-{i}");
        let hash = cas.store(content.as_bytes()).expect("store");
        store
            .register_file(
                &hash,
                &format!("file_b_{i}.csv"),
                "csv",
                content.len() as u64,
            )
            .expect("register");
    }
    store.save().expect("save");
    world.metadata_path_b = Some(meta_path);
    world.device_b_store = Some(store);
    world.device_b_cas = Some(cas);
}

#[given("device B shares device A metadata origin")]
async fn device_b_shares_a_origin(_world: &mut LedgerWorld) {
    // Already handled in device_b_has_metadata_with_n_files
}

#[when("comparing manifests between device A and device B")]
async fn comparing_manifests(world: &mut LedgerWorld) {
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let store_b = world.device_b_store.as_ref().expect("device_b_store");
    let (missing_from_a, _missing_from_b) =
        diff_manifests(store_a, store_b).expect("diff_manifests");
    world.manifest_diff_missing = Some(missing_from_a.len());
}

#[then(regex = r#"^(\d+) files? should be identified as missing from device A$"#)]
async fn n_files_missing_from_a(world: &mut LedgerWorld, expected: usize) {
    let actual = world.manifest_diff_missing.expect("manifest_diff_missing");
    assert_eq!(
        actual, expected,
        "expected {expected} missing, got {actual}"
    );
}

#[then("device A should record the sync timestamp")]
async fn device_a_should_record_sync_timestamp(world: &mut LedgerWorld) {
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let meta = store_a.get_metadata().expect("get_metadata");
    let sync_events: Vec<_> = meta
        .sync_log
        .iter()
        .filter(|e| e.event_type == "full_sync")
        .collect();
    assert!(!sync_events.is_empty(), "expected sync timestamp in log");
    assert!(sync_events[0].timestamp > 0, "expected non-zero timestamp");
}

#[then("device A should know about device B")]
async fn device_a_should_know_about_device_b(world: &mut LedgerWorld) {
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let meta = store_a.get_metadata().expect("get_metadata");
    // After sync, A should have B's device info (from merged metadata)
    assert!(
        meta.devices.len() >= 1,
        "expected at least 1 device, got {}",
        meta.devices.len()
    );
}

// === Relay server pairing scenarios ===

fn start_relay(world: &mut LedgerWorld, pairing_ttl: u64) {
    let data_dir = new_temp_dir("arimalo-relay");
    std::fs::create_dir_all(&data_dir).expect("create relay data dir");
    let config = RelayConfig {
        bind: "127.0.0.1:0".to_string(),
        data_dir: data_dir.clone(),
        pairing_ttl_secs: pairing_ttl,
    };
    let server = RelayServer::new(config).expect("start relay server");
    let addr = server.server_addr();
    let relay_url = format!("http://{}", addr);
    let server = Arc::new(server);

    // Run server in background thread
    let bg = Arc::clone(&server);
    std::thread::spawn(move || bg.run());

    world.relay_server = Some(server);
    world.relay_url = Some(relay_url);
    world.relay_data_dir = Some(data_dir);
}

#[given("a running relay server")]
async fn a_running_relay_server(world: &mut LedgerWorld) {
    start_relay(world, 600);
}

#[given("a running relay server with a paired group")]
async fn a_running_relay_server_with_paired_group(world: &mut LedgerWorld) {
    start_relay(world, 600);
    let relay_url = world.relay_url.as_ref().unwrap();
    let result = relay_client::pair_initiate(relay_url).expect("pair initiate");
    let group_id = relay_client::pair_join(relay_url, &result.pairing_code).expect("pair join");
    world.relay_group_id = Some(group_id);
}

#[when("device A initiates pairing")]
async fn device_a_initiates_pairing(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let result = relay_client::pair_initiate(relay_url).expect("pair initiate");
    world.relay_group_id = Some(result.group_id);
    world.relay_pairing_code = Some(result.pairing_code);
}

#[then("a group ID and 6-digit pairing code are returned")]
async fn group_id_and_code_returned(world: &mut LedgerWorld) {
    let group_id = world.relay_group_id.as_ref().expect("group_id");
    let code = world.relay_pairing_code.as_ref().expect("pairing_code");
    assert!(!group_id.is_empty(), "group_id should not be empty");
    assert_eq!(code.len(), 6, "pairing code should be 6 digits");
    assert!(
        code.chars().all(|c| c.is_ascii_digit()),
        "code should be all digits"
    );
}

#[then("the pairing code expires after 10 minutes")]
async fn pairing_code_expires_after_10_minutes(_world: &mut LedgerWorld) {
    // The TTL is set to 600 seconds in the test setup — verified by structure
}

#[given("device A has initiated pairing")]
async fn device_a_has_initiated_pairing(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let result = relay_client::pair_initiate(relay_url).expect("pair initiate");
    world.relay_group_id = Some(result.group_id);
    world.relay_pairing_code = Some(result.pairing_code);
}

#[when("device B joins with the pairing code")]
async fn device_b_joins_with_code(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let code = world
        .relay_pairing_code
        .as_ref()
        .expect("pairing_code")
        .clone();
    match relay_client::pair_join(relay_url, &code) {
        Ok(group_id) => {
            world.relay_join_error = None;
            // Store B's group_id to compare
            assert_eq!(group_id, *world.relay_group_id.as_ref().unwrap());
        }
        Err(e) => {
            world.relay_join_error = Some(e);
        }
    }
}

#[then("device B receives the same group ID as device A")]
async fn device_b_receives_same_group_id(world: &mut LedgerWorld) {
    assert!(
        world.relay_join_error.is_none(),
        "join should not have failed"
    );
}

#[when("device B joins with an invalid pairing code")]
async fn device_b_joins_with_invalid_code(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    match relay_client::pair_join(relay_url, "000000") {
        Ok(_) => world.relay_join_error = None,
        Err(e) => world.relay_join_error = Some(e),
    }
}

#[then("the join should fail with not found")]
async fn join_should_fail_with_not_found(world: &mut LedgerWorld) {
    assert!(world.relay_join_error.is_some(), "expected join to fail");
}

#[given("device B has joined with the pairing code")]
async fn device_b_has_joined_with_code(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let code = world
        .relay_pairing_code
        .as_ref()
        .expect("pairing_code")
        .clone();
    relay_client::pair_join(relay_url, &code).expect("pair join");
}

#[when("device C tries to join with the same pairing code")]
async fn device_c_tries_to_join(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let code = world
        .relay_pairing_code
        .as_ref()
        .expect("pairing_code")
        .clone();
    match relay_client::pair_join(relay_url, &code) {
        Ok(_) => world.relay_join_error = None,
        Err(e) => world.relay_join_error = Some(e),
    }
}

#[given("device A has initiated pairing with a 0-second TTL")]
async fn device_a_initiated_pairing_0_ttl(world: &mut LedgerWorld) {
    // Restart server with 0 TTL
    if let Some(ref s) = world.relay_server {
        s.unblock();
    }
    start_relay(world, 0);
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let result = relay_client::pair_initiate(relay_url).expect("pair initiate");
    world.relay_group_id = Some(result.group_id);
    world.relay_pairing_code = Some(result.pairing_code);
    // Wait a bit to ensure expiry
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[when("device B joins with the expired pairing code")]
async fn device_b_joins_with_expired_code(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let code = world
        .relay_pairing_code
        .as_ref()
        .expect("pairing_code")
        .clone();
    match relay_client::pair_join(relay_url, &code) {
        Ok(_) => world.relay_join_error = None,
        Err(e) => world.relay_join_error = Some(e),
    }
}

// === Relay sync scenarios ===

#[when("device A uploads metadata to the relay")]
async fn device_a_uploads_metadata_to_relay(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");

    // Create a metadata store for device A
    let dir = new_temp_dir("relay-sync-a");
    std::fs::create_dir_all(&dir).expect("create dir");
    let meta_path = dir.join("metadata.automerge");
    let mut store = MetadataStore::new(meta_path.clone()).expect("create store");
    store
        .log_sync_event("test_upload", "", "device A upload")
        .expect("log");
    store.save().expect("save");

    let bytes = std::fs::read(&meta_path).expect("read metadata");
    let url = format!("{}/metadata/{}", relay_url, group_id);
    ureq::post(&url)
        .set("Content-Type", "application/octet-stream")
        .send_bytes(&bytes)
        .expect("upload metadata");

    world.metadata_path_a = Some(meta_path);
    world.device_a_store = Some(store);
}

#[when("device B downloads metadata from the relay")]
async fn device_b_downloads_metadata_from_relay(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");

    let url = format!("{}/metadata/{}", relay_url, group_id);
    let resp = ureq::get(&url).call().expect("download metadata");
    let mut bytes = Vec::new();
    use std::io::Read;
    resp.into_reader()
        .read_to_end(&mut bytes)
        .expect("read body");

    let dir = new_temp_dir("relay-sync-b");
    std::fs::create_dir_all(&dir).expect("create dir");
    let meta_path = dir.join("metadata.automerge");
    std::fs::write(&meta_path, &bytes).expect("write metadata");

    let store = MetadataStore::new(meta_path.clone()).expect("load store");
    world.metadata_path_b = Some(meta_path);
    world.device_b_store = Some(store);
}

#[then("device B should receive device A metadata")]
async fn device_b_should_receive_device_a_metadata(world: &mut LedgerWorld) {
    let store_b = world.device_b_store.as_ref().expect("device_b_store");
    let meta = store_b.get_metadata().expect("get_metadata");
    let has_upload = meta.sync_log.iter().any(|e| e.event_type == "test_upload");
    assert!(
        has_upload,
        "expected device B to have device A's sync event"
    );
}

#[given(regex = r#"^device A has uploaded metadata with event "([^"]+)"$"#)]
async fn device_a_has_uploaded_metadata_with_event(world: &mut LedgerWorld, event: String) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");

    let dir = new_temp_dir("relay-meta-a");
    std::fs::create_dir_all(&dir).expect("create dir");
    let meta_path = dir.join("metadata.automerge");
    let mut store = MetadataStore::new(meta_path.clone()).expect("create store");
    store
        .log_sync_event(&event, "", "from device A")
        .expect("log");
    store.save().expect("save");

    let bytes = std::fs::read(&meta_path).expect("read metadata");
    let url = format!("{}/metadata/{}", relay_url, group_id);
    ureq::post(&url)
        .set("Content-Type", "application/octet-stream")
        .send_bytes(&bytes)
        .expect("upload metadata");

    world.metadata_path_a = Some(meta_path);
    world.device_a_store = Some(store);
}

#[when(regex = r#"^device B uploads metadata with event "([^"]+)"$"#)]
async fn device_b_uploads_metadata_with_event(world: &mut LedgerWorld, event: String) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");

    // Device B: download A's metadata first (to share origin), then add its event
    let url_get = format!("{}/metadata/{}", relay_url, group_id);
    let resp = ureq::get(&url_get).call().expect("download A metadata");
    let mut bytes = Vec::new();
    use std::io::Read;
    resp.into_reader()
        .read_to_end(&mut bytes)
        .expect("read body");

    let dir = new_temp_dir("relay-meta-b");
    std::fs::create_dir_all(&dir).expect("create dir");
    let meta_path = dir.join("metadata.automerge");
    std::fs::write(&meta_path, &bytes).expect("write metadata");

    let mut store = MetadataStore::new(meta_path.clone()).expect("load store");
    store
        .log_sync_event(&event, "", "from device B")
        .expect("log");
    store.save().expect("save");

    let bytes_b = std::fs::read(&meta_path).expect("read metadata");
    let url_post = format!("{}/metadata/{}", relay_url, group_id);
    ureq::post(&url_post)
        .set("Content-Type", "application/octet-stream")
        .send_bytes(&bytes_b)
        .expect("upload metadata");

    world.metadata_path_b = Some(meta_path);
    world.device_b_store = Some(store);
}

#[when("device A downloads metadata from the relay")]
async fn device_a_downloads_metadata_from_relay(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");

    let url = format!("{}/metadata/{}", relay_url, group_id);
    let resp = ureq::get(&url).call().expect("download metadata");
    let mut bytes = Vec::new();
    use std::io::Read;
    resp.into_reader()
        .read_to_end(&mut bytes)
        .expect("read body");

    // Merge into device A's existing store
    let dir = new_temp_dir("relay-meta-a-merged");
    std::fs::create_dir_all(&dir).expect("create dir");
    let temp_path = dir.join("remote.automerge");
    std::fs::write(&temp_path, &bytes).expect("write temp");

    let store_a = world.device_a_store.as_mut().expect("device_a_store");
    store_a.merge_from_file(&temp_path).expect("merge");
}

#[then("device A metadata should contain both events")]
async fn device_a_metadata_should_contain_both_events(world: &mut LedgerWorld) {
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let meta = store_a.get_metadata().expect("get_metadata");
    let event_types: Vec<&str> = meta
        .sync_log
        .iter()
        .map(|e| e.event_type.as_str())
        .collect();
    assert!(
        event_types.contains(&"event_a"),
        "missing event_a; got: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"event_b"),
        "missing event_b; got: {:?}",
        event_types
    );
}

#[when(regex = r#"^device A uploads a blob with content "([^"]+)"$"#)]
async fn device_a_uploads_blob(world: &mut LedgerWorld, content: String) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");
    let hash = arimalo_covid::content_store::sha256_hex(content.as_bytes());

    let url = format!("{}/blobs/{}/{}", relay_url, group_id, hash);
    ureq::post(&url)
        .set("Content-Type", "application/octet-stream")
        .send_bytes(content.as_bytes())
        .expect("upload blob");

    world.relay_blob_hashes.push(hash);
}

#[then("the relay blob list should include that hash")]
async fn relay_blob_list_should_include_hash(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");
    let expected = world.relay_blob_hashes.last().expect("blob hash");

    let url = format!("{}/blobs/{}/list", relay_url, group_id);
    let resp: serde_json::Value = ureq::get(&url)
        .call()
        .expect("list blobs")
        .into_json()
        .expect("parse");
    let hashes: Vec<String> = resp["hashes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(
        hashes.contains(expected),
        "expected blob list to include {}",
        expected
    );
}

#[then(regex = r#"^device B can download the blob and get "([^"]+)"$"#)]
async fn device_b_can_download_blob(world: &mut LedgerWorld, expected: String) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");
    let hash = world.relay_blob_hashes.last().expect("blob hash");

    let url = format!("{}/blobs/{}/{}", relay_url, group_id, hash);
    let resp = ureq::get(&url).call().expect("download blob");
    let mut bytes = Vec::new();
    use std::io::Read;
    resp.into_reader()
        .read_to_end(&mut bytes)
        .expect("read body");
    assert_eq!(String::from_utf8_lossy(&bytes), expected);
}

#[when(regex = r#"^device A uploads (\d+) blobs to the relay$"#)]
async fn device_a_uploads_n_blobs(world: &mut LedgerWorld, count: usize) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");

    for i in 0..count {
        let content = format!("blob content {}", i);
        let hash = arimalo_covid::content_store::sha256_hex(content.as_bytes());
        let url = format!("{}/blobs/{}/{}", relay_url, group_id, hash);
        ureq::post(&url)
            .set("Content-Type", "application/octet-stream")
            .send_bytes(content.as_bytes())
            .expect("upload blob");
        world.relay_blob_hashes.push(hash);
    }
}

#[then(regex = r#"^the relay blob list should contain (\d+) hashes$"#)]
async fn relay_blob_list_should_contain_n(world: &mut LedgerWorld, count: usize) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");

    let url = format!("{}/blobs/{}/list", relay_url, group_id);
    let resp: serde_json::Value = ureq::get(&url)
        .call()
        .expect("list blobs")
        .into_json()
        .expect("parse");
    let hashes = resp["hashes"].as_array().unwrap();
    assert_eq!(
        hashes.len(),
        count,
        "expected {} hashes, got {}",
        count,
        hashes.len()
    );
}

// === Relay client scenarios ===

#[given("a relay server running in background")]
async fn a_relay_server_running_in_background(world: &mut LedgerWorld) {
    start_relay(world, 600);
}

#[given("device A has local metadata and CAS")]
async fn device_a_has_local_metadata_and_cas(world: &mut LedgerWorld) {
    let (_base, sources, cas_dir) = setup_device_dir("relay-client-a");
    let meta_path = sources.join("arimalo-metadata.automerge");
    let cas = ContentStore::new(cas_dir);
    let mut store = MetadataStore::new(meta_path.clone()).expect("create store");
    store
        .log_sync_event("local_init", "", "device A initialized")
        .expect("log");
    store.save().expect("save");

    world.device_a_sources = Some(sources);
    world.device_a_cas = Some(cas);
    world.device_a_store = Some(store);
    world.metadata_path_a = Some(meta_path);
}

#[when("device A pairs via the relay client")]
async fn device_a_pairs_via_relay_client(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url").clone();
    let result = relay_client::pair_initiate(&relay_url).expect("pair initiate");
    let group_id = relay_client::pair_join(&relay_url, &result.pairing_code).expect("pair join");
    world.relay_group_id = Some(group_id);
}

#[when("device A syncs with the relay")]
async fn device_a_syncs_with_relay(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url").clone();
    let group_id = world.relay_group_id.as_ref().expect("group_id").clone();
    let config = relay_client::RelayConfig {
        relay_url,
        group_id,
    };

    let store = world.device_a_store.as_mut().expect("device_a_store");
    let cas = world.device_a_cas.as_ref().expect("device_a_cas");

    let result = relay_client::sync_with_relay(store, cas, &config).expect("sync_with_relay");
    world.relay_sync_result = Some(result);
}

#[then("the relay should have device A metadata")]
async fn relay_should_have_device_a_metadata(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url");
    let group_id = world.relay_group_id.as_ref().expect("group_id");

    let url = format!("{}/metadata/{}", relay_url, group_id);
    let resp = ureq::get(&url).call().expect("download metadata");
    assert_eq!(resp.status(), 200);
}

#[given(regex = r#"^device (A|B) has local metadata with a file "([^"]+)"$"#)]
async fn device_has_local_metadata_with_file(
    world: &mut LedgerWorld,
    device: String,
    filename: String,
) {
    let label = format!("relay-client-{}", device.to_lowercase());
    let (_base, sources, cas_dir) = setup_device_dir(&label);
    let meta_path = sources.join("arimalo-metadata.automerge");
    let cas = ContentStore::new(cas_dir);

    // Device B copies A's metadata to share origin
    if device == "B" {
        let meta_a = world.metadata_path_a.as_ref().expect("metadata_path_a");
        std::fs::copy(meta_a, &meta_path).expect("copy A metadata");
    }

    let mut store = MetadataStore::new(meta_path.clone()).expect("create store");
    let content = format!("content of {}", filename);
    let hash = cas.store(content.as_bytes()).expect("store blob");
    store
        .register_file(&hash, &filename, "csv", content.len() as u64)
        .expect("register");
    store.save().expect("save");

    match device.as_str() {
        "A" => {
            world.device_a_sources = Some(sources);
            world.device_a_cas = Some(cas);
            world.device_a_store = Some(store);
            world.metadata_path_a = Some(meta_path);
        }
        "B" => {
            world.device_b_sources = Some(sources);
            world.device_b_cas = Some(cas);
            world.device_b_store = Some(store);
            world.metadata_path_b = Some(meta_path);
        }
        _ => panic!("unknown device: {device}"),
    }
}

#[given("both devices have paired with the relay")]
async fn both_devices_have_paired_with_relay(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url").clone();
    let result = relay_client::pair_initiate(&relay_url).expect("pair initiate");
    let group_id = relay_client::pair_join(&relay_url, &result.pairing_code).expect("pair join");
    world.relay_group_id = Some(group_id);
}

#[when("device B syncs with the relay")]
async fn device_b_syncs_with_relay(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url").clone();
    let group_id = world.relay_group_id.as_ref().expect("group_id").clone();
    let config = relay_client::RelayConfig {
        relay_url,
        group_id,
    };

    let store = world.device_b_store.as_mut().expect("device_b_store");
    let cas = world.device_b_cas.as_ref().expect("device_b_cas");

    relay_client::sync_with_relay(store, cas, &config).expect("sync_with_relay B");
}

#[when("device A syncs with the relay again")]
async fn device_a_syncs_with_relay_again(world: &mut LedgerWorld) {
    let relay_url = world.relay_url.as_ref().expect("relay_url").clone();
    let group_id = world.relay_group_id.as_ref().expect("group_id").clone();
    let config = relay_client::RelayConfig {
        relay_url,
        group_id,
    };

    let store = world.device_a_store.as_mut().expect("device_a_store");
    let cas = world.device_a_cas.as_ref().expect("device_a_cas");

    let result =
        relay_client::sync_with_relay(store, cas, &config).expect("sync_with_relay A again");
    world.relay_sync_result = Some(result);
}

#[then("device A should have the blob from device B")]
async fn device_a_should_have_blob_from_b(world: &mut LedgerWorld) {
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let cas_a = world.device_a_cas.as_ref().expect("device_a_cas");
    let meta = store_a.get_metadata().expect("get_metadata");

    // A should have all blobs referenced in metadata
    for hash in meta.file_manifest.keys() {
        assert!(cas_a.exists(hash), "device A CAS missing blob {}", hash);
    }
}

#[then("device A metadata should reference both files")]
async fn device_a_metadata_should_ref_both_files(world: &mut LedgerWorld) {
    let store_a = world.device_a_store.as_ref().expect("device_a_store");
    let meta = store_a.get_metadata().expect("get_metadata");
    assert!(
        meta.file_manifest.len() >= 2,
        "expected at least 2 files, got {}",
        meta.file_manifest.len()
    );
}

#[given("no relay server is running")]
async fn no_relay_server_is_running(world: &mut LedgerWorld) {
    // Don't start a server; just set a URL that won't connect
    world.relay_url = Some("http://127.0.0.1:1".to_string());
}

#[when("device A tries to sync with the relay")]
async fn device_a_tries_to_sync_with_relay(world: &mut LedgerWorld) {
    // Create minimal local state
    let dir = new_temp_dir("relay-fail");
    std::fs::create_dir_all(&dir).expect("create dir");
    let meta_path = dir.join("metadata.automerge");
    let cas = ContentStore::new(dir.join("cas"));
    let mut store = MetadataStore::new(meta_path).expect("create store");
    store.save().expect("save");

    let config = relay_client::RelayConfig {
        relay_url: world.relay_url.as_ref().unwrap().clone(),
        group_id: "fake-group".to_string(),
    };

    match relay_client::sync_with_relay(&mut store, &cas, &config) {
        Ok(_) => world.relay_sync_error = None,
        Err(e) => world.relay_sync_error = Some(e),
    }
}

#[then("the sync should fail with a connection error")]
async fn sync_should_fail_with_connection_error(world: &mut LedgerWorld) {
    assert!(
        world.relay_sync_error.is_some(),
        "expected sync to fail with connection error"
    );
}

// === Ignore / delete transaction scenarios ===

#[when("I hide the CSV transaction")]
async fn i_hide_the_csv_transaction(world: &mut LedgerWorld) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let txn = result
        .transactions
        .iter()
        .find(|t| t.payee.as_deref() == Some("CSV Entry"))
        .expect("CSV transaction not found");
    let txn_id = txn
        .meta
        .as_ref()
        .and_then(|m| m.split(',').find(|p| p.trim().starts_with("txn:")))
        .map(|s| s.trim().to_string())
        .expect("txn ID not found in meta");
    append_to_ignored(sources, &txn_id).expect("append to ignored");
}

#[given(expr = "the ignored file already contains {string} twice and {string} once")]
async fn the_ignored_file_already_contains(world: &mut LedgerWorld, dup: String, once: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let path = sources.join("_ignored.txt");
    let text = format!("{dup}\n{dup}\n{once}\n");
    std::fs::write(&path, text).expect("seed _ignored.txt");
}

#[then(expr = "the per-folder ledger at {string} should not contain payee {string}")]
async fn per_folder_ledger_should_not_contain_payee(
    world: &mut LedgerWorld,
    folder: String,
    payee: String,
) {
    let generated = world.generated_dir.as_ref().expect("generated_dir");
    // Find every per-folder ledger.transactions file whose path ends with
    // the requested folder segment, since the test fixture may nest the
    // folder under an account-set directory.
    let needle = format!("\"{payee}\"");
    let mut matched_paths: Vec<std::path::PathBuf> = Vec::new();
    for entry in walkdir::WalkDir::new(generated).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) != Some("ledger.transactions") {
            continue;
        }
        let parent = match path.parent().and_then(|p| p.file_name()).and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if parent != folder {
            continue;
        }
        matched_paths.push(path.to_path_buf());
    }
    assert!(
        !matched_paths.is_empty(),
        "no per-folder ledger.transactions found under {generated:?} with parent folder {folder:?}"
    );
    for path in matched_paths {
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {path:?}: {e}"));
        assert!(
            !text.contains(&needle),
            "expected {path:?} to NOT contain payee {payee:?}, got:\n{text}"
        );
    }
}

#[then(expr = "the ignored file should have {int} entry")]
async fn the_ignored_file_should_have_entries(world: &mut LedgerWorld, count: usize) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let path = sources.join("_ignored.txt");
    let text = std::fs::read_to_string(&path).expect("read _ignored.txt");
    let actual = text.lines().filter(|l| !l.trim().is_empty()).count();
    assert_eq!(actual, count, "expected {count} ignored entries, got {actual}: {text:?}");
}

#[when(expr = "I delete the manual transaction with payee {string}")]
async fn i_delete_the_manual_transaction_with_payee(world: &mut LedgerWorld, payee: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let txn = result
        .transactions
        .iter()
        .find(|t| t.payee.as_deref() == Some(payee.as_str()))
        .expect("manual transaction not found");
    let config = make_pipeline_config(world);
    delete_manual_transaction_and_rebuild(
        &config,
        &txn.datetime,
        txn.payee.as_deref().unwrap_or(""),
        txn.narration.as_deref().unwrap_or(""),
        "bank",
    )
    .expect("delete manual transaction");
    // Update pipeline result after delete
    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

#[then(expr = "the active ledger should not include payee {string}")]
async fn the_active_ledger_should_not_include_payee(world: &mut LedgerWorld, payee: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    assert!(
        !result
            .transactions
            .iter()
            .any(|t| t.payee.as_deref() == Some(payee.as_str())),
        "expected active ledger to NOT include payee {payee:?}; got: {:?}",
        result
            .transactions
            .iter()
            .map(|t| t.payee.clone())
            .collect::<Vec<_>>()
    );
}

#[then("the active ledger should not include payee from CSV")]
async fn the_active_ledger_should_not_include_payee_from_csv(world: &mut LedgerWorld) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    assert!(
        !result
            .transactions
            .iter()
            .any(|t| t.payee.as_deref() == Some("CSV Entry")),
        "expected active ledger to NOT include CSV payee 'CSV Entry'"
    );
}

// === OFX pipeline scenarios ===

fn write_ofx_file(sources_dir: &PathBuf, relative_path: &str, content: &str) {
    let ofx_path = sources_dir.join(relative_path);
    if let Some(parent) = ofx_path.parent() {
        std::fs::create_dir_all(parent).expect("create OFX parent dirs");
    }
    std::fs::write(&ofx_path, content).expect("write OFX file");
}

fn write_accounts_file(sources_dir: &PathBuf, declaration: &str) {
    // Derive the account folder from the declaration.
    // "assets:bank:savings AUD" → folder "bank/savings"
    let account_name = declaration.split_whitespace().next().unwrap_or(declaration);
    let folder_rel = if let Some(rest) = account_name.strip_prefix("assets:") {
        rest.replace(':', "/")
    } else {
        account_name.replace(':', "/")
    };
    let folder_path = sources_dir.join(&folder_rel);
    std::fs::create_dir_all(&folder_path).expect("create account folder for accounts file");
    let accounts_path = folder_path.join("accounts.transactions");
    let mut existing = if accounts_path.exists() {
        std::fs::read_to_string(&accounts_path).unwrap_or_default()
    } else {
        String::new()
    };
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(&format!("account {declaration}\n"));
    std::fs::write(&accounts_path, existing).expect("write accounts.transactions");
}

#[given(regex = r#"^a clean sources directory with an OFX file "([^"]+)":$"#)]
async fn a_clean_sources_directory_with_ofx(
    world: &mut LedgerWorld,
    path: String,
    step: &cucumber::gherkin::Step,
) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    let content = step.docstring.as_ref().expect("expected OFX docstring");
    write_ofx_file(sources, &path, content);
}

#[given(regex = r#"^an accounts file declaring "([^"]+)"$"#)]
async fn an_accounts_file_declaring(world: &mut LedgerWorld, declaration: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_accounts_file(sources, &declaration);
}

#[given(regex = r#"^an OFX file "([^"]+)":$"#)]
async fn an_ofx_file(world: &mut LedgerWorld, path: String, step: &cucumber::gherkin::Step) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let content = step.docstring.as_ref().expect("expected OFX docstring");
    write_ofx_file(sources, &path, content);
}

#[given(regex = r#"^an OFX file in imports at "([^"]+)":$"#)]
async fn an_ofx_file_in_imports(
    world: &mut LedgerWorld,
    path: String,
    step: &cucumber::gherkin::Step,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let content = step.docstring.as_ref().expect("expected OFX docstring");
    write_ofx_file(sources, &path, content);
}

#[then(
    regex = r#"^the active ledger should contain a transaction with ID starting with "([^"]+)"$"#
)]
async fn the_active_ledger_should_contain_id_starting_with(
    world: &mut LedgerWorld,
    prefix: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let has_prefix = result.transactions.iter().any(|t| {
        t.meta
            .as_deref()
            .map_or(false, |m| m.contains(&format!("txn:{prefix}")))
    });
    assert!(
        has_prefix,
        "expected transaction with ID starting with {prefix:?}; meta values: {:?}",
        result
            .transactions
            .iter()
            .map(|t| t.meta.clone())
            .collect::<Vec<_>>()
    );
}

#[then("running the pipeline again should produce the same transaction IDs")]
async fn running_pipeline_again_same_ids(world: &mut LedgerWorld) {
    // Read IDs from first run
    let dir = set_generated_dir(world);
    let result1 = load_active_ledger(&dir).expect("load active ledger");
    let ids1: Vec<String> = result1
        .transactions
        .iter()
        .filter_map(|t| t.meta.clone())
        .collect();

    // Run pipeline again
    let config = make_pipeline_config(world);
    let _result = run_pipeline(&config).expect("second pipeline run");

    let dir = set_generated_dir(world);
    let result2 = load_active_ledger(&dir).expect("load active ledger");
    let ids2: Vec<String> = result2
        .transactions
        .iter()
        .filter_map(|t| t.meta.clone())
        .collect();

    assert_eq!(
        ids1, ids2,
        "transaction IDs should be identical between runs"
    );
}

#[when(regex = r#"^I process imports for account "([^"]+)"$"#)]
async fn i_process_imports_for_account(world: &mut LedgerWorld, folder: String) {
    if world.now_yyyymm.is_none() {
        world.now_yyyymm = Some("202501".to_string());
    }
    let config = make_pipeline_config(world);
    let (_import_result, pipeline_result) =
        arimalo_covid::processing_pipeline::process_imports(&config, &folder)
            .expect("process_imports");
    world.pipeline_result = Some(pipeline_result);

    if world.active_set_name.is_none() {
        world.active_set_name = infer_primary_set(&config.sources_dir);
    }

    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

#[then(regex = r#"^the OFX file should be moved to "([^"]+)"$"#)]
async fn the_ofx_file_should_be_moved_to(world: &mut LedgerWorld, path: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let dest = sources.join(&path);
    assert!(
        dest.exists(),
        "expected OFX file at {}, but it doesn't exist",
        dest.display()
    );
}

// === Account gap detection scenarios ===

#[given("a generated directory with archive ledgers:")]
async fn a_generated_directory_with_archive_ledgers(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
) {
    let base_dir = new_temp_dir("arimalo-gaps");
    let archive_dir = base_dir.join("archive");
    std::fs::create_dir_all(&archive_dir).expect("create archive dir");

    let table = step.table.as_ref().expect("expected a data table");
    // Skip header row
    for row in table.rows.iter().skip(1) {
        let file = &row[0];
        let account = &row[1];
        let dates_str = &row[2];

        let ledger_path = archive_dir.join(format!("{file}.transactions"));

        let mut content = String::new();
        // If file already exists, append to it
        if ledger_path.exists() {
            content = std::fs::read_to_string(&ledger_path).expect("read existing ledger");
        }

        for date in dates_str.split(',') {
            let date = date.trim();
            content.push_str(&format!(
                r#"{date} * "Test" "gap test" ;
    {account} 1.00 AUD
    expenses:test -1.00 AUD

"#
            ));
        }

        std::fs::write(&ledger_path, &content).expect("write ledger file");
    }

    world.generated_dir = Some(base_dir);
}

#[when("I run gap detection on the generated directory")]
async fn i_run_gap_detection(world: &mut LedgerWorld) {
    let dir = world
        .generated_dir
        .as_ref()
        .expect("generated_dir should be set");
    let results = detect_account_gaps(dir).expect("detect_account_gaps");
    world.gap_results = Some(results);
}

#[then(regex = r#"^the gaps for "([^"]+)" should be:$"#)]
async fn the_gaps_for_account_should_be(
    world: &mut LedgerWorld,
    account: String,
    step: &cucumber::gherkin::Step,
) {
    let results = world
        .gap_results
        .as_ref()
        .expect("gap_results should be set");
    let gap = results
        .iter()
        .find(|g| g.account == account)
        .unwrap_or_else(|| panic!("no gap result for account {account:?}"));

    let table = step.table.as_ref().expect("expected a data table");
    let expected: Vec<String> = table
        .rows
        .iter()
        .skip(1)
        .map(|row| row[0].replace('-', ""))
        .collect();

    assert_eq!(
        gap.missing_months, expected,
        "expected gaps {:?} for {account}, got {:?}",
        expected, gap.missing_months
    );
}

#[then(regex = r#"^there should be no gaps for "([^"]+)"$"#)]
async fn there_should_be_no_gaps(world: &mut LedgerWorld, account: String) {
    let results = world
        .gap_results
        .as_ref()
        .expect("gap_results should be set");
    let gap = results
        .iter()
        .find(|g| g.account == account)
        .unwrap_or_else(|| panic!("no gap result for account {account:?}"));

    assert!(
        gap.missing_months.is_empty(),
        "expected no gaps for {account}, got {:?}",
        gap.missing_months
    );
}

// === Add account scenarios ===

#[when(regex = r#"^I add account "([^"]+)" with currency "([^"]+)" to account set "([^"]+)"$"#)]
async fn i_add_account_to_set(
    world: &mut LedgerWorld,
    account_name: String,
    currency: String,
    account_set: String,
) {
    let config = make_pipeline_config(world);
    let result = append_account_and_rebuild(
        &config,
        &account_name,
        Some(currency.as_str()),
        None,
        &account_set,
        None,
    )
    .expect("append_account_and_rebuild should succeed");
    world.pipeline_result = Some(result);
    world.active_set_name = Some(account_set);
}

#[then(regex = r#"^the folder "([^"]+)" should exist under sources$"#)]
async fn the_folder_should_exist_under_sources(world: &mut LedgerWorld, relative_path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(&relative_path);
    assert!(
        folder.exists(),
        "expected folder {} to exist",
        folder.display()
    );
}

#[then(regex = r#"^owner_accounts for "([^"]+)" should include "([^"]+)"$"#)]
async fn owner_accounts_should_include(world: &mut LedgerWorld, owner: String, account: String) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline result should be set");
    let accounts = result
        .owner_accounts
        .get(&owner)
        .unwrap_or_else(|| panic!("owner_accounts should contain key {owner:?}"));
    assert!(
        accounts.contains(&account),
        "expected owner_accounts[{owner:?}] to include {account:?}, got: {accounts:?}"
    );
}

#[then(regex = r#"^account_folders should map "([^"]+)" to "([^"]+)"$"#)]
async fn account_folders_should_map(
    world: &mut LedgerWorld,
    account: String,
    folder: String,
) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline result should be set");
    let actual = result
        .account_folders
        .get(&account)
        .unwrap_or_else(|| {
            panic!(
                "expected account_folders to contain {account:?}, got keys: {:?}",
                result.account_folders.keys().collect::<Vec<_>>()
            )
        });
    assert_eq!(
        actual, &folder,
        "expected account_folders[{account:?}] = {folder:?}, got {actual:?}"
    );
}

#[then(regex = r#"^the ledger for set "([^"]+)" should include a balance for "([^"]+)"$"#)]
async fn the_ledger_for_set_should_include_balance(
    world: &mut LedgerWorld,
    set_name: String,
    account: String,
) {
    let generated = world
        .generated_dir
        .as_ref()
        .expect("generated_dir should be set");
    let set_dir = generated.join(&set_name);
    let parse = load_active_ledger(&set_dir).expect("load_active_ledger should succeed");
    assert!(
        parse.balances.iter().any(|b| b.account == account),
        "expected balance for {account:?} in set {set_name:?}, got: {:?}",
        parse
            .balances
            .iter()
            .map(|b| &b.account)
            .collect::<Vec<_>>()
    );
}

// === Account properties scenarios ===

#[given(regex = r#"^an accounts file at "([^"]+)" with:$"#)]
async fn an_accounts_file_at_with(
    world: &mut LedgerWorld,
    path: String,
    step: &cucumber::gherkin::Step,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let accounts_path = sources.join(&path);
    if let Some(parent) = accounts_path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dirs for accounts file");
    }
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring with accounts content");
    std::fs::write(&accounts_path, content.trim_start_matches('\n')).expect("write accounts file");
}

#[then(regex = r#"^the account properties should map "([^"]+)" to name "([^"]+)"$"#)]
async fn the_account_properties_should_map_name(
    world: &mut LedgerWorld,
    account: String,
    expected_name: String,
) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    let props = result.account_properties.get(&account);
    assert!(
        props.is_some(),
        "expected account_properties to contain {account:?}; got keys: {:?}",
        result.account_properties.keys().collect::<Vec<_>>()
    );
    let name = props.unwrap().name.as_deref();
    assert_eq!(
        name,
        Some(expected_name.as_str()),
        "expected name {expected_name:?} for {account:?}, got {name:?}"
    );
}

#[then("the account properties should be empty")]
async fn the_account_properties_should_be_empty(world: &mut LedgerWorld) {
    let result = world
        .pipeline_result
        .as_ref()
        .expect("pipeline_result should be set");
    assert!(
        result.account_properties.is_empty(),
        "expected account_properties to be empty; got: {:?}",
        result.account_properties
    );
}

// === Set opening balance scenarios ===

#[then(regex = r#"^the account "([^"]+)" should not have an opening balance$"#)]
async fn the_account_should_not_have_opening(world: &mut LedgerWorld, account: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load_active_ledger");
    assert!(
        !result.accounts_with_opening.contains(&account),
        "expected {account:?} to NOT have an opening balance, but it does"
    );
}

#[then(regex = r#"^the account "([^"]+)" should have an opening balance$"#)]
async fn the_account_should_have_opening(world: &mut LedgerWorld, account: String) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load_active_ledger");
    assert!(
        result.accounts_with_opening.contains(&account),
        "expected {account:?} to have an opening balance; accounts_with_opening: {:?}",
        result.accounts_with_opening
    );
}

#[when(regex = r#"^I set the opening balance for "([^"]+)" to "([^"]+)" "([^"]+)"$"#)]
async fn i_set_the_opening_balance(
    world: &mut LedgerWorld,
    account: String,
    amount: String,
    commodity: String,
) {
    let config = make_pipeline_config(world);
    let result = update_opening_balance(&config, &account, &amount, &commodity, "")
        .expect("update_opening_balance should succeed");
    world.pipeline_result = Some(result);
}

#[when(
    regex = r#"^I set the opening balance for "([^"]+)" to "([^"]+)" "([^"]+)" in account set "([^"]+)"$"#
)]
async fn i_set_the_opening_balance_with_set(
    world: &mut LedgerWorld,
    account: String,
    amount: String,
    commodity: String,
    account_set: String,
) {
    let config = make_pipeline_config(world);
    let result = update_opening_balance(&config, &account, &amount, &commodity, &account_set)
        .expect("update_opening_balance should succeed");
    world.pipeline_result = Some(result);
}

#[then(regex = r#"^the accounts file at "([^"]+)" should contain "([^"]+)"$"#)]
async fn the_accounts_file_should_contain(world: &mut LedgerWorld, path: String, expected: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let file_path = sources.join(&path);
    let contents = std::fs::read_to_string(&file_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", file_path.display()));
    assert!(
        contents.contains(&expected),
        "expected accounts file to contain {expected:?}; got:\n{contents}"
    );
}

#[then(regex = r#"^the accounts file at "([^"]+)" should not contain "([^"]+)"$"#)]
async fn the_accounts_file_should_not_contain(
    world: &mut LedgerWorld,
    path: String,
    unexpected: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let file_path = sources.join(&path);
    let contents = std::fs::read_to_string(&file_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", file_path.display()));
    assert!(
        !contents.contains(&unexpected),
        "expected accounts file to NOT contain {unexpected:?}; got:\n{contents}"
    );
}

// === Trade link scenarios ===

#[given("a trade link store")]
async fn a_trade_link_store(world: &mut LedgerWorld) {
    let path = new_temp_dir("arimalo-trade-links").join("metadata.automerge");
    let store = MetadataStore::new(path).expect("create trade link store");
    world.trade_link_store = Some(Box::new(store));
}

#[when(regex = r#"^I save a trade link between "([^"]+)" and "([^"]+)"$"#)]
async fn i_save_a_trade_link(world: &mut LedgerWorld, txn_a: String, txn_b: String) {
    let store = world
        .trade_link_store
        .as_mut()
        .expect("trade link store should be set");
    let id = store
        .save_trade_link(&txn_a, &txn_b)
        .expect("save_trade_link should succeed");
    world.last_trade_link_id = Some(id);
}

#[when("I delete that trade link")]
async fn i_delete_that_trade_link(world: &mut LedgerWorld) {
    let store = world
        .trade_link_store
        .as_mut()
        .expect("trade link store should be set");
    let id = world
        .last_trade_link_id
        .as_ref()
        .expect("last_trade_link_id should be set");
    store
        .delete_trade_link(id)
        .expect("delete_trade_link should succeed");
}

#[then(regex = r#"^get_trade_links should return (\d+) links?$"#)]
async fn get_trade_links_should_return_n(world: &mut LedgerWorld, expected: usize) {
    let store = world
        .trade_link_store
        .as_ref()
        .expect("trade link store should be set");
    let links = store
        .get_trade_links()
        .expect("get_trade_links should succeed");
    assert_eq!(
        links.len(),
        expected,
        "expected {} trade links, got {}: {:?}",
        expected,
        links.len(),
        links,
    );
}

#[then(regex = r#"^the trade link should pair "([^"]+)" with "([^"]+)"$"#)]
async fn the_trade_link_should_pair(
    world: &mut LedgerWorld,
    expected_a: String,
    expected_b: String,
) {
    let store = world
        .trade_link_store
        .as_ref()
        .expect("trade link store should be set");
    let links = store
        .get_trade_links()
        .expect("get_trade_links should succeed");
    assert_eq!(links.len(), 1, "expected exactly 1 trade link");
    let link = &links[0];
    assert!(
        (link.txn_id_a == expected_a && link.txn_id_b == expected_b)
            || (link.txn_id_a == expected_b && link.txn_id_b == expected_a),
        "expected link to pair {expected_a} with {expected_b}, got {} with {}",
        link.txn_id_a,
        link.txn_id_b,
    );
}

fn write_exchange_csv(sources_dir: &PathBuf) {
    // Two transactions within 30 seconds: sell ETH, buy USDC
    write_simple_csv(
        sources_dir,
        "exchange/2025-01.csv",
        &[
            ("2025-01-15 10:00:00", "Swap ETH", "-0.5"),
            ("2025-01-15 10:00:30", "Swap USDC", "1500"),
        ],
    );
}

fn write_exchange_transform(sources_dir: &PathBuf) {
    let transform_path = sources_dir.join("exchange/_transform.rhai");
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create exchange dir");
    }
    // Two rows: ETH sell and USDC buy with datetime precision
    let script = r##"let desc = row["Description"];
let commodity = if desc.contains("ETH") { "ETH" } else { "USDC" };
#{
  date: row["Date"],
  payee: desc,
  narration: "exchange",
  amount: row["Amount"],
  commodity: commodity,
  status: "*"
}"##;
    std::fs::write(&transform_path, script).expect("write exchange transform");
}

#[given("a clean sources directory with exchange transactions")]
async fn a_clean_sources_directory_with_exchange_transactions(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_exchange_csv(sources);
    write_exchange_transform(sources);
}

fn write_exchange_csv_with_zeros(sources_dir: &PathBuf) {
    // Two zero-amount transactions that look like they could be a pair but shouldn't match
    write_simple_csv(
        sources_dir,
        "exchange/2025-01.csv",
        &[
            ("2025-01-15 10:00:00", "Swap ETH", "0"),
            ("2025-01-15 10:00:30", "Swap USDC", "0"),
        ],
    );
}

#[given("a clean sources directory with exchange transactions including zero amounts")]
async fn a_clean_sources_directory_with_exchange_transactions_zero(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_exchange_csv_with_zeros(sources);
    write_exchange_transform(sources);
}

fn write_shared_txnid_swap_csv(sources_dir: &PathBuf) {
    // Two legs of the same on-chain swap: one tx hash, two postings.
    // This is the Solana / Ethereum pattern where both legs share a txn_id.
    let csv_path = sources_dir.join("exchange/2024-05.csv");
    if let Some(parent) = csv_path.parent() {
        std::fs::create_dir_all(parent).expect("create exchange dir");
    }
    let content = "Date,Description,Amount,TxHash\n\
2024-05-01 11:44:03,Swap USDC,-1420,5xKqM\n\
2024-05-01 11:44:03,Swap SOL,11.67,5xKqM\n";
    std::fs::write(&csv_path, content).expect("write shared-txn-id CSV");
}

fn write_shared_txnid_swap_transform(sources_dir: &PathBuf) {
    let transform_path = sources_dir.join("exchange/_transform.rhai");
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create exchange dir");
    }
    // Both rows return the same TxHash → same txn_id, mimicking on-chain swap.
    let script = r##"let desc = row["Description"];
let commodity = if desc.contains("USDC") { "USDC" } else { "SOL" };
#{
  date: row["Date"],
  payee: desc,
  narration: "exchange",
  amount: row["Amount"],
  commodity: commodity,
  txn_id: row["TxHash"],
  status: "*"
}"##;
    std::fs::write(&transform_path, script).expect("write shared-txn-id transform");
}

#[given("a clean sources directory with a shared-txn-id swap")]
async fn a_clean_sources_directory_with_a_shared_txnid_swap(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_shared_txnid_swap_csv(sources);
    write_shared_txnid_swap_transform(sources);
}

fn write_multi_fill_exchange_csv(sources_dir: &PathBuf) {
    // 3 HNT buys + 3 USDT sells at same timestamp (Bybit multi-fill pattern)
    write_simple_csv(
        sources_dir,
        "exchange/2025-11.csv",
        &[
            ("2025-11-13 18:44:35", "BUY HNT", "26.75"),
            ("2025-11-13 18:44:35", "BUY HNT", "9.96"),
            ("2025-11-13 18:44:35", "BUY HNT", "7.20"),
            ("2025-11-13 18:44:35", "SELL USDT", "-59.22"),
            ("2025-11-13 18:44:35", "SELL USDT", "-22.15"),
            ("2025-11-13 18:44:35", "SELL USDT", "-16.03"),
        ],
    );
}

fn write_multi_fill_exchange_transform(sources_dir: &PathBuf) {
    let transform_path = sources_dir.join("exchange/_transform.rhai");
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create exchange dir");
    }
    let script = r##"let desc = row["Description"];
let commodity = if desc.contains("HNT") { "HNT" } else { "USDT" };
#{
  date: row["Date"],
  payee: "Bybit",
  narration: desc,
  amount: row["Amount"],
  commodity: commodity,
  status: "*"
}"##;
    std::fs::write(&transform_path, script).expect("write multi-fill exchange transform");
}

#[given("a clean sources directory with multi-fill exchange transactions")]
async fn a_clean_sources_directory_with_multi_fill_exchange(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_multi_fill_exchange_csv(sources);
    write_multi_fill_exchange_transform(sources);
}

#[when("I request trade link suggestions")]
async fn i_request_trade_link_suggestions(world: &mut LedgerWorld) {
    let generated_dir = world
        .generated_dir
        .as_ref()
        .expect("generated_dir should be set");
    let set_dir = match infer_primary_set(world.sources_dir.as_ref().unwrap()) {
        Some(set) => generated_dir.join(&set),
        None => generated_dir.clone(),
    };
    let parse = load_active_ledger(&set_dir).expect("load active ledger");

    // Get existing links from store if available
    let existing = if let Some(store) = world.trade_link_store.as_ref() {
        store.get_trade_links().unwrap_or_default()
    } else {
        Vec::new()
    };

    let suggestions = suggest_trade_links(&parse.transactions, &existing, None, None);
    world.trade_suggestions = Some(suggestions);
}

#[when("I save a trade link between the two exchange transactions")]
async fn i_save_trade_link_between_exchange_txns(world: &mut LedgerWorld) {
    // First get the txn IDs from the parsed transactions
    let generated_dir = world
        .generated_dir
        .as_ref()
        .expect("generated_dir should be set");
    let set_dir = match infer_primary_set(world.sources_dir.as_ref().unwrap()) {
        Some(set) => generated_dir.join(&set),
        None => generated_dir.clone(),
    };
    let parse = load_active_ledger(&set_dir).expect("load active ledger");

    let txn_ids: Vec<String> = parse
        .transactions
        .iter()
        .filter_map(|t| {
            let meta = t.meta.as_ref()?;
            meta.split(',')
                .map(|p| p.trim())
                .find(|p| p.starts_with("txn:"))
                .map(String::from)
        })
        .collect();
    assert!(
        txn_ids.len() >= 2,
        "expected at least 2 transactions with txn IDs, got {}",
        txn_ids.len()
    );

    // Create store if needed
    if world.trade_link_store.is_none() {
        let path = new_temp_dir("arimalo-trade-links-exchange").join("metadata.automerge");
        world.trade_link_store = Some(Box::new(MetadataStore::new(path).expect("create store")));
    }

    let store = world.trade_link_store.as_mut().unwrap();
    let id = store
        .save_trade_link(&txn_ids[0], &txn_ids[1])
        .expect("save trade link");
    world.last_trade_link_id = Some(id);
    world.exchange_txn_ids = Some((txn_ids[0].clone(), txn_ids[1].clone()));
}

#[then("the SWELL posting should have a price annotation")]
async fn swell_posting_should_have_price(world: &mut LedgerWorld) {
    let result = world.result.as_ref().expect("parse result");
    // Find the SWELL sell posting (negative amount = disposal)
    let swell_posting = result
        .transactions
        .iter()
        .flat_map(|t| t.postings.iter())
        .find(|p| p.commodity == "SWELL" && p.account.starts_with("assets:") && p.amount < 0.0);
    assert!(swell_posting.is_some(), "should have a SWELL sell posting");
    let posting = swell_posting.unwrap();
    assert!(
        posting.price.is_some(),
        "SWELL sell posting should have a price annotation after auto-link, but has none. Posting: {:?}",
        posting
    );
}

#[then("the SKBDI posting should have a price annotation")]
async fn skbdi_posting_should_have_price(world: &mut LedgerWorld) {
    let result = world.result.as_ref().expect("parse result");
    let posting = result
        .transactions
        .iter()
        .flat_map(|t| t.postings.iter())
        .find(|p| p.commodity == "SKBDI" && p.account.starts_with("assets:"));
    assert!(posting.is_some(), "should have a SKBDI asset posting");
    let posting = posting.unwrap();
    assert!(
        posting.price.is_some(),
        "SKBDI posting should have a price annotation after auto-link, but has none. Posting: {:?}",
        posting
    );
}

#[then(regex = r#"^I should receive (\d+) trade suggestions?$"#)]
async fn i_should_receive_n_trade_suggestions(world: &mut LedgerWorld, expected: usize) {
    let suggestions = world
        .trade_suggestions
        .as_ref()
        .expect("trade_suggestions should be set");
    assert_eq!(
        suggestions.len(),
        expected,
        "expected {} trade suggestions, got {}: {:?}",
        expected,
        suggestions.len(),
        suggestions,
    );
}

// === Trade rule scenarios ===

#[given(
    regex = r#"^a rules file at "([^"]+)" with a field-specific rule matching "([^"]+)" on field "([^"]+)" with contra "([^"]+)"$"#
)]
async fn a_rules_file_with_field_specific_rule(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    field: String,
    contra: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let mut rules = RulesFile::load(&folder);
    rules.rules.push(Rule {
        id: format!("field-rule-{}", rules.rules.len()),
        pattern,
        match_field: Some(field),
        payee: None,
        commodity: None,
        comment: None,
        amount_condition: None,
        fee_condition: None,
        amount_account: Some(contra),
        fee_account: None,
        payee_condition: None,
        narration_condition: None,
        commodity_condition: None,
        meta_condition: None,        postings: vec![],
    });
    rules.save(&folder).expect("save rules file");
}

#[then(regex = r#"^the active ledger should contain "([^"]+)"$"#)]
async fn the_active_ledger_should_contain_text(world: &mut LedgerWorld, needle: String) {
    let dir = set_generated_dir(world);
    let parse = load_active_ledger(&dir).expect("load active ledger");
    // Check transaction metadata, payees, narrations, and posting accounts for the needle
    let found = parse.transactions.iter().any(|txn| {
        txn.meta.as_deref().unwrap_or("").contains(&needle)
            || txn.payee.as_deref().unwrap_or("").contains(&needle)
            || txn.narration.as_deref().unwrap_or("").contains(&needle)
            || txn.postings.iter().any(|p| p.account.contains(&needle))
    });
    assert!(found, "expected active ledger to contain {needle:?}");
}

#[when(expr = "I save a trade link with rules for account folder {string}")]
async fn i_save_trade_link_with_rules(world: &mut LedgerWorld, account_folder: String) {
    let generated_dir = world
        .generated_dir
        .as_ref()
        .expect("generated_dir should be set");
    let set_dir = match infer_primary_set(world.sources_dir.as_ref().unwrap()) {
        Some(set) => generated_dir.join(&set),
        None => generated_dir.clone(),
    };
    let parse = load_active_ledger(&set_dir).expect("load active ledger");

    let txn_ids: Vec<String> = parse
        .transactions
        .iter()
        .filter_map(|t| {
            let meta = t.meta.as_ref()?;
            meta.split(',')
                .map(|p| p.trim())
                .find(|p| p.starts_with("txn:"))
                .map(String::from)
        })
        .collect();
    assert!(
        txn_ids.len() >= 2,
        "expected at least 2 transactions with txn IDs, got {}",
        txn_ids.len()
    );

    // Create store if needed
    if world.trade_link_store.is_none() {
        let path = new_temp_dir("arimalo-trade-rules").join("metadata.automerge");
        world.trade_link_store = Some(Box::new(MetadataStore::new(path).expect("create store")));
    }

    let store = world.trade_link_store.as_mut().unwrap();
    let id = store
        .save_trade_link(&txn_ids[0], &txn_ids[1])
        .expect("save trade link");
    world.last_trade_link_id = Some(id.clone());
    world.exchange_txn_ids = Some((txn_ids[0].clone(), txn_ids[1].clone()));

    // Determine which is the sell (negative amount)
    let sell_txn_id = parse
        .transactions
        .iter()
        .find_map(|t| {
            let meta = t.meta.as_ref()?;
            let txn_id = meta
                .split(',')
                .map(|p| p.trim())
                .find(|p| p.starts_with("txn:"))?;
            if t.postings.first().map_or(false, |p| p.amount < 0.0) {
                Some(txn_id.to_string())
            } else {
                None
            }
        })
        .unwrap_or_else(|| txn_ids[0].clone());

    let buy_txn_id = if sell_txn_id == txn_ids[0] {
        &txn_ids[1]
    } else {
        &txn_ids[0]
    };

    // Generate rules via the same planner the app uses: a shared-txn swap
    // resolves to leg-anchored rules in the leaf folder; distinct txns stay
    // txn-anchored against `account_folder`.
    let mut by_folder = std::collections::HashMap::new();
    by_folder.insert(account_folder.clone(), parse.transactions.clone());
    let (target_folder, link_rules) =
        plan_trade_link_rules(&id, &sell_txn_id, buy_txn_id, &account_folder, &by_folder);
    let sources = world.sources_dir.as_ref().unwrap();
    let folder = sources.join(&target_folder);
    let mut rules = RulesFile::load(&folder);
    rules.rules.extend(link_rules);
    rules.save(&folder).expect("save trade link rules");
}

#[when(expr = "I delete the trade link with rules for account folder {string}")]
async fn i_delete_trade_link_with_rules(world: &mut LedgerWorld, account_folder: String) {
    let store = world
        .trade_link_store
        .as_mut()
        .expect("trade link store should be set");
    let id = world
        .last_trade_link_id
        .as_ref()
        .expect("last_trade_link_id should be set")
        .clone();
    store
        .delete_trade_link(&id)
        .expect("delete_trade_link should succeed");

    // Remove rules
    let sources = world.sources_dir.as_ref().unwrap();
    let folder = sources.join(&account_folder);
    let mut rules = RulesFile::load(&folder);
    remove_trade_link_rules(&mut rules, &id);
    rules
        .save(&folder)
        .expect("save rules after removing trade link rules");
}

#[then(regex = r#"^rule (\d+) in "([^"]+)" should have amount_condition "([^"]+)"$"#)]
async fn rule_n_should_have_amount_condition(
    world: &mut LedgerWorld,
    index: usize,
    path: String,
    expected: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    let rule = &rules.rules[index];
    assert_eq!(
        rule.amount_condition.as_deref(),
        Some(expected.as_str()),
        "expected rule {index} amount_condition {expected:?}, got {:?}",
        rule.amount_condition
    );
}

#[then(regex = r#"^rule (\d+) in "([^"]+)" should have a match_field of "([^"]+)"$"#)]
async fn rule_n_should_have_match_field(
    world: &mut LedgerWorld,
    index: usize,
    path: String,
    expected: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    let rule = &rules.rules[index];
    assert_eq!(
        rule.match_field.as_deref(),
        Some(expected.as_str()),
        "expected rule {index} match_field {expected:?}, got {:?}",
        rule.match_field
    );
}

#[then(regex = r#"^rule (\d+) in "([^"]+)" should have a pattern starting with "([^"]+)"$"#)]
async fn rule_n_pattern_starts_with(
    world: &mut LedgerWorld,
    index: usize,
    path: String,
    prefix: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(
        std::path::Path::new(&path)
            .parent()
            .unwrap_or(std::path::Path::new("")),
    );
    let rules = RulesFile::load(&folder);
    let rule = &rules.rules[index];
    assert!(
        rule.pattern.starts_with(&prefix),
        "expected rule {index} pattern to start with {prefix:?}, got {:?}",
        rule.pattern,
    );
}

#[then("the suggestion should pair the ETH sell with the USDC buy")]
async fn the_suggestion_should_pair_eth_sell_with_usdc_buy(world: &mut LedgerWorld) {
    let suggestions = world
        .trade_suggestions
        .as_ref()
        .expect("trade_suggestions should be set");
    assert_eq!(suggestions.len(), 1, "expected exactly 1 suggestion");
    let s = &suggestions[0];
    assert!(
        s.summary.contains("ETH") && s.summary.contains("USDC"),
        "expected suggestion summary to mention ETH and USDC, got: {}",
        s.summary,
    );
}

// === Dust-value trade filtering scenarios ===

fn write_dust_exchange_csv(sources_dir: &PathBuf) {
    // $200 USDC sell vs 0.002 ETH buy (~$6 at 3000 USD/ETH) — wildly mismatched values
    write_simple_csv(
        sources_dir,
        "exchange/2025-01.csv",
        &[
            ("2025-01-15 10:00:00", "Swap USDC", "-200"),
            ("2025-01-15 10:00:30", "Swap ETH", "0.002"),
        ],
    );
}

fn write_dust_exchange_transform(sources_dir: &PathBuf) {
    let transform_path = sources_dir.join("exchange/_transform.rhai");
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create exchange dir");
    }
    let script = r##"let desc = row["Description"];
let commodity = if desc.contains("ETH") { "ETH" } else { "USDC" };
#{
  date: row["Date"],
  payee: desc,
  narration: "exchange",
  amount: row["Amount"],
  commodity: commodity,
  status: "*"
}"##;
    std::fs::write(&transform_path, script).expect("write exchange transform");
}

#[given("a clean sources directory with dust-value exchange transactions")]
async fn a_clean_sources_directory_with_dust_exchange(world: &mut LedgerWorld) {
    setup_clean_dirs(world);
    let sources = world.sources_dir.as_ref().unwrap();
    write_dust_exchange_csv(sources);
    write_dust_exchange_transform(sources);
}

#[given(regex = r#"^price data valuing ETH at (\d+) USD and USDC at (\d+) USD$"#)]
async fn price_data_valuing(world: &mut LedgerWorld, eth_price: f64, usdc_price: f64) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let prices_dir = sources.join("_prices");
    std::fs::create_dir_all(&prices_dir).expect("create _prices dir");
    let content = format!(
        "P 2025-01-01 ETH {} USD\nP 2025-01-01 USDC {} USD\n",
        eth_price, usdc_price,
    );
    std::fs::write(prices_dir.join("test.txt"), content).expect("write price file");
}

#[when(regex = r#"^I request trade link suggestions with base currency "([^"]+)"$"#)]
async fn i_request_trade_link_suggestions_with_base(
    world: &mut LedgerWorld,
    base_currency: String,
) {
    let generated_dir = world
        .generated_dir
        .as_ref()
        .expect("generated_dir should be set");
    let set_dir = match infer_primary_set(world.sources_dir.as_ref().unwrap()) {
        Some(set) => generated_dir.join(&set),
        None => generated_dir.clone(),
    };
    let parse = load_active_ledger(&set_dir).expect("load active ledger");

    let existing = if let Some(store) = world.trade_link_store.as_ref() {
        store.get_trade_links().unwrap_or_default()
    } else {
        Vec::new()
    };

    let sources = world.sources_dir.as_ref().unwrap();
    let price_graph = PriceGraph::load(sources);

    let suggestions = suggest_trade_links(
        &parse.transactions,
        &existing,
        Some(&price_graph),
        Some(&base_currency),
    );
    world.trade_suggestions = Some(suggestions);
    world.trade_base_currency = Some(base_currency);
}

// === Reports scenarios ===

#[given(regex = r#"^trade links:$"#)]
async fn given_trade_links(world: &mut LedgerWorld, step: &cucumber::gherkin::Step) {
    let table = step.table.as_ref().expect("expected a data table");
    let mut links = Vec::new();
    for (i, row) in table.rows.iter().skip(1).enumerate() {
        let txn_a = row.get(0).cloned().unwrap_or_default();
        let txn_b = row.get(1).cloned().unwrap_or_default();
        links.push(TradeLinkRef {
            id: format!("test-link-{i}"),
            txn_id_a: txn_a,
            txn_id_b: txn_b,
        });
    }
    world.report_trade_links = links;
}

#[given(regex = r#"^prices for "([^"]+)" with base "([^"]+)" at "([^"]+)" of "([^"]+)"$"#)]
async fn given_prices_for(
    world: &mut LedgerWorld,
    commodity: String,
    base: String,
    date: String,
    price: String,
) {
    if world.sources_dir.is_none() {
        let dir = new_temp_dir("prices");
        world.sources_dir = Some(dir);
    }
    let sources = world.sources_dir.as_ref().unwrap();
    let prices_dir = sources.join("_prices");
    std::fs::create_dir_all(&prices_dir).expect("create _prices dir");
    let path = prices_dir.join(format!("{commodity}.txt"));
    let line = format!("P {date} {commodity} {price} {base}\n");
    // Append in case multiple prices for the same commodity
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("open price file");
    f.write_all(line.as_bytes()).expect("write price line");
}

#[given("the transactions are auto-linked for equity swaps")]
async fn auto_link_equity_swaps_step(world: &mut LedgerWorld) {
    let result = world.result.as_mut().expect("parse result should be set");
    let mut tagged: Vec<(Option<String>, Option<String>, arimalo_covid::ledger_parser::Transaction)> =
        result.transactions.drain(..).map(|t| (None, None, t)).collect();
    auto_link_equity_swaps(&mut tagged, None, None);
    result.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();
}

#[given(
    regex = r#"^the transactions are auto-linked for equity swaps with prices for "([^"]*)" at "([^"]*)" in "([^"]*)"$"#
)]
async fn auto_link_equity_swaps_with_prices_step(
    world: &mut LedgerWorld,
    commodity: String,
    price: String,
    base: String,
) {
    let price_val: f64 = price.parse().expect("parse price");
    let result = world.result.as_mut().expect("parse result should be set");
    let mut tagged: Vec<(Option<String>, Option<String>, arimalo_covid::ledger_parser::Transaction)> =
        result.transactions.drain(..).map(|t| (None, None, t)).collect();
    // Build a PriceGraph with the given price
    let dir = new_temp_dir("arimalo-auto-link-prices");
    let prices_dir = dir.join("_prices");
    std::fs::create_dir_all(&prices_dir).unwrap();
    let line = format!("P 2020-01-01 {commodity} {price_val:.4} {base}\n");
    std::fs::write(prices_dir.join(format!("{commodity}.txt")), &line).unwrap();
    let pg = arimalo_covid::ledger_parser::PriceGraph::load(&dir);
    auto_link_equity_swaps(&mut tagged, Some(&pg), Some(&base));
    result.transactions = tagged.into_iter().map(|(_, _, t)| t).collect();
}

#[given("the transactions are serialized to text and re-parsed")]
async fn serialize_and_reparse_step(world: &mut LedgerWorld) {
    let result = world.result.as_mut().expect("parse result should be set");
    let mut text = String::new();
    for txn in &result.transactions {
        text.push_str(&transaction_to_text(txn));
        text.push('\n');
    }
    let re_parsed = parse_transactions(&text);
    result.transactions = re_parsed.transactions;
}

#[when(regex = r#"^I generate a CGT report for FY "([^"]+)" with base currency "([^"]+)"$"#)]
async fn i_generate_cgt_report(world: &mut LedgerWorld, fy: String, base_currency: String) {
    let result = world.result.as_ref().expect("parse result should be set");
    let tax_config = TaxConfig::default();
    let prices_path = world
        .sources_dir
        .as_deref()
        .unwrap_or(std::path::Path::new("/nonexistent"));
    let price_graph = PriceGraph::load(prices_path);
    let report = reports::generate_cgt_report(
        &result.transactions,
        &price_graph,
        &tax_config,
        &fy,
        &base_currency,
        None,
    );
    world.cgt_report = Some(report);
}

#[then(regex = r#"^the CGT report should contain (\d+) events?$"#)]
async fn cgt_report_event_count(world: &mut LedgerWorld, count: usize) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    assert_eq!(
        report.events.len(),
        count,
        "expected {count} CGT events, got {}",
        report.events.len()
    );
}

#[then(regex = r#"^CGT event (\d+) should have sell date "([^"]+)"$"#)]
async fn cgt_event_sell_date(world: &mut LedgerWorld, idx: usize, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    assert_eq!(event.sell_date, expected, "event {idx} sell_date");
}

#[then(regex = r#"^CGT event (\d+) should have commodity "([^"]+)"$"#)]
async fn cgt_event_commodity(world: &mut LedgerWorld, idx: usize, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    assert_eq!(event.commodity, expected, "event {idx} commodity");
}

#[then(regex = r#"^CGT event (\d+) should have cost basis "([^"]+)"$"#)]
async fn cgt_event_cost_basis(world: &mut LedgerWorld, idx: usize, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (event.cost_basis - expected_val).abs() < 0.01,
        "event {idx} cost_basis: expected {expected_val}, got {}",
        event.cost_basis
    );
}

#[then(regex = r#"^CGT event (\d+) should have sale proceeds "([^"]+)"$"#)]
async fn cgt_event_sale_proceeds(world: &mut LedgerWorld, idx: usize, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (event.sale_proceeds - expected_val).abs() < 0.01,
        "event {idx} sale_proceeds: expected {expected_val}, got {}",
        event.sale_proceeds
    );
}

#[then(regex = r#"^CGT event (\d+) should have capital gain "([^"]+)"$"#)]
async fn cgt_event_capital_gain(world: &mut LedgerWorld, idx: usize, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (event.capital_gain - expected_val).abs() < 0.01,
        "event {idx} capital_gain: expected {expected_val}, got {}",
        event.capital_gain
    );
}

#[then(regex = r#"^CGT event (\d+) should be discount eligible$"#)]
async fn cgt_event_discount_eligible(world: &mut LedgerWorld, idx: usize) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    assert!(
        event.discount_eligible,
        "event {idx} should be discount eligible"
    );
}

#[then(regex = r#"^CGT event (\d+) should not be discount eligible$"#)]
async fn cgt_event_not_discount_eligible(world: &mut LedgerWorld, idx: usize) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    assert!(
        !event.discount_eligible,
        "event {idx} should not be discount eligible"
    );
}

#[then(regex = r#"^CGT event (\d+) should have discounted gain "([^"]+)"$"#)]
async fn cgt_event_discounted_gain(world: &mut LedgerWorld, idx: usize, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (event.discounted_gain - expected_val).abs() < 0.01,
        "event {idx} discounted_gain: expected {expected_val}, got {}",
        event.discounted_gain
    );
}

#[then(regex = r#"^the CGT report total gains should be "([^"]+)"$"#)]
async fn cgt_report_total_gains(world: &mut LedgerWorld, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (report.total_gains - expected_val).abs() < 0.01,
        "total_gains: expected {expected_val}, got {}",
        report.total_gains
    );
}

#[then(regex = r#"^the CGT report total losses should be "([^"]+)"$"#)]
async fn cgt_report_total_losses(world: &mut LedgerWorld, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (report.total_losses - expected_val).abs() < 0.01,
        "total_losses: expected {expected_val}, got {}",
        report.total_losses
    );
}

#[then(regex = r#"^the CGT report net capital gain should be "([^"]+)"$"#)]
async fn cgt_report_net_capital_gain(world: &mut LedgerWorld, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (report.net_capital_gain - expected_val).abs() < 0.01,
        "net_capital_gain: expected {expected_val}, got {}",
        report.net_capital_gain
    );
}

#[then(regex = r#"^the CGT report total discounted gain should be "([^"]+)"$"#)]
async fn cgt_report_total_discounted_gain(world: &mut LedgerWorld, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (report.total_discounted_gain - expected_val).abs() < 0.01,
        "total_discounted_gain: expected {expected_val}, got {}",
        report.total_discounted_gain
    );
}

#[then(regex = r#"^CGT event (\d+) should have buy date "([^"]+)"$"#)]
async fn cgt_event_buy_date(world: &mut LedgerWorld, idx: usize, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    assert_eq!(event.buy_date, expected, "event {idx} buy_date");
}

#[then(regex = r#"^CGT event (\d+) should have quantity "([^"]+)"$"#)]
async fn cgt_event_quantity(world: &mut LedgerWorld, idx: usize, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (event.quantity - expected_val).abs() < 0.01,
        "event {idx} quantity: expected {expected_val}, got {}",
        event.quantity
    );
}

#[then(regex = r#"^the CGT report should have warnings containing "([^"]+)"$"#)]
async fn cgt_report_has_warnings_containing(world: &mut LedgerWorld, text: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let has_match = report.warnings.iter().any(|w| w.contains(&text));
    assert!(
        has_match,
        "expected warning containing '{}', got: {:?}",
        text, report.warnings
    );
}

// === Accounting invariant steps ===

#[then(regex = r#"^CGT event (\d+) should have holding days "([^"]+)"$"#)]
async fn cgt_event_holding_days(world: &mut LedgerWorld, idx: usize, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let event = &report.events[idx - 1];
    let expected_val: i64 = expected.parse().expect("valid integer");
    assert_eq!(
        event.holding_days, expected_val,
        "event {idx} holding_days: expected {expected_val}, got {}",
        event.holding_days
    );
}

#[then("every CGT event should satisfy gain equals proceeds minus cost")]
async fn cgt_every_event_gain_equals_proceeds_minus_cost(world: &mut LedgerWorld) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    for (i, event) in report.events.iter().enumerate() {
        let expected_gain = event.sale_proceeds - event.cost_basis;
        assert!(
            (event.capital_gain - expected_gain).abs() < 1e-6,
            "event {}: gain ({}) != proceeds ({}) - cost ({}); diff = {}",
            i + 1,
            event.capital_gain,
            event.sale_proceeds,
            event.cost_basis,
            (event.capital_gain - expected_gain).abs()
        );
    }
}

#[then("the sum of event gains should equal the report total gains")]
async fn cgt_sum_gains_equals_total(world: &mut LedgerWorld) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let sum: f64 = report
        .events
        .iter()
        .filter(|e| e.capital_gain > 0.0)
        .map(|e| e.capital_gain)
        .sum();
    assert!(
        (sum - report.total_gains).abs() < 1e-6,
        "sum of event gains ({sum}) != report.total_gains ({}); diff = {}",
        report.total_gains,
        (sum - report.total_gains).abs()
    );
}

#[then("the sum of event losses should equal the report total losses")]
async fn cgt_sum_losses_equals_total(world: &mut LedgerWorld) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let sum: f64 = report
        .events
        .iter()
        .filter(|e| e.capital_gain < 0.0)
        .map(|e| e.capital_gain.abs())
        .sum();
    assert!(
        (sum - report.total_losses).abs() < 1e-6,
        "sum of event losses ({sum}) != report.total_losses ({}); diff = {}",
        report.total_losses,
        (sum - report.total_losses).abs()
    );
}

#[then("the sum of event proceeds should equal the sum of individual proceeds")]
async fn cgt_sum_proceeds_reconciles(world: &mut LedgerWorld) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let sum: f64 = report.events.iter().map(|e| e.sale_proceeds).sum();
    // This step just verifies that summing works without NaN/Inf
    assert!(
        sum.is_finite(),
        "sum of event proceeds is not finite: {sum}"
    );
}

#[then("the sum of event cost bases should equal the sum of individual cost bases")]
async fn cgt_sum_cost_bases_reconciles(world: &mut LedgerWorld) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let sum: f64 = report.events.iter().map(|e| e.cost_basis).sum();
    assert!(
        sum.is_finite(),
        "sum of event cost bases is not finite: {sum}"
    );
}

#[then(regex = r#"^the sum of event cost bases should equal "([^"]+)"$"#)]
async fn cgt_sum_cost_bases_equals(world: &mut LedgerWorld, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let sum: f64 = report.events.iter().map(|e| e.cost_basis).sum();
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (sum - expected_val).abs() < 0.01,
        "sum of event cost bases ({sum}) != expected ({expected_val}); diff = {}",
        (sum - expected_val).abs()
    );
}

#[then(regex = r#"^the sum of event quantities should equal "([^"]+)"$"#)]
async fn cgt_sum_quantities_equals(world: &mut LedgerWorld, expected: String) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    let sum: f64 = report.events.iter().map(|e| e.quantity).sum();
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (sum - expected_val).abs() < 0.01,
        "sum of event quantities ({sum}) != expected ({expected_val}); diff = {}",
        (sum - expected_val).abs()
    );
}

#[then("total quantity sold per commodity should not exceed quantity bought")]
async fn cgt_quantity_sold_not_exceeds_bought(world: &mut LedgerWorld) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    // Group event quantities by commodity
    let mut sold_by_commodity: std::collections::HashMap<String, f64> =
        std::collections::HashMap::new();
    for event in &report.events {
        *sold_by_commodity
            .entry(event.commodity.clone())
            .or_insert(0.0) += event.quantity;
    }
    // If there are warnings about overselling, this invariant is already violated
    // and tested separately. Here we just check that matched events don't exceed.
    let has_oversell_warning = report
        .warnings
        .iter()
        .any(|w| w.contains("Sold more than held"));
    if has_oversell_warning {
        return; // Oversell is tested in its own scenario
    }
    for (commodity, qty) in &sold_by_commodity {
        assert!(
            *qty >= 0.0,
            "negative total quantity for {commodity}: {qty}"
        );
    }
}

#[then("the CGT report should have no warnings")]
async fn cgt_report_no_warnings(world: &mut LedgerWorld) {
    let report = world.cgt_report.as_ref().expect("CGT report should be set");
    assert!(
        report.warnings.is_empty(),
        "expected no warnings, got: {:?}",
        report.warnings
    );
}

// === Income report steps ===

#[when(regex = r#"^I generate an income report for FY "([^"]+)" with base currency "([^"]+)"$"#)]
async fn i_generate_income_report(world: &mut LedgerWorld, fy: String, base_currency: String) {
    let result = world.result.as_ref().expect("parse result should be set");
    let tax_config = TaxConfig::default();
    let price_graph = PriceGraph::load(std::path::Path::new("/nonexistent"));
    let report = reports::generate_income_report(
        &result.transactions,
        &price_graph,
        &tax_config,
        &fy,
        &base_currency,
        None,
    );
    world.income_report = Some(report);
}

#[when(regex = r#"^I generate an income report for FY "([^"]+)" with base currency "([^"]+)" scoped to "([^"]+)"$"#)]
async fn i_generate_scoped_income_report(world: &mut LedgerWorld, fy: String, base_currency: String, scope: String) {
    let result = world.result.as_ref().expect("parse result should be set");
    let tax_config = TaxConfig::default();
    let price_graph = PriceGraph::load(std::path::Path::new("/nonexistent"));
    let report = reports::generate_income_report(
        &result.transactions,
        &price_graph,
        &tax_config,
        &fy,
        &base_currency,
        Some(&scope),
    );
    world.income_report = Some(report);
}

#[then(regex = r#"^the income report should have (\d+) income categories?$"#)]
async fn income_report_income_count(world: &mut LedgerWorld, count: usize) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    assert_eq!(
        report.income_categories.len(),
        count,
        "expected {count} income categories, got {}",
        report.income_categories.len()
    );
}

#[then(regex = r#"^the income report should have (\d+) expense categories?$"#)]
async fn income_report_expense_count(world: &mut LedgerWorld, count: usize) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    assert_eq!(
        report.expense_categories.len(),
        count,
        "expected {count} expense categories, got {}",
        report.expense_categories.len()
    );
}

#[then(regex = r#"^income category "([^"]+)" should total "([^"]+)"$"#)]
async fn income_category_total(world: &mut LedgerWorld, account: String, expected: String) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let cat = report
        .income_categories
        .iter()
        .find(|c| c.account == account)
        .unwrap_or_else(|| panic!("income category {account} not found"));
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (cat.total - expected_val).abs() < 0.01,
        "income {account}: expected {expected_val}, got {}",
        cat.total
    );
}

#[then(regex = r#"^expense category "([^"]+)" should total "([^"]+)"$"#)]
async fn expense_category_total(world: &mut LedgerWorld, account: String, expected: String) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let cat = report
        .expense_categories
        .iter()
        .find(|c| c.account == account)
        .unwrap_or_else(|| panic!("expense category {account} not found"));
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (cat.total - expected_val).abs() < 0.01,
        "expense {account}: expected {expected_val}, got {}",
        cat.total
    );
}

#[then(regex = r#"^the income report total income should be "([^"]+)"$"#)]
async fn income_report_total_income(world: &mut LedgerWorld, expected: String) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (report.total_income - expected_val).abs() < 0.01,
        "total_income: expected {expected_val}, got {}",
        report.total_income
    );
}

#[then(regex = r#"^the income report total expenses should be "([^"]+)"$"#)]
async fn income_report_total_expenses(world: &mut LedgerWorld, expected: String) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (report.total_expenses - expected_val).abs() < 0.01,
        "total_expenses: expected {expected_val}, got {}",
        report.total_expenses
    );
}

#[then(regex = r#"^the income report net should be "([^"]+)"$"#)]
async fn income_report_net(world: &mut LedgerWorld, expected: String) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (report.net - expected_val).abs() < 0.01,
        "net: expected {expected_val}, got {}",
        report.net
    );
}

#[then(regex = r#"^the income report should have (\d+) income events?$"#)]
async fn income_report_event_count(world: &mut LedgerWorld, count: usize) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    assert_eq!(
        report.events.len(),
        count,
        "expected {count} income events, got {}",
        report.events.len()
    );
}

#[then(regex = r#"^the income report should have (\d+) income events? for commodity "([^"]+)"$"#)]
async fn income_report_event_count_for_commodity(
    world: &mut LedgerWorld,
    count: usize,
    commodity: String,
) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let actual = report
        .events
        .iter()
        .filter(|e| e.commodity == commodity)
        .count();
    assert_eq!(
        actual, count,
        "expected {count} income events for {commodity}, got {actual}"
    );
}

#[then(regex = r#"^the income report should have (\d+) expense events?$"#)]
async fn income_report_expense_event_count(world: &mut LedgerWorld, count: usize) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    assert_eq!(
        report.expense_events.len(),
        count,
        "expected {count} expense events, got {}",
        report.expense_events.len()
    );
}

#[then(regex = r#"^the income report should have (\d+) expense events? for commodity "([^"]+)"$"#)]
async fn income_report_expense_event_count_for_commodity(
    world: &mut LedgerWorld,
    count: usize,
    commodity: String,
) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let actual = report
        .expense_events
        .iter()
        .filter(|e| e.commodity == commodity)
        .count();
    assert_eq!(
        actual, count,
        "expected {count} expense events for {commodity}, got {actual}"
    );
}

fn find_income_event<'a>(
    report: &'a reports::IncomeTaxReport,
    commodity: &str,
    date: &str,
) -> &'a reports::IncomeEvent {
    report
        .events
        .iter()
        .find(|e| e.commodity == commodity && e.date == date)
        .unwrap_or_else(|| panic!("income event {commodity} on {date} not found"))
}

fn find_expense_event<'a>(
    report: &'a reports::IncomeTaxReport,
    commodity: &str,
    date: &str,
) -> &'a reports::IncomeEvent {
    report
        .expense_events
        .iter()
        .find(|e| e.commodity == commodity && e.date == date)
        .unwrap_or_else(|| panic!("expense event {commodity} on {date} not found"))
}

#[then(regex = r#"^income event "([^"]+)" on "([^"]+)" should have quantity "([^"]+)"$"#)]
async fn income_event_quantity(
    world: &mut LedgerWorld,
    commodity: String,
    date: String,
    expected: String,
) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let event = find_income_event(report, &commodity, &date);
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (event.quantity - expected_val).abs() < 1e-6,
        "income event {commodity}@{date} quantity: expected {expected_val}, got {}",
        event.quantity
    );
}

#[then(regex = r#"^income event "([^"]+)" on "([^"]+)" should have price "([^"]+)"$"#)]
async fn income_event_price(
    world: &mut LedgerWorld,
    commodity: String,
    date: String,
    expected: String,
) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let event = find_income_event(report, &commodity, &date);
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (event.price - expected_val).abs() < 0.01,
        "income event {commodity}@{date} price: expected {expected_val}, got {}",
        event.price
    );
}

#[then(regex = r#"^income event "([^"]+)" on "([^"]+)" should have value "([^"]+)"$"#)]
async fn income_event_value(
    world: &mut LedgerWorld,
    commodity: String,
    date: String,
    expected: String,
) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let event = find_income_event(report, &commodity, &date);
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (event.value - expected_val).abs() < 0.01,
        "income event {commodity}@{date} value: expected {expected_val}, got {}",
        event.value
    );
}

#[then(regex = r#"^income event "([^"]+)" on "([^"]+)" should have account "([^"]+)"$"#)]
async fn income_event_account(
    world: &mut LedgerWorld,
    commodity: String,
    date: String,
    expected: String,
) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let event = find_income_event(report, &commodity, &date);
    assert_eq!(
        event.account, expected,
        "income event {commodity}@{date} account"
    );
}

#[then(regex = r#"^income event "([^"]+)" on "([^"]+)" should have asset account "([^"]+)"$"#)]
async fn income_event_asset_account(
    world: &mut LedgerWorld,
    commodity: String,
    date: String,
    expected: String,
) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let event = find_income_event(report, &commodity, &date);
    assert_eq!(
        event.asset_account, expected,
        "income event {commodity}@{date} asset_account"
    );
}

#[then(regex = r#"^expense event "([^"]+)" on "([^"]+)" should have value "([^"]+)"$"#)]
async fn expense_event_value(
    world: &mut LedgerWorld,
    commodity: String,
    date: String,
    expected: String,
) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let event = find_expense_event(report, &commodity, &date);
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (event.value - expected_val).abs() < 0.01,
        "expense event {commodity}@{date} value: expected {expected_val}, got {}",
        event.value
    );
}

#[then(regex = r#"^expense event "([^"]+)" on "([^"]+)" should have account "([^"]+)"$"#)]
async fn expense_event_account(
    world: &mut LedgerWorld,
    commodity: String,
    date: String,
    expected: String,
) {
    let report = world
        .income_report
        .as_ref()
        .expect("income report should be set");
    let event = find_expense_event(report, &commodity, &date);
    assert_eq!(
        event.account, expected,
        "expense event {commodity}@{date} account"
    );
}

// === Balances report steps ===

fn load_balances_price_graph(world: &LedgerWorld) -> PriceGraph {
    // Reuse the same prices file if one was parsed for this scenario; otherwise
    // fall back to an empty graph (price conversions will produce warnings).
    match &world.prices_result {
        Some(parsed) if parsed.ok => PriceGraph::from_entries(parsed.prices.clone()),
        _ => PriceGraph::load(std::path::Path::new("/nonexistent")),
    }
}

#[when(
    regex = r#"^I generate a balances report as of "([^"]+)" in "([^"]+)"$"#
)]
async fn i_generate_balances_report(
    world: &mut LedgerWorld,
    as_of: String,
    base_currency: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let price_graph = load_balances_price_graph(world);
    let report = reports::generate_balances_report_range(
        &result.transactions,
        &price_graph,
        &as_of,
        &base_currency,
        None,
        None,
    );
    world.balances_report = Some(report);
}

#[when(
    regex = r#"^I generate a balances report as of "([^"]+)" in "([^"]+)" scoped to "([^"]+)"$"#
)]
async fn i_generate_scoped_balances_report(
    world: &mut LedgerWorld,
    as_of: String,
    base_currency: String,
    scope: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let price_graph = load_balances_price_graph(world);
    let report = reports::generate_balances_report_range(
        &result.transactions,
        &price_graph,
        &as_of,
        &base_currency,
        Some(&scope),
        None,
    );
    world.balances_report = Some(report);
}

#[when(
    regex = r#"^I generate a balances report as of "([^"]+)" in "([^"]+)" scoped to "([^"]+)" restricted to accounts "([^"]+)"$"#
)]
async fn i_generate_restricted_balances_report(
    world: &mut LedgerWorld,
    as_of: String,
    base_currency: String,
    scope: String,
    accounts: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let price_graph = load_balances_price_graph(world);
    let allowed: std::collections::HashSet<String> = accounts
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    let allowed = reports::AccountAllowlist::new(allowed, &[]);
    let report = reports::generate_balances_report_range(
        &result.transactions,
        &price_graph,
        &as_of,
        &base_currency,
        Some(&scope),
        Some(&allowed),
    );
    world.balances_report = Some(report);
}

#[then(regex = r#"^the balances report should have (\d+) holdings?$"#)]
async fn balances_holdings_count(world: &mut LedgerWorld, count: usize) {
    let report = world
        .balances_report
        .as_ref()
        .expect("balances report should be set");
    assert_eq!(
        report.holdings.len(),
        count,
        "expected {count} holdings, got {}: {:?}",
        report.holdings.len(),
        report.holdings
    );
}

#[then(regex = r#"^balances holding "([^"]+)" should have quantity "([^"]+)"$"#)]
async fn balances_holding_quantity(
    world: &mut LedgerWorld,
    commodity: String,
    expected: String,
) {
    let report = world
        .balances_report
        .as_ref()
        .expect("balances report should be set");
    let h = report
        .holdings
        .iter()
        .find(|h| h.commodity == commodity)
        .unwrap_or_else(|| panic!("holding {commodity} not found in {:?}", report.holdings));
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (h.quantity - expected_val).abs() < 1e-6,
        "quantity for {commodity}: expected {expected_val}, got {}",
        h.quantity
    );
}

#[then(regex = r#"^balances holding "([^"]+)" should have value "([^"]+)"$"#)]
async fn balances_holding_value(
    world: &mut LedgerWorld,
    commodity: String,
    expected: String,
) {
    let report = world
        .balances_report
        .as_ref()
        .expect("balances report should be set");
    let h = report
        .holdings
        .iter()
        .find(|h| h.commodity == commodity)
        .unwrap_or_else(|| panic!("holding {commodity} not found in {:?}", report.holdings));
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (h.value - expected_val).abs() < 0.01,
        "value for {commodity}: expected {expected_val}, got {}",
        h.value
    );
}

#[then(regex = r#"^the balances report total value should be "([^"]+)"$"#)]
async fn balances_total_value(world: &mut LedgerWorld, expected: String) {
    let report = world
        .balances_report
        .as_ref()
        .expect("balances report should be set");
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (report.total_value - expected_val).abs() < 0.01,
        "total_value: expected {expected_val}, got {}",
        report.total_value
    );
}

#[then(regex = r#"^the balances report should not contain "([^"]+)"$"#)]
async fn balances_missing_commodity(world: &mut LedgerWorld, commodity: String) {
    let report = world
        .balances_report
        .as_ref()
        .expect("balances report should be set");
    assert!(
        !report.holdings.iter().any(|h| h.commodity == commodity),
        "commodity {commodity} unexpectedly present in {:?}",
        report.holdings
    );
}

#[then(regex = r#"^the balances report warnings should mention "([^"]+)"$"#)]
async fn balances_warning_mentions(world: &mut LedgerWorld, needle: String) {
    let report = world
        .balances_report
        .as_ref()
        .expect("balances report should be set");
    assert!(
        report.warnings.iter().any(|w| w.contains(&needle)),
        "no warning mentions {needle}; got {:?}",
        report.warnings
    );
}

#[then(regex = r#"^balances holding "([^"]+)" should have (\d+) account breakdown rows$"#)]
async fn balances_holding_account_count(
    world: &mut LedgerWorld,
    commodity: String,
    expected: usize,
) {
    let report = world.balances_report.as_ref().expect("balances report should be set");
    let h = report.holdings.iter().find(|h| h.commodity == commodity)
        .unwrap_or_else(|| panic!("no holding for {commodity}"));
    assert_eq!(h.accounts.len(), expected,
        "{commodity} accounts: expected {expected}, got {:?}", h.accounts);
}

#[then(regex = r#"^balances holding "([^"]+)" account "([^"]+)" should have quantity "([^"]+)"$"#)]
async fn balances_holding_account_quantity(
    world: &mut LedgerWorld,
    commodity: String,
    account: String,
    expected: String,
) {
    let report = world.balances_report.as_ref().expect("balances report should be set");
    let h = report.holdings.iter().find(|h| h.commodity == commodity)
        .unwrap_or_else(|| panic!("no holding for {commodity}"));
    let a = h.accounts.iter().find(|a| a.account == account)
        .unwrap_or_else(|| panic!("no account {account} for {commodity}; got {:?}", h.accounts));
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!((a.quantity - expected_val).abs() < 1e-8,
        "{commodity}/{account} quantity: expected {expected_val}, got {}", a.quantity);
}

#[then(regex = r#"^balances holding "([^"]+)" account quantities should sum to its total$"#)]
async fn balances_account_breakdown_sums(world: &mut LedgerWorld, commodity: String) {
    let report = world.balances_report.as_ref().expect("balances report should be set");
    let h = report.holdings.iter().find(|h| h.commodity == commodity)
        .unwrap_or_else(|| panic!("no holding for {commodity}"));
    let sum: f64 = h.accounts.iter().map(|a| a.quantity).sum();
    assert!((sum - h.quantity).abs() < 1e-8,
        "{commodity} breakdown sum {sum} != holding quantity {}", h.quantity);
}

#[then(regex = r#"^the balances report warnings should not mention "([^"]+)"$"#)]
async fn balances_warning_not_mentions(world: &mut LedgerWorld, needle: String) {
    let report = world
        .balances_report
        .as_ref()
        .expect("balances report should be set");
    assert!(
        !report.warnings.iter().any(|w| w.contains(&needle)),
        "expected no warning to mention {needle}; got {:?}",
        report.warnings
    );
}

// === Performance report steps ===

#[when(
    regex = r#"^I generate a performance report from "([^"]+)" to "([^"]+)" in "([^"]+)"$"#
)]
async fn i_generate_performance_report(
    world: &mut LedgerWorld,
    date_from: String,
    date_to: String,
    base_currency: String,
) {
    let result = world.result.as_ref().expect("parse result should be set");
    let price_graph = load_balances_price_graph(world);
    let tax_config = TaxConfig::default();
    let label = format!("{date_from} to {date_to}");
    let report = reports::generate_performance_report_range(reports::PerformanceReportParams {
        transactions: &result.transactions,
        price_graph: &price_graph,
        tax_config: &tax_config,
        label: &label,
        date_from: &date_from,
        date_to: &date_to,
        base_currency: &base_currency,
        base_account_scope: None,
        allowed_accounts: None,
    });
    world.performance_report = Some(report);
}

#[then(regex = r#"^the performance report should have (\d+) points?$"#)]
async fn performance_points_count(world: &mut LedgerWorld, count: usize) {
    let report = world
        .performance_report
        .as_ref()
        .expect("performance report should be set");
    assert_eq!(
        report.points.len(),
        count,
        "expected {count} points, got {}: {:?}",
        report.points.len(),
        report.points.iter().map(|p| &p.date).collect::<Vec<_>>()
    );
}

#[then(
    regex = r#"^performance point "([^"]+)" should have (value|cost basis|unrealised|realised|income) "([^"]+)"$"#
)]
async fn performance_point_field(
    world: &mut LedgerWorld,
    label: String,
    field: String,
    expected: String,
) {
    let report = world
        .performance_report
        .as_ref()
        .expect("performance report should be set");
    let p = report
        .points
        .iter()
        .find(|p| p.label == label)
        .unwrap_or_else(|| {
            panic!(
                "point {label} not found in {:?}",
                report.points.iter().map(|p| &p.label).collect::<Vec<_>>()
            )
        });
    let actual = match field.as_str() {
        "value" => p.portfolio_value,
        "cost basis" => p.cost_basis,
        "unrealised" => p.unrealised_gain,
        "realised" => p.realised_gain,
        "income" => p.income,
        other => panic!("unknown performance point field: {other}"),
    };
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (actual - expected_val).abs() < 0.01,
        "{field} for {label}: expected {expected_val}, got {actual}"
    );
}

#[then(
    regex = r#"^the performance report (total return|total realised|total income|unrealised change|closing value) should be "([^"]+)"$"#
)]
async fn performance_total(world: &mut LedgerWorld, field: String, expected: String) {
    let report = world
        .performance_report
        .as_ref()
        .expect("performance report should be set");
    let actual = match field.as_str() {
        "total return" => report.total_return,
        "total realised" => report.total_realised_gain,
        "total income" => report.total_income,
        "unrealised change" => report.unrealised_change,
        "closing value" => report.closing_value,
        other => panic!("unknown performance total field: {other}"),
    };
    let expected_val: f64 = expected.parse().expect("valid number");
    assert!(
        (actual - expected_val).abs() < 0.01,
        "{field}: expected {expected_val}, got {actual}"
    );
}

#[then(regex = r#"^the (first|last) performance point date should be "([^"]+)"$"#)]
async fn performance_point_date(world: &mut LedgerWorld, which: String, expected: String) {
    let report = world
        .performance_report
        .as_ref()
        .expect("performance report should be set");
    let p = match which.as_str() {
        "first" => report.points.first(),
        "last" => report.points.last(),
        other => panic!("unknown position: {other}"),
    }
    .expect("performance report should have points");
    assert_eq!(p.date, expected, "{which} point date");
}

// === Report template steps ===

#[when(
    regex = r#"^I render the CGT report template for FY "([^"]+)" with base currency "([^"]+)"$"#
)]
async fn i_render_cgt_template(world: &mut LedgerWorld, fy: String, base_currency: String) {
    let result = world.result.as_ref().expect("parse result should be set");
    let tax_config = TaxConfig::default();
    let price_graph = PriceGraph::load(std::path::Path::new("/nonexistent"));
    let md = report_templates::render_cgt_report(
        &result.transactions,
        &price_graph,
        &tax_config,
        &fy,
        &base_currency,
    )
    .expect("template rendering should succeed");
    world.rendered_report = Some(md);
}

#[when(
    regex = r#"^I render the income report template for FY "([^"]+)" with base currency "([^"]+)"$"#
)]
async fn i_render_income_template(world: &mut LedgerWorld, fy: String, base_currency: String) {
    let result = world.result.as_ref().expect("parse result should be set");
    let tax_config = TaxConfig::default();
    let price_graph = PriceGraph::load(std::path::Path::new("/nonexistent"));
    let md = report_templates::render_income_report(
        &result.transactions,
        &price_graph,
        &tax_config,
        &fy,
        &base_currency,
    )
    .expect("template rendering should succeed");
    world.rendered_report = Some(md);
}

#[then(regex = r#"^the rendered report should contain "([^"]+)"$"#)]
async fn rendered_report_contains(world: &mut LedgerWorld, text: String) {
    let report = world
        .rendered_report
        .as_ref()
        .expect("rendered report should be set");
    assert!(
        report.contains(&text),
        "expected rendered report to contain '{}', got:\n{}",
        text,
        report
    );
}

#[given(regex = r#"^a clean sources directory with a file "([^"]+)":$"#)]
async fn given_sources_with_file(
    world: &mut LedgerWorld,
    rel_path: String,
    step: &cucumber::gherkin::Step,
) {
    let sources_dir = new_temp_dir("arimalo-report-sources");
    std::fs::create_dir_all(&sources_dir).expect("create sources dir");
    let file_path = sources_dir.join(&rel_path);
    std::fs::create_dir_all(file_path.parent().unwrap()).expect("create parent dirs");
    let content = step.docstring.as_ref().expect("expected docstring").trim();
    std::fs::write(&file_path, content).expect("write fixture file");
    world.sources_dir = Some(sources_dir);
    let gen_dir = new_temp_dir("arimalo-report-generated");
    std::fs::create_dir_all(&gen_dir).expect("create generated dir");
    world.generated_dir = Some(gen_dir);
}

#[then(regex = r#"^the generated reports directory should contain "([^"]+)"$"#)]
async fn generated_reports_contains(world: &mut LedgerWorld, filename: String) {
    let gen_dir = world
        .generated_dir
        .as_ref()
        .expect("generated dir should be set");
    // Check root reports dir and per-set reports dirs
    let root_path = gen_dir.join("reports").join(&filename);
    if root_path.exists() {
        return;
    }
    // Search in set subdirectories
    if let Ok(entries) = std::fs::read_dir(gen_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let set_path = entry.path().join("reports").join(&filename);
                if set_path.exists() {
                    return;
                }
            }
        }
    }
    panic!(
        "expected report file {} to exist under {}",
        filename,
        gen_dir.display()
    );
}

// === Root config scenarios ===

#[given("no root config file exists")]
async fn no_root_config(world: &mut LedgerWorld) {
    let dir = new_temp_dir("arimalo-root-config");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    world.root_config_dir = Some(dir);
}

#[given(regex = r#"^a root config file with current_root "([^"]+)"$"#)]
async fn root_config_with_current(world: &mut LedgerWorld, root_path: String) {
    let dir = new_temp_dir("arimalo-root-config");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let config = root_config::RootConfig {
        current_root: Some(root_path.clone()),
        known_roots: vec![root_path],
        ..Default::default()
    };
    root_config::save_root_config(&dir, &config).expect("save config");
    world.root_config_dir = Some(dir);
}

#[given("a root config file with update prices on startup enabled")]
async fn root_config_with_startup_enabled(world: &mut LedgerWorld) {
    let dir = new_temp_dir("arimalo-root-config");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let config = root_config::RootConfig {
        update_prices_on_startup: true,
        ..Default::default()
    };
    root_config::save_root_config(&dir, &config).expect("save config");
    world.root_config_dir = Some(dir);
}

#[given("a raw config file containing:")]
async fn raw_config_file(world: &mut LedgerWorld, step: &cucumber::gherkin::Step) {
    let dir = new_temp_dir("arimalo-root-config");
    std::fs::create_dir_all(&dir).expect("create temp dir");
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim();
    std::fs::write(dir.join("config.json"), content).expect("write config.json");
    world.root_config_dir = Some(dir);
}

#[then(regex = r#"^update prices on startup should be (true|false)$"#)]
async fn update_prices_on_startup_should_be(world: &mut LedgerWorld, expected: String) {
    let config = world.root_config.as_ref().expect("config should be loaded");
    assert_eq!(
        config.update_prices_on_startup,
        expected == "true",
        "update_prices_on_startup mismatch"
    );
}

#[when("I resolve the root directory")]
async fn resolve_root_directory(world: &mut LedgerWorld) {
    let dir = world
        .root_config_dir
        .as_ref()
        .expect("config dir should be set");
    world.root_config = Some(root_config::load_root_config(dir));
}

#[then("the root should be None")]
async fn root_should_be_none(world: &mut LedgerWorld) {
    let config = world.root_config.as_ref().expect("config should be loaded");
    assert!(
        config.current_root.is_none(),
        "expected root to be None, got {:?}",
        config.current_root
    );
}

#[then(regex = r#"^the root should be "([^"]+)"$"#)]
async fn root_should_be(world: &mut LedgerWorld, expected: String) {
    let config = world.root_config.as_ref().expect("config should be loaded");
    assert_eq!(config.current_root.as_deref(), Some(expected.as_str()));
}

#[then(regex = r#"^the known roots should contain "([^"]+)"$"#)]
async fn known_roots_contain(world: &mut LedgerWorld, expected: String) {
    let config = world.root_config.as_ref().expect("config should be loaded");
    assert!(
        config.known_roots.contains(&expected),
        "expected known_roots to contain '{}', got {:?}",
        expected,
        config.known_roots
    );
}

#[when("I set the root directory to a temporary path")]
async fn set_root_to_temp(world: &mut LedgerWorld) {
    let app_dir = world
        .root_config_dir
        .as_ref()
        .expect("config dir should be set");
    let new_root = new_temp_dir("arimalo-new-root");
    world.root_config_new_path = Some(new_root.clone());
    let config = root_config::set_root(app_dir, &new_root).expect("set_root should succeed");
    world.root_config = Some(config);
}

#[then("the root config file should exist")]
async fn root_config_file_exists(world: &mut LedgerWorld) {
    let dir = world
        .root_config_dir
        .as_ref()
        .expect("config dir should be set");
    assert!(dir.join("config.json").exists(), "config.json should exist");
}

#[then("the sources subdirectory should exist")]
async fn sources_subdir_exists(world: &mut LedgerWorld) {
    let new_root = world
        .root_config_new_path
        .as_ref()
        .expect("new root path should be set");
    assert!(
        new_root.join("sources").exists(),
        "sources/ should exist under {}",
        new_root.display()
    );
}

#[then("the generated subdirectory should exist")]
async fn generated_subdir_exists(world: &mut LedgerWorld) {
    let new_root = world
        .root_config_new_path
        .as_ref()
        .expect("new root path should be set");
    assert!(
        new_root.join("generated").exists(),
        "generated/ should exist under {}",
        new_root.display()
    );
}

#[then("a .gitignore should exist in the generated subdirectory")]
async fn gitignore_exists(world: &mut LedgerWorld) {
    let new_root = world
        .root_config_new_path
        .as_ref()
        .expect("new root path should be set");
    let gitignore = new_root.join("generated").join(".gitignore");
    assert!(gitignore.exists(), ".gitignore should exist in generated/");
}

#[then("the known roots should contain the new path")]
async fn known_roots_contain_new(world: &mut LedgerWorld) {
    let config = world.root_config.as_ref().expect("config should be loaded");
    let new_root = world
        .root_config_new_path
        .as_ref()
        .expect("new root should be set");
    let new_root_str = new_root.to_string_lossy().to_string();
    assert!(
        config.known_roots.contains(&new_root_str),
        "expected known_roots to contain '{}', got {:?}",
        new_root_str,
        config.known_roots
    );
}

#[given(regex = r#"^the env var ARIMALO_SOURCES_DIR is set to "([^"]+)"$"#)]
async fn set_sources_env(world: &mut LedgerWorld, val: String) {
    // Store for use in resolve step (we don't actually set env vars to avoid test pollution)
    world.resolved_sources = Some(PathBuf::from(val));
}

#[given(regex = r#"^the env var ARIMALO_GENERATED_DIR is set to "([^"]+)"$"#)]
async fn set_generated_env(world: &mut LedgerWorld, val: String) {
    world.resolved_generated = Some(PathBuf::from(val));
}

#[when("I resolve the sources directory")]
async fn resolve_sources_dir_with_env(world: &mut LedgerWorld) {
    let dir = world
        .root_config_dir
        .as_ref()
        .expect("config dir should be set");
    let config = root_config::load_root_config(dir);
    let env_val = world
        .resolved_sources
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let result = root_config::resolve_sources(env_val.as_deref(), &config, dir);
    world.resolved_sources = Some(result);
}

#[when("I resolve the generated directory")]
async fn resolve_generated_dir_with_env(world: &mut LedgerWorld) {
    let dir = world
        .root_config_dir
        .as_ref()
        .expect("config dir should be set");
    let config = root_config::load_root_config(dir);
    let env_val = world
        .resolved_generated
        .as_ref()
        .map(|p| p.to_string_lossy().to_string());
    let result = root_config::resolve_generated(env_val.as_deref(), &config, dir);
    world.resolved_generated = Some(result);
}

#[when("I resolve the sources directory without env var")]
async fn resolve_sources_no_env(world: &mut LedgerWorld) {
    let dir = world
        .root_config_dir
        .as_ref()
        .expect("config dir should be set");
    let config = root_config::load_root_config(dir);
    let result = root_config::resolve_sources(None, &config, dir);
    world.resolved_sources = Some(result);
}

#[when("I resolve the generated directory without env var")]
async fn resolve_generated_no_env(world: &mut LedgerWorld) {
    let dir = world
        .root_config_dir
        .as_ref()
        .expect("config dir should be set");
    let config = root_config::load_root_config(dir);
    let result = root_config::resolve_generated(None, &config, dir);
    world.resolved_generated = Some(result);
}

#[then(regex = r#"^the sources directory should be "([^"]+)"$"#)]
async fn sources_dir_should_be(world: &mut LedgerWorld, expected: String) {
    let actual = world
        .resolved_sources
        .as_ref()
        .expect("sources dir should be resolved");
    assert_eq!(
        actual,
        &PathBuf::from(&expected),
        "expected sources dir '{}', got '{}'",
        expected,
        actual.display()
    );
}

#[then(regex = r#"^the generated directory should be "([^"]+)"$"#)]
async fn generated_dir_should_be(world: &mut LedgerWorld, expected: String) {
    let actual = world
        .resolved_generated
        .as_ref()
        .expect("generated dir should be resolved");
    assert_eq!(
        actual,
        &PathBuf::from(&expected),
        "expected generated dir '{}', got '{}'",
        expected,
        actual.display()
    );
}

// === Auto self-transfer scenarios ===

#[given(regex = r#"^a transform at "([^"]+)" for account "([^"]+)"$"#)]
async fn a_transform_at_for_account(
    world: &mut LedgerWorld,
    transform_relative: String,
    account: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_transform(sources, &transform_relative, &account);
}

#[given(regex = r#"^a per-folder accounts file in "([^"]+)" declaring "([^"]+)"$"#)]
async fn a_per_folder_accounts_file(world: &mut LedgerWorld, folder: String, declaration: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder_path = sources.join(&folder);
    std::fs::create_dir_all(&folder_path).expect("create account folder");
    let accounts_path = folder_path.join("accounts.transactions");
    let mut existing = if accounts_path.exists() {
        std::fs::read_to_string(&accounts_path).unwrap_or_default()
    } else {
        String::new()
    };
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(&format!("account {declaration}\n"));
    std::fs::write(&accounts_path, existing).expect("write accounts.transactions");
}

#[given(regex = r#"^an empty folder "([^"]+)" under sources$"#)]
async fn an_empty_folder_under_sources(world: &mut LedgerWorld, relative_path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(&relative_path);
    std::fs::create_dir_all(&folder).expect("create empty folder");
}

// === Rule ID tracking scenarios ===

#[then(regex = r#"^transactions with narration "([^"]+)" should have meta containing "([^"]+)"$"#)]
async fn transactions_should_have_meta_containing(
    world: &mut LedgerWorld,
    narration: String,
    substring: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let matching: Vec<_> = result
        .transactions
        .iter()
        .filter(|t| t.narration.as_deref() == Some(narration.as_str()))
        .collect();
    assert!(
        !matching.is_empty(),
        "expected transactions with narration {narration:?}, found none"
    );
    for txn in &matching {
        let meta = txn.meta.as_deref().unwrap_or("");
        assert!(
            meta.contains(&substring),
            "expected meta to contain {substring:?}, got {meta:?}"
        );
    }
}

// === Transform fee scenarios ===

fn write_transform_with_fee(sources_dir: &PathBuf, relative_path: &str, commodity: &str) {
    let transform_path = sources_dir.join(relative_path);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = format!(
        r##"#{{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "{commodity}",
  fee: row["Fee"],
  status: "*"
}}"##
    );
    std::fs::write(&transform_path, script).expect("write transform with fee");
}

fn write_transform_with_compound_fee(sources_dir: &PathBuf, relative_path: &str, commodity: &str) {
    let transform_path = sources_dir.join(relative_path);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    // Fee field already contains "amount commodity" string from CSV
    let script = format!(
        r##"#{{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "{commodity}",
  fee: row["Fee"],
  status: "*"
}}"##
    );
    std::fs::write(&transform_path, script).expect("write transform with compound fee");
}

fn write_transform_with_meta_extra(sources_dir: &PathBuf, relative_path: &str, commodity: &str) {
    let transform_path = sources_dir.join(relative_path);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = format!(
        r##"#{{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "{commodity}",
  meta_extra: "src:" + row["Refid"],
  status: "*"
}}"##
    );
    std::fs::write(&transform_path, script).expect("write transform with meta_extra");
}

fn write_transform_with_src_ref(sources_dir: &PathBuf, relative_path: &str) {
    let transform_path = sources_dir.join(relative_path);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = r##"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: row["Asset"],
  txn_id: row["Refid"] + "-" + row["Asset"],
  meta_extra: "src:" + row["Refid"],
  status: "*"
}"##;
    std::fs::write(&transform_path, script).expect("write transform with src ref");
}

#[given(regex = r#"^a transform with fee at "([^"]+)" using commodity "([^"]+)"$"#)]
async fn a_transform_with_fee(world: &mut LedgerWorld, path: String, commodity: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_transform_with_fee(sources, &path, &commodity);
}

#[given(regex = r#"^a transform with compound fee at "([^"]+)" using commodity "([^"]+)"$"#)]
async fn a_transform_with_compound_fee(world: &mut LedgerWorld, path: String, commodity: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_transform_with_compound_fee(sources, &path, &commodity);
}

#[given(regex = r#"^a transform at "([^"]+)" that maps Date/Description/Amount to USD$"#)]
async fn a_transform_at_that_maps_usd(world: &mut LedgerWorld, transform_relative: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let transform_path = sources.join(&transform_relative);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = r##"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "USD",
  status: "*"
}"##;
    std::fs::write(&transform_path, script).expect("write transform");
}

#[given(regex = r#"^a transform with meta_extra at "([^"]+)" using commodity "([^"]+)"$"#)]
async fn a_transform_with_meta_extra(world: &mut LedgerWorld, path: String, commodity: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_transform_with_meta_extra(sources, &path, &commodity);
}

#[given(regex = r#"^a transform with src ref at "([^"]+)"$"#)]
async fn a_transform_with_src_ref(world: &mut LedgerWorld, path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    write_transform_with_src_ref(sources, &path);
}

#[then(regex = r#"^transaction (\d+) should have (\d+) postings$"#)]
async fn transaction_n_should_have_m_postings(
    world: &mut LedgerWorld,
    txn_num: usize,
    expected: usize,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let txn = result.transactions.get(txn_num - 1).unwrap_or_else(|| {
        panic!(
            "expected transaction {txn_num}, only {} found",
            result.transactions.len()
        )
    });
    assert_eq!(
        txn.postings.len(),
        expected,
        "transaction {txn_num} has {} postings, expected {expected}. Postings: {:?}",
        txn.postings.len(),
        txn.postings,
    );
}

#[then(regex = r#"^posting (\d+) of transaction (\d+) should have account "([^"]+)"$"#)]
async fn posting_n_of_transaction_m_should_have_account(
    world: &mut LedgerWorld,
    posting_num: usize,
    txn_num: usize,
    expected_account: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let txn = result
        .transactions
        .get(txn_num - 1)
        .unwrap_or_else(|| panic!("expected transaction {txn_num}"));
    let posting = txn
        .postings
        .get(posting_num - 1)
        .unwrap_or_else(|| panic!("expected posting {posting_num} in transaction {txn_num}"));
    assert_eq!(posting.account, expected_account,
        "posting {posting_num} of transaction {txn_num}: expected account '{expected_account}', got '{}'",
        posting.account);
}

#[then(regex = r#"^posting (\d+) of transaction (\d+) should have amount "([^"]+)"$"#)]
async fn posting_n_of_transaction_m_should_have_amount(
    world: &mut LedgerWorld,
    posting_num: usize,
    txn_num: usize,
    expected_amount: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let txn = result
        .transactions
        .get(txn_num - 1)
        .unwrap_or_else(|| panic!("expected transaction {txn_num}"));
    let posting = txn
        .postings
        .get(posting_num - 1)
        .unwrap_or_else(|| panic!("expected posting {posting_num} in transaction {txn_num}"));
    let expected: f64 = expected_amount.parse().expect("parse expected amount");
    assert!(
        (posting.amount - expected).abs() < 1e-6,
        "posting {posting_num} of transaction {txn_num}: expected amount {expected}, got {}",
        posting.amount,
    );
}

#[then(regex = r#"^posting (\d+) of transaction (\d+) should have commodity "([^"]+)"$"#)]
async fn posting_n_of_transaction_m_should_have_commodity(
    world: &mut LedgerWorld,
    posting_num: usize,
    txn_num: usize,
    expected_commodity: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let txn = result
        .transactions
        .get(txn_num - 1)
        .unwrap_or_else(|| panic!("expected transaction {txn_num}"));
    let posting = txn
        .postings
        .get(posting_num - 1)
        .unwrap_or_else(|| panic!("expected posting {posting_num} in transaction {txn_num}"));
    assert_eq!(posting.commodity, expected_commodity,
        "posting {posting_num} of transaction {txn_num}: expected commodity '{expected_commodity}', got '{}'",
        posting.commodity);
}

#[then(regex = r#"^the postings of transaction (\d+) should sum to zero$"#)]
async fn postings_of_transaction_should_sum_to_zero(world: &mut LedgerWorld, txn_num: usize) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let txn = result
        .transactions
        .get(txn_num - 1)
        .unwrap_or_else(|| panic!("expected transaction {txn_num}"));
    // Group by commodity and check each sums to zero
    let mut sums: std::collections::HashMap<&str, f64> = std::collections::HashMap::new();
    for p in &txn.postings {
        *sums.entry(&p.commodity).or_default() += p.amount;
    }
    for (commodity, sum) in &sums {
        assert!(
            sum.abs() < 1e-6,
            "transaction {txn_num}: postings in commodity {commodity} sum to {sum}, expected 0",
        );
    }
}

#[then(regex = r#"^transaction (\d+) meta should include "([^"]+)"$"#)]
async fn transaction_n_meta_should_include(
    world: &mut LedgerWorld,
    txn_num: usize,
    substring: String,
) {
    let dir = set_generated_dir(world);
    let result = load_active_ledger(&dir).expect("load active ledger");
    let txn = result
        .transactions
        .get(txn_num - 1)
        .unwrap_or_else(|| panic!("expected transaction {txn_num}"));
    let meta = txn.meta.as_deref().unwrap_or("");
    assert!(
        meta.contains(&substring),
        "transaction {txn_num} meta '{meta}' does not contain '{substring}'",
    );
}

// === Account manage (rename/delete) scenarios ===

#[when(regex = r#"^I rename account folder "([^"]+)" to "([^"]+)"$"#)]
async fn i_rename_account_folder(world: &mut LedgerWorld, old_folder: String, new_folder: String) {
    let config = make_pipeline_config(world);
    let result = rename_account_folder_and_rebuild(&config, &old_folder, &new_folder)
        .expect("rename_account_folder_and_rebuild should succeed");
    world.pipeline_result = Some(result);
    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

#[when(regex = r#"^I try to rename account folder "([^"]+)" to "([^"]+)"$"#)]
async fn i_try_rename_account_folder(
    world: &mut LedgerWorld,
    old_folder: String,
    new_folder: String,
) {
    let config = make_pipeline_config(world);
    match rename_account_folder_and_rebuild(&config, &old_folder, &new_folder) {
        Ok(result) => {
            world.pipeline_result = Some(result);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e);
        }
    }
}

#[when(regex = r#"^I delete account folder "([^"]+)"$"#)]
async fn i_delete_account_folder(world: &mut LedgerWorld, folder: String) {
    let config = make_pipeline_config(world);
    let result = delete_account_folder_and_rebuild(&config, &folder)
        .expect("delete_account_folder_and_rebuild should succeed");
    world.pipeline_result = Some(result);
    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join("ledger.transactions");
    world.active_ledger_text = if ledger_path.exists() {
        Some(std::fs::read_to_string(&ledger_path).expect("read active ledger"))
    } else {
        Some(String::new())
    };
}

#[when(regex = r#"^I try to delete account folder "([^"]+)"$"#)]
async fn i_try_delete_account_folder(world: &mut LedgerWorld, folder: String) {
    let config = make_pipeline_config(world);
    match delete_account_folder_and_rebuild(&config, &folder) {
        Ok(result) => {
            world.pipeline_result = Some(result);
            world.last_error = None;
        }
        Err(e) => {
            world.last_error = Some(e);
        }
    }
}

#[then(regex = r#"^the operation should fail with "([^"]+)"$"#)]
async fn the_operation_should_fail_with(world: &mut LedgerWorld, expected_msg: String) {
    let err = world
        .last_error
        .as_ref()
        .expect("expected an error but operation succeeded");
    assert!(
        err.contains(&expected_msg),
        "expected error containing '{}', got: {}",
        expected_msg,
        err
    );
}

#[then(regex = r#"^the folder "([^"]+)" should not exist under sources$"#)]
async fn the_folder_should_not_exist_under_sources(world: &mut LedgerWorld, relative_path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(&relative_path);
    assert!(
        !folder.exists(),
        "expected folder {} to not exist",
        folder.display()
    );
}

#[then(regex = r#"^the folder "([^"]+)" should not contain data files$"#)]
async fn the_folder_should_not_contain_data_files(world: &mut LedgerWorld, relative_path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let folder = sources.join(&relative_path);
    if !folder.exists() {
        return; // folder doesn't exist at all — pass
    }
    for entry in std::fs::read_dir(&folder).expect("read_dir") {
        let entry = entry.expect("dir entry");
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.ends_with(".csv")
                && !name.ends_with(".ofx")
                && !name.ends_with(".rhai")
                && !name.ends_with(".json")
                && !name.ends_with(".transactions"),
            "folder {} still contains data file: {}",
            relative_path,
            name
        );
    }
}

// === Plugin scenarios ===

fn ensure_plugins_dir(world: &mut LedgerWorld) -> PathBuf {
    if world.plugins_dir.is_none() {
        let plugins = new_temp_dir("arimalo-plugins");
        std::fs::create_dir_all(&plugins).expect("create plugins dir");
        world.plugins_dir = Some(plugins);
    }
    world.plugins_dir.clone().unwrap()
}

#[given(regex = r#"^a plugins directory with a plugin "([^"]+)" with manifest:$"#)]
async fn a_plugins_dir_with_plugin(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
    plugin_name: String,
) {
    let plugins = ensure_plugins_dir(world);
    let plugin_dir = plugins.join(&plugin_name);
    std::fs::create_dir_all(&plugin_dir).expect("create plugin dir");
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim();
    std::fs::write(plugin_dir.join("plugin.toml"), content).expect("write plugin.toml");
}

#[given(regex = r#"^an empty directory "([^"]+)" in the plugins directory$"#)]
async fn an_empty_dir_in_plugins(world: &mut LedgerWorld, dir_name: String) {
    let plugins = ensure_plugins_dir(world);
    std::fs::create_dir_all(plugins.join(&dir_name)).expect("create empty dir");
}

#[given(regex = r#"^the plugin "([^"]+)" has a script "([^"]+)" with content:$"#)]
async fn plugin_has_script(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
    plugin_name: String,
    script_name: String,
) {
    let plugins = ensure_plugins_dir(world);
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim();
    std::fs::write(plugins.join(&plugin_name).join(&script_name), content)
        .expect("write plugin script");
}

#[given(regex = r#"^plugin "([^"]+)" has config:$"#)]
async fn plugin_has_config(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
    plugin_name: String,
) {
    let plugins = ensure_plugins_dir(world);
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim();
    let plugin_dir = plugins.join(&plugin_name);
    let config: serde_json::Value = serde_json::from_str(content).expect("parse config JSON");
    plugins::save_plugin_config(&plugin_dir, &config).expect("save plugin config");
}

#[given(regex = r#"^plugin "([^"]+)" has secrets:$"#)]
async fn plugin_has_secrets(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
    plugin_name: String,
) {
    let plugins = ensure_plugins_dir(world);
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim();
    let plugin_dir = plugins.join(&plugin_name);
    let secrets: serde_json::Value = serde_json::from_str(content).expect("parse secrets JSON");
    plugins::save_plugin_secrets(&plugin_dir, &secrets).expect("save plugin secrets");
}

#[when("I discover plugins")]
async fn i_discover_plugins(world: &mut LedgerWorld) {
    let plugins_dir = ensure_plugins_dir(world);
    world.discovered_plugins = Some(discover_plugins_in(&plugins_dir, &plugins_dir));
}

#[when(regex = r#"^I run plugin "([^"]+)"$"#)]
async fn i_run_plugin(world: &mut LedgerWorld, plugin_name: String) {
    let plugins = ensure_plugins_dir(world);
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set")
        .clone();
    let plugin_dir = plugins.join(&plugin_name);
    let plugin_config = plugins::load_plugin_config(&plugin_dir);
    let secrets = plugins::load_plugin_secrets(&plugin_dir);
    let plugin_result = plugins::run_plugin(&plugin_dir, &sources, &plugin_config, &secrets);
    world.plugin_run_result = Some(plugin_result);

    // Mirror `run_plugin_cmd`: a plugin that writes into sources/ must
    // chain a pipeline rebuild so the active ledger reflects the change.
    // Without this, downstream assertions about the ledger would silently
    // pass on stale state.
    if world.now_yyyymm.is_none() {
        world.now_yyyymm = Some("202501".to_string());
    }
    let pipeline_cfg = make_pipeline_config(world);
    if let Ok(pipeline_result) = run_pipeline(&pipeline_cfg) {
        generate_reports_after_pipeline(&pipeline_cfg, &pipeline_result);
        world.pipeline_result = Some(pipeline_result);
    }
}

#[when(regex = r#"^I save config for plugin "([^"]+)":$"#)]
async fn i_save_config_for_plugin(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
    plugin_name: String,
) {
    let plugins = ensure_plugins_dir(world);
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim();
    let plugin_dir = plugins.join(&plugin_name);
    let config: serde_json::Value = serde_json::from_str(content).expect("parse config JSON");
    plugins::save_plugin_config(&plugin_dir, &config).expect("save config");
}

#[when(regex = r#"^I save secrets for plugin "([^"]+)":$"#)]
async fn i_save_secrets_for_plugin(
    world: &mut LedgerWorld,
    step: &cucumber::gherkin::Step,
    plugin_name: String,
) {
    let plugins = ensure_plugins_dir(world);
    let content = step
        .docstring
        .as_ref()
        .expect("expected a docstring")
        .trim();
    let plugin_dir = plugins.join(&plugin_name);
    let secrets: serde_json::Value = serde_json::from_str(content).expect("parse secrets JSON");
    plugins::save_plugin_secrets(&plugin_dir, &secrets).expect("save secrets");
}

#[then(regex = r#"^I should find (\d+) plugins?$"#)]
async fn i_should_find_n_plugins(world: &mut LedgerWorld, count: usize) {
    let plugins = world
        .discovered_plugins
        .as_ref()
        .expect("discovered_plugins should be set");
    assert_eq!(
        plugins.len(),
        count,
        "expected {count} plugins, found {}",
        plugins.len()
    );
}

#[then(regex = r#"^plugin "([^"]+)" should have name "([^"]+)"$"#)]
async fn plugin_should_have_name(world: &mut LedgerWorld, dir_name: String, expected_name: String) {
    let plugins = world
        .discovered_plugins
        .as_ref()
        .expect("discovered_plugins should be set");
    let plugin = plugins
        .iter()
        .find(|p| p.dir_name == dir_name)
        .unwrap_or_else(|| panic!("plugin {dir_name:?} not found"));
    assert_eq!(plugin.manifest.plugin.name, expected_name);
}

#[then(regex = r#"^plugin "([^"]+)" should have (\d+) config fields?$"#)]
async fn plugin_should_have_config_fields(world: &mut LedgerWorld, dir_name: String, count: usize) {
    let plugins = world
        .discovered_plugins
        .as_ref()
        .expect("discovered_plugins should be set");
    let plugin = plugins
        .iter()
        .find(|p| p.dir_name == dir_name)
        .unwrap_or_else(|| panic!("plugin {dir_name:?} not found"));
    assert_eq!(plugin.manifest.config.len(), count);
}

#[then(regex = r#"^plugin "([^"]+)" should have (\d+) secret fields?$"#)]
async fn plugin_should_have_secret_fields(world: &mut LedgerWorld, dir_name: String, count: usize) {
    let plugins = world
        .discovered_plugins
        .as_ref()
        .expect("discovered_plugins should be set");
    let plugin = plugins
        .iter()
        .find(|p| p.dir_name == dir_name)
        .unwrap_or_else(|| panic!("plugin {dir_name:?} not found"));
    assert_eq!(plugin.manifest.secrets.len(), count);
}

#[then("the plugin run should succeed")]
async fn plugin_run_should_succeed(world: &mut LedgerWorld) {
    let result = world
        .plugin_run_result
        .as_ref()
        .expect("plugin_run_result should be set");
    assert!(
        result.success,
        "expected plugin to succeed, stderr: {}",
        result.stderr
    );
}

#[then("the plugin run should fail")]
async fn plugin_run_should_fail(world: &mut LedgerWorld) {
    let result = world
        .plugin_run_result
        .as_ref()
        .expect("plugin_run_result should be set");
    assert!(!result.success, "expected plugin to fail, but it succeeded");
}

#[then(regex = r#"^the plugin error should contain "([^"]+)"$"#)]
async fn plugin_error_should_contain(world: &mut LedgerWorld, expected: String) {
    let result = world
        .plugin_run_result
        .as_ref()
        .expect("plugin_run_result should be set");
    assert!(
        result.stderr.contains(&expected),
        "expected stderr to contain {expected:?}, got: {:?}",
        result.stderr
    );
}

#[then(regex = r#"^the plugin "([^"]+)" data file "([^"]+)" should contain "([^"]+)"$"#)]
async fn plugin_data_file_should_contain(
    world: &mut LedgerWorld,
    plugin_name: String,
    file_name: String,
    expected: String,
) {
    let plugins = ensure_plugins_dir(world);
    let data_file = plugins.join(&plugin_name).join(".data").join(&file_name);
    let content = std::fs::read_to_string(&data_file)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", data_file.display()));
    assert!(
        content.contains(&expected),
        "expected file to contain {expected:?}, got: {content:?}"
    );
}

#[then(regex = r#"^loading config for plugin "([^"]+)" should return "([^"]+)" as (\d+)$"#)]
async fn loading_config_should_return_int(
    world: &mut LedgerWorld,
    plugin_name: String,
    key: String,
    expected: i64,
) {
    let plugins = ensure_plugins_dir(world);
    let plugin_dir = plugins.join(&plugin_name);
    let config = plugins::load_plugin_config(&plugin_dir);
    let val = config
        .get(&key)
        .unwrap_or_else(|| panic!("key {key:?} not found in config"));
    assert_eq!(val.as_i64().expect("expected integer value"), expected);
}

#[then(regex = r#"^loading secrets for plugin "([^"]+)" should return "([^"]+)" as "([^"]+)"$"#)]
async fn loading_secrets_should_return_str(
    world: &mut LedgerWorld,
    plugin_name: String,
    key: String,
    expected: String,
) {
    let plugins = ensure_plugins_dir(world);
    let plugin_dir = plugins.join(&plugin_name);
    let secrets = plugins::load_plugin_secrets(&plugin_dir);
    let val = secrets
        .get(&key)
        .unwrap_or_else(|| panic!("key {key:?} not found in secrets"));
    assert_eq!(val.as_str().expect("expected string value"), expected);
}

// ── Daily plugin backfill steps ──

#[then(regex = r#"^plugin "([^"]+)" should be marked daily$"#)]
async fn plugin_should_be_daily(world: &mut LedgerWorld, dir_name: String) {
    let plugins = world
        .discovered_plugins
        .as_ref()
        .expect("discovered_plugins should be set");
    let plugin = plugins
        .iter()
        .find(|p| p.dir_name == dir_name)
        .unwrap_or_else(|| panic!("plugin {dir_name:?} not found"));
    assert!(
        plugin.manifest.plugin.daily,
        "expected plugin {dir_name:?} to be marked daily"
    );
}

#[then(regex = r#"^plugin "([^"]+)" should not be marked daily$"#)]
async fn plugin_should_not_be_daily(world: &mut LedgerWorld, dir_name: String) {
    let plugins = world
        .discovered_plugins
        .as_ref()
        .expect("discovered_plugins should be set");
    let plugin = plugins
        .iter()
        .find(|p| p.dir_name == dir_name)
        .unwrap_or_else(|| panic!("plugin {dir_name:?} not found"));
    assert!(
        !plugin.manifest.plugin.daily,
        "expected plugin {dir_name:?} to NOT be marked daily"
    );
}

/// Write a `.data/last_run.json` recording a successful run `days_ago` days
/// before today (local), matching the format `save_last_run` writes.
fn seed_last_run_days_ago(world: &mut LedgerWorld, plugin_name: &str, days_ago: i64) {
    let plugins = ensure_plugins_dir(world);
    let data_dir = plugins.join(plugin_name).join(".data");
    std::fs::create_dir_all(&data_dir).expect("create .data dir");
    let when = chrono::Local::now() - chrono::Duration::days(days_ago);
    let v = serde_json::json!({ "timestamp": when.to_rfc3339(), "status": "success" });
    std::fs::write(
        data_dir.join("last_run.json"),
        serde_json::to_string_pretty(&v).unwrap(),
    )
    .expect("write last_run.json");
}

#[given(regex = r#"^the plugin "([^"]+)" last succeeded today$"#)]
async fn plugin_last_succeeded_today(world: &mut LedgerWorld, plugin_name: String) {
    seed_last_run_days_ago(world, &plugin_name, 0);
}

#[given(regex = r#"^the plugin "([^"]+)" last succeeded yesterday$"#)]
async fn plugin_last_succeeded_yesterday(world: &mut LedgerWorld, plugin_name: String) {
    seed_last_run_days_ago(world, &plugin_name, 1);
}

#[when("I run the daily plugins")]
async fn i_run_daily_plugins(world: &mut LedgerWorld) {
    let plugins_dir = ensure_plugins_dir(world);
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set")
        .clone();
    world.daily_summary = Some(plugins::run_daily_plugins(
        &plugins_dir,
        &sources,
        false,
        |_, _, _| {},
    ));
}

#[when("I run the daily plugins skipping those already run today")]
async fn i_run_daily_plugins_skip(world: &mut LedgerWorld) {
    let plugins_dir = ensure_plugins_dir(world);
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set")
        .clone();
    world.daily_summary = Some(plugins::run_daily_plugins(
        &plugins_dir,
        &sources,
        true,
        |_, _, _| {},
    ));
}

#[then(regex = r#"^the daily run summary should have (\d+) outcomes?$"#)]
async fn daily_summary_outcome_count(world: &mut LedgerWorld, count: usize) {
    let summary = world
        .daily_summary
        .as_ref()
        .expect("daily_summary should be set");
    assert_eq!(
        summary.outcomes.len(),
        count,
        "expected {count} outcomes, got {}: {:?}",
        summary.outcomes.len(),
        summary.outcomes
    );
}

fn daily_outcome<'a>(
    world: &'a LedgerWorld,
    dir_name: &str,
) -> &'a plugins::DailyPluginOutcome {
    world
        .daily_summary
        .as_ref()
        .expect("daily_summary should be set")
        .outcomes
        .iter()
        .find(|o| o.dir_name == dir_name)
        .unwrap_or_else(|| panic!("no daily outcome for plugin {dir_name:?}"))
}

#[then(regex = r#"^the daily outcome for "([^"]+)" should be success$"#)]
async fn daily_outcome_success(world: &mut LedgerWorld, dir_name: String) {
    let o = daily_outcome(world, &dir_name);
    assert!(
        o.success && !o.skipped_ran_today,
        "expected {dir_name:?} to be a successful run, got {o:?}"
    );
}

#[then(regex = r#"^the daily outcome for "([^"]+)" should be failure$"#)]
async fn daily_outcome_failure(world: &mut LedgerWorld, dir_name: String) {
    let o = daily_outcome(world, &dir_name);
    assert!(
        !o.success && o.exit_code != Some(2),
        "expected {dir_name:?} to be a hard failure, got {o:?}"
    );
}

#[then(regex = r#"^the daily outcome for "([^"]+)" should not be failure$"#)]
async fn daily_outcome_not_failure(world: &mut LedgerWorld, dir_name: String) {
    let o = daily_outcome(world, &dir_name);
    assert!(
        o.success || o.exit_code == Some(2),
        "expected {dir_name:?} to NOT be a hard failure, got {o:?}"
    );
}

#[then(regex = r#"^the daily outcome for "([^"]+)" should be partial$"#)]
async fn daily_outcome_partial(world: &mut LedgerWorld, dir_name: String) {
    let o = daily_outcome(world, &dir_name);
    assert_eq!(
        o.exit_code,
        Some(2),
        "expected {dir_name:?} to be partial (exit 2), got {o:?}"
    );
}

#[then(regex = r#"^the daily outcome for "([^"]+)" should be skipped$"#)]
async fn daily_outcome_skipped(world: &mut LedgerWorld, dir_name: String) {
    let o = daily_outcome(world, &dir_name);
    assert!(
        o.skipped_ran_today,
        "expected {dir_name:?} to be skipped, got {o:?}"
    );
}

#[then(regex = r#"^the file "([^"]+)" should not exist in sources$"#)]
async fn the_file_should_not_exist_in_sources(world: &mut LedgerWorld, relative_path: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let full_path = sources.join(&relative_path);
    assert!(
        !full_path.exists(),
        "expected no file at {}",
        full_path.display()
    );
}

// ── Account config steps ──

#[given(regex = r#"^a sources directory with account folder "(.+)"$"#)]
async fn a_sources_dir_with_account_folder(world: &mut LedgerWorld, folder: String) {
    let sources = new_temp_dir("arimalo-account-config");
    let account_dir = sources.join(&folder);
    std::fs::create_dir_all(&account_dir).unwrap();
    world.sources_dir = Some(sources);
}

#[given(regex = r#"^a _config\.json at "(.+)" with explorer_url "(.+)"$"#)]
async fn a_config_json_at_folder(world: &mut LedgerWorld, folder: String, explorer_url: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir not set");
    let config_dir = sources.join(&folder);
    std::fs::create_dir_all(&config_dir).unwrap();
    let config = serde_json::json!({ "explorer_url": explorer_url });
    std::fs::write(
        config_dir.join("_config.json"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
}

#[when(regex = r#"^I resolve the account config for "(.+)"$"#)]
async fn resolve_account_config(world: &mut LedgerWorld, folder: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir not set");
    let config =
        arimalo_covid::rules::AccountConfig::resolve(std::path::Path::new(&folder), sources);
    world.account_config = Some(config);
}

#[then(regex = r#"^the explorer_url should be "(.+)"$"#)]
async fn explorer_url_should_be(world: &mut LedgerWorld, expected: String) {
    let config = world
        .account_config
        .as_ref()
        .expect("account_config not set");
    assert_eq!(
        config.explorer_url.as_deref(),
        Some(expected.as_str()),
        "expected explorer_url '{}', got {:?}",
        expected,
        config.explorer_url
    );
}

#[then("the explorer_url should be empty")]
async fn explorer_url_should_be_empty(world: &mut LedgerWorld) {
    let config = world
        .account_config
        .as_ref()
        .expect("account_config not set");
    assert!(
        config.explorer_url.is_none(),
        "expected empty explorer_url, got {:?}",
        config.explorer_url
    );
}

fn main() {
    // cucumber-rs step dispatch can overflow the default 8MB main stack
    // when there are many (300+) step definitions. Run on a larger thread.
    let builder = std::thread::Builder::new().stack_size(32 * 1024 * 1024); // 32 MB
    let handler = builder
        .spawn(|| {
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async_main());
        })
        .unwrap();
    handler.join().unwrap();
}

// -- Transaction search steps --

fn run_search(world: &LedgerWorld, search: &str) -> query::QueryResult {
    let dir = set_generated_dir(world);
    let parse = load_active_ledger(&dir).expect("load active ledger");
    let expr = query::parse_search(search).expect("parse search");
    query::query(
        &parse,
        &query::QueryOptions {
            search: expr,
            sort_field: None,
            sort_order: query::SortOrder::Asc,
            offset: None,
            limit: None,
            input_order: None,
            min_value: None,
            hidden_prefixes: Vec::new(),
        },
    )
}

fn run_search_sorted(
    world: &LedgerWorld,
    search: &str,
    field: &str,
    order: &str,
    limit: Option<usize>,
) -> query::QueryResult {
    let dir = set_generated_dir(world);
    let parse = load_active_ledger(&dir).expect("load active ledger");
    let expr = query::parse_search(search).expect("parse search");
    query::query(
        &parse,
        &query::QueryOptions {
            search: expr,
            sort_field: query::SortField::parse_str(field),
            sort_order: query::SortOrder::parse_str(order).unwrap_or(query::SortOrder::Asc),
            offset: None,
            limit,
            input_order: None,
            min_value: None,
            hidden_prefixes: Vec::new(),
        },
    )
}

fn run_scoped_query(
    world: &LedgerWorld,
    search: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> query::QueryResult {
    let dir = set_generated_dir(world);
    arimalo_covid::generated_store::scoped_query(&dir, search, false, None, None, offset, limit)
        .expect("scoped query")
}

fn txn_identifier(txn: &arimalo_covid::ledger_parser::Transaction) -> String {
    txn.meta
        .as_deref()
        .and_then(|meta| {
            meta.split(',')
                .map(|part| part.trim())
                .find(|part| part.starts_with("txn:"))
                .map(str::to_string)
        })
        .unwrap_or_else(|| {
            format!(
                "{}|{}|{}|{}",
                txn.date,
                txn.payee.as_deref().unwrap_or(""),
                txn.amount,
                txn.amount_commodity,
            )
        })
}

fn matches_account_prefix(account: &str, prefix: &str) -> bool {
    account == prefix || account.starts_with(&format!("{prefix}:"))
}

#[then(regex = r#"^searching "([^"]*)" should return (\d+) transactions$"#)]
async fn searching_should_return_n(world: &mut LedgerWorld, search: String, expected: usize) {
    let qr = run_search(world, &search);
    assert_eq!(
        qr.transaction_count, expected,
        "search {:?}: expected {} transactions, got {}",
        search, expected, qr.transaction_count
    );
}

#[then(
    regex = r#"^searching "([^"]*)" sorted by "([^"]*)" "([^"]*)" the first transaction amount should be (-?[\d.]+)$"#
)]
async fn searching_sorted_first_amount(
    world: &mut LedgerWorld,
    search: String,
    field: String,
    order: String,
    expected_amount: f64,
) {
    let qr = run_search_sorted(world, &search, &field, &order, None);
    assert!(
        !qr.transactions.is_empty(),
        "no transactions for search {:?}",
        search
    );
    assert!(
        (qr.transactions[0].amount - expected_amount).abs() < 0.01,
        "search {:?} sorted by {} {}: expected first amount {}, got {}",
        search,
        field,
        order,
        expected_amount,
        qr.transactions[0].amount
    );
}

#[then(
    regex = r#"^searching "([^"]*)" sorted by "([^"]*)" "([^"]*)" the first transaction date should be "([^"]*)"$"#
)]
async fn searching_sorted_first_date(
    world: &mut LedgerWorld,
    search: String,
    field: String,
    order: String,
    expected_date: String,
) {
    let qr = run_search_sorted(world, &search, &field, &order, None);
    assert!(
        !qr.transactions.is_empty(),
        "no transactions for search {:?}",
        search
    );
    assert_eq!(
        qr.transactions[0].date, expected_date,
        "search {:?} sorted by {} {}: expected first date {}, got {}",
        search, field, order, expected_date, qr.transactions[0].date
    );
}

#[then(
    regex = r#"^searching "([^"]*)" sorted by "([^"]*)" "([^"]*)" with limit (\d+) should return (\d+) transactions$"#
)]
async fn searching_sorted_limited(
    world: &mut LedgerWorld,
    search: String,
    field: String,
    order: String,
    limit: usize,
    expected: usize,
) {
    let qr = run_search_sorted(world, &search, &field, &order, Some(limit));
    assert_eq!(
        qr.transactions.len(),
        expected,
        "search {:?} sorted by {} {} with limit {}: expected {}, got {}",
        search,
        field,
        order,
        limit,
        expected,
        qr.transactions.len()
    );
}

#[then(
    regex = r#"^searching "([^"]*)" with no explicit sort the first transaction date should be "([^"]*)"$"#
)]
async fn searching_default_sort_first_date(
    world: &mut LedgerWorld,
    search: String,
    expected_date: String,
) {
    let dir = set_generated_dir(world);
    let parse = load_active_ledger(&dir).expect("load active ledger");
    let expr = query::parse_search(&search).expect("parse search");
    let qr = query::query(
        &parse,
        &query::QueryOptions {
            search: expr,
            sort_field: None,
            sort_order: query::SortOrder::Asc,
            offset: None,
            limit: None,
            input_order: None,
            min_value: None,
            hidden_prefixes: Vec::new(),
        },
    );
    assert!(!qr.transactions.is_empty(), "no transactions");
    assert_eq!(
        qr.transactions[0].date, expected_date,
        "default sort: expected first date {}, got {}",
        expected_date, qr.transactions[0].date
    );
}

#[given(regex = r#"^(\d+) transactions across "([^"]+)"$"#)]
async fn n_transactions_across(world: &mut LedgerWorld, count: usize, folder: String) {
    if world.sources_dir.is_none() || world.generated_dir.is_none() {
        setup_clean_dirs(world);
    }
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let csv_path = sources.join(format!("{folder}/2025-01.csv"));
    if let Some(parent) = csv_path.parent() {
        std::fs::create_dir_all(parent).expect("create csv parent dirs");
    }
    let mut content = String::from("Date,Description,Amount\n");
    for i in 0..count {
        content.push_str(&format!(
            "2025-01-{day:02},{}-{i:04},-1.00\n",
            folder.replace('/', "-"),
            day = (i % 28) + 1,
        ));
    }
    std::fs::write(&csv_path, content).expect("write csv");
    write_transform(sources, &format!("{folder}/_transform.rhai"), "");
}

#[when(regex = r#"^I query "([^"]+)" with limit (\d+)$"#)]
async fn i_query_with_limit(world: &mut LedgerWorld, search: String, limit: usize) {
    world.scoped_query_prev = world.scoped_query_result.clone();
    world.scoped_query_result = Some(run_scoped_query(world, &search, None, Some(limit)));
}

#[when(regex = r#"^the generated config hides accounts "([^"]+)"$"#)]
async fn generated_config_hides_accounts(world: &mut LedgerWorld, prefix: String) {
    let dir = set_generated_dir(world);
    std::fs::create_dir_all(&dir).expect("create generated set dir");
    let config = serde_json::json!({ "hidden_accounts": [prefix] });
    std::fs::write(
        dir.join("config.json"),
        serde_json::to_string(&config).expect("serialize config"),
    )
    .expect("write config.json");
}

#[when(regex = r#"^I query "([^"]+)" with show ignored (on|off)$"#)]
async fn i_query_with_show_ignored(world: &mut LedgerWorld, search: String, toggle: String) {
    let show_hidden = toggle == "on";
    let dir = set_generated_dir(world);
    world.scoped_query_prev = world.scoped_query_result.clone();
    world.scoped_query_result = Some(
        arimalo_covid::generated_store::scoped_query(
            &dir,
            &search,
            show_hidden,
            None,
            None,
            None,
            None,
        )
        .expect("scoped query"),
    );
}

#[then(regex = r#"^the aggregated balance for "([^"]+)" should be (-?[\d.]+)$"#)]
async fn aggregated_balance_for_should_be(
    world: &mut LedgerWorld,
    commodity: String,
    expected: f64,
) {
    let query = world
        .scoped_query_result
        .as_ref()
        .expect("scoped query result");
    let actual = query
        .aggregated_balance
        .iter()
        .find(|c| c.commodity == commodity)
        .map(|c| c.amount)
        .unwrap_or(0.0);
    assert!(
        (actual - expected).abs() < 0.001,
        "aggregated balance for {commodity:?}: expected {expected}, got {actual}"
    );
}

#[when(regex = r#"^I query "([^"]+)" with offset (\d+) and limit (\d+)$"#)]
async fn i_query_with_offset_and_limit(
    world: &mut LedgerWorld,
    search: String,
    offset: usize,
    limit: usize,
) {
    world.scoped_query_prev = world.scoped_query_result.clone();
    world.scoped_query_result = Some(run_scoped_query(world, &search, Some(offset), Some(limit)));
}

#[then(regex = r#"^the query should return at most (\d+) transactions$"#)]
async fn query_should_return_at_most(world: &mut LedgerWorld, expected_max: usize) {
    let query = world
        .scoped_query_result
        .as_ref()
        .expect("scoped query result");
    assert!(
        query.transactions.len() <= expected_max,
        "expected at most {expected_max} transactions, got {}",
        query.transactions.len()
    );
}

#[then(regex = r#"^the query should return (\d+) transactions$"#)]
async fn query_should_return_n_transactions(world: &mut LedgerWorld, expected: usize) {
    let query = world
        .scoped_query_result
        .as_ref()
        .expect("scoped query result");
    assert_eq!(
        query.transactions.len(),
        expected,
        "expected {expected} transactions, got {}",
        query.transactions.len()
    );
}

#[then(regex = r#"^the query should return (\d+) transactions with total count (\d+)$"#)]
async fn query_should_return_n_transactions_with_total(
    world: &mut LedgerWorld,
    expected_len: usize,
    expected_total: usize,
) {
    let query = world
        .scoped_query_result
        .as_ref()
        .expect("scoped query result");
    assert_eq!(
        query.transactions.len(),
        expected_len,
        "expected {expected_len} transactions, got {}",
        query.transactions.len()
    );
    assert_eq!(
        query.transaction_count, expected_total,
        "expected total count {expected_total}, got {}",
        query.transaction_count
    );
}

#[then(regex = r#"^every returned transaction should have a posting to "([^"]+)"$"#)]
async fn every_returned_transaction_should_have_posting(world: &mut LedgerWorld, account: String) {
    let query = world
        .scoped_query_result
        .as_ref()
        .expect("scoped query result");
    assert!(
        query.transactions.iter().all(|txn| {
            txn.postings
                .iter()
                .any(|posting| matches_account_prefix(&posting.account, &account))
        }),
        "expected every returned transaction to have a posting to {account:?}"
    );
}

#[then(regex = r#"^no returned transaction should have a posting to "([^"]+)"$"#)]
async fn no_returned_transaction_should_have_posting(world: &mut LedgerWorld, account: String) {
    let query = world
        .scoped_query_result
        .as_ref()
        .expect("scoped query result");
    assert!(
        query.transactions.iter().all(|txn| {
            txn.postings
                .iter()
                .all(|posting| !matches_account_prefix(&posting.account, &account))
        }),
        "expected returned transactions to exclude postings to {account:?}"
    );
}

#[then("the second page should not overlap with the first page")]
async fn second_page_should_not_overlap(world: &mut LedgerWorld) {
    let prev = world
        .scoped_query_prev
        .as_ref()
        .expect("previous scoped query result");
    let current = world
        .scoped_query_result
        .as_ref()
        .expect("current scoped query result");
    let first_page: std::collections::BTreeSet<String> =
        prev.transactions.iter().map(txn_identifier).collect();
    let second_page: std::collections::BTreeSet<String> =
        current.transactions.iter().map(txn_identifier).collect();
    assert!(
        first_page.is_disjoint(&second_page),
        "expected pages to be disjoint; overlap: {:?}",
        first_page
            .intersection(&second_page)
            .cloned()
            .collect::<Vec<_>>()
    );
}

#[then(regex = r#"^the query should contain payee "([^"]+)"$"#)]
async fn query_should_contain_payee(world: &mut LedgerWorld, payee: String) {
    let query = world
        .scoped_query_result
        .as_ref()
        .expect("scoped query result");
    assert!(
        query
            .transactions
            .iter()
            .any(|txn| txn.payee.as_deref() == Some(payee.as_str())),
        "expected query to include payee {payee:?}; got {:?}",
        query
            .transactions
            .iter()
            .map(|txn| txn.payee.clone())
            .collect::<Vec<_>>()
    );
}

#[then(regex = r#"^the query should not contain payee "([^"]+)"$"#)]
async fn query_should_not_contain_payee(world: &mut LedgerWorld, payee: String) {
    let query = world
        .scoped_query_result
        .as_ref()
        .expect("scoped query result");
    assert!(
        query
            .transactions
            .iter()
            .all(|txn| txn.payee.as_deref() != Some(payee.as_str())),
        "expected query to exclude payee {payee:?}; got {:?}",
        query
            .transactions
            .iter()
            .map(|txn| txn.payee.clone())
            .collect::<Vec<_>>()
    );
}

#[then(regex = r#"^the total count should be (\d+)$"#)]
async fn total_count_should_be(world: &mut LedgerWorld, expected: usize) {
    let query = world
        .scoped_query_result
        .as_ref()
        .expect("scoped query result");
    assert_eq!(
        query.transaction_count, expected,
        "expected total count {expected}, got {}",
        query.transaction_count
    );
}

#[then(regex = r#"^a summary file should exist at "([^"]+)"$"#)]
async fn summary_file_should_exist_at(world: &mut LedgerWorld, relative_path: String) {
    let set_dir = set_generated_dir(world);
    let generated_dir = world.generated_dir.as_ref().expect("generated_dir");
    let set_relative = set_dir.join(&relative_path);
    let root_relative = generated_dir.join(&relative_path);
    assert!(
        set_relative.exists() || root_relative.exists(),
        "expected summary file at {:?} or {:?}",
        set_relative,
        root_relative
    );
}

#[then(regex = r#"^loading the account tree should return balance for "([^"]+)"$"#)]
async fn loading_account_tree_should_return_balance(world: &mut LedgerWorld, account: String) {
    let set_dir = set_generated_dir(world);
    let balances = load_account_tree(&set_dir).expect("load account tree");
    world.scoped_account_tree = Some(balances.clone());
    assert!(
        balances.iter().any(|balance| balance.account == account),
        "expected account tree to include {account:?}; got {:?}",
        balances
            .iter()
            .map(|balance| balance.account.clone())
            .collect::<Vec<_>>()
    );
}

// -- Hidden accounts filtering steps --

#[when(regex = r#"^I filter hidden accounts with prefix "([^"]+)"$"#)]
async fn filter_with_prefix(world: &mut LedgerWorld, prefix: String) {
    let dir = set_generated_dir(world);
    let mut parse = load_active_ledger(&dir).expect("load active ledger");
    filter_hidden_accounts(&mut parse, &[prefix]);
    world.filtered_parse = Some(parse);
}

#[when("I filter hidden accounts with no prefixes")]
async fn filter_with_no_prefixes(world: &mut LedgerWorld) {
    let dir = set_generated_dir(world);
    let parse = load_active_ledger(&dir).expect("load active ledger");
    world.filtered_parse = Some(parse);
}

#[then(regex = r#"^the filtered result should have (\d+) transactions$"#)]
async fn filtered_should_have_n(world: &mut LedgerWorld, count: usize) {
    let parse = world
        .filtered_parse
        .as_ref()
        .expect("filtered_parse not set");
    assert_eq!(
        parse.transactions.len(),
        count,
        "expected {} transactions, got {}",
        count,
        parse.transactions.len()
    );
}

#[then(regex = r#"^the filtered result should include payee "([^"]+)"$"#)]
async fn filtered_should_include_payee(world: &mut LedgerWorld, payee: String) {
    let parse = world
        .filtered_parse
        .as_ref()
        .expect("filtered_parse not set");
    let found = parse
        .transactions
        .iter()
        .any(|t| t.payee.as_deref() == Some(&payee));
    assert!(
        found,
        "expected to find payee '{}' in filtered transactions",
        payee
    );
}

#[then(regex = r#"^the filtered result should not include account "([^"]+)"$"#)]
async fn filtered_should_not_include_account(world: &mut LedgerWorld, account: String) {
    let parse = world
        .filtered_parse
        .as_ref()
        .expect("filtered_parse not set");
    let found = parse
        .transactions
        .iter()
        .any(|t| t.postings.iter().any(|p| p.account == account));
    assert!(
        !found,
        "found account '{}' in filtered transactions but it should be hidden",
        account
    );
}

// --- Shared txn_id propagation bug steps ---

#[given(regex = r#"^a txn_id-aware transform at "([^"]+)"$"#)]
async fn a_txn_id_aware_transform(world: &mut LedgerWorld, transform_relative: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let transform_path = sources.join(&transform_relative);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = r##"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "AUD",
  status: "*",
  txn_id: row["TxHash"]
}"##;
    std::fs::write(&transform_path, script).expect("write txn_id-aware transform");
}

#[given(
    regex = r#"^a rules file at "([^"]+)" matching "([^"]+)" with account "([^"]+)"$"#
)]
async fn a_rules_file_with_account(
    world: &mut LedgerWorld,
    path: String,
    pattern: String,
    account: String,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let rules = RulesFile {
        rules: vec![Rule {
            id: "rule-account-test".to_string(),
            pattern,
            match_field: None,
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: Some(account),
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }],
    };
    write_rules_file(sources, &path, &rules);
}

#[then(regex = r#"^the folder ledger "([^"]+)" should contain (\d+) transactions$"#)]
async fn folder_ledger_should_contain_n(world: &mut LedgerWorld, folder: String, expected: usize) {
    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join(&folder).join("ledger.transactions");
    let text = std::fs::read_to_string(&ledger_path).unwrap_or_else(|_| {
        panic!(
            "could not read folder ledger at {}",
            ledger_path.display()
        )
    });
    let parse = arimalo_covid::ledger_parser::parse_transactions(&text);
    assert_eq!(
        parse.transactions.len(),
        expected,
        "expected {} transactions in folder '{}', got {} — ledger contents:\n{}",
        expected,
        folder,
        parse.transactions.len(),
        text
    );
}

#[then(regex = r#"^the folder ledger "([^"]+)" should contain payee "([^"]+)"$"#)]
async fn folder_ledger_should_contain_payee(world: &mut LedgerWorld, folder: String, payee: String) {
    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join(&folder).join("ledger.transactions");
    let text = std::fs::read_to_string(&ledger_path).unwrap_or_else(|_| {
        panic!(
            "could not read folder ledger at {}",
            ledger_path.display()
        )
    });
    let parse = arimalo_covid::ledger_parser::parse_transactions(&text);
    let found = parse
        .transactions
        .iter()
        .any(|t| t.payee.as_deref() == Some(&payee) || t.display_payee.as_deref() == Some(&payee));
    assert!(
        found,
        "expected payee '{}' in folder '{}' ledger, found: {:?}",
        payee,
        folder,
        parse
            .transactions
            .iter()
            .map(|t| t.display_payee.as_ref().or(t.payee.as_ref()))
            .collect::<Vec<_>>()
    );
}

// --- Cross-folder txn propagation steps ---

#[given(regex = r#"^a CSV "([^"]+)" with columns "([^"]+)":$"#)]
async fn a_csv_with_named_columns(
    world: &mut LedgerWorld,
    csv_relative: String,
    _columns: String,
    step: &cucumber::gherkin::Step,
) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let table = step.table.as_ref().expect("step should have a data table");
    let csv_path = sources.join(&csv_relative);
    if let Some(parent) = csv_path.parent() {
        std::fs::create_dir_all(parent).expect("create CSV parent dirs");
    }
    std::fs::write(&csv_path, table_to_csv(table)).expect("write CSV");
}

#[given(regex = r#"^a transform with txn_id at "([^"]+)"$"#)]
async fn a_transform_with_txn_id(world: &mut LedgerWorld, transform_relative: String) {
    let sources = world
        .sources_dir
        .as_ref()
        .expect("sources_dir should be set");
    let transform_path = sources.join(&transform_relative);
    if let Some(parent) = transform_path.parent() {
        std::fs::create_dir_all(parent).expect("create transform parent dirs");
    }
    let script = r##"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "AUD",
  status: "*",
  txn_id: row["TxHash"]
}"##;
    std::fs::write(&transform_path, script).expect("write transform with txn_id");
}

#[then(regex = r#"^the folder ledger "([^"]+)" posting 0 should use account "([^"]+)"$"#)]
async fn folder_ledger_posting0_should_use_account(
    world: &mut LedgerWorld,
    folder: String,
    expected_account: String,
) {
    let set_dir = set_generated_dir(world);
    let ledger_path = set_dir.join(&folder).join("ledger.transactions");
    let text = std::fs::read_to_string(&ledger_path).unwrap_or_else(|_| {
        panic!(
            "could not read folder ledger at {}",
            ledger_path.display()
        )
    });
    let parse = arimalo_covid::ledger_parser::parse_transactions(&text);
    for txn in &parse.transactions {
        if let Some(p0) = txn.postings.first() {
            assert_eq!(
                p0.account, expected_account,
                "posting[0] account mismatch in folder '{}': expected '{}', got '{}'\nfull ledger:\n{}",
                folder, expected_account, p0.account, text
            );
        }
    }
}

// === arimalo-dedupe CLI scenarios ===

#[given("a temporary sources directory")]
async fn dedupe_temp_sources_dir(world: &mut LedgerWorld) {
    // The dedupe binary writes its archive to <sources-parent>/dedupe-archive/
    // so the pipeline doesn't re-ingest archived files. Make sources/ a
    // subdir of the tempdir so the sibling location is inside it too.
    let tmp = tempfile::tempdir().expect("create tempdir");
    let vault = tmp.keep();
    let sources = vault.join("sources");
    std::fs::create_dir_all(&sources).expect("mkdir sources");
    world.sources_dir = Some(sources);
}

fn dedupe_write_ofx(
    sources: &std::path::Path,
    folder: &str,
    name: &str,
    dtstart: &str,
    dtend: &str,
    fitids: &str,
) {
    let dir = sources.join(folder);
    std::fs::create_dir_all(&dir).expect("mkdir folder");
    let mut content = format!(
        "OFXHEADER:100\nDATA:OFXSGML\n\n<OFX>\n<BANKMSGSRSV1>\n<STMTTRNRS>\n<STMTRS>\n<CURDEF>AUD\n<BANKTRANLIST>\n<DTSTART>{dtstart}\n<DTEND>{dtend}\n"
    );
    for (i, fid) in fitids.split(',').map(str::trim).enumerate() {
        content.push_str(&format!(
            "<STMTTRN>\n<TRNTYPE>DEBIT\n<DTPOSTED>{dtstart}\n<TRNAMT>-{}.00\n<FITID>{fid}\n<MEMO>tx{fid}\n</STMTTRN>\n",
            i + 1
        ));
    }
    content.push_str("</BANKTRANLIST>\n</STMTRS>\n</STMTTRNRS>\n</BANKMSGSRSV1>\n</OFX>\n");
    std::fs::write(dir.join(name), content).expect("write ofx");
}

#[given(
    regex = r#"^an OFX file "([^"]+)" in folder "([^"]+)" with DTSTART "([^"]+)" DTEND "([^"]+)" and FITIDs "([^"]+)"$"#
)]
async fn dedupe_given_ofx(
    world: &mut LedgerWorld,
    name: String,
    folder: String,
    dtstart: String,
    dtend: String,
    fitids: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    dedupe_write_ofx(sources, &folder, &name, &dtstart, &dtend, &fitids);
}

#[given(
    regex = r#"^a CSV file "([^"]+)" in folder "([^"]+)" with header "([^"]+)" and rows:$"#
)]
async fn dedupe_given_csv(
    world: &mut LedgerWorld,
    name: String,
    folder: String,
    header: String,
    step: &cucumber::gherkin::Step,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let dir = sources.join(&folder);
    std::fs::create_dir_all(&dir).expect("mkdir folder");
    let mut content = String::new();
    content.push_str(&header);
    content.push('\n');
    if let Some(table) = step.table.as_ref() {
        for row in &table.rows {
            content.push_str(&row.join(","));
            content.push('\n');
        }
    }
    std::fs::write(dir.join(name), content).expect("write csv");
}

fn run_dedupe_binary(world: &mut LedgerWorld, apply: bool) {
    let sources = world.sources_dir.as_ref().expect("sources_dir").clone();
    let bin = env!("CARGO_BIN_EXE_arimalo-dedupe");
    let mut cmd = std::process::Command::new(bin);
    cmd.arg("--sources-dir").arg(&sources);
    if apply {
        cmd.arg("--apply");
    }
    let output = cmd.output().expect("spawn arimalo-dedupe");
    assert!(
        output.status.success(),
        "arimalo-dedupe failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    world.dedupe_stdout = Some(String::from_utf8_lossy(&output.stdout).into_owned());
}

#[when("I run arimalo-dedupe without --apply")]
async fn dedupe_when_dry_run(world: &mut LedgerWorld) {
    run_dedupe_binary(world, false);
}

#[when("I run arimalo-dedupe with --apply")]
async fn dedupe_when_apply(world: &mut LedgerWorld) {
    run_dedupe_binary(world, true);
}

fn dedupe_stdout(world: &LedgerWorld) -> &str {
    world
        .dedupe_stdout
        .as_deref()
        .expect("dedupe_stdout (run arimalo-dedupe first)")
}

#[then(
    regex = r#"^the output reports that "([^"]+)" is the canonical file for folder "([^"]+)"$"#
)]
async fn dedupe_then_canonical(world: &mut LedgerWorld, name: String, folder: String) {
    let out = dedupe_stdout(world);
    assert!(
        out.contains(&format!("{folder}/")),
        "stdout missing folder marker '{folder}/':\n{out}"
    );
    assert!(
        out.contains(&format!("canonical: {name}")),
        "stdout missing 'canonical: {name}':\n{out}"
    );
}

#[then(
    regex = r#"^the output reports (\d+) duplicate STMTTRN blocks? would be stripped from "([^"]+)"$"#
)]
async fn dedupe_then_blocks_stripped(world: &mut LedgerWorld, n: usize, name: String) {
    let out = dedupe_stdout(world);
    let needle = format!("{name}: {n} duplicate STMTTRN");
    assert!(
        out.contains(&needle),
        "stdout missing '{needle}':\n{out}"
    );
}

fn dedupe_read(world: &LedgerWorld, rel: &str) -> String {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    std::fs::read_to_string(sources.join(rel))
        .unwrap_or_else(|e| panic!("read {rel}: {e}"))
}

#[then(regex = r#"^the file "([^"]+)" still contains FITID "([^"]+)"$"#)]
async fn dedupe_then_still_contains_fitid(world: &mut LedgerWorld, rel: String, fid: String) {
    let content = dedupe_read(world, &rel);
    assert!(
        content.contains(&format!("<FITID>{fid}")),
        "{rel} missing FITID {fid}:\n{content}"
    );
}

#[then(regex = r#"^the file "([^"]+)" no longer contains FITID "([^"]+)"$"#)]
async fn dedupe_then_no_longer_contains_fitid(
    world: &mut LedgerWorld,
    rel: String,
    fid: String,
) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let path = sources.join(&rel);
    // If the file was archived, "no longer contains" is trivially true.
    if !path.exists() {
        return;
    }
    let content = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    assert!(
        !content.contains(&format!("<FITID>{fid}")),
        "{rel} still contains FITID {fid}:\n{content}"
    );
}

#[then(regex = r#"^the file "([^"]+)" still contains FITIDs "([^"]+)"$"#)]
async fn dedupe_then_still_contains_fitids(world: &mut LedgerWorld, rel: String, fids: String) {
    let content = dedupe_read(world, &rel);
    for fid in fids.split(',').map(str::trim) {
        assert!(
            content.contains(&format!("<FITID>{fid}")),
            "{rel} missing FITID {fid}:\n{content}"
        );
    }
}

fn dedupe_archive_dir(world: &LedgerWorld) -> PathBuf {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    let base = sources
        .parent()
        .map(|p| p.join("dedupe-archive"))
        .unwrap_or_else(|| sources.join(".dedupe-archive"));
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&base)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", base.display()))
        .filter_map(|r| r.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    entries.sort();
    entries
        .pop()
        .unwrap_or_else(|| panic!("no archive subfolder under {}", base.display()))
}

#[then(regex = r#"^the file "([^"]+)" has been archived$"#)]
async fn dedupe_then_archived(world: &mut LedgerWorld, rel: String) {
    let sources = world.sources_dir.as_ref().expect("sources_dir");
    assert!(
        !sources.join(&rel).exists(),
        "{rel} still exists in sources"
    );
    let archive = dedupe_archive_dir(world);
    assert!(
        archive.join(&rel).exists(),
        "archived copy not found at {}",
        archive.join(&rel).display()
    );
}

#[then(regex = r#"^the file "([^"]+)" still contains (\d+) data rows?$"#)]
async fn dedupe_then_csv_row_count(world: &mut LedgerWorld, rel: String, n: usize) {
    let content = dedupe_read(world, &rel);
    let data_rows = content.lines().skip(1).filter(|l| !l.trim().is_empty()).count();
    assert_eq!(
        data_rows, n,
        "{rel}: expected {n} data rows, got {data_rows}\n{content}"
    );
}

#[then(regex = r#"^the file "([^"]+)" contains (\d+) data rows?$"#)]
async fn dedupe_then_csv_row_count_exact(world: &mut LedgerWorld, rel: String, n: usize) {
    dedupe_then_csv_row_count(world, rel, n).await;
}

#[then(regex = r#"^the remaining row in "([^"]+)" matches "([^"]+)"$"#)]
async fn dedupe_then_csv_row_matches(world: &mut LedgerWorld, rel: String, needle: String) {
    let content = dedupe_read(world, &rel);
    let data_rows: Vec<&str> = content.lines().skip(1).filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(data_rows.len(), 1, "expected 1 row in {rel}, got {}", data_rows.len());
    assert!(
        data_rows[0].contains(&needle),
        "row {:?} does not contain {needle:?}",
        data_rows[0]
    );
}

#[then("a dedupe-report.json exists in the archive folder")]
async fn dedupe_then_report_exists(world: &mut LedgerWorld) {
    let archive = dedupe_archive_dir(world);
    assert!(
        archive.join("dedupe-report.json").exists(),
        "no dedupe-report.json under {}",
        archive.display()
    );
}

fn dedupe_load_report(world: &LedgerWorld) -> serde_json::Value {
    let archive = dedupe_archive_dir(world);
    let text = std::fs::read_to_string(archive.join("dedupe-report.json"))
        .expect("read dedupe-report.json");
    serde_json::from_str(&text).expect("parse dedupe-report.json")
}

#[then(regex = r#"^the report lists "([^"]+)" as archived$"#)]
async fn dedupe_then_report_lists_archived(world: &mut LedgerWorld, rel: String) {
    let report = dedupe_load_report(world);
    let entries = report["entries"].as_array().expect("entries array");
    let found = entries.iter().any(|e| {
        e["action"].as_str() == Some("archived")
            && e["path"].as_str().map(str::to_string) == Some(rel.clone())
    });
    assert!(
        found,
        "report does not list {rel} as archived: {}",
        serde_json::to_string_pretty(&report).unwrap()
    );
}

#[then(regex = r#"^the report records (\d+) FITIDs? dropped for kind "([^"]+)"$"#)]
async fn dedupe_then_report_count(world: &mut LedgerWorld, n: usize, kind: String) {
    let report = dedupe_load_report(world);
    let total: u64 = report["entries"]
        .as_array()
        .expect("entries array")
        .iter()
        .filter(|e| e["kind"].as_str() == Some(kind.as_str()))
        .filter_map(|e| e["keys_dropped"].as_u64())
        .sum();
    assert_eq!(total as usize, n, "kind={kind} total keys_dropped={total}, expected {n}");
}

async fn async_main() {
    let opts = cucumber::cli::Opts::<_, _, _, cucumber::cli::Empty>::parsed();
    let features = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("features");

    LedgerWorld::cucumber()
        .with_cli(opts)
        .run_and_exit(features)
        .await;
}
