#![deny(warnings)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

use arimalo_covid::generated_store::load_active_ledger;
use arimalo_covid::ledger_parser::Transaction;
use arimalo_covid::processing_pipeline::PipelineMetadata;
use arimalo_covid::root_config;
use arimalo_covid::rules::{ai_rule_id, RulesFile};

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

fn resolve_sources_dir() -> PathBuf {
    let env_override = std::env::var("ARIMALO_SOURCES_DIR").ok();
    let app_dir = platform_app_data_dir();
    let config = root_config::load_root_config(&app_dir);
    root_config::resolve_sources(env_override.as_deref(), &config, &app_dir)
}

#[derive(Debug, serde::Deserialize)]
struct AiRuleSuggestion {
    pattern: String,
    amount_account: String,
    #[serde(default)]
    payee: Option<String>,
    #[serde(default)]
    match_field: Option<String>,
    #[serde(default)]
    explanation: String,
}

const DEFAULT_CLASSIFY_PROMPT: &str = include_str!("../../prompts/classify.md");

/// Resolve the classify prompt template.
/// Order: ARIMALO_CLASSIFY_PROMPT env var > config.classify_prompt_path > embedded default.
fn load_classify_prompt(config_path: Option<&str>) -> String {
    let from_env = std::env::var("ARIMALO_CLASSIFY_PROMPT").ok();
    let path = from_env.as_deref().or(config_path);
    if let Some(p) = path {
        match std::fs::read_to_string(p) {
            Ok(s) => {
                eprintln!("[classify] Using prompt from {p}");
                return s;
            }
            Err(e) => {
                eprintln!("[classify] WARN: failed to read prompt {p}: {e} — using embedded default");
            }
        }
    }
    DEFAULT_CLASSIFY_PROMPT.to_string()
}

fn render_prompt(
    template: &str,
    acct: &str,
    txn_lines: &str,
    rules_json: &str,
    categorised_section: &str,
) -> String {
    template
        .replace("{acct}", acct)
        .replace("{txn_lines}", txn_lines)
        .replace("{rules_json}", rules_json)
        .replace("{categorised_section}", categorised_section)
}

fn find_claude_bin() -> String {
    ["/opt/homebrew/bin/claude", "/usr/local/bin/claude"]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "claude".to_string())
}

/// Extract a JSON array from text that may contain markdown code fences.
fn extract_json_array(text: &str) -> Option<&str> {
    if let Some(start) = text.find("```json") {
        let after = &text[start + 7..];
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim());
        }
    }
    if let Some(start) = text.find("```") {
        let after = &text[start + 3..];
        let after = if let Some(nl) = after.find('\n') {
            &after[nl + 1..]
        } else {
            after
        };
        if let Some(end) = after.find("```") {
            return Some(after[..end].trim());
        }
    }
    if let Some(start) = text.find('[') {
        if let Some(end) = text.rfind(']') {
            if end > start {
                return Some(text[start..=end].trim());
            }
        }
    }
    None
}

/// Format a transaction for the AI prompt.
fn fmt_txn(t: &Transaction) -> String {
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
}

fn print_usage() {
    eprintln!("Usage: arimalo-classify [OPTIONS]");
    eprintln!();
    eprintln!("Batch-classify expenses:unknown transactions using Claude AI.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --account-set NAME    Account set to classify (default: first available)");
    eprintln!("  --dry-run             Show what would be classified without calling Claude");
    eprintln!("  --batch-size N        Transactions per Claude call (default: 30)");
    eprintln!("  --account FILTER      Only classify for this asset account (prefix match)");
    eprintln!("  --txid ID             Classify a single transaction by meta substring (prints, does not save)");
    eprintln!("  --print-prompt        Render and print the prompt(s) sent to Claude; do not call Claude");
    eprintln!("  --help                Show this help");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        return;
    }

    let mut account_set = String::new();
    let mut dry_run = false;
    let mut batch_size: usize = 30;
    let mut account_filter: Option<String> = None;
    let mut txid_filter: Option<String> = None;
    let mut print_prompt = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--account-set" => {
                i += 1;
                account_set = args.get(i).cloned().unwrap_or_default();
            }
            "--dry-run" => {
                dry_run = true;
            }
            "--batch-size" => {
                i += 1;
                batch_size = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(30);
            }
            "--account" => {
                i += 1;
                account_filter = args.get(i).cloned();
            }
            "--txid" => {
                i += 1;
                txid_filter = args.get(i).cloned();
            }
            "--print-prompt" => {
                print_prompt = true;
            }
            other => {
                eprintln!("Unknown option: {other}");
                print_usage();
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let generated_dir = resolve_generated_dir();
    let sources_dir = resolve_sources_dir();
    let app_dir = platform_app_data_dir();
    let root_cfg = root_config::load_root_config(&app_dir);
    let prompt_template = load_classify_prompt(root_cfg.classify_prompt_path.as_deref());

    // Resolve account set
    let set_dir = if account_set.is_empty() {
        // Find first available account set
        if let Ok(entries) = std::fs::read_dir(&generated_dir) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if name != "prices" && !name.starts_with('.') {
                        account_set = name;
                        break;
                    }
                }
            }
        }
        generated_dir.join(&account_set)
    } else {
        generated_dir.join(&account_set)
    };

    eprintln!("[classify] Account set: {account_set}");
    eprintln!("[classify] Sources: {}", sources_dir.display());
    eprintln!("[classify] Generated: {}", set_dir.display());

    // Load ledger
    let parse = match load_active_ledger(&set_dir) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to load ledger: {e}");
            std::process::exit(1);
        }
    };

    // Load pipeline metadata for account→folder mapping
    let metadata = PipelineMetadata::load(&generated_dir);
    let account_folders: HashMap<String, String> =
        metadata.map(|m| m.account_folders).unwrap_or_default();

    // Find all expenses:unknown transactions, grouped by source asset account
    let mut by_source: HashMap<String, Vec<&Transaction>> = HashMap::new();
    for txn in &parse.transactions {
        if let Some(ref id) = txid_filter {
            if !txn.meta.as_deref().is_some_and(|m| m.contains(id.as_str())) {
                continue;
            }
        } else {
            let has_unknown = txn.postings.iter().any(|p| p.account == "expenses:unknown");
            if !has_unknown {
                continue;
            }
        }

        // Find the asset-side account
        let asset_acct = txn
            .postings
            .iter()
            .find(|p| {
                p.account.starts_with("assets:") || p.account.starts_with("liabilities:")
            })
            .or_else(|| {
                txn.postings
                    .iter()
                    .find(|p| p.account != "expenses:unknown")
            })
            .map(|p| p.account.clone())
            .unwrap_or_default();

        // Apply account filter
        if let Some(ref filter) = account_filter {
            if !asset_acct.starts_with(filter.as_str()) {
                continue;
            }
        }

        by_source.entry(asset_acct).or_default().push(txn);
    }

    if by_source.is_empty() {
        eprintln!("[classify] No expenses:unknown transactions found.");
        return;
    }

    // Summary
    let total: usize = by_source.values().map(|v| v.len()).sum();
    eprintln!(
        "\n[classify] Found {total} uncategorised transactions across {} accounts:\n",
        by_source.len()
    );
    for (acct, txns) in by_source.iter() {
        let folder = account_folders.get(acct).map(|s| s.as_str()).unwrap_or("?");
        eprintln!("  {:>5}  {acct:<55} folder: {folder}", txns.len());
    }

    if dry_run {
        eprintln!(
            "\n[dry-run] Would classify these transactions. Run without --dry-run to proceed."
        );
        // In dry-run mode, show sample transactions per account
        for (acct, txns) in by_source.iter() {
            eprintln!("\n--- {acct} ({} txns) ---", txns.len());
            // Deduplicate by narration to show unique patterns
            let mut seen = std::collections::HashSet::new();
            let mut unique = 0;
            for t in txns.iter() {
                let key = t.narration.as_deref().unwrap_or("").to_string();
                if seen.insert(key) {
                    unique += 1;
                    if unique <= 10 {
                        eprintln!("{}", fmt_txn(t));
                    }
                }
            }
            if unique > 10 {
                eprintln!("  ... and {} more unique patterns", unique - 10);
            }
        }
        return;
    }

    let claude_bin = find_claude_bin();
    let root_dir = sources_dir.parent().unwrap_or(&sources_dir);
    let mut total_rules_added = 0;

    // Process each source account
    for (acct, txns) in &by_source {
        let folder_rel = match account_folders.get(acct) {
            Some(f) => f,
            None => {
                eprintln!("[classify] No folder mapping for {acct}, skipping");
                continue;
            }
        };
        let folder_path = sources_dir.join(folder_rel);

        eprintln!("\n{}", "=".repeat(60));
        eprintln!("[classify] Processing {acct} ({} txns)", txns.len());
        eprintln!("[classify] Folder: {folder_rel}");

        // Load existing rules for context
        let existing_rules = RulesFile::load(&folder_path);
        let rules_json = serde_json::to_string_pretty(&existing_rules.rules)
            .unwrap_or_else(|_| "[]".to_string());

        // Deduplicate transactions by narration pattern to avoid redundancy
        let mut seen_narrations = std::collections::HashSet::new();
        let mut unique_txns: Vec<&Transaction> = Vec::new();
        for t in txns {
            let key = format!(
                "{}|{}",
                t.narration.as_deref().unwrap_or(""),
                t.amount_commodity
            );
            if seen_narrations.insert(key) {
                unique_txns.push(t);
            }
        }
        eprintln!(
            "[classify] {} unique narration patterns (from {} total)",
            unique_txns.len(),
            txns.len()
        );

        // Build context: categorised transactions from this account (for examples)
        let categorised: Vec<String> = parse
            .transactions
            .iter()
            .filter(|t| {
                t.postings.iter().any(|p| p.account == *acct)
                    && !t.postings.iter().any(|p| p.account == "expenses:unknown")
            })
            .take(30)
            .map(fmt_txn)
            .collect();

        let categorised_section = if categorised.is_empty() {
            String::new()
        } else {
            format!(
                "\n\nExamples of already-categorised transactions on this account:\n{}",
                categorised.join("\n")
            )
        };

        // Process in batches
        for (batch_idx, chunk) in unique_txns.chunks(batch_size).enumerate() {
            eprintln!(
                "[classify] Batch {}/{} ({} transactions)",
                batch_idx + 1,
                unique_txns.len().div_ceil(batch_size),
                chunk.len()
            );

            let txn_lines: Vec<String> = chunk
                .iter()
                .map(|t| {
                    let payee = t
                        .display_payee
                        .as_deref()
                        .or(t.payee.as_deref())
                        .unwrap_or("—");
                    let narration = t.narration.as_deref().unwrap_or("—");
                    format!(
                        "  {} | {} | {} | {} {}",
                        t.date, payee, narration, t.amount, t.amount_commodity
                    )
                })
                .collect();

            let prompt = render_prompt(
                &prompt_template,
                acct,
                &txn_lines.join("\n"),
                &rules_json,
                &categorised_section,
            );

            if print_prompt {
                println!(
                    "===== prompt: {acct} batch {}/{} =====",
                    batch_idx + 1,
                    unique_txns.len().div_ceil(batch_size)
                );
                println!("{prompt}");
                continue;
            }

            let output = match Command::new(&claude_bin)
                .arg("-p")
                .arg("--dangerously-skip-permissions")
                .arg(&prompt)
                .current_dir(root_dir)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .output()
            {
                Err(e) => {
                    eprintln!("[classify] ERROR: Failed to run claude: {e}");
                    continue;
                }
                Ok(o) => o,
            };

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                eprintln!("[classify] ERROR: Claude returned error: {stderr}");
                continue;
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let json_str = extract_json_array(&stdout).unwrap_or(&stdout);

            let suggestions: Vec<AiRuleSuggestion> = match serde_json::from_str(json_str) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[classify] ERROR: Could not parse suggestions: {e}");
                    eprintln!(
                        "[classify] Raw output: {}",
                        &stdout[..stdout.len().min(500)]
                    );
                    continue;
                }
            };

            eprintln!("[classify] Got {} suggestions", suggestions.len());
            for s in &suggestions {
                eprintln!("  {} → {} ({})", s.pattern, s.amount_account, s.explanation);
            }

            if txid_filter.is_some() {
                // Print-only mode: emit suggestions as JSON and skip rules save.
                match serde_json::to_string_pretty(&suggestions.iter().map(|s| {
                    serde_json::json!({
                        "pattern": s.pattern,
                        "amount_account": s.amount_account,
                        "payee": s.payee,
                        "match_field": s.match_field,
                        "explanation": s.explanation,
                    })
                }).collect::<Vec<_>>()) {
                    Ok(j) => println!("{j}"),
                    Err(e) => eprintln!("[classify] ERROR serialising: {e}"),
                }
                continue;
            }

            // Append to rules file
            let mut rules = RulesFile::load(&folder_path);
            let mut added = 0;
            for s in &suggestions {
                // Skip if a rule with the same pattern already exists (covers
                // the manual-rule-shadowing case; an identical AI rule is
                // caught downstream by the content-derived id check).
                if rules.rules.iter().any(|r| r.pattern == s.pattern) {
                    eprintln!("  [skip] pattern already exists: {}", s.pattern);
                    continue;
                }
                let mut rule = arimalo_covid::rules::Rule {
                    id: String::new(),
                    pattern: s.pattern.clone(),
                    match_field: s.match_field.clone(),
                    payee: s.payee.clone(),
                    commodity: None,
                    comment: Some(format!("ai: {}", s.explanation)),
                    amount_condition: None,
                    fee_condition: None,
                    amount_account: Some(s.amount_account.clone()),
                    fee_account: None,
                    postings: vec![],
                    payee_condition: None,
                    narration_condition: None,
                    commodity_condition: None,
                    meta_condition: None,
                };
                rule.id = ai_rule_id(&rule);
                // Skip if an identical rule (same content-derived id) already exists.
                if rules.rules.iter().any(|r| r.id == rule.id) {
                    eprintln!("  [skip] identical rule already exists: {}", rule.pattern);
                    continue;
                }
                rules.insert_rule(rule);
                added += 1;
            }

            if added > 0 {
                if let Err(e) = rules.save(&folder_path) {
                    eprintln!("[classify] ERROR saving rules: {e}");
                } else {
                    eprintln!("[classify] Saved {added} new rules to {folder_rel}/_rules.json");
                    total_rules_added += added;
                }
            }
        }
    }

    eprintln!("\n[classify] Done. Added {total_rules_added} rules total.");
    if total_rules_added > 0 {
        eprintln!("[classify] Run arimalo-regenerate to apply the new rules.");
    }
}
