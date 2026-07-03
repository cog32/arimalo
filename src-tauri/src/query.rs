#![deny(warnings)]

//! Unified query engine — search, filter, sort, balance aggregation.
//!
//! Powers both the CLI (`arimalo-query`) and Tauri UI. Supports `field:value`
//! search terms with AND/OR operators, negation, amount conditions, commodity
//! exact/glob matching, sorting, and limits.

use std::collections::BTreeMap;
use std::sync::Arc;

use regex::Regex;
use serde::Serialize;

use crate::ledger_parser::{AccountBalance, CommodityAmount, ParseResult, PriceGraph, Transaction};

// ===========================================================================
// Query result
// ===========================================================================

#[derive(Debug, Clone, Serialize)]
pub struct QueryResult {
    pub transactions: Vec<Transaction>,
    pub balances: Vec<AccountBalance>,
    pub aggregated_balance: Vec<CommodityAmount>,
    pub accounts: Vec<String>,
    pub transaction_count: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputOrder {
    DateDesc,
}

// ===========================================================================
// Amount conditions
// ===========================================================================

#[derive(Debug, Clone)]
pub enum AmountOp {
    Gt,
    Gte,
    Lt,
    Lte,
    Eq,
}

#[derive(Debug, Clone)]
pub struct AmountCondition {
    pub op: AmountOp,
    pub value: f64,
}

fn parse_amount_condition(s: &str) -> Option<AmountCondition> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix(">=") {
        rest.parse().ok().map(|v| AmountCondition {
            op: AmountOp::Gte,
            value: v,
        })
    } else if let Some(rest) = s.strip_prefix("<=") {
        rest.parse().ok().map(|v| AmountCondition {
            op: AmountOp::Lte,
            value: v,
        })
    } else if let Some(rest) = s.strip_prefix('>') {
        rest.parse().ok().map(|v| AmountCondition {
            op: AmountOp::Gt,
            value: v,
        })
    } else if let Some(rest) = s.strip_prefix('<') {
        rest.parse().ok().map(|v| AmountCondition {
            op: AmountOp::Lt,
            value: v,
        })
    } else if let Some(rest) = s.strip_prefix('=') {
        rest.parse().ok().map(|v| AmountCondition {
            op: AmountOp::Eq,
            value: v,
        })
    } else {
        s.parse().ok().map(|v| AmountCondition {
            op: AmountOp::Eq,
            value: v,
        })
    }
}

fn amount_matches(cond: &AmountCondition, val: f64) -> bool {
    match cond.op {
        AmountOp::Gt => val > cond.value,
        AmountOp::Gte => val >= cond.value,
        AmountOp::Lt => val < cond.value,
        AmountOp::Lte => val <= cond.value,
        AmountOp::Eq => (val - cond.value).abs() < f64::EPSILON,
    }
}

// ===========================================================================
// Date conditions
// ===========================================================================

#[derive(Debug, Clone)]
pub enum DateCondition {
    /// date >= value
    Gte(String),
    /// date > value
    Gt(String),
    /// date <= value
    Lte(String),
    /// date < value
    Lt(String),
    /// date == value (exact prefix match, e.g. "2025" matches "2025-*")
    Eq(String),
    /// date >= from AND date <= to
    Range(String, String),
}

fn parse_date_condition(s: &str) -> Option<DateCondition> {
    let s = s.trim();
    // Range: 2025-01-01..2025-12-31
    if let Some(dotdot) = s.find("..") {
        let from = s[..dotdot].trim();
        let to = s[dotdot + 2..].trim();
        if !from.is_empty() && !to.is_empty() {
            return Some(DateCondition::Range(from.to_string(), to.to_string()));
        }
        return None;
    }
    // Comparison operators
    if let Some(rest) = s.strip_prefix(">=") {
        let v = rest.trim();
        if !v.is_empty() {
            return Some(DateCondition::Gte(v.to_string()));
        }
    } else if let Some(rest) = s.strip_prefix("<=") {
        let v = rest.trim();
        if !v.is_empty() {
            return Some(DateCondition::Lte(v.to_string()));
        }
    } else if let Some(rest) = s.strip_prefix('>') {
        let v = rest.trim();
        if !v.is_empty() {
            return Some(DateCondition::Gt(v.to_string()));
        }
    } else if let Some(rest) = s.strip_prefix('<') {
        let v = rest.trim();
        if !v.is_empty() {
            return Some(DateCondition::Lt(v.to_string()));
        }
    }
    None
}

fn date_matches(cond: &DateCondition, date: &str) -> bool {
    match cond {
        DateCondition::Gte(v) => date >= v.as_str(),
        DateCondition::Gt(v) => date > v.as_str(),
        DateCondition::Lte(v) => date <= v.as_str(),
        DateCondition::Lt(v) => date < v.as_str(),
        DateCondition::Eq(v) => date.starts_with(v.as_str()),
        DateCondition::Range(from, to) => date >= from.as_str() && date <= to.as_str(),
    }
}

// ===========================================================================
// Search expression AST
// ===========================================================================

const SEARCH_FIELDS: &[&str] = &[
    "payee",
    "narration",
    "meta",
    "date",
    "account",
    "commodity",
    "amount",
    "fee",
    "is",
];

/// Threshold below which a transaction's primary amount is treated as a
/// no-op leg (e.g. a 0 ETH contract-interaction posting recorded alongside
/// an ERC-20 transfer). Mirrors `buildTradeGroup3Map` in `src/main.ts`.
const SWAP_AMOUNT_EPSILON: f64 = 1e-9;

fn is_valid_leg(txn: &Transaction) -> bool {
    txn.amount.is_finite() && txn.amount.abs() >= SWAP_AMOUNT_EPSILON
}

/// Per-hash aggregate used to gate `is:swap` matching.
#[derive(Debug, Default, Clone)]
pub struct HashAggregate {
    /// Number of legs sharing this `txn:<hash>` whose primary amount is
    /// non-zero (i.e. real legs, not no-op postings).
    pub valid_count: usize,
    /// Distinct primary commodities across the valid legs.
    pub distinct_commodities: usize,
}

/// Per-query context computed once over the full transaction set so that
/// classification predicates (e.g. `is:swap`, which depends on whether a
/// transaction's `txn:<hash>` is shared with others) don't require an
/// O(N²) scan inside the per-term matcher.
#[derive(Debug, Default)]
pub struct SearchContext {
    /// Map from `txn:<hash>` (full meta segment, including the `txn:` prefix)
    /// to its aggregate. A txn is part of a swap when its hash group has
    /// (valid_count, distinct_commodities) of (2, 2) or (3, 3) — same gates
    /// as the UI's bracket detector.
    pub hash_aggregates: std::collections::HashMap<String, HashAggregate>,
}

impl SearchContext {
    pub fn from_transactions(transactions: &[Transaction]) -> Self {
        use std::collections::{HashMap, HashSet};
        let mut commodities: HashMap<String, HashSet<String>> = HashMap::new();
        let mut valid_counts: HashMap<String, usize> = HashMap::new();
        for txn in transactions {
            if !is_valid_leg(txn) {
                continue;
            }
            let Some(meta) = txn.meta.as_deref() else {
                continue;
            };
            for segment in meta.split(',') {
                let trimmed = segment.trim();
                if trimmed.starts_with("txn:") {
                    *valid_counts.entry(trimmed.to_string()).or_insert(0) += 1;
                    commodities
                        .entry(trimmed.to_string())
                        .or_default()
                        .insert(txn.amount_commodity.clone());
                }
            }
        }
        let hash_aggregates = valid_counts
            .into_iter()
            .map(|(key, valid_count)| {
                let distinct_commodities =
                    commodities.get(&key).map(|s| s.len()).unwrap_or(0);
                (
                    key,
                    HashAggregate {
                        valid_count,
                        distinct_commodities,
                    },
                )
            })
            .collect();
        SearchContext { hash_aggregates }
    }
}

/// Compute the synthetic label string for the `is:` field.
/// `swap` covers shared-`txn:<hash>` groups whose post-zero-filter shape is
/// a clean 2-leg (2 legs, 2 distinct commodities) or 3-leg (3 legs, 3
/// distinct commodities) — same gates as the UI's bracket detector. Same-
/// commodity legs (self-transfers, split fills) and 0-amount no-op legs are
/// rejected so phantom swaps don't surface in search results.
///
/// A transaction with a 0-amount itself never matches: it's a noise leg,
/// not a real swap participant.
fn is_labels_for_txn(txn: &Transaction, ctx: &SearchContext) -> String {
    if !is_valid_leg(txn) {
        return String::new();
    }
    let Some(meta) = txn.meta.as_deref() else {
        return String::new();
    };
    for segment in meta.split(',') {
        let trimmed = segment.trim();
        if !trimmed.starts_with("txn:") {
            continue;
        }
        if let Some(agg) = ctx.hash_aggregates.get(trimmed) {
            let is_swap = (agg.valid_count == 2 && agg.distinct_commodities == 2)
                || (agg.valid_count == 3 && agg.distinct_commodities == 3);
            if is_swap {
                return "swap".to_string();
            }
        }
    }
    String::new()
}

#[derive(Debug, Clone)]
pub struct SearchTerm {
    pub field: Option<String>,
    pub regex: Regex,
    pub amount_condition: Option<AmountCondition>,
    pub date_condition: Option<DateCondition>,
    pub negated: bool,
}

#[derive(Debug, Clone)]
pub enum SearchExpr {
    Term(SearchTerm),
    And(Box<SearchExpr>, Box<SearchExpr>),
    Or(Box<SearchExpr>, Box<SearchExpr>),
}

// ===========================================================================
// Parser
// ===========================================================================

pub fn parse_search(input: &str) -> Result<SearchExpr, String> {
    let raw = input.trim();
    if raw.is_empty() {
        return Ok(SearchExpr::Term(SearchTerm {
            field: None,
            regex: Regex::new("(?s)").unwrap(),
            amount_condition: None,
            date_condition: None,
            negated: false,
        }));
    }

    // Tokenise: split on \bAND\b and \bOR\b while preserving them
    let splitter = Regex::new(r"\b(AND|OR)\b").unwrap();
    let mut tokens: Vec<String> = Vec::new();
    let mut last = 0;
    for m in splitter.find_iter(raw) {
        let before = raw[last..m.start()].trim();
        if !before.is_empty() {
            tokens.push(before.to_string());
        }
        tokens.push(m.as_str().to_string());
        last = m.end();
    }
    let tail = raw[last..].trim();
    if !tail.is_empty() {
        tokens.push(tail.to_string());
    }

    if tokens.is_empty() {
        return Ok(SearchExpr::Term(SearchTerm {
            field: None,
            regex: Regex::new("(?s)").unwrap(),
            amount_condition: None,
            date_condition: None,
            negated: false,
        }));
    }

    // Validate token sequence
    if tokens[0] == "AND" || tokens[0] == "OR" {
        return Err(format!("Unexpected operator \"{}\" at start", tokens[0]));
    }
    if let Some(last_tok) = tokens.last() {
        if last_tok == "AND" || last_tok == "OR" {
            return Err(format!("Trailing operator \"{last_tok}\""));
        }
    }

    let mut parsed: Vec<TermOrOp> = Vec::new();
    for (i, tok) in tokens.iter().enumerate() {
        if tok == "AND" || tok == "OR" {
            if i > 0 && (tokens[i - 1] == "AND" || tokens[i - 1] == "OR") {
                return Err(format!(
                    "Consecutive operators \"{} {}\"",
                    tokens[i - 1],
                    tok
                ));
            }
            parsed.push(if tok == "AND" {
                TermOrOp::And
            } else {
                TermOrOp::Or
            });
            continue;
        }
        if i > 0 && tokens[i - 1] != "AND" && tokens[i - 1] != "OR" {
            return Err(format!(
                "Missing AND/OR between \"{}\" and \"{}\"",
                tokens[i - 1],
                tok
            ));
        }
        parsed.push(TermOrOp::Term(parse_term(tok)?));
    }

    Ok(build_expr(&parsed))
}

#[derive(Debug, Clone)]
enum TermOrOp {
    Term(SearchTerm),
    And,
    Or,
}

fn parse_term(tok: &str) -> Result<SearchTerm, String> {
    let mut term = tok;
    let mut negated = false;
    if term.starts_with('-') && term.len() > 1 {
        negated = true;
        term = &term[1..];
    }

    let (field, pattern) = if let Some(colon_idx) = term.find(':') {
        if colon_idx > 0 && !term[..colon_idx].contains(' ') {
            let f = term[..colon_idx].to_lowercase();
            if !SEARCH_FIELDS.contains(&f.as_str()) {
                return Err(format!(
                    "Unknown field \"{f}\". Valid: {}",
                    SEARCH_FIELDS.join(", ")
                ));
            }
            (Some(f), term[colon_idx + 1..].to_string())
        } else {
            (None, term.to_string())
        }
    } else {
        (None, term.to_string())
    };

    // For amount/fee, try numeric condition first
    if matches!(field.as_deref(), Some("amount" | "fee")) {
        if let Some(first_ch) = pattern.chars().next() {
            if first_ch == '>'
                || first_ch == '<'
                || first_ch == '='
                || first_ch.is_ascii_digit()
                || first_ch == '-'
            {
                if let Some(cond) = parse_amount_condition(&pattern) {
                    return Ok(SearchTerm {
                        field,
                        regex: Regex::new("(?s)").unwrap(),
                        amount_condition: Some(cond),
                        date_condition: None,
                        negated,
                    });
                }
            }
        }
    }

    // For date, try comparison/range condition first
    if field.as_deref() == Some("date") {
        if let Some(first_ch) = pattern.chars().next() {
            if first_ch == '>' || first_ch == '<' || pattern.contains("..") {
                if let Some(cond) = parse_date_condition(&pattern) {
                    return Ok(SearchTerm {
                        field,
                        regex: Regex::new("(?s)").unwrap(),
                        amount_condition: None,
                        date_condition: Some(cond),
                        negated,
                    });
                }
            }
        }
    }

    // For commodity, use exact match (or glob if wildcards)
    let effective_pattern = if field.as_deref() == Some("commodity") {
        if pattern.contains('*') {
            let escaped = regex::escape(&pattern).replace(r"\*", ".*");
            format!("^{escaped}$")
        } else {
            format!("^{}$", regex::escape(&pattern))
        }
    } else {
        pattern
    };

    let regex = Regex::new(&format!("(?i){effective_pattern}"))
        .map_err(|e| format!("Invalid regex \"{effective_pattern}\": {e}"))?;

    Ok(SearchTerm {
        field,
        regex,
        amount_condition: None,
        date_condition: None,
        negated,
    })
}

fn build_expr(items: &[TermOrOp]) -> SearchExpr {
    if let Some(pos) = items.iter().rposition(|t| matches!(t, TermOrOp::Or)) {
        return SearchExpr::Or(
            Box::new(build_expr(&items[..pos])),
            Box::new(build_expr(&items[pos + 1..])),
        );
    }
    if let Some(pos) = items.iter().rposition(|t| matches!(t, TermOrOp::And)) {
        return SearchExpr::And(
            Box::new(build_expr(&items[..pos])),
            Box::new(build_expr(&items[pos + 1..])),
        );
    }
    match &items[0] {
        TermOrOp::Term(t) => SearchExpr::Term(t.clone()),
        _ => unreachable!("operator without terms"),
    }
}

// ===========================================================================
// Matcher
// ===========================================================================

/// Convenience wrapper for callers that don't need the classification
/// predicates (e.g. unit tests of regex/AND/OR semantics). Equivalent to
/// `matches_search_with_context(expr, txn, &SearchContext::default())`.
pub fn matches_search(expr: &SearchExpr, txn: &Transaction) -> bool {
    matches_search_with_context(expr, txn, &SearchContext::default())
}

pub fn matches_search_with_context(
    expr: &SearchExpr,
    txn: &Transaction,
    ctx: &SearchContext,
) -> bool {
    match expr {
        SearchExpr::And(left, right) => {
            matches_search_with_context(left, txn, ctx)
                && matches_search_with_context(right, txn, ctx)
        }
        SearchExpr::Or(left, right) => {
            matches_search_with_context(left, txn, ctx)
                || matches_search_with_context(right, txn, ctx)
        }
        SearchExpr::Term(term) => {
            let result = match_term(term, txn, ctx);
            if term.negated {
                !result
            } else {
                result
            }
        }
    }
}

/// Like [`matches_search_with_context`], but `account:` field terms are treated
/// as satisfied. Used for "Show Ignored": a fully-hidden transaction has no leg
/// on the viewed account, yet folder scoping already proved it belongs there, so
/// the account predicate must not exclude it. All other predicates (date, amount,
/// payee, free text) are evaluated normally.
fn matches_search_relaxing_account(
    expr: &SearchExpr,
    txn: &Transaction,
    ctx: &SearchContext,
) -> bool {
    match expr {
        SearchExpr::And(left, right) => {
            matches_search_relaxing_account(left, txn, ctx)
                && matches_search_relaxing_account(right, txn, ctx)
        }
        SearchExpr::Or(left, right) => {
            matches_search_relaxing_account(left, txn, ctx)
                || matches_search_relaxing_account(right, txn, ctx)
        }
        SearchExpr::Term(term) => {
            if term.field.as_deref() == Some("account") {
                return true;
            }
            let result = match_term(term, txn, ctx);
            if term.negated {
                !result
            } else {
                result
            }
        }
    }
}

/// True when any posting lands on a hidden prefix — the same predicate the
/// scoped loader uses to exclude rows when "Show Ignored" is off, so the on
/// state re-admits exactly that set.
fn has_hidden_posting(txn: &Transaction, hidden_prefixes: &[String]) -> bool {
    txn.postings.iter().any(|p| {
        hidden_prefixes
            .iter()
            .any(|pfx| p.account.starts_with(pfx.as_str()))
    })
}

fn match_term(term: &SearchTerm, txn: &Transaction, ctx: &SearchContext) -> bool {
    if let Some(ref field) = term.field {
        match field.as_str() {
            "payee" => {
                let dp = txn.display_payee.as_deref().unwrap_or("");
                let p = txn.payee.as_deref().unwrap_or("");
                term.regex.is_match(dp)
                    || (txn.display_payee.is_some() && term.regex.is_match(p))
                    || (txn.display_payee.is_none() && term.regex.is_match(p))
            }
            "narration" => term.regex.is_match(txn.narration.as_deref().unwrap_or("")),
            "meta" => term.regex.is_match(txn.meta.as_deref().unwrap_or("")),
            "date" => {
                if let Some(ref cond) = term.date_condition {
                    date_matches(cond, &txn.date)
                } else {
                    term.regex.is_match(&txn.date)
                }
            }
            "account" => txn.postings.iter().any(|p| term.regex.is_match(&p.account)),
            "commodity" => txn
                .postings
                .iter()
                .any(|p| term.regex.is_match(&p.commodity)),
            "amount" => {
                if let Some(ref cond) = term.amount_condition {
                    amount_matches(cond, txn.amount)
                } else {
                    term.regex.is_match(&txn.amount.to_string())
                }
            }
            "fee" => {
                if let Some(fee) = txn.fee {
                    if let Some(ref cond) = term.amount_condition {
                        amount_matches(cond, fee)
                    } else {
                        term.regex.is_match(&fee.to_string())
                    }
                } else {
                    false
                }
            }
            "is" => {
                let labels = is_labels_for_txn(txn, ctx);
                term.regex.is_match(&labels)
            }
            _ => false,
        }
    } else {
        // Free text: search across all fields
        let postings_text: String = txn
            .postings
            .iter()
            .map(|p| format!("{} {} {}", p.account, p.commodity, p.amount_text))
            .collect::<Vec<_>>()
            .join(" ");
        let payee_text = [txn.display_payee.as_deref(), txn.payee.as_deref()]
            .iter()
            .filter_map(|x| *x)
            .collect::<Vec<_>>()
            .join(" ");
        let haystack = format!(
            "{} {} {} {} {}",
            txn.date,
            payee_text,
            txn.narration.as_deref().unwrap_or(""),
            txn.meta.as_deref().unwrap_or(""),
            postings_text
        );
        term.regex.is_match(&haystack)
    }
}

// ===========================================================================
// Sort
// ===========================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortField {
    Date,
    Amount,
    Payee,
    Account,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

impl SortField {
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "date" => Some(Self::Date),
            "amount" | "value" => Some(Self::Amount),
            "payee" => Some(Self::Payee),
            "account" => Some(Self::Account),
            _ => None,
        }
    }
}

impl SortOrder {
    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "asc" => Some(Self::Asc),
            "desc" => Some(Self::Desc),
            _ => None,
        }
    }
}

pub fn sort_transactions(txns: &mut [Transaction], field: SortField, order: SortOrder) {
    txns.sort_by(|a, b| {
        let cmp = match field {
            SortField::Date => a.datetime.cmp(&b.datetime),
            SortField::Amount => a
                .amount
                .partial_cmp(&b.amount)
                .unwrap_or(std::cmp::Ordering::Equal),
            SortField::Payee => {
                let pa = a
                    .display_payee
                    .as_deref()
                    .or(a.payee.as_deref())
                    .unwrap_or("");
                let pb = b
                    .display_payee
                    .as_deref()
                    .or(b.payee.as_deref())
                    .unwrap_or("");
                pa.to_lowercase().cmp(&pb.to_lowercase())
            }
            SortField::Account => {
                let aa = a.postings.first().map(|p| p.account.as_str()).unwrap_or("");
                let ab = b.postings.first().map(|p| p.account.as_str()).unwrap_or("");
                aa.cmp(ab)
            }
        };
        match order {
            SortOrder::Asc => cmp,
            SortOrder::Desc => cmp.reverse(),
        }
    });
}

// ===========================================================================
// Helpers
// ===========================================================================

/// Extract all non-negated account field regexes from a search expression.
fn extract_account_filters(expr: &SearchExpr) -> Vec<Regex> {
    let mut out = Vec::new();
    collect_account_filters(expr, &mut out);
    out
}

fn collect_account_filters(expr: &SearchExpr, out: &mut Vec<Regex>) {
    match expr {
        SearchExpr::And(left, right) | SearchExpr::Or(left, right) => {
            collect_account_filters(left, out);
            collect_account_filters(right, out);
        }
        SearchExpr::Term(term) => {
            if !term.negated && term.field.as_deref() == Some("account") {
                out.push(term.regex.clone());
            }
        }
    }
}

/// Aggregate per-commodity balances from a query result at the **posting** level.
///
/// Unlike `QueryResult.aggregated_balance` — which pulls from parse-time
/// per-account totals and therefore reflects all-time ledger state — this walks
/// the filtered transactions in `result` and sums their postings. This means
/// `date:` / `commodity:` / `payee:` search filters propagate through to the
/// balance output, which is essential for point-in-time and scoped balance
/// queries (e.g. `--balances date:<=2026-06-30`).
///
/// When `search` contains `account:` filters, only postings whose account
/// matches contribute, so a scope like `account:assets:crypto` gives the
/// balance held in that scope rather than the balance of every posting in
/// transactions that touch it.
pub fn aggregate_posting_balances(
    result: &QueryResult,
    search: &SearchExpr,
    min_value: Option<&MinValueFilter>,
) -> Vec<CommodityAmount> {
    let account_filters = extract_account_filters(search);
    let mut by_commodity: BTreeMap<String, f64> = BTreeMap::new();
    for txn in &result.transactions {
        for posting in &txn.postings {
            let keep = account_filters.is_empty()
                || account_filters
                    .iter()
                    .any(|r| r.is_match(&posting.account));
            if keep {
                *by_commodity
                    .entry(posting.commodity.clone())
                    .or_insert(0.0) += posting.amount;
            }
        }
    }
    let items: Vec<CommodityAmount> = by_commodity
        .into_iter()
        .map(|(commodity, amount)| CommodityAmount { commodity, amount })
        .collect();
    match min_value {
        Some(filter) => filter_commodities(items, filter),
        None => items,
    }
}

// ===========================================================================
// Min-value (spam) filter
// ===========================================================================

/// Drops balances whose `|amount| × latest_price` in `currency` is below
/// `threshold`. Commodities with no available price → treated as value 0
/// → dropped. Used by `arimalo-query --min-value-usd N` for spam filtering.
pub struct MinValueFilter {
    pub threshold: f64,
    pub currency: String,
    pub price_graph: Arc<PriceGraph>,
}

impl MinValueFilter {
    fn keeps(&self, commodity: &str, amount: f64) -> bool {
        self.price_graph
            .convert_to_base_latest(commodity, amount.abs(), &self.currency)
            .map(|v| v >= self.threshold)
            .unwrap_or(false)
    }
}

fn filter_commodities(
    items: Vec<CommodityAmount>,
    filter: &MinValueFilter,
) -> Vec<CommodityAmount> {
    items
        .into_iter()
        .filter(|c| filter.keeps(&c.commodity, c.amount))
        .collect()
}

fn filter_account_balances(
    balances: Vec<AccountBalance>,
    filter: &MinValueFilter,
) -> Vec<AccountBalance> {
    balances
        .into_iter()
        .filter_map(|b| {
            let totals = filter_commodities(b.totals, filter);
            if totals.is_empty() {
                None
            } else {
                Some(AccountBalance {
                    account: b.account,
                    totals,
                })
            }
        })
        .collect()
}

// ===========================================================================
// High-level query
// ===========================================================================

/// Options for a full query: search expression, sort, offset/limit pagination.
pub struct QueryOptions {
    pub search: SearchExpr,
    pub sort_field: Option<SortField>,
    pub sort_order: SortOrder,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
    pub input_order: Option<InputOrder>,
    pub min_value: Option<MinValueFilter>,
    /// Hidden-account prefixes to surface ("Show Ignored" on). When non-empty,
    /// a transaction whose postings are all hidden still matches if it satisfies
    /// the search with its `account:` terms treated as in-scope — folder scoping
    /// already constrains it to the viewed account, so only the account predicate
    /// is relaxed (date/amount/etc. still apply). Empty = hidden rows excluded.
    /// Balance aggregation always uses the original `search`, so totals are
    /// unaffected by this flag.
    pub hidden_prefixes: Vec<String>,
}

/// Run a full query: filter by search expression, sort, limit, aggregate balances.
pub fn query(parse: &ParseResult, opts: &QueryOptions) -> QueryResult {
    // Build the per-query context once so classification predicates
    // (`is:swap`, etc.) don't re-scan the full set per transaction.
    let ctx = SearchContext::from_transactions(&parse.transactions);

    // Filter transactions
    let mut transactions: Vec<Transaction> = parse
        .transactions
        .iter()
        .filter(|t| {
            if matches_search_with_context(&opts.search, t, &ctx) {
                return true;
            }
            // Show Ignored: a hidden transaction still in the scoped folder
            // reappears if it satisfies the search with its account predicate
            // relaxed (folder scoping already proved folder membership).
            !opts.hidden_prefixes.is_empty()
                && has_hidden_posting(t, &opts.hidden_prefixes)
                && matches_search_relaxing_account(&opts.search, t, &ctx)
        })
        .cloned()
        .collect();

    let transaction_count = transactions.len();

    // Sort (default: date descending — most recent first)
    let field = opts.sort_field.unwrap_or(SortField::Date);
    let order = if opts.sort_field.is_some() {
        opts.sort_order
    } else {
        SortOrder::Desc
    };
    match (opts.input_order, field, order) {
        (Some(InputOrder::DateDesc), SortField::Date, SortOrder::Desc) => {
            // Results are already sorted newest-first by the scoped loader.
        }
        (Some(InputOrder::DateDesc), SortField::Date, SortOrder::Asc) => {
            transactions.reverse();
        }
        _ => sort_transactions(&mut transactions, field, order),
    }

    // Offset + Limit (pagination)
    if let Some(offset) = opts.offset {
        if offset < transactions.len() {
            transactions = transactions.split_off(offset);
        } else {
            transactions.clear();
        }
    }
    if let Some(limit) = opts.limit {
        transactions.truncate(limit);
    }

    // Filter balances: if the search contains account terms, use them to filter
    // balances directly. Otherwise collect accounts from matched transactions.
    let account_regexes = extract_account_filters(&opts.search);
    let balances: Vec<AccountBalance> = if !account_regexes.is_empty() {
        parse
            .balances
            .iter()
            .filter(|b| account_regexes.iter().any(|r| r.is_match(&b.account)))
            .cloned()
            .collect()
    } else {
        let mut acct_set = std::collections::BTreeSet::new();
        for t in &transactions {
            for p in &t.postings {
                acct_set.insert(p.account.clone());
            }
        }
        parse
            .balances
            .iter()
            .filter(|b| acct_set.contains(&b.account))
            .cloned()
            .collect()
    };

    let accounts: Vec<String> = balances.iter().map(|b| b.account.clone()).collect();

    let mut merged: BTreeMap<String, f64> = BTreeMap::new();
    for b in &balances {
        for t in &b.totals {
            *merged.entry(t.commodity.clone()).or_default() += t.amount;
        }
    }
    let aggregated_balance: Vec<CommodityAmount> = merged
        .into_iter()
        .map(|(commodity, amount)| CommodityAmount { commodity, amount })
        .collect();

    let (balances, aggregated_balance, accounts) = match opts.min_value.as_ref() {
        Some(filter) => {
            let balances = filter_account_balances(balances, filter);
            let aggregated_balance = filter_commodities(aggregated_balance, filter);
            let accounts = balances.iter().map(|b| b.account.clone()).collect();
            (balances, aggregated_balance, accounts)
        }
        None => (balances, aggregated_balance, accounts),
    };

    QueryResult {
        transactions,
        balances,
        aggregated_balance,
        accounts,
        transaction_count,
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger_parser::{Posting, Transaction};

    fn mk_txn(
        date: &str,
        narration: &str,
        amount: f64,
        account: &str,
        commodity: &str,
    ) -> Transaction {
        Transaction {
            date: date.to_string(),
            datetime: date.to_string(),
            status: None,
            payee: None,
            narration: Some(narration.to_string()),
            meta: None,
            display_payee: None,
            amount,
            amount_commodity: commodity.to_string(),
            display_amount_commodity: None,
            fee: None,
            fee_commodity: None,
            postings: vec![Posting {
                account: account.to_string(),
                amount,
                amount_text: amount.to_string(),
                commodity: commodity.to_string(),
                remainder: None,
                cost: None,
                price: None,
            }],
        }
    }

    #[test]
    fn empty_search_matches_all() {
        let expr = parse_search("").unwrap();
        let txn = mk_txn("2025-01-01", "test", 100.0, "assets:bank", "USD");
        assert!(matches_search(&expr, &txn));
    }

    #[test]
    fn account_field_search() {
        let expr = parse_search("account:crypto").unwrap();
        let yes = mk_txn("2025-01-01", "buy", 100.0, "assets:crypto:btc", "BTC");
        let no = mk_txn("2025-01-01", "interest", 5.0, "assets:bank", "USD");
        assert!(matches_search(&expr, &yes));
        assert!(!matches_search(&expr, &no));
    }

    #[test]
    fn amount_gt_condition() {
        let expr = parse_search("amount:>50").unwrap();
        let big = mk_txn("2025-01-01", "buy", 100.0, "assets:bank", "USD");
        let small = mk_txn("2025-01-01", "fee", 5.0, "assets:bank", "USD");
        assert!(matches_search(&expr, &big));
        assert!(!matches_search(&expr, &small));
    }

    #[test]
    fn amount_lt_condition() {
        let expr = parse_search("amount:<10").unwrap();
        let small = mk_txn("2025-01-01", "fee", 5.0, "assets:bank", "USD");
        let big = mk_txn("2025-01-01", "buy", 100.0, "assets:bank", "USD");
        assert!(matches_search(&expr, &small));
        assert!(!matches_search(&expr, &big));
    }

    #[test]
    fn and_or_operators() {
        let expr = parse_search("narration:buy OR narration:sell").unwrap();
        let buy = mk_txn("2025-01-01", "buy BTC", 100.0, "assets:bank", "USD");
        let sell = mk_txn("2025-01-01", "sell ETH", 50.0, "assets:bank", "USD");
        let hold = mk_txn("2025-01-01", "hold", 0.0, "assets:bank", "USD");
        assert!(matches_search(&expr, &buy));
        assert!(matches_search(&expr, &sell));
        assert!(!matches_search(&expr, &hold));
    }

    #[test]
    fn negation() {
        let expr = parse_search("-narration:buy").unwrap();
        let buy = mk_txn("2025-01-01", "buy BTC", 100.0, "assets:bank", "USD");
        let sell = mk_txn("2025-01-01", "sell ETH", 50.0, "assets:bank", "USD");
        assert!(!matches_search(&expr, &buy));
        assert!(matches_search(&expr, &sell));
    }

    #[test]
    fn free_text_search() {
        let expr = parse_search("Interest").unwrap();
        let yes = mk_txn("2025-01-01", "Interest payment", 5.0, "assets:bank", "USD");
        let no = mk_txn("2025-01-01", "Buy BTC", 100.0, "assets:crypto", "BTC");
        assert!(matches_search(&expr, &yes));
        assert!(!matches_search(&expr, &no));
    }

    #[test]
    fn sort_by_amount_desc() {
        let mut txns = vec![
            mk_txn("2025-01-01", "a", 100.0, "x", "USD"),
            mk_txn("2025-01-02", "b", 500.0, "x", "USD"),
            mk_txn("2025-01-03", "c", 200.0, "x", "USD"),
        ];
        sort_transactions(&mut txns, SortField::Amount, SortOrder::Desc);
        assert_eq!(txns[0].amount, 500.0);
        assert_eq!(txns[1].amount, 200.0);
        assert_eq!(txns[2].amount, 100.0);
    }

    #[test]
    fn sort_by_date_asc() {
        let mut txns = vec![
            mk_txn("2025-01-15", "b", 0.0, "x", "USD"),
            mk_txn("2025-01-05", "a", 0.0, "x", "USD"),
            mk_txn("2025-01-25", "c", 0.0, "x", "USD"),
        ];
        sort_transactions(&mut txns, SortField::Date, SortOrder::Asc);
        assert_eq!(txns[0].date, "2025-01-05");
        assert_eq!(txns[1].date, "2025-01-15");
        assert_eq!(txns[2].date, "2025-01-25");
    }

    #[test]
    fn unknown_field_error() {
        let result = parse_search("bogus:foo");
        assert!(result.is_err());
    }

    fn mk_parse(txns: Vec<Transaction>) -> ParseResult {
        ParseResult {
            ok: true,
            diagnostics: Vec::new(),
            transactions: txns,
            balances: Vec::new(),
            accounts_with_opening: Vec::new(),
            account_properties: std::collections::HashMap::new(),
        }
    }

    /// A fully-hidden transaction: both legs land on `ignore:hidden`, the shape
    /// the hide rule produces, so no leg references the original account.
    fn mk_hidden_txn(date: &str, narration: &str, amount: f64) -> Transaction {
        let mut t = mk_txn(date, narration, amount, "ignore:hidden", "AUD");
        t.postings.push(Posting {
            account: "ignore:hidden".to_string(),
            amount: -amount,
            amount_text: format!("{}", -amount),
            commodity: "AUD".to_string(),
            remainder: None,
            cost: None,
            price: None,
        });
        t
    }

    fn scoped_opts(search: &str, hidden_prefixes: Vec<String>) -> QueryOptions {
        QueryOptions {
            search: parse_search(search).unwrap(),
            sort_field: None,
            sort_order: SortOrder::Asc,
            offset: None,
            limit: None,
            input_order: None,
            min_value: None,
            hidden_prefixes,
        }
    }

    #[test]
    fn show_ignored_reveals_scoped_hidden_rows_only_when_enabled() {
        let parse = mk_parse(vec![
            mk_txn("2025-01-15", "Coffee Shop", -4.5, "assets:savings", "AUD"),
            mk_hidden_txn("2025-01-20", "Junk Airdrop", 100.0),
        ]);

        // Off: the hidden row has no leg on the account, so it stays out.
        let off = query(&parse, &scoped_opts("account:assets:savings", Vec::new()));
        assert_eq!(off.transaction_count, 1);
        assert_eq!(off.transactions[0].narration.as_deref(), Some("Coffee Shop"));

        // On: the fully-hidden row in the same scope reappears.
        let on = query(
            &parse,
            &scoped_opts("account:assets:savings", vec!["ignore:".to_string()]),
        );
        assert_eq!(on.transaction_count, 2);
        assert!(on
            .transactions
            .iter()
            .any(|t| t.narration.as_deref() == Some("Junk Airdrop")));
    }

    #[test]
    fn show_ignored_still_honours_non_account_predicates() {
        let parse = mk_parse(vec![
            mk_hidden_txn("2025-01-20", "Old Junk", 100.0),
            mk_hidden_txn("2025-03-20", "New Junk", 200.0),
        ]);
        // A date predicate must still filter hidden rows; only the account
        // predicate is relaxed for them.
        let on = query(
            &parse,
            &scoped_opts(
                "account:assets:savings AND date:>=2025-02-01",
                vec!["ignore:".to_string()],
            ),
        );
        assert_eq!(on.transaction_count, 1);
        assert_eq!(on.transactions[0].narration.as_deref(), Some("New Junk"));
    }

    #[test]
    fn query_preserves_date_desc_input_order_when_hint_is_present() {
        let parse = mk_parse(vec![
            mk_txn("2025-01-25", "newest", 3.0, "assets:cash", "USD"),
            mk_txn("2025-01-15", "middle", 2.0, "assets:cash", "USD"),
            mk_txn("2025-01-05", "oldest", 1.0, "assets:cash", "USD"),
        ]);
        let result = query(
            &parse,
            &QueryOptions {
                search: parse_search("").unwrap(),
                sort_field: Some(SortField::Date),
                sort_order: SortOrder::Desc,
                offset: Some(1),
                limit: Some(1),
                input_order: Some(InputOrder::DateDesc),
                min_value: None,
                hidden_prefixes: Vec::new(),
            },
        );
        assert_eq!(result.transaction_count, 3);
        assert_eq!(result.transactions.len(), 1);
        assert_eq!(result.transactions[0].date, "2025-01-15");
    }

    #[test]
    fn query_reverses_date_desc_input_order_for_date_asc() {
        let parse = mk_parse(vec![
            mk_txn("2025-01-25", "newest", 3.0, "assets:cash", "USD"),
            mk_txn("2025-01-15", "middle", 2.0, "assets:cash", "USD"),
            mk_txn("2025-01-05", "oldest", 1.0, "assets:cash", "USD"),
        ]);
        let result = query(
            &parse,
            &QueryOptions {
                search: parse_search("").unwrap(),
                sort_field: Some(SortField::Date),
                sort_order: SortOrder::Asc,
                offset: None,
                limit: Some(2),
                input_order: Some(InputOrder::DateDesc),
                min_value: None,
                hidden_prefixes: Vec::new(),
            },
        );
        assert_eq!(result.transaction_count, 3);
        assert_eq!(
            result
                .transactions
                .iter()
                .map(|t| t.date.as_str())
                .collect::<Vec<_>>(),
            vec!["2025-01-05", "2025-01-15",]
        );
    }

    #[test]
    fn query_still_globally_sorts_non_date_fields_with_hint() {
        let parse = mk_parse(vec![
            mk_txn("2025-01-25", "middle", 20.0, "assets:cash", "USD"),
            mk_txn("2025-01-15", "largest", 30.0, "assets:cash", "USD"),
            mk_txn("2025-01-05", "smallest", 10.0, "assets:cash", "USD"),
        ]);
        let result = query(
            &parse,
            &QueryOptions {
                search: parse_search("").unwrap(),
                sort_field: Some(SortField::Amount),
                sort_order: SortOrder::Desc,
                offset: Some(1),
                limit: Some(1),
                input_order: Some(InputOrder::DateDesc),
                min_value: None,
                hidden_prefixes: Vec::new(),
            },
        );
        assert_eq!(result.transaction_count, 3);
        assert_eq!(result.transactions.len(), 1);
        assert_eq!(result.transactions[0].amount, 20.0);
    }

    #[test]
    fn commodity_exact_match() {
        let expr = parse_search("commodity:BTC").unwrap();
        let yes = mk_txn("2025-01-01", "buy", 1.0, "assets:crypto", "BTC");
        let no = mk_txn("2025-01-01", "buy", 1.0, "assets:crypto", "BTCX");
        assert!(matches_search(&expr, &yes));
        assert!(!matches_search(&expr, &no));
    }

    #[test]
    fn commodity_glob_match() {
        let expr = parse_search("commodity:*BTC*").unwrap();
        let yes = mk_txn("2025-01-01", "buy", 1.0, "assets:crypto", "WBTC");
        let no = mk_txn("2025-01-01", "buy", 1.0, "assets:crypto", "ETH");
        assert!(matches_search(&expr, &yes));
        assert!(!matches_search(&expr, &no));
    }

    #[test]
    fn date_gte() {
        let expr = parse_search("date:>=2025-01-15").unwrap();
        let yes = mk_txn("2025-01-15", "a", 0.0, "x", "USD");
        let also_yes = mk_txn("2025-02-01", "b", 0.0, "x", "USD");
        let no = mk_txn("2025-01-10", "c", 0.0, "x", "USD");
        assert!(matches_search(&expr, &yes));
        assert!(matches_search(&expr, &also_yes));
        assert!(!matches_search(&expr, &no));
    }

    #[test]
    fn date_lt() {
        let expr = parse_search("date:<2025-01-15").unwrap();
        let yes = mk_txn("2025-01-10", "a", 0.0, "x", "USD");
        let no = mk_txn("2025-01-15", "b", 0.0, "x", "USD");
        assert!(matches_search(&expr, &yes));
        assert!(!matches_search(&expr, &no));
    }

    #[test]
    fn date_range() {
        let expr = parse_search("date:2025-01-10..2025-01-20").unwrap();
        let yes = mk_txn("2025-01-15", "a", 0.0, "x", "USD");
        let before = mk_txn("2025-01-05", "b", 0.0, "x", "USD");
        let after = mk_txn("2025-01-25", "c", 0.0, "x", "USD");
        let edge_start = mk_txn("2025-01-10", "d", 0.0, "x", "USD");
        let edge_end = mk_txn("2025-01-20", "e", 0.0, "x", "USD");
        assert!(matches_search(&expr, &yes));
        assert!(!matches_search(&expr, &before));
        assert!(!matches_search(&expr, &after));
        assert!(matches_search(&expr, &edge_start));
        assert!(matches_search(&expr, &edge_end));
    }

    #[test]
    fn date_regex_still_works() {
        // Plain date:2025-01 should still regex-match any date starting with 2025-01
        let expr = parse_search("date:2025-01").unwrap();
        let yes = mk_txn("2025-01-15", "a", 0.0, "x", "USD");
        let no = mk_txn("2025-02-01", "b", 0.0, "x", "USD");
        assert!(matches_search(&expr, &yes));
        assert!(!matches_search(&expr, &no));
    }

    fn mk_filter(threshold: f64, prices: Vec<(&str, f64, &str)>) -> MinValueFilter {
        use crate::ledger_parser::PriceDirective;
        let entries = prices
            .into_iter()
            .map(|(c, p, q)| PriceDirective {
                datetime: "2026-01-01".into(),
                commodity: c.into(),
                price_amount: p,
                price_amount_text: p.to_string(),
                quote_commodity: q.into(),
            })
            .collect();
        MinValueFilter {
            threshold,
            currency: "USD".into(),
            price_graph: Arc::new(PriceGraph::from_entries(entries)),
        }
    }

    #[test]
    fn min_value_filter_drops_unpriced_and_low_value_keeps_high_value() {
        let filter = mk_filter(
            10.0,
            vec![("BTC", 50_000.0, "USD"), ("USDC", 1.0, "USD")],
        );
        let items = vec![
            CommodityAmount {
                commodity: "BTC".into(),
                amount: 0.001,
            }, // 0.001 × 50k = 50 USD → keep
            CommodityAmount {
                commodity: "USDC".into(),
                amount: 5.0,
            }, // 5 USD → drop (< 10)
            CommodityAmount {
                commodity: "USDC".into(),
                amount: 100.0,
            }, // 100 USD → keep
            CommodityAmount {
                commodity: "FRUIT".into(),
                amount: 999_000_000.0,
            }, // no price → drop
            CommodityAmount {
                commodity: "BTC".into(),
                amount: -0.001,
            }, // |−0.001| × 50k = 50 USD → keep (negatives count by magnitude)
        ];
        let kept = filter_commodities(items, &filter);
        let symbols: Vec<&str> = kept.iter().map(|c| c.commodity.as_str()).collect();
        assert_eq!(symbols, vec!["BTC", "USDC", "BTC"]);
    }

    #[test]
    fn query_applies_min_value_to_aggregated_balance_and_drops_empty_account_balances() {
        use crate::ledger_parser::AccountBalance;
        let mut parse = mk_parse(vec![mk_txn(
            "2026-01-01",
            "buy",
            1.0,
            "assets:crypto:btc",
            "BTC",
        )]);
        parse.balances = vec![
            AccountBalance {
                account: "assets:crypto:btc".into(),
                totals: vec![
                    CommodityAmount {
                        commodity: "BTC".into(),
                        amount: 0.001,
                    }, // 50 USD → keep
                    CommodityAmount {
                        commodity: "FRUIT".into(),
                        amount: 999.0,
                    }, // unpriced → drop
                ],
            },
            AccountBalance {
                account: "assets:spam".into(),
                totals: vec![CommodityAmount {
                    commodity: "FRUIT".into(),
                    amount: 1.0,
                }], // entire account drops out
            },
        ];

        let filter = mk_filter(10.0, vec![("BTC", 50_000.0, "USD")]);
        let result = query(
            &parse,
            &QueryOptions {
                search: parse_search("account:assets:crypto").unwrap(),
                sort_field: None,
                sort_order: SortOrder::Asc,
                offset: None,
                limit: None,
                input_order: None,
                min_value: Some(filter),
                hidden_prefixes: Vec::new(),
            },
        );

        assert_eq!(result.aggregated_balance.len(), 1);
        assert_eq!(result.aggregated_balance[0].commodity, "BTC");
        // assets:spam is excluded by the search filter; assets:crypto:btc keeps BTC and drops FRUIT
        assert_eq!(result.balances.len(), 1);
        assert_eq!(result.balances[0].account, "assets:crypto:btc");
        assert_eq!(result.balances[0].totals.len(), 1);
        assert_eq!(result.balances[0].totals[0].commodity, "BTC");
    }

    fn mk_txn_with_meta(
        date: &str,
        narration: &str,
        amount: f64,
        account: &str,
        commodity: &str,
        meta: Option<&str>,
    ) -> Transaction {
        let mut t = mk_txn(date, narration, amount, account, commodity);
        t.meta = meta.map(String::from);
        t
    }

    #[test]
    fn is_swap_matches_two_leg_shared_hash_groups() {
        let txns = vec![
            mk_txn_with_meta("2025-01-01", "sell", -1.0, "assets:eth", "ETH", Some("txn:abc")),
            mk_txn_with_meta("2025-01-01", "buy", 3000.0, "assets:usd", "USD", Some("txn:abc")),
            mk_txn_with_meta("2025-01-02", "deposit", 5.0, "assets:bank", "USD", None),
        ];
        let ctx = SearchContext::from_transactions(&txns);
        let expr = parse_search("is:swap").unwrap();
        let matched: Vec<_> = txns
            .iter()
            .filter(|t| matches_search_with_context(&expr, t, &ctx))
            .collect();
        assert_eq!(matched.len(), 2);
        assert!(matched.iter().all(|t| t.meta.as_deref() == Some("txn:abc")));
    }

    #[test]
    fn is_swap_matches_three_leg_shared_hash_groups() {
        let txns = vec![
            mk_txn_with_meta("2025-01-01", "sol", -1.0, "assets:sol", "SOL", Some("txn:lp1")),
            mk_txn_with_meta("2025-01-01", "usdc", -10.0, "assets:usdc", "USDC", Some("txn:lp1")),
            mk_txn_with_meta("2025-01-01", "lp", 5.0, "assets:lp", "SOLUSDC", Some("txn:lp1")),
            mk_txn_with_meta("2025-01-02", "lone", 1.0, "assets:misc", "X", Some("txn:lone")),
        ];
        let ctx = SearchContext::from_transactions(&txns);
        let expr = parse_search("is:swap").unwrap();
        let matched: Vec<_> = txns
            .iter()
            .filter(|t| matches_search_with_context(&expr, t, &ctx))
            .collect();
        assert_eq!(matched.len(), 3);
    }

    #[test]
    fn is_swap_excludes_lone_and_4plus_groups() {
        // 1 standalone, 4-leg (out of bracket scope), and 2-leg (in scope).
        let txns = vec![
            mk_txn_with_meta("2025-01-01", "lone", 1.0, "assets:a", "X", Some("txn:lone")),
            mk_txn_with_meta("2025-01-02", "p1", 1.0, "assets:a", "X", Some("txn:big")),
            mk_txn_with_meta("2025-01-02", "p2", 1.0, "assets:b", "Y", Some("txn:big")),
            mk_txn_with_meta("2025-01-02", "p3", 1.0, "assets:c", "Z", Some("txn:big")),
            mk_txn_with_meta("2025-01-02", "p4", 1.0, "assets:d", "W", Some("txn:big")),
            mk_txn_with_meta("2025-01-03", "two-a", -1.0, "assets:e", "ETH", Some("txn:two")),
            mk_txn_with_meta("2025-01-03", "two-b", 100.0, "assets:f", "USD", Some("txn:two")),
        ];
        let ctx = SearchContext::from_transactions(&txns);
        let expr = parse_search("is:swap").unwrap();
        let matched: Vec<_> = txns
            .iter()
            .filter(|t| matches_search_with_context(&expr, t, &ctx))
            .collect();
        assert_eq!(matched.len(), 2);
        assert!(matched.iter().all(|t| t.meta.as_deref() == Some("txn:two")));
    }

    #[test]
    fn is_swap_combines_with_other_filters_via_and() {
        let txns = vec![
            mk_txn_with_meta("2025-01-01", "buy", -1.0, "assets:eth", "ETH", Some("txn:k1")),
            mk_txn_with_meta("2025-01-01", "buy", 100.0, "assets:usd", "USD", Some("txn:k1")),
            mk_txn_with_meta("2025-01-02", "buy", -1.0, "assets:btc", "BTC", Some("txn:k2")),
            mk_txn_with_meta("2025-01-02", "buy", 50000.0, "assets:usd", "USD", Some("txn:k2")),
        ];
        let ctx = SearchContext::from_transactions(&txns);
        let expr = parse_search("is:swap AND commodity:ETH").unwrap();
        let matched: Vec<_> = txns
            .iter()
            .filter(|t| matches_search_with_context(&expr, t, &ctx))
            .collect();
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].amount_commodity, "ETH");
    }

    #[test]
    fn is_swap_rejects_three_leg_with_duplicate_commodity() {
        // Phantom 3-leg: two USDC postings + one HNT. Not a real swap because
        // distinct_commodities == 2, not 3.
        let txns = vec![
            mk_txn_with_meta("2025-01-01", "p1", 5.0, "assets:a", "USDC", Some("txn:dup3")),
            mk_txn_with_meta("2025-01-01", "p2", -3.0, "assets:b", "USDC", Some("txn:dup3")),
            mk_txn_with_meta("2025-01-01", "p3", 1.0, "assets:c", "HNT", Some("txn:dup3")),
        ];
        let ctx = SearchContext::from_transactions(&txns);
        let expr = parse_search("is:swap").unwrap();
        let matched: Vec<_> = txns
            .iter()
            .filter(|t| matches_search_with_context(&expr, t, &ctx))
            .collect();
        assert_eq!(matched.len(), 0);
    }

    #[test]
    fn is_swap_three_leg_with_zero_leg_collapses_to_two_leg_swap() {
        // 3 raw legs but one has 0 amount → filtered down to 2 distinct legs,
        // which still forms a valid 2-leg swap shape. Only the two non-zero
        // legs match; the 0 leg itself is a noise posting and is excluded.
        let txns = vec![
            mk_txn_with_meta("2025-01-01", "noop", 0.0, "assets:contract", "ETH", Some("txn:lp2")),
            mk_txn_with_meta("2025-01-01", "out", -10.0, "assets:usdc", "USDC", Some("txn:lp2")),
            mk_txn_with_meta("2025-01-01", "in", 5.0, "assets:hnt", "HNT", Some("txn:lp2")),
        ];
        let ctx = SearchContext::from_transactions(&txns);
        let expr = parse_search("is:swap").unwrap();
        let matched: Vec<_> = txns
            .iter()
            .filter(|t| matches_search_with_context(&expr, t, &ctx))
            .collect();
        assert_eq!(matched.len(), 2);
        let commodities: Vec<&str> = matched.iter().map(|t| t.amount_commodity.as_str()).collect();
        assert!(commodities.contains(&"USDC"));
        assert!(commodities.contains(&"HNT"));
    }

    #[test]
    fn is_swap_rejects_two_leg_self_transfer_same_commodity() {
        // 2-leg group sharing the same commodity is a self-transfer (e.g. wallet
        // rotation), not a swap. distinct_commodities == 1.
        let txns = vec![
            mk_txn_with_meta("2025-01-01", "out", -1.0, "assets:walletA", "ETH", Some("txn:self")),
            mk_txn_with_meta("2025-01-01", "in", 1.0, "assets:walletB", "ETH", Some("txn:self")),
        ];
        let ctx = SearchContext::from_transactions(&txns);
        let expr = parse_search("is:swap").unwrap();
        let matched: Vec<_> = txns
            .iter()
            .filter(|t| matches_search_with_context(&expr, t, &ctx))
            .collect();
        assert_eq!(matched.len(), 0);
    }

    #[test]
    fn is_swap_zero_amount_leg_never_matches_even_in_valid_swap() {
        // A clean 3-leg swap exists, but a 4th 0-amount leg also carries the
        // hash. The 3 non-zero legs match; the 0 leg does not — its own amount
        // disqualifies it as a noise posting.
        let txns = vec![
            mk_txn_with_meta("2025-01-01", "sol", -1.0, "assets:sol", "SOL", Some("txn:lp3")),
            mk_txn_with_meta("2025-01-01", "usdc", -10.0, "assets:usdc", "USDC", Some("txn:lp3")),
            mk_txn_with_meta("2025-01-01", "lp", 5.0, "assets:lp", "SOLUSDC", Some("txn:lp3")),
            mk_txn_with_meta("2025-01-01", "noop", 0.0, "assets:contract", "ETH", Some("txn:lp3")),
        ];
        let ctx = SearchContext::from_transactions(&txns);
        let expr = parse_search("is:swap").unwrap();
        let matched: Vec<_> = txns
            .iter()
            .filter(|t| matches_search_with_context(&expr, t, &ctx))
            .collect();
        assert_eq!(matched.len(), 3);
        let commodities: Vec<&str> = matched.iter().map(|t| t.amount_commodity.as_str()).collect();
        assert!(commodities.contains(&"SOL"));
        assert!(commodities.contains(&"USDC"));
        assert!(commodities.contains(&"SOLUSDC"));
        assert!(!commodities.contains(&"ETH"));
    }

    #[test]
    fn is_swap_treats_subepsilon_amount_as_zero() {
        // Floating-point rounding artifact below the 1e-9 epsilon should be
        // treated as a noise leg and not inflate the group shape.
        let txns = vec![
            mk_txn_with_meta("2025-01-01", "dust", 1e-12, "assets:a", "USDC", Some("txn:dust")),
            mk_txn_with_meta("2025-01-01", "out", -10.0, "assets:b", "ETH", Some("txn:dust")),
            mk_txn_with_meta("2025-01-01", "in", 1.0, "assets:c", "ETH", Some("txn:dust")),
        ];
        let ctx = SearchContext::from_transactions(&txns);
        let expr = parse_search("is:swap").unwrap();
        let matched: Vec<_> = txns
            .iter()
            .filter(|t| matches_search_with_context(&expr, t, &ctx))
            .collect();
        // After filtering the dust leg, the group is 2 ETH legs sharing one
        // commodity → not a swap (self-transfer shape).
        assert_eq!(matched.len(), 0);
    }
}
