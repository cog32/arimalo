use crate::ledger_parser::{
    parse_transactions, AccountBalance, CommodityAmount, ParseResult, Posting,
};
use crate::processing_pipeline::{folder_to_account_name, txn_sort_key};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use walkdir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualPostingInput {
    pub account: String,
    pub amount: String,
    pub commodity: String,
    pub remainder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManualTransactionInput {
    /// Either `YYYY-MM-DD` or a full datetime supported by the ledger grammar.
    pub datetime: String,
    pub status: Option<char>,
    pub payee: String,
    pub narration: String,
    pub postings: Vec<ManualPostingInput>,
}

pub fn quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

pub fn posting_to_text(posting: &Posting) -> String {
    let mut out = format!(
        "    {} {} {}",
        posting.account, posting.amount_text, posting.commodity
    );
    // Price annotation (e.g. "@ 0.042 AUD")
    if let Some(ref price) = posting.price {
        if posting
            .remainder
            .as_ref()
            .is_none_or(|r| !r.contains('@'))
        {
            if price.is_total {
                out.push_str(&format!(" @@ {} {}", price.amount_text, price.commodity));
            } else {
                out.push_str(&format!(" @ {} {}", price.amount_text, price.commodity));
            }
        }
    }
    if let Some(rem) = posting.remainder.as_ref().and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }) {
        out.push(' ');
        out.push_str(rem);
    }
    out
}

fn looks_like_header(line: &str) -> bool {
    let b = line.as_bytes();
    if b.len() < 10 {
        return false;
    }
    b[0].is_ascii_digit()
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit()
        && b[4] == b'-'
        && b[5].is_ascii_digit()
        && b[6].is_ascii_digit()
        && b[7] == b'-'
        && b[8].is_ascii_digit()
        && b[9].is_ascii_digit()
}

pub fn normalize_blank_lines(contents: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    let mut prev_nonblank_was_posting = false;

    for raw_line in contents.split('\n') {
        let is_blank = raw_line.trim().is_empty();
        let is_posting = raw_line.starts_with("    ");
        let is_header = looks_like_header(raw_line);

        if is_header && prev_nonblank_was_posting
            && out.last().is_some_and(|l| !l.trim().is_empty()) {
                out.push(String::new());
            }

        out.push(raw_line.to_string());

        if is_blank {
            prev_nonblank_was_posting = false;
        } else {
            prev_nonblank_was_posting = is_posting;
        }
    }

    while out.last().is_some_and(|l| l.is_empty()) && out.len() > 1 {
        out.pop();
    }

    out.join("\n")
}

pub fn append_text(path: &Path, text: &str) -> io::Result<()> {
    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, text)?;
        return Ok(());
    }

    let mut existing = fs::read_to_string(path)?;
    existing = normalize_blank_lines(&existing);
    if !existing.is_empty() {
        if existing.ends_with("\n\n") {
            // ok
        } else if existing.ends_with('\n') {
            existing.push('\n');
        } else {
            existing.push_str("\n\n");
        }
    }
    existing.push_str(text);
    fs::write(path, existing)
}

/// Remove the transaction block(s) containing `txn_id` from ledger text.
/// Blocks are separated by blank lines; any block whose header contains `txn_id` is dropped.
fn remove_txn_block_from_text(text: &str, txn_id: &str) -> String {
    let mut result = String::new();
    let mut current_block = String::new();
    let mut block_has_txn = false;

    for line in text.lines() {
        if line.trim().is_empty() {
            if !block_has_txn && !current_block.trim().is_empty() {
                result.push_str(&current_block);
                result.push('\n');
            }
            current_block.clear();
            block_has_txn = false;
        } else {
            if line.contains(txn_id) {
                block_has_txn = true;
            }
            current_block.push_str(line);
            current_block.push('\n');
        }
    }
    // Flush last block
    if !block_has_txn && !current_block.trim().is_empty() {
        result.push_str(&current_block);
    }
    result
}

/// Like `load_active_ledger` but removes the transaction identified by `txn_id` from
/// the in-memory text before parsing. Does not touch any files on disk.
pub fn load_active_ledger_excluding(base_dir: &Path, txn_id: &str) -> Result<ParseResult, String> {
    let ledger = load_ledger_text(base_dir)?;
    let filtered = remove_txn_block_from_text(&ledger.text, txn_id);
    if filtered.trim().is_empty() {
        return Ok(ParseResult {
            ok: true,
            diagnostics: Vec::new(),
            transactions: Vec::new(),
            balances: Vec::new(),
            accounts_with_opening: Vec::new(),
            account_properties: std::collections::HashMap::new(),
        });
    }
    let mut result = parse_transactions(&filtered);
    for diag in &mut result.diagnostics {
        if diag.file.is_none() {
            diag.file =
                resolve_file_for_line(&ledger.file_ranges, diag.line).map(|s| s.to_string());
        }
    }
    Ok(result)
}

/// A range of lines in the merged text that came from a specific file.
struct FileLineRange {
    file_path: String,
    start_line: usize, // 1-based, inclusive
    end_line: usize,   // 1-based, inclusive
}

struct LedgerText {
    text: String,
    file_ranges: Vec<FileLineRange>,
}

fn load_ledger_text(base_dir: &Path) -> Result<LedgerText, String> {
    let mut all_text = String::new();
    let mut file_ranges: Vec<FileLineRange> = Vec::new();
    let mut current_line: usize = 1;

    // Load account declarations (at set root)
    let accounts_path = base_dir.join("accounts.transactions");
    if accounts_path.exists() {
        append_file_to_text(
            &accounts_path,
            &mut all_text,
            &mut file_ranges,
            &mut current_line,
        )?;
    }

    // Walk the folder tree for all per-folder ledger.transactions files.
    // Also check legacy layout (archive/ + ledger.transactions at root) for backward compat.
    let mut ledger_files: Vec<PathBuf> = Vec::new();

    // New layout: per-folder ledger.transactions files throughout the tree
    for entry in walkdir::WalkDir::new(base_dir)
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
        // Skip the old root-level ledger.transactions (handled below for migration)
        if path.parent() == Some(base_dir) {
            continue;
        }
        ledger_files.push(path.to_path_buf());
    }
    ledger_files.sort();

    if !ledger_files.is_empty() {
        // New per-folder layout
        for path in &ledger_files {
            append_file_to_text(path, &mut all_text, &mut file_ranges, &mut current_line)?;
        }
    } else {
        // Legacy layout: archive/*.transactions + root ledger.transactions
        let archive_dir = base_dir.join("archive");
        if archive_dir.exists() {
            let mut archive_files: Vec<PathBuf> = fs::read_dir(&archive_dir)
                .map_err(|e| format!("failed to read archive dir: {e}"))?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("transactions"))
                .collect();
            archive_files.sort();
            for path in &archive_files {
                append_file_to_text(path, &mut all_text, &mut file_ranges, &mut current_line)?;
            }
        }
        let ledger = base_dir.join("ledger.transactions");
        if ledger.exists() {
            append_file_to_text(&ledger, &mut all_text, &mut file_ranges, &mut current_line)?;
        }
    }

    Ok(LedgerText {
        text: all_text,
        file_ranges,
    })
}

fn append_file_to_text(
    path: &Path,
    all_text: &mut String,
    file_ranges: &mut Vec<FileLineRange>,
    current_line: &mut usize,
) -> Result<(), String> {
    let contents =
        fs::read_to_string(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    if contents.trim().is_empty() {
        return Ok(());
    }
    let start_line = *current_line;
    let line_count = contents.chars().filter(|&c| c == '\n').count()
        + if contents.ends_with('\n') { 0 } else { 1 };
    all_text.push_str(&contents);
    if !all_text.ends_with('\n') {
        all_text.push('\n');
    }
    *current_line += line_count;
    all_text.push('\n');
    file_ranges.push(FileLineRange {
        file_path: path.to_string_lossy().to_string(),
        start_line,
        end_line: *current_line - 1,
    });
    *current_line += 1;
    Ok(())
}

/// Given a global line number in the merged text, find which file it came from.
fn resolve_file_for_line(file_ranges: &[FileLineRange], line: usize) -> Option<&str> {
    for range in file_ranges {
        if line >= range.start_line && line <= range.end_line {
            return Some(&range.file_path);
        }
    }
    None
}

pub fn load_active_ledger(base_dir: &Path) -> Result<ParseResult, String> {
    let ledger = load_ledger_text(base_dir)?;
    if ledger.text.trim().is_empty() {
        return Ok(ParseResult {
            ok: true,
            diagnostics: Vec::new(),
            transactions: Vec::new(),
            balances: Vec::new(),
            accounts_with_opening: Vec::new(),
            account_properties: std::collections::HashMap::new(),
        });
    }
    let mut result = parse_transactions(&ledger.text);
    // Sort transactions by datetime (per-folder files are individually sorted but need global sort).
    // The 3-key sort `(datetime, meta, txn_sort_key)` matches `sort_tagged_txns` in the
    // pipeline — without the third key, two transactions sharing (datetime, meta) would
    // retain file-input order, which is a different answer to what the pipeline wrote,
    // making downstream report output non-reproducible across (pipeline write → loader read).
    result.transactions.sort_by(|a, b| {
        a.datetime
            .cmp(&b.datetime)
            .then_with(|| {
                a.meta
                    .as_deref()
                    .unwrap_or("")
                    .cmp(b.meta.as_deref().unwrap_or(""))
            })
            .then_with(|| txn_sort_key(a).cmp(&txn_sort_key(b)))
    });
    // Annotate diagnostics with the source file path
    for diag in &mut result.diagnostics {
        if diag.file.is_none() {
            diag.file =
                resolve_file_for_line(&ledger.file_ranges, diag.line).map(|s| s.to_string());
        }
    }
    Ok(result)
}

/// Load account tree from per-folder summary.json files.
/// Returns aggregated balances without parsing any transactions.
pub fn load_account_tree(set_dir: &Path) -> Result<Vec<AccountBalance>, String> {
    let mut all_balances: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, f64>,
    > = std::collections::BTreeMap::new();

    // Also load account declarations for accounts_with_opening info
    let accounts_path = set_dir.join("accounts.transactions");
    if accounts_path.exists() {
        if let Ok(text) = fs::read_to_string(&accounts_path) {
            let parsed = parse_transactions(&text);
            for b in &parsed.balances {
                for t in &b.totals {
                    *all_balances
                        .entry(b.account.clone())
                        .or_default()
                        .entry(t.commodity.clone())
                        .or_insert(0.0) += t.amount;
                }
            }
        }
    }

    // Walk for summary.json files
    for entry in walkdir::WalkDir::new(set_dir)
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
        if let Ok(text) = fs::read_to_string(path) {
            if let Ok(summary) =
                serde_json::from_str::<crate::processing_pipeline::FolderSummary>(&text)
            {
                for b in &summary.balances {
                    for t in &b.totals {
                        *all_balances
                            .entry(b.account.clone())
                            .or_default()
                            .entry(t.commodity.clone())
                            .or_insert(0.0) += t.amount;
                    }
                }
            }
        }
    }

    Ok(all_balances
        .into_iter()
        .map(|(account, by_commodity)| AccountBalance {
            account,
            totals: by_commodity
                .into_iter()
                .map(|(commodity, amount)| CommodityAmount { commodity, amount })
                .collect(),
        })
        .collect())
}

/// Load transactions from per-folder files scoped to an account prefix.
/// Only reads folders whose derived account name matches the prefix.
/// Returns parsed, sorted transactions.
pub fn load_scoped_transactions(
    set_dir: &Path,
    account_prefix: &str,
) -> Result<Vec<crate::ledger_parser::Transaction>, String> {
    let mut all_txns = Vec::new();

    // Walk for ledger.transactions files
    for entry in walkdir::WalkDir::new(set_dir)
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
        if path.parent() == Some(set_dir) {
            continue;
        } // skip root-level legacy
        if !folder_matches_account_prefix(set_dir, path, account_prefix) {
            continue;
        }

        let text = fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        let result = parse_transactions(&text);
        all_txns.extend(result.transactions);
    }

    // Sort by datetime desc (most recent first)
    all_txns.sort_by(|a, b| {
        b.datetime.cmp(&a.datetime).then_with(|| {
            b.meta
                .as_deref()
                .unwrap_or("")
                .cmp(a.meta.as_deref().unwrap_or(""))
        })
    });

    Ok(all_txns)
}

fn folder_matches_account_prefix(set_dir: &Path, ledger_path: &Path, account_prefix: &str) -> bool {
    if account_prefix.is_empty() {
        return true;
    }
    let Some(folder_path) = ledger_path.parent() else {
        return false;
    };
    let Ok(folder_relative) = folder_path.strip_prefix(set_dir) else {
        return false;
    };
    let folder_relative = folder_relative.to_string_lossy();
    if folder_relative.is_empty() {
        return false;
    }
    let derived = folder_to_account_name(&format!("__set__/{folder_relative}"));
    derived == account_prefix || derived.starts_with(&format!("{account_prefix}:"))
}

/// Read the `hidden_accounts` list from `config.json` in a generated set
/// directory. Bare names (no `:`) get a `:` appended so the returned strings
/// are always usable as `starts_with` prefixes — matching `ignore` against
/// `ignore:hidden` etc. Returns an empty vec when the file or key is absent.
pub fn load_hidden_accounts(set_dir: &Path) -> Vec<String> {
    let config_path = set_dir.join("config.json");
    let Ok(contents) = std::fs::read_to_string(&config_path) else {
        return Vec::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return Vec::new();
    };
    let Some(arr) = json.get("hidden_accounts").and_then(|v| v.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|v| {
            v.as_str().map(|s| {
                let prefix = s.to_string();
                if prefix.contains(':') {
                    prefix
                } else {
                    format!("{prefix}:")
                }
            })
        })
        .collect()
}

/// Remove transactions that post to any hidden account prefix and recompute balances.
pub fn filter_hidden_accounts(parse: &mut ParseResult, prefixes: &[String]) {
    parse.transactions.retain(|txn| {
        !txn.postings.iter().any(|p| {
            prefixes
                .iter()
                .any(|pfx| p.account.starts_with(pfx.as_str()))
        })
    });
    // Recompute balances from remaining transactions
    let mut balances_map: std::collections::BTreeMap<
        String,
        std::collections::BTreeMap<String, f64>,
    > = std::collections::BTreeMap::new();
    for txn in &parse.transactions {
        for posting in &txn.postings {
            *balances_map
                .entry(posting.account.clone())
                .or_default()
                .entry(posting.commodity.clone())
                .or_insert(0.0) += posting.amount;
        }
    }
    parse.balances = balances_map
        .into_iter()
        .map(|(account, totals)| AccountBalance {
            account,
            totals: totals
                .into_iter()
                .map(|(commodity, amount)| CommodityAmount { commodity, amount })
                .collect(),
        })
        .collect();
}

/// Extract the first `account:` prefix from a search string. Used to scope
/// which per-folder ledgers a query reads; empty means a whole-set scan.
pub fn account_prefix_from_search(search: &str) -> String {
    for part in search.split(" AND ") {
        if let Some(rest) = part.trim().strip_prefix("account:") {
            return rest.to_string();
        }
    }
    String::new()
}

/// Aggregate per-account, per-commodity balances from a transaction list.
fn balances_from_transactions(
    transactions: &[crate::ledger_parser::Transaction],
) -> Vec<AccountBalance> {
    let mut bmap: std::collections::BTreeMap<String, std::collections::BTreeMap<String, f64>> =
        std::collections::BTreeMap::new();
    for txn in transactions {
        for p in &txn.postings {
            *bmap
                .entry(p.account.clone())
                .or_default()
                .entry(p.commodity.clone())
                .or_insert(0.0) += p.amount;
        }
    }
    bmap.into_iter()
        .map(|(account, by_commodity)| AccountBalance {
            account,
            totals: by_commodity
                .into_iter()
                .map(|(commodity, amount)| CommodityAmount { commodity, amount })
                .collect(),
        })
        .collect()
}

/// Run a scoped query against a generated set: read only the per-folder
/// ledgers the `account:` prefix touches, drop hidden-account transactions
/// when `show_hidden` is off, then filter/sort/paginate via the query engine.
///
/// This is the single code path behind the desktop `query_search` command, the
/// web dispatch, and the BDD harness — they call it rather than re-deriving the
/// scope/filter/balance logic, so they can never silently diverge.
///
/// When `show_hidden` is on, the configured hidden prefixes are handed to the
/// query engine so a fully-hidden transaction still in the scoped folder
/// reappears in the row list. Balance aggregation always uses the original
/// search terms, so totals are unaffected by the toggle.
pub fn scoped_query(
    set_dir: &Path,
    search: &str,
    show_hidden: bool,
    sort_field: Option<&str>,
    sort_order: Option<&str>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<crate::query::QueryResult, String> {
    use crate::query;

    let account_prefix = account_prefix_from_search(search);
    let hidden_prefixes = load_hidden_accounts(set_dir);
    let mut transactions = load_scoped_transactions(set_dir, &account_prefix)?;
    if !show_hidden && !hidden_prefixes.is_empty() {
        transactions.retain(|txn| {
            !txn.postings.iter().any(|p| {
                hidden_prefixes
                    .iter()
                    .any(|pfx| p.account.starts_with(pfx.as_str()))
            })
        });
    }

    let balances = balances_from_transactions(&transactions);
    let parse = ParseResult {
        ok: true,
        diagnostics: Vec::new(),
        transactions,
        balances,
        accounts_with_opening: Vec::new(),
        account_properties: std::collections::HashMap::new(),
    };

    let expr = query::parse_search(search).map_err(|e| e.to_string())?;
    let sf = sort_field
        .map(|s| query::SortField::parse_str(s).ok_or_else(|| format!("Unknown sort field: {s}")))
        .transpose()?;
    let so = sort_order
        .map(|s| query::SortOrder::parse_str(s).ok_or_else(|| format!("Unknown sort order: {s}")))
        .transpose()?
        .unwrap_or(query::SortOrder::Asc);

    Ok(query::query(
        &parse,
        &query::QueryOptions {
            search: expr,
            sort_field: sf,
            sort_order: so,
            offset,
            limit,
            input_order: Some(query::InputOrder::DateDesc),
            min_value: None,
            hidden_prefixes: if show_hidden {
                hidden_prefixes
            } else {
                Vec::new()
            },
        },
    ))
}

/// Run a query against the UNION of every per-folder ledger in a set (no folder
/// pruning), so non-folder-backed accounts — `income:*`, `expenses:*`,
/// `equity:*`, `liabilities:*`, and the `assets` contras (staking, lending,
/// transfer, …) — are queryable. The folder-pruned [`scoped_query`] returns zero
/// rows for these because no source folder derives to their account name. Mirrors
/// the `arimalo-query` CLI path: [`load_active_ledger`] (union) + `query::query`.
///
/// NOTE: `load_active_ledger` sorts ascending, unlike `load_scoped_transactions`
/// (descending), so this passes `input_order: None` to force a real sort in
/// `query::query`. Passing `Some(InputOrder::DateDesc)` here would silently
/// reverse the default newest-first ordering.
pub fn global_query(
    set_dir: &Path,
    search: &str,
    show_hidden: bool,
    sort_field: Option<&str>,
    sort_order: Option<&str>,
    offset: Option<usize>,
    limit: Option<usize>,
) -> Result<crate::query::QueryResult, String> {
    use crate::query;

    let hidden_prefixes = load_hidden_accounts(set_dir);
    let mut parse = load_active_ledger(set_dir)?;
    if !show_hidden && !hidden_prefixes.is_empty() {
        filter_hidden_accounts(&mut parse, &hidden_prefixes);
    }

    let expr = query::parse_search(search).map_err(|e| e.to_string())?;
    let sf = sort_field
        .map(|s| query::SortField::parse_str(s).ok_or_else(|| format!("Unknown sort field: {s}")))
        .transpose()?;
    let so = sort_order
        .map(|s| query::SortOrder::parse_str(s).ok_or_else(|| format!("Unknown sort order: {s}")))
        .transpose()?
        .unwrap_or(query::SortOrder::Asc);

    Ok(query::query(
        &parse,
        &query::QueryOptions {
            search: expr,
            sort_field: sf,
            sort_order: so,
            offset,
            limit,
            input_order: None,
            min_value: None,
            hidden_prefixes: if show_hidden {
                hidden_prefixes
            } else {
                Vec::new()
            },
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_ledger(dir: &Path, folder: &str, contents: &str) {
        let path = dir.join(folder).join("ledger.transactions");
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent");
        }
        fs::write(path, contents).expect("write ledger");
    }

    fn txn(payee: &str, account: &str) -> String {
        format!(
      "2025-01-01 * \"{payee}\" \"{payee}\"\n    {account} 1 USD\n    expenses:unknown -1 USD\n"
    )
    }

    #[test]
    fn scoped_load_only_reads_exact_and_descendant_folders() {
        let set_dir = std::env::temp_dir().join(format!(
            "arimalo-scoped-load-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        ));
        fs::create_dir_all(&set_dir).expect("create temp dir");

        write_ledger(&set_dir, "cash", &txn("parent", "assets:cash"));
        write_ledger(
            &set_dir,
            "cash/bank",
            &(txn("ancestor", "assets:cash:bank:savings")),
        );
        write_ledger(
            &set_dir,
            "cash/bank/savings",
            &txn("exact", "assets:cash:bank:savings"),
        );
        write_ledger(
            &set_dir,
            "cash/bank/savings/bonus",
            &txn("descendant", "assets:cash:bank:savings:bonus"),
        );
        write_ledger(
            &set_dir,
            "cash/bank/checking",
            &txn("sibling", "assets:cash:bank:checking"),
        );

        let txns = load_scoped_transactions(&set_dir, "assets:cash:bank:savings")
            .expect("load scoped transactions");
        let payees: Vec<&str> = txns.iter().filter_map(|t| t.payee.as_deref()).collect();

        assert_eq!(payees, vec!["descendant", "exact"]);

        let _ = fs::remove_dir_all(&set_dir);
    }

    #[test]
    fn global_query_finds_non_folder_backed_accounts_scoped_misses() {
        let set_dir = std::env::temp_dir().join(format!(
            "arimalo-global-query-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
        ));
        fs::create_dir_all(&set_dir).expect("create temp dir");

        // A staking move lives INSIDE a wallet folder: the contra `assets:staking`
        // leg is recorded in the wallet's ledger, but no source folder derives to
        // the name `assets:staking`, so the folder-pruned scoped query can't reach it.
        write_ledger(
            &set_dir,
            "crypto/wallet/solana/ABC",
            "2025-01-01 * \"Stake\" \"Stake LFNTY\"\n    assets:crypto:wallet:solana:ABC -100 LFNTY\n    assets:staking 100 LFNTY\n",
        );

        let scoped = scoped_query(&set_dir, "account:assets:staking", false, None, None, None, None)
            .expect("scoped_query");
        assert_eq!(
            scoped.transaction_count, 0,
            "scoped_query folder-prunes and should find no staking rows"
        );

        let global = global_query(&set_dir, "account:assets:staking", false, None, None, None, None)
            .expect("global_query");
        assert_eq!(
            global.transaction_count, 1,
            "global_query unions all ledgers and should find the staking row"
        );
        let staking = global
            .aggregated_balance
            .iter()
            .find(|c| c.commodity == "LFNTY")
            .expect("LFNTY balance present in aggregated_balance");
        assert!(
            (staking.amount - 100.0).abs() < 1e-9,
            "staking balance should aggregate to 100 LFNTY, got {}",
            staking.amount
        );

        let _ = fs::remove_dir_all(&set_dir);
    }

    fn tmp_set_dir(suffix: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arimalo-hidden-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            suffix
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn load_hidden_accounts_appends_colon_to_bare_prefix() {
        let dir = tmp_set_dir("bare");
        fs::write(
            dir.join("config.json"),
            r#"{"hidden_accounts": ["ignore"]}"#,
        )
        .unwrap();
        let prefixes = load_hidden_accounts(&dir);
        assert_eq!(prefixes, vec!["ignore:"]);
    }

    #[test]
    fn load_hidden_accounts_keeps_explicit_prefix_with_colon() {
        let dir = tmp_set_dir("explicit");
        fs::write(
            dir.join("config.json"),
            r#"{"hidden_accounts": ["ignore:hidden", "trash:bin"]}"#,
        )
        .unwrap();
        let prefixes = load_hidden_accounts(&dir);
        assert_eq!(prefixes, vec!["ignore:hidden", "trash:bin"]);
    }

    #[test]
    fn load_hidden_accounts_returns_empty_when_config_missing() {
        let dir = tmp_set_dir("missing");
        // No config.json written
        assert!(load_hidden_accounts(&dir).is_empty());
    }

    #[test]
    fn load_hidden_accounts_returns_empty_when_key_absent() {
        let dir = tmp_set_dir("no-key");
        fs::write(dir.join("config.json"), r#"{"base_currency": "AUD"}"#).unwrap();
        assert!(load_hidden_accounts(&dir).is_empty());
    }

    /// Invariant: `load_active_ledger` MUST apply the same `(datetime, meta,
    /// txn_sort_key)` 3-key sort as `sort_tagged_txns` in the pipeline.
    /// Without the third key, two transactions sharing `(datetime, meta)`
    /// keep their file-input order — which depends on per-folder write order
    /// upstream and is not guaranteed stable. Then downstream report writers
    /// produce different output run-to-run.
    ///
    /// This test writes two same-(datetime, meta) transactions to disk in
    /// the OPPOSITE order in two separate runs and asserts the loaded result
    /// is identical.
    #[test]
    fn load_active_ledger_breaks_ties_deterministically_across_file_input_order() {
        // Two transactions with identical datetime and identical (empty) meta.
        // Distinguishable only by payee — `transaction_to_text` is the third
        // sort key, so the deterministic order is whichever payee-suffix
        // serialises first as ledger text.
        let txn_alpha = "2026-01-15 12:00:00 * \"Alpha payee\" \"alpha\"\n    assets:wallet:a  10 USD\n    income:other  -10 USD\n";
        let txn_omega = "2026-01-15 12:00:00 * \"Omega payee\" \"omega\"\n    assets:wallet:o  20 USD\n    income:other  -20 USD\n";

        // Run A: alpha file lex-sorts before omega → loader appends alpha first.
        let dir_a = tmp_set_dir("tiebreak-a");
        fs::create_dir_all(dir_a.join("folder_a")).unwrap();
        fs::create_dir_all(dir_a.join("folder_b")).unwrap();
        fs::write(dir_a.join("folder_a").join("ledger.transactions"), txn_alpha).unwrap();
        fs::write(dir_a.join("folder_b").join("ledger.transactions"), txn_omega).unwrap();
        let r_a = load_active_ledger(&dir_a).expect("load A");

        // Run B: swap which folder holds which transaction — now omega comes
        // off disk first. Without the txn-id tiebreak the 2-key sort would
        // keep that file-input order; with the tiebreak the output matches A.
        let dir_b = tmp_set_dir("tiebreak-b");
        fs::create_dir_all(dir_b.join("folder_a")).unwrap();
        fs::create_dir_all(dir_b.join("folder_b")).unwrap();
        fs::write(dir_b.join("folder_a").join("ledger.transactions"), txn_omega).unwrap();
        fs::write(dir_b.join("folder_b").join("ledger.transactions"), txn_alpha).unwrap();
        let r_b = load_active_ledger(&dir_b).expect("load B");

        let payees_a: Vec<_> = r_a
            .transactions
            .iter()
            .map(|t| t.payee.clone().unwrap_or_default())
            .collect();
        let payees_b: Vec<_> = r_b
            .transactions
            .iter()
            .map(|t| t.payee.clone().unwrap_or_default())
            .collect();

        assert_eq!(
            payees_a, payees_b,
            "load_active_ledger must produce deterministic order across file-input shuffles; A={payees_a:?} B={payees_b:?}"
        );

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }
}
