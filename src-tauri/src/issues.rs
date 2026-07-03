//! Data-quality issue collection.
//!
//! Single source of truth for the list of issues surfaced by the UI and the
//! `arimalo-issues` CLI. Ports the logic previously implemented in
//! `src/issues.ts` to Rust so the UI can become a thin client.
//!
//! Pure collectors take already-loaded inputs and are unit-tested with fixtures.
//! The `collect_all` orchestrator wires disk loads together for CLI/Tauri use.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::automerge_store::{suggest_trade_links, MetadataStore, TradeSuggestion};
use crate::generated_store::load_active_ledger;
use crate::ledger_parser::{AccountBalance, Diagnostic, ParseResult, PriceGraph, Transaction};
use crate::processing_pipeline::{detect_account_gaps, AccountGap, PipelineMetadata};

// ── Public types (serde camelCase to match the existing TS Issue type) ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueSeverity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub severity: IssueSeverity,
    pub group: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reveal_path: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub accounts: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trade_suggestion_idx: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scroll_to_txn_id: Option<String>,
}

impl Issue {
    fn plain(severity: IssueSeverity, group: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            severity,
            group: group.into(),
            message: message.into(),
            filter_kind: None,
            reveal_path: None,
            accounts: Vec::new(),
            trade_suggestion_idx: None,
            scroll_to_txn_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueGroup {
    pub label: String,
    pub severity: IssueSeverity,
    pub issues: Vec<Issue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reveal_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    ParseErrors,
    Uncategorised,
    PipelineWarnings,
    AccountGaps,
    UnverifiedBalances,
    TradeSuggestions,
    Unpriced,
}

impl Category {
    pub const ALL: [Category; 7] = [
        Category::ParseErrors,
        Category::Uncategorised,
        Category::PipelineWarnings,
        Category::AccountGaps,
        Category::UnverifiedBalances,
        Category::TradeSuggestions,
        Category::Unpriced,
    ];

    pub fn flag(&self) -> &'static str {
        match self {
            Category::ParseErrors => "--parse-errors",
            Category::Uncategorised => "--uncategorised",
            Category::PipelineWarnings => "--pipeline-warnings",
            Category::AccountGaps => "--gaps",
            Category::UnverifiedBalances => "--unverified",
            Category::TradeSuggestions => "--trade-suggestions",
            Category::Unpriced => "--unpriced",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct CollectFilter {
    pub categories: BTreeSet<Category>,
    pub account: Option<String>,
}

impl CollectFilter {
    pub fn all_categories() -> Self {
        Self {
            categories: Category::ALL.iter().copied().collect(),
            account: None,
        }
    }
    fn wants(&self, c: Category) -> bool {
        self.categories.contains(&c)
    }
}

// ── Persisted sidecar for pipeline warnings ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PipelineWarningsFile {
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl PipelineWarningsFile {
    pub const FILE_NAME: &'static str = "warnings.json";

    pub fn load(generated_dir: &Path) -> Self {
        let path = generated_dir.join(Self::FILE_NAME);
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
}

// ── Pure collectors (unit-tested below) ──

pub fn collect_parse_errors(diagnostics: &[Diagnostic]) -> Option<IssueGroup> {
    if diagnostics.is_empty() {
        return None;
    }
    let issues: Vec<Issue> = diagnostics
        .iter()
        .map(|d| {
            let file_tail = d
                .file
                .as_deref()
                .and_then(|f| f.rsplit('/').next())
                .map(|t| format!(" ({t})"))
                .unwrap_or_default();
            Issue {
                severity: IssueSeverity::Error,
                group: "Parse Errors".into(),
                message: format!(
                    "line {}, col {} — {}{}",
                    d.line, d.column, d.message, file_tail
                ),
                filter_kind: None,
                reveal_path: d.file.clone(),
                accounts: Vec::new(),
                trade_suggestion_idx: None,
                scroll_to_txn_id: None,
            }
        })
        .collect();
    Some(IssueGroup {
        label: "Parse Errors".into(),
        severity: IssueSeverity::Error,
        issues,
        filter_kind: None,
        reveal_path: None,
        account: None,
    })
}

fn is_uncategorised(txn: &Transaction) -> bool {
    if txn.postings.len() < 2 {
        return true;
    }
    let unique: HashSet<&str> = txn.postings.iter().map(|p| p.account.as_str()).collect();
    if unique.len() < 2 {
        return true;
    }
    txn.postings.iter().any(|p| p.account == "expenses:unknown")
}

pub fn collect_uncategorised(
    transactions: &[Transaction],
    for_account: Option<&str>,
) -> Option<IssueGroup> {
    let relevant: Vec<&Transaction> = if let Some(acct) = for_account {
        transactions
            .iter()
            .filter(|t| t.postings.iter().any(|p| p.account == acct))
            .collect()
    } else {
        transactions.iter().collect()
    };
    let uncategorised: Vec<&Transaction> = relevant.into_iter().filter(|t| is_uncategorised(t)).collect();
    if uncategorised.is_empty() {
        return None;
    }

    let mut issues: Vec<Issue> = Vec::with_capacity(uncategorised.len().min(11) + 1);
    issues.push(Issue {
        severity: IssueSeverity::Warning,
        group: "Uncategorised".into(),
        message: format!(
            "{} uncategorised transaction{}",
            uncategorised.len(),
            if uncategorised.len() == 1 { "" } else { "s" }
        ),
        filter_kind: Some("uncategorised".into()),
        reveal_path: None,
        accounts: Vec::new(),
        trade_suggestion_idx: None,
        scroll_to_txn_id: None,
    });

    let detail_count = uncategorised.len().min(10);
    for t in uncategorised.iter().take(detail_count) {
        let payee = t
            .display_payee
            .as_deref()
            .or(t.payee.as_deref())
            .unwrap_or("");
        let narration = t.narration.as_deref().unwrap_or("");
        issues.push(Issue::plain(
            IssueSeverity::Warning,
            "Uncategorised",
            format!("{} {} — {}", t.date, payee, narration),
        ));
    }
    if uncategorised.len() > detail_count {
        issues.push(Issue::plain(
            IssueSeverity::Warning,
            "Uncategorised",
            format!("...and {} more", uncategorised.len() - detail_count),
        ));
    }

    Some(IssueGroup {
        label: "Uncategorised".into(),
        severity: IssueSeverity::Warning,
        issues,
        filter_kind: Some("uncategorised".into()),
        reveal_path: None,
        account: None,
    })
}

pub fn collect_pipeline_warnings(
    warnings: &[String],
    account_folders: &HashMap<String, String>,
    sources_dir: &Path,
    for_account: Option<&str>,
) -> Vec<IssueGroup> {
    let mut by_account: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for w in warnings {
        if let Some(idx) = w.find(':') {
            let acct = w[..idx].trim().to_string();
            let msg = w[idx + 1..].trim().to_string();
            by_account.entry(acct).or_default().push(msg);
        } else {
            by_account.entry("Other".into()).or_default().push(w.clone());
        }
    }

    let mut groups: Vec<IssueGroup> = Vec::new();
    for (account, msgs) in by_account {
        if let Some(filter) = for_account {
            if account != filter {
                continue;
            }
        }
        let reveal_path = account_folders.get(&account).map(|folder| {
            sources_dir
                .join(folder)
                .to_string_lossy()
                .to_string()
        });
        let issues: Vec<Issue> = msgs
            .into_iter()
            .map(|m| Issue {
                severity: IssueSeverity::Warning,
                group: account.clone(),
                message: m,
                filter_kind: None,
                reveal_path: reveal_path.clone(),
                accounts: Vec::new(),
                trade_suggestion_idx: None,
                scroll_to_txn_id: None,
            })
            .collect();
        groups.push(IssueGroup {
            label: account.clone(),
            severity: IssueSeverity::Warning,
            issues,
            filter_kind: None,
            reveal_path,
            account: Some(account),
        });
    }
    groups
}

pub fn collect_account_gaps(gaps: &[AccountGap], for_account: Option<&str>) -> Vec<IssueGroup> {
    gaps.iter()
        .filter(|g| !g.missing_months.is_empty())
        .filter(|g| for_account.is_none_or(|a| g.account == a))
        .map(|g| IssueGroup {
            label: g.account.clone(),
            severity: IssueSeverity::Info,
            issues: vec![Issue::plain(
                IssueSeverity::Info,
                g.account.clone(),
                format!("Missing data for: {}", g.missing_months.join(", ")),
            )],
            filter_kind: None,
            reveal_path: None,
            account: Some(g.account.clone()),
        })
        .collect()
}

pub fn collect_unverified_balances(
    balances: &[AccountBalance],
    accounts_with_opening: &[String],
    for_account: Option<&str>,
) -> Option<IssueGroup> {
    let opening: HashSet<&str> = accounts_with_opening.iter().map(|s| s.as_str()).collect();
    let unverified: Vec<&str> = balances
        .iter()
        .map(|b| b.account.as_str())
        .filter(|a| a.starts_with("assets:") && !opening.contains(a))
        .filter(|a| for_account.is_none_or(|f| *a == f))
        .collect();
    if unverified.is_empty() {
        return None;
    }
    Some(IssueGroup {
        label: "Unverified Balance".into(),
        severity: IssueSeverity::Info,
        issues: unverified
            .into_iter()
            .map(|a| {
                Issue::plain(
                    IssueSeverity::Info,
                    "Unverified Balance",
                    format!("{a} — no opening balance"),
                )
            })
            .collect(),
        filter_kind: None,
        reveal_path: None,
        account: None,
    })
}

pub fn collect_trade_suggestions(suggestions: &[TradeSuggestion]) -> Option<IssueGroup> {
    if suggestions.is_empty() {
        return None;
    }
    Some(IssueGroup {
        label: "Suggested Trades".into(),
        severity: IssueSeverity::Info,
        issues: suggestions
            .iter()
            .enumerate()
            .map(|(idx, s)| Issue {
                severity: IssueSeverity::Info,
                group: "Suggested Trades".into(),
                message: s.summary.clone(),
                filter_kind: None,
                reveal_path: None,
                accounts: Vec::new(),
                trade_suggestion_idx: Some(idx),
                scroll_to_txn_id: Some(s.txn_id_a.clone()),
            })
            .collect(),
        filter_kind: None,
        reveal_path: None,
        account: None,
    })
}

pub fn collect_unpriced(
    transactions: &[Transaction],
    graph: &PriceGraph,
    base_currency: &str,
) -> Option<IssueGroup> {
    struct Info {
        count: usize,
        first: String,
        last: String,
        accounts: BTreeSet<String>,
    }
    let mut by_commodity: BTreeMap<String, Info> = BTreeMap::new();
    for txn in transactions {
        for p in &txn.postings {
            if p.commodity.is_empty() || p.commodity == base_currency {
                continue;
            }
            if graph
                .convert_to_base(&p.commodity, 1.0, &txn.datetime, base_currency)
                .is_some()
            {
                continue;
            }
            let entry = by_commodity.entry(p.commodity.clone()).or_insert_with(|| Info {
                count: 0,
                first: txn.date.clone(),
                last: txn.date.clone(),
                accounts: BTreeSet::new(),
            });
            entry.count += 1;
            if txn.date < entry.first {
                entry.first = txn.date.clone();
            }
            if txn.date > entry.last {
                entry.last = txn.date.clone();
            }
            entry.accounts.insert(p.account.clone());
        }
    }
    if by_commodity.is_empty() {
        return None;
    }
    let issues = by_commodity
        .into_iter()
        .map(|(commodity, info)| {
            let accounts = info.accounts.into_iter().collect::<Vec<_>>().join(", ");
            Issue::plain(
                IssueSeverity::Warning,
                "Unpriced",
                format!(
                    "{commodity} — {} postings, {} → {} ({accounts})",
                    info.count, info.first, info.last
                ),
            )
        })
        .collect();
    Some(IssueGroup {
        label: "Unpriced".into(),
        severity: IssueSeverity::Warning,
        issues,
        filter_kind: None,
        reveal_path: None,
        account: None,
    })
}

// ── Per-account issue counts (for sidebar badges) ──

fn bump(counts: &mut BTreeMap<String, usize>, account: &str) {
    *counts.entry(account.to_string()).or_insert(0) += 1;
}

fn count_uncategorised(transactions: &[Transaction], counts: &mut BTreeMap<String, usize>) {
    for t in transactions {
        if !is_uncategorised(t) {
            continue;
        }
        for p in &t.postings {
            if p.account.starts_with("assets:") {
                bump(counts, &p.account);
            }
        }
    }
}

fn count_pipeline_warnings(warnings: &[String], counts: &mut BTreeMap<String, usize>) {
    for w in warnings {
        if let Some(idx) = w.find(':') {
            bump(counts, w[..idx].trim());
        }
    }
}

fn count_account_gaps(gaps: &[AccountGap], counts: &mut BTreeMap<String, usize>) {
    for g in gaps {
        if !g.missing_months.is_empty() {
            bump(counts, &g.account);
        }
    }
}

fn count_unverified(
    balances: &[AccountBalance],
    accounts_with_opening: &[String],
    counts: &mut BTreeMap<String, usize>,
) {
    let opening: HashSet<&str> = accounts_with_opening.iter().map(|s| s.as_str()).collect();
    for b in balances {
        if b.account.starts_with("assets:") && !opening.contains(b.account.as_str()) {
            bump(counts, &b.account);
        }
    }
}

// ── Orchestrator ──

/// Aggregated result: the issue groups the UI shows plus per-account badge counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectedIssues {
    pub groups: Vec<IssueGroup>,
    pub account_counts: BTreeMap<String, usize>,
}

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

fn push<T: Extend<IssueGroup>>(target: &mut T, group: Option<IssueGroup>) {
    if let Some(g) = group {
        target.extend(std::iter::once(g));
    }
}

/// Load all inputs from `generated_dir`/`sources_dir` and assemble matching issues.
/// `generated_dir` is the root — issues are aggregated across every account set
/// directory inside it, matching how the UI presents them.
pub fn collect_all(
    sources_dir: &Path,
    generated_dir: &Path,
    filter: &CollectFilter,
) -> Result<CollectedIssues, String> {
    let mut groups: Vec<IssueGroup> = Vec::new();
    let mut account_counts: BTreeMap<String, usize> = BTreeMap::new();

    // An account set is a directory holding a ledger or an account set config.
    // Either `ledger.transactions` (flat layout, used by tests) or `config.json`
    // (real layout — ledger text is assembled by `load_active_ledger` from
    // archives/subfolders).
    let set_dirs: Vec<std::path::PathBuf> = std::fs::read_dir(generated_dir)
        .map_err(|e| format!("Cannot read generated dir {}: {e}", generated_dir.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && (p.join("ledger.transactions").exists() || p.join("config.json").exists())
        })
        .collect();

    // Inputs that are global (not per-set)
    let warnings_file = PipelineWarningsFile::load(generated_dir);
    let account_folders = PipelineMetadata::load(generated_dir)
        .map(|m| m.account_folders)
        .unwrap_or_default();

    if filter.wants(Category::PipelineWarnings) {
        groups.extend(collect_pipeline_warnings(
            &warnings_file.warnings,
            &account_folders,
            sources_dir,
            filter.account.as_deref(),
        ));
        count_pipeline_warnings(&warnings_file.warnings, &mut account_counts);
    }

    let graph = if filter.wants(Category::Unpriced) {
        Some(PriceGraph::load(sources_dir))
    } else {
        None
    };

    // Trade suggestions need the MetadataStore — open once.
    let metadata_store = if filter.wants(Category::TradeSuggestions) {
        let path = sources_dir.join("arimalo-metadata.automerge");
        Some(MetadataStore::new(path)?)
    } else {
        None
    };

    for set_dir in &set_dirs {
        let parse: ParseResult = load_active_ledger(set_dir)
            .map_err(|e| format!("load_active_ledger({}): {e}", set_dir.display()))?;

        if filter.wants(Category::ParseErrors) {
            push(&mut groups, collect_parse_errors(&parse.diagnostics));
        }
        if filter.wants(Category::Uncategorised) {
            push(
                &mut groups,
                collect_uncategorised(&parse.transactions, filter.account.as_deref()),
            );
            count_uncategorised(&parse.transactions, &mut account_counts);
        }
        if filter.wants(Category::UnverifiedBalances) {
            push(
                &mut groups,
                collect_unverified_balances(
                    &parse.balances,
                    &parse.accounts_with_opening,
                    filter.account.as_deref(),
                ),
            );
            count_unverified(&parse.balances, &parse.accounts_with_opening, &mut account_counts);
        }
        if filter.wants(Category::AccountGaps) {
            let gaps = detect_account_gaps(set_dir).unwrap_or_default();
            groups.extend(collect_account_gaps(&gaps, filter.account.as_deref()));
            count_account_gaps(&gaps, &mut account_counts);
        }
        if let (Some(graph), true) = (graph.as_ref(), filter.wants(Category::Unpriced)) {
            let base = load_base_currency(set_dir);
            push(&mut groups, collect_unpriced(&parse.transactions, graph, &base));
        }
        if let Some(store) = metadata_store.as_ref() {
            let existing = store.get_trade_links()?;
            let base = load_base_currency(set_dir);
            let price_graph_for_trades = PriceGraph::load(sources_dir);
            let suggestions = suggest_trade_links(
                &parse.transactions,
                &existing,
                Some(&price_graph_for_trades),
                Some(&base),
            );
            push(&mut groups, collect_trade_suggestions(&suggestions));
        }
    }

    Ok(CollectedIssues {
        groups,
        account_counts,
    })
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger_parser::{CommodityAmount, Posting};

    fn txn(date: &str, postings: Vec<Posting>) -> Transaction {
        Transaction {
            date: date.into(),
            datetime: format!("{date}T00:00:00Z"),
            status: None,
            payee: None,
            narration: None,
            meta: None,
            postings,
            display_payee: None,
            amount: 0.0,
            amount_commodity: "AUD".into(),
            display_amount_commodity: None,
            fee: None,
            fee_commodity: None,
        }
    }

    fn posting(account: &str) -> Posting {
        Posting {
            account: account.into(),
            amount: 0.0,
            amount_text: "0".into(),
            commodity: "AUD".into(),
            remainder: None,
            cost: None,
            price: None,
        }
    }

    #[test]
    fn uncategorised_flags_single_posting_txn() {
        let t = txn("2024-01-01", vec![posting("assets:bank")]);
        assert!(is_uncategorised(&t));
    }

    #[test]
    fn uncategorised_flags_expenses_unknown() {
        let t = txn(
            "2024-01-01",
            vec![posting("assets:bank"), posting("expenses:unknown")],
        );
        assert!(is_uncategorised(&t));
    }

    #[test]
    fn uncategorised_flags_all_same_account() {
        let t = txn(
            "2024-01-01",
            vec![posting("assets:bank"), posting("assets:bank")],
        );
        assert!(is_uncategorised(&t));
    }

    #[test]
    fn uncategorised_skips_normal_txn() {
        let t = txn(
            "2024-01-01",
            vec![posting("assets:bank"), posting("expenses:food")],
        );
        assert!(!is_uncategorised(&t));
    }

    #[test]
    fn collect_uncategorised_empty_returns_none() {
        let txns = vec![txn(
            "2024-01-01",
            vec![posting("assets:bank"), posting("expenses:food")],
        )];
        assert!(collect_uncategorised(&txns, None).is_none());
    }

    #[test]
    fn collect_uncategorised_summary_count_matches() {
        let txns = (0..3)
            .map(|i| txn(&format!("2024-01-0{}", i + 1), vec![posting("assets:bank")]))
            .collect::<Vec<_>>();
        let g = collect_uncategorised(&txns, None).expect("group");
        // First issue is the summary line
        assert_eq!(g.issues[0].message, "3 uncategorised transactions");
        assert_eq!(g.filter_kind.as_deref(), Some("uncategorised"));
    }

    #[test]
    fn collect_uncategorised_singular_suffix() {
        let txns = vec![txn("2024-01-01", vec![posting("assets:bank")])];
        let g = collect_uncategorised(&txns, None).expect("group");
        assert_eq!(g.issues[0].message, "1 uncategorised transaction");
    }

    #[test]
    fn collect_uncategorised_account_filter_narrows() {
        let mut bank = txn("2024-01-01", vec![posting("assets:bank")]);
        bank.postings[0].account = "assets:bank".into();
        let crypto = txn("2024-01-02", vec![posting("assets:eth")]);
        let txns = vec![bank, crypto];
        assert!(collect_uncategorised(&txns, Some("assets:bank")).is_some());
        let only_bank = collect_uncategorised(&txns, Some("assets:bank")).unwrap();
        assert_eq!(only_bank.issues[0].message, "1 uncategorised transaction");
    }

    #[test]
    fn unverified_balance_requires_assets_prefix() {
        let balances = vec![
            AccountBalance {
                account: "assets:bank".into(),
                totals: vec![CommodityAmount { commodity: "AUD".into(), amount: 0.0 }],
            },
            AccountBalance {
                account: "expenses:food".into(),
                totals: vec![],
            },
        ];
        let g = collect_unverified_balances(&balances, &[], None).expect("group");
        assert_eq!(g.issues.len(), 1);
        assert!(g.issues[0].message.contains("assets:bank"));
    }

    #[test]
    fn unverified_balance_skipped_when_opening_declared() {
        let balances = vec![AccountBalance {
            account: "assets:bank".into(),
            totals: vec![],
        }];
        let g = collect_unverified_balances(&balances, &["assets:bank".into()], None);
        assert!(g.is_none());
    }

    #[test]
    fn pipeline_warning_split_on_first_colon() {
        // Matches `src/issues.ts:102-106`: split at the first colon. The left
        // side is used as a bucket key; the right side is the message.
        let warnings = vec!["bank_account: something broke".into()];
        let groups = collect_pipeline_warnings(&warnings, &HashMap::new(), Path::new("/tmp"), None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "bank_account");
        assert_eq!(groups[0].issues[0].message, "something broke");
    }

    #[test]
    fn pipeline_warning_preserves_embedded_colons_in_message() {
        // An account-style "assets:bank" prefix is split at the first colon,
        // so the remainder (including the next colon) is kept as the message.
        let warnings = vec!["assets:bank: oops".into()];
        let groups = collect_pipeline_warnings(&warnings, &HashMap::new(), Path::new("/tmp"), None);
        assert_eq!(groups[0].label, "assets");
        assert_eq!(groups[0].issues[0].message, "bank: oops");
    }

    #[test]
    fn pipeline_warning_no_colon_goes_to_other_bucket() {
        let warnings = vec!["global warning no colon".into()];
        let groups = collect_pipeline_warnings(&warnings, &HashMap::new(), Path::new("/tmp"), None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "Other");
    }

    #[test]
    fn pipeline_warning_reveal_path_joined_from_folders() {
        let warnings = vec!["bank: oops".into()];
        let mut folders = HashMap::new();
        folders.insert("bank".into(), "richard/bank".into());
        let groups = collect_pipeline_warnings(
            &warnings,
            &folders,
            Path::new("/src"),
            None,
        );
        assert_eq!(
            groups[0].reveal_path.as_deref(),
            Some("/src/richard/bank")
        );
    }

    #[test]
    fn pipeline_warning_account_filter_narrows() {
        let warnings = vec!["bank: a".into(), "eth: b".into()];
        let groups = collect_pipeline_warnings(
            &warnings,
            &HashMap::new(),
            Path::new("/tmp"),
            Some("bank"),
        );
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "bank");
    }

    #[test]
    fn account_gaps_filters_by_account_and_skips_empty() {
        let gaps = vec![
            AccountGap {
                account: "assets:bank".into(),
                first_month: "2024-01".into(),
                last_month: "2024-03".into(),
                missing_months: vec!["2024-02".into()],
            },
            AccountGap {
                account: "assets:eth".into(),
                first_month: "2024-01".into(),
                last_month: "2024-01".into(),
                missing_months: vec![],
            },
        ];
        let groups = collect_account_gaps(&gaps, None);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].label, "assets:bank");
        assert!(collect_account_gaps(&gaps, Some("assets:eth")).is_empty());
    }

    #[test]
    fn parse_errors_empty_is_none() {
        assert!(collect_parse_errors(&[]).is_none());
    }

    #[test]
    fn parse_errors_formats_line_col_message() {
        let d = Diagnostic {
            line: 12,
            column: 3,
            message: "bad".into(),
            file: Some("/abs/path/ledger.transactions".into()),
        };
        let g = collect_parse_errors(std::slice::from_ref(&d)).unwrap();
        assert_eq!(g.issues.len(), 1);
        assert_eq!(
            g.issues[0].message,
            "line 12, col 3 — bad (ledger.transactions)"
        );
    }

    #[test]
    fn trade_suggestions_empty_is_none() {
        assert!(collect_trade_suggestions(&[]).is_none());
    }
}
