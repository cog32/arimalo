#![deny(warnings)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use arimalo_covid::generated_store::load_active_ledger;
use arimalo_covid::ledger_parser::{CommodityAmount, PriceGraph};
use arimalo_covid::query::{self, MinValueFilter, QueryOptions, SortField, SortOrder};
use arimalo_covid::root_config;

fn platform_app_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/com.cog32.arimalocovid");
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(data) = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| format!("{h}/.local/share"))
        }) {
            return PathBuf::from(data).join("com.cog32.arimalocovid");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("com.cog32.arimalocovid");
        }
    }
    PathBuf::from(".")
}

fn resolve_generated_dir() -> PathBuf {
    let env_override = std::env::var("ARIMALO_GENERATED_DIR").ok();
    let app_dir = platform_app_data_dir();
    let config = root_config::load_root_config(&app_dir);
    root_config::resolve_generated(env_override.as_deref(), &config, &app_dir)
}

/// Resolve the directory containing `_prices/` for `--min-value-usd`.
/// Precedence: explicit flag > `ARIMALO_SOURCES_DIR` env > root_config.
fn resolve_sources_dir(prices_dir_flag: Option<&str>) -> PathBuf {
    if let Some(p) = prices_dir_flag {
        return PathBuf::from(p);
    }
    let env_override = std::env::var("ARIMALO_SOURCES_DIR").ok();
    let app_dir = platform_app_data_dir();
    let config = root_config::load_root_config(&app_dir);
    root_config::resolve_sources(env_override.as_deref(), &config, &app_dir)
}

fn build_min_value_filter(
    threshold: f64,
    prices_dir_flag: Option<&str>,
) -> Result<MinValueFilter, String> {
    let sources = resolve_sources_dir(prices_dir_flag);
    if !sources.join("_prices").is_dir() {
        return Err(format!(
            "--min-value-usd requires a `_prices/` directory; \
             checked {} — set ARIMALO_SOURCES_DIR or pass --prices-dir PATH",
            sources.display()
        ));
    }
    Ok(MinValueFilter {
        threshold,
        currency: "USD".into(),
        price_graph: Arc::new(PriceGraph::load(&sources)),
    })
}

/// A positional arg is treated as DIR when it names an existing directory on disk.
/// Search terms (`key:value`, `AND`, `OR`, free text) never collide with real paths.
fn looks_like_dir(arg: &str) -> bool {
    Path::new(arg).is_dir()
}

fn print_usage() {
    eprintln!("Usage: arimalo-query [DIR] [SEARCH...] [OPTIONS]");
    eprintln!();
    eprintln!("Query transactions under DIR (defaults to the vault's generated/ folder).");
    eprintln!("All `ledger.transactions` files beneath DIR are unioned — DIR may point at");
    eprintln!("the vault root, a user subtree, or a single account folder.");
    eprintln!();
    eprintln!("Search terms:");
    eprintln!("  account:PREFIX        Filter by account (regex match on postings)");
    eprintln!("  payee:PATTERN         Filter by payee (regex)");
    eprintln!("  narration:PATTERN     Filter by narration (regex)");
    eprintln!("  date:PATTERN          Filter by date (regex, e.g. 2025-01)");
    eprintln!("  amount:CONDITION      Filter by amount (e.g. >100, <50, >=0)");
    eprintln!("  commodity:PATTERN     Filter by commodity (exact, or *glob*)");
    eprintln!("  meta:PATTERN          Filter by meta field (regex)");
    eprintln!("  fee:CONDITION         Filter by fee amount");
    eprintln!("  FREE_TEXT             Search across all fields");
    eprintln!();
    eprintln!("  Combine with AND / OR. Negate with - prefix (e.g. -commodity:SPAM).");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --balances            Output per-commodity balances instead of transactions");
    eprintln!("  --sort FIELD ORDER    Sort by: date, amount/value, payee, account");
    eprintln!("                        Order: asc, desc (default: asc)");
    eprintln!("  --limit N             Limit number of results");
    eprintln!("  --format FORMAT       Output format: json, summary, register (default: register)");
    eprintln!("  --min-value-usd N     Drop balances worth less than N USD at latest price.");
    eprintln!("                        Unpriced commodities (typical of spam) are dropped.");
    eprintln!("  --prices-dir PATH     Override sources dir (parent of _prices/); usually");
    eprintln!("                        resolved via ARIMALO_SOURCES_DIR or the vault root.");
    eprintln!("  --help                Show this help");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  arimalo-query account:expenses:unknown --sort value desc");
    eprintln!("  arimalo-query generated/richard commodity:HNT --format json");
    eprintln!("  arimalo-query account:crypto AND amount:>1000 --sort date desc --limit 20");
    eprintln!("  arimalo-query -commodity:SPAM --sort amount desc");
    eprintln!("  arimalo-query --balances account:assets:crypto date:<=2026-06-30 --format json");
    eprintln!("  arimalo-query account:assets:crypto:wallet:ethereum --min-value-usd 1");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut sort_field: Option<SortField> = None;
    let mut sort_order: Option<SortOrder> = None;
    let mut limit: Option<usize> = None;
    let mut format = "register".to_string();
    let mut base_dir: Option<PathBuf> = None;
    let mut search_parts: Vec<String> = Vec::new();
    let mut balances_mode = false;
    let mut min_value_usd: Option<f64> = None;
    let mut prices_dir: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--balances" => {
                balances_mode = true;
            }
            "--sort" => {
                i += 1;
                let field_str = args.get(i).unwrap_or_else(|| {
                    eprintln!("--sort requires a field (date, amount, value, payee, account)");
                    std::process::exit(2);
                });
                sort_field = Some(SortField::parse_str(field_str).unwrap_or_else(|| {
                    eprintln!("Unknown sort field: {field_str}. Valid: date, amount, value, payee, account");
                    std::process::exit(2);
                }));
                // Optional order argument (peek next arg)
                if let Some(next) = args.get(i + 1) {
                    if let Some(order) = SortOrder::parse_str(next) {
                        sort_order = Some(order);
                        i += 1;
                    }
                }
            }
            "--limit" => {
                i += 1;
                let n_str = args.get(i).unwrap_or_else(|| {
                    eprintln!("--limit requires a number");
                    std::process::exit(2);
                });
                limit = Some(n_str.parse().unwrap_or_else(|_| {
                    eprintln!("--limit must be a positive integer, got: {n_str}");
                    std::process::exit(2);
                }));
            }
            "--format" => {
                i += 1;
                format = args
                    .get(i)
                    .unwrap_or_else(|| {
                        eprintln!("--format requires a value");
                        std::process::exit(2);
                    })
                    .clone();
            }
            "--min-value-usd" => {
                i += 1;
                let n_str = args.get(i).unwrap_or_else(|| {
                    eprintln!("--min-value-usd requires a number (USD threshold)");
                    std::process::exit(2);
                });
                min_value_usd = Some(n_str.parse().unwrap_or_else(|_| {
                    eprintln!("--min-value-usd must be a number, got: {n_str}");
                    std::process::exit(2);
                }));
            }
            "--prices-dir" => {
                i += 1;
                prices_dir = Some(
                    args.get(i)
                        .unwrap_or_else(|| {
                            eprintln!("--prices-dir requires a path (parent of _prices/)");
                            std::process::exit(2);
                        })
                        .clone(),
                );
            }
            // First positional arg that names an existing directory is DIR; everything
            // else is part of the search expression.
            arg if base_dir.is_none() && looks_like_dir(arg) => {
                base_dir = Some(PathBuf::from(arg));
            }
            _ => {
                search_parts.push(args[i].clone());
            }
        }
        i += 1;
    }

    let base_dir = base_dir.unwrap_or_else(resolve_generated_dir);
    if !base_dir.is_dir() {
        eprintln!("Directory not found: {}", base_dir.display());
        std::process::exit(1);
    }

    let parse = match load_active_ledger(&base_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error loading ledger: {e}");
            std::process::exit(1);
        }
    };

    // Parse search expression
    let search_str = search_parts.join(" ");
    let search = match query::parse_search(&search_str) {
        Ok(expr) => expr,
        Err(e) => {
            eprintln!("Search error: {e}");
            std::process::exit(2);
        }
    };

    // --min-value-usd is shared between QueryOptions (for default-mode aggregated_balance
    // / per-account balances) and aggregate_posting_balances (for --balances mode).
    // Build it once; clone the Arc for the second use.
    let min_value: Option<MinValueFilter> = match min_value_usd {
        Some(threshold) => match build_min_value_filter(threshold, prices_dir.as_deref()) {
            Ok(f) => Some(f),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(2);
            }
        },
        None => None,
    };
    let min_value_for_balances = min_value.as_ref().map(|f| MinValueFilter {
        threshold: f.threshold,
        currency: f.currency.clone(),
        price_graph: Arc::clone(&f.price_graph),
    });

    let opts = QueryOptions {
        search,
        sort_field,
        sort_order: sort_order.unwrap_or(SortOrder::Asc),
        offset: None,
        limit,
        input_order: None,
        min_value,
        hidden_prefixes: Vec::new(),
    };

    let result = query::query(&parse, &opts);

    if balances_mode {
        let balances = query::aggregate_posting_balances(
            &result,
            &opts.search,
            min_value_for_balances.as_ref(),
        );
        render_balances(&balances, result.transaction_count, &format, &search_str);
        return;
    }

    match format.as_str() {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&result).expect("JSON serialization")
            );
        }
        "summary" => {
            println!(
                "Search: {}",
                if search_str.is_empty() {
                    "(all)"
                } else {
                    &search_str
                }
            );
            println!(
                "Transactions: {} (showing {})",
                result.transaction_count,
                result.transactions.len()
            );
            println!("Accounts: {}", result.accounts.len());
            for acct in &result.accounts {
                println!("  {acct}");
            }
            if !result.aggregated_balance.is_empty() {
                println!("Balances:");
                for b in &result.aggregated_balance {
                    println!("  {:.4} {}", b.amount, b.commodity);
                }
            }
        }
        "register" => {
            // Human-readable tabular output
            if result.transactions.is_empty() {
                println!("No transactions found.");
                if !search_str.is_empty() {
                    println!("Search: {search_str}");
                }
                std::process::exit(0);
            }

            // Header
            println!(
                "{:<12} {:>14} {:<8} DESCRIPTION",
                "DATE", "AMOUNT", "CURR"
            );
            println!("{}", "-".repeat(70));

            for txn in &result.transactions {
                let desc = txn
                    .display_payee
                    .as_deref()
                    .or(txn.payee.as_deref())
                    .or(txn.narration.as_deref())
                    .unwrap_or("");
                let commodity = txn
                    .display_amount_commodity
                    .as_deref()
                    .unwrap_or(&txn.amount_commodity);
                println!(
                    "{:<12} {:>14.4} {:<8} {}",
                    txn.date, txn.amount, commodity, desc,
                );
            }

            println!("{}", "-".repeat(70));
            println!(
                "{} transactions (of {})",
                result.transactions.len(),
                result.transaction_count
            );
        }
        other => {
            eprintln!("Unknown format: {other}. Use 'json', 'summary', or 'register'.");
            std::process::exit(2);
        }
    }
}

fn render_balances(
    balances: &[CommodityAmount],
    transaction_count: usize,
    format: &str,
    search_str: &str,
) {
    match format {
        "json" => {
            let entries: Vec<serde_json::Value> = balances
                .iter()
                .map(|b| serde_json::json!({ "commodity": b.commodity, "quantity": b.amount }))
                .collect();
            let payload = serde_json::json!({
                "transaction_count": transaction_count,
                "balances": entries,
            });
            println!(
                "{}",
                serde_json::to_string_pretty(&payload).expect("JSON serialization")
            );
        }
        "summary" => {
            println!("Balances ({transaction_count} transactions):");
            for b in balances {
                println!("  {:.4} {}", b.amount, b.commodity);
            }
        }
        "register" => {
            if balances.is_empty() {
                println!("No balances.");
                if !search_str.is_empty() {
                    println!("Search: {search_str}");
                }
                return;
            }
            println!("{:<12} {:>14}", "COMMODITY", "QUANTITY");
            println!("{}", "-".repeat(28));
            for b in balances {
                println!("{:<12} {:>14.4}", b.commodity, b.amount);
            }
            println!("{}", "-".repeat(28));
            let noun = if balances.len() == 1 {
                "commodity"
            } else {
                "commodities"
            };
            println!(
                "{} {} · {transaction_count} transactions",
                balances.len(),
                noun,
            );
        }
        other => {
            eprintln!("Unknown format: {other}. Use 'json', 'summary', or 'register'.");
            std::process::exit(2);
        }
    }
}
