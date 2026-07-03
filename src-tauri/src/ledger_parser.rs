use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Diagnostic {
    pub line: usize,
    pub column: usize,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PriceAnnotation {
    pub is_total: bool,
    pub amount: f64,
    pub amount_text: String,
    pub commodity: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CostAnnotation {
    pub is_total: bool,
    pub amount: Option<f64>,
    pub amount_text: Option<String>,
    pub commodity: Option<String>,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PriceDirective {
    pub datetime: String,
    pub commodity: String,
    pub price_amount: f64,
    pub price_amount_text: String,
    pub quote_commodity: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PricesParseResult {
    pub ok: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub prices: Vec<PriceDirective>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Posting {
    pub account: String,
    pub amount: f64,
    pub amount_text: String,
    pub commodity: String,
    pub remainder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<CostAnnotation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<PriceAnnotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    pub date: String,
    pub datetime: String,
    pub status: Option<char>,
    pub payee: Option<String>,
    pub narration: Option<String>,
    pub meta: Option<String>,
    pub postings: Vec<Posting>,
    /// Display-friendly payee set by rules (raw payee is immutable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_payee: Option<String>,
    /// Primary transaction amount (from first posting).
    pub amount: f64,
    /// Commodity of the primary amount (immutable raw value from CSV).
    pub amount_commodity: String,
    /// Display-friendly commodity set by rules (raw amount_commodity is immutable).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_amount_commodity: Option<String>,
    /// Fee amount, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee: Option<f64>,
    /// Commodity of the fee, if present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_commodity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommodityAmount {
    pub commodity: String,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountBalance {
    pub account: String,
    pub totals: Vec<CommodityAmount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountProperties {
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ParseResult {
    pub ok: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub transactions: Vec<Transaction>,
    pub balances: Vec<AccountBalance>,
    pub accounts_with_opening: Vec<String>,
    pub account_properties: HashMap<String, AccountProperties>,
}

#[derive(Debug, Clone, PartialEq)]
struct AccountDeclaration {
    account: String,
    default_commodity: Option<String>,
    opening: Option<CommodityAmount>,
    name: Option<String>,
}

/// Normalise a datetime string so date-only, ISO `T...Z`, and space-separated
/// forms are lexically comparable. Used by `PriceGraph` when sorting prices
/// and resolving the nearest-prior price at a target datetime.
///
/// - `"YYYY-MM-DD"` → `"YYYY-MM-DD 00:00:00"` (treated as start-of-day)
/// - `"YYYY-MM-DDTHH:MM:SS..."` → `"YYYY-MM-DD HH:MM:SS"` (T → space, fractional seconds and timezone dropped)
/// - `"YYYY-MM-DD HH:MM:SS"` → unchanged
fn normalize_datetime_for_compare(s: &str) -> String {
    let s = s.trim();
    if s.len() == 10 {
        return format!("{s} 00:00:00");
    }
    if s.len() > 10 && s.as_bytes()[10] == b'T' {
        let date = &s[..10];
        let after_t = &s[11..];
        let time: String = after_t
            .chars()
            .take_while(|c| *c != '.' && *c != 'Z' && *c != '+')
            .collect();
        return format!("{date} {time}");
    }
    if s.len() > 19 {
        return s[..19].to_string();
    }
    s.to_string()
}

fn diag(line: usize, column: usize, message: impl Into<String>) -> Diagnostic {
    Diagnostic {
        line,
        column,
        message: message.into(),
        file: None,
    }
}

fn is_blank(line: &str) -> bool {
    line.trim().is_empty()
}

fn is_directive(line: &str) -> bool {
    let trimmed = line.trim_start_matches([' ', '\t']);
    trimmed.starts_with(';')
}

static HEADER_DATETIME_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    // Accepts both ISO T-separator (2022-08-15T14:30:00Z) and space separator (2022-08-15 14:30:00)
    Regex::new(
        r"^(?P<date>\d{4}-\d{2}-\d{2})(?P<time>[T ]\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|[+-]\d{2}:\d{2})?)?\s+",
    )
    .expect("datetime regex")
});

static ACCOUNT_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    // Allow 2+ segments (e.g. `expenses:unknown`) for usability.
    // \p{L} = Unicode letter, \p{N} = Unicode digit.
    Regex::new(r"^[\p{L}\p{N}_.\-]+:[\p{L}\p{N}_.\-]+(?::[\p{L}\p{N}_.\-]+)*$")
        .expect("account regex")
});

static AMOUNT_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"^[+-]?\d+(?:\.\d+)?$").expect("amount regex")
});

static COMMODITY_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    // \p{L} = Unicode letter, \p{N} = Unicode digit.
    Regex::new(r"^[\p{L}\p{N}_][\p{L}\p{N}_.\-]*$").expect("commodity regex")
});

fn header_datetime_re() -> &'static Regex { &HEADER_DATETIME_RE }
fn account_re() -> &'static Regex { &ACCOUNT_RE }
fn amount_re() -> &'static Regex { &AMOUNT_RE }
fn commodity_re() -> &'static Regex { &COMMODITY_RE }

fn take_token(input: &str) -> Option<(String, &str)> {
    let trimmed = input.trim_start_matches([' ', '\t']);
    if trimmed.is_empty() {
        return None;
    }

    if let Some(quoted) = trimmed.strip_prefix('"') {
        let mut chars = quoted.chars();
        let mut out = String::new();
        while let Some(c) = chars.next() {
            match c {
                '\\' => {
                    if let Some(next) = chars.next() {
                        out.push(next);
                    }
                }
                '"' => {
                    let rest = chars.as_str();
                    return Some((out, rest));
                }
                c => out.push(c),
            }
        }

        // Unterminated quote; treat the entire thing as a single token (minus the opening quote).
        return Some((quoted.to_string(), ""));
    }

    let end = trimmed.find([' ', '\t']).unwrap_or(trimmed.len());
    Some((trimmed[..end].to_string(), &trimmed[end..]))
}

/// Extract a named field (e.g. "source_payee") from a comma-separated meta string.
/// Returns (remaining meta without that field, extracted value).
fn extract_meta_field(meta: &Option<String>, key: &str) -> (Option<String>, Option<String>) {
    let meta_str = match meta {
        Some(s) => s,
        None => return (None, None),
    };
    let prefix = format!("{key}:");
    let parts: Vec<&str> = meta_str.split(',').map(|s| s.trim()).collect();
    let mut value = None;
    let mut remaining = Vec::new();
    for part in parts {
        if part.starts_with(&prefix) {
            value = Some(part[prefix.len()..].to_string());
        } else {
            remaining.push(part);
        }
    }
    let clean = remaining.join(", ");
    let clean_meta = if clean.is_empty() { None } else { Some(clean) };
    (clean_meta, value)
}

fn parse_header_fields(
    header_after_datetime: &str,
) -> (Option<char>, Option<String>, Option<String>) {
    let mut rest = header_after_datetime;
    let mut tokens: Vec<String> = Vec::new();
    while let Some((tok, next)) = take_token(rest) {
        tokens.push(tok);
        rest = next;
    }

    let mut idx = 0;
    let status = tokens.first().and_then(|t| match t.as_str() {
        "*" => {
            idx = 1;
            Some('*')
        }
        "!" => {
            idx = 1;
            Some('!')
        }
        _ => None,
    });

    let payee = tokens.get(idx).cloned();
    let narration = match tokens.get(idx + 1..) {
        Some([]) | None => None,
        Some([only]) => Some(only.clone()),
        Some(many) => Some(many.join(" ")),
    };

    (status, payee, narration)
}

fn flush_transaction(
    current: &mut Option<(usize, Transaction)>,
    diagnostics: &mut Vec<Diagnostic>,
    transactions: &mut Vec<Transaction>,
) {
    if let Some((header_line, mut txn)) = current.take() {
        if txn.postings.is_empty() {
            diagnostics.push(diag(header_line, 0, "transaction missing postings"));
        }
        // Populate first-class amount/fee fields from postings
        if let Some(first) = txn.postings.first() {
            txn.amount = first.amount;
            if txn.amount_commodity.is_empty() {
                // Normal case: populate from posting commodity
                txn.amount_commodity = first.commodity.clone();
            } else {
                // Re-parsed: amount_commodity already has raw value from source_commodity meta.
                // The posting commodity is the display (effective) value.
                txn.display_amount_commodity = Some(first.commodity.clone());
            }
        }
        // Fee heuristic: 3+ postings and last account starts with "expenses:fees"
        if txn.postings.len() >= 3 {
            if let Some(last) = txn.postings.last() {
                if last.account.starts_with("expenses:fees") {
                    txn.fee = Some(last.amount);
                    txn.fee_commodity = Some(last.commodity.clone());
                }
            }
        }
        transactions.push(txn);
    }
}

pub fn parse_transactions(contents: &str) -> ParseResult {
    let header_re = header_datetime_re();
    let account_re = account_re();
    let amount_re = amount_re();
    let commodity_re = commodity_re();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut current: Option<(usize, Transaction)> = None;
    let mut current_account: Option<(usize, AccountDeclaration)> = None;
    let mut transactions: Vec<Transaction> = Vec::new();
    let mut account_declarations: Vec<AccountDeclaration> = Vec::new();

    for (idx, raw_line) in contents.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim_end_matches('\r');

        if is_blank(line) {
            flush_transaction(&mut current, &mut diagnostics, &mut transactions);
            if let Some((_, decl)) = current_account.take() {
                account_declarations.push(decl);
            }
            continue;
        }

        if is_directive(line) {
            continue;
        }

        if line == "account" || line.starts_with("account ") || line.starts_with("account\t") {
            flush_transaction(&mut current, &mut diagnostics, &mut transactions);
            if let Some((_, decl)) = current_account.take() {
                account_declarations.push(decl);
            }

            let before_meta = line.split_once(';').map(|(l, _)| l).unwrap_or(line);
            let mut parts = before_meta.split_whitespace();
            let kw = parts.next().unwrap_or_default();
            if kw != "account" {
                diagnostics.push(diag(
                    line_no,
                    0,
                    "invalid line: expected transaction header, posting, or directive",
                ));
                continue;
            }
            let Some(account) = parts.next() else {
                diagnostics.push(diag(line_no, 0, "account declaration missing account path"));
                continue;
            };

            if !account_re.is_match(account) {
                diagnostics.push(diag(line_no, 0, format!("invalid account path: {account}")));
            }

            let default_commodity = parts.next().map(|s| s.to_string());
            if let Some(c) = default_commodity.as_deref() {
                if !commodity_re.is_match(c) {
                    diagnostics.push(diag(line_no, 0, format!("invalid commodity: {c}")));
                }
            }

            if parts.next().is_some() {
                diagnostics.push(diag(
          line_no,
          0,
          "unexpected extra tokens in account declaration (expected: account <path> [COMMODITY])",
        ));
            }

            current_account = Some((
                line_no,
                AccountDeclaration {
                    account: account.to_string(),
                    default_commodity,
                    opening: None,
                    name: None,
                },
            ));
            continue;
        }

        if let Some(stripped) = line.strip_prefix("    ") {
            let Some((_, txn)) = current.as_mut() else {
                let Some((_, decl)) = current_account.as_mut() else {
                    diagnostics.push(diag(line_no, 0, "unexpected indented line"));
                    continue;
                };

                let before_meta = stripped
                    .split_once(';')
                    .map(|(l, _)| l)
                    .unwrap_or(stripped);
                let mut parts = before_meta.split_whitespace();
                let Some(kind) = parts.next() else {
                    diagnostics.push(diag(line_no, 4, "invalid account declaration line"));
                    continue;
                };

                match kind {
                    "opening" => {
                        let Some(amount_text) = parts.next() else {
                            diagnostics.push(diag(line_no, 4, "opening missing amount"));
                            continue;
                        };

                        if !amount_re.is_match(amount_text) {
                            diagnostics.push(diag(
                                line_no,
                                4,
                                format!("invalid amount: {amount_text}"),
                            ));
                        }

                        let commodity = match parts.next() {
                            Some(c) => Some(c.to_string()),
                            None => decl.default_commodity.clone(),
                        };

                        let Some(commodity) = commodity else {
                            diagnostics.push(diag(
                                line_no,
                                4,
                                "opening missing commodity and account has no default commodity",
                            ));
                            continue;
                        };

                        if !commodity_re.is_match(&commodity) {
                            diagnostics.push(diag(
                                line_no,
                                4,
                                format!("invalid commodity: {commodity}"),
                            ));
                        }

                        if parts.next().is_some() {
                            diagnostics.push(diag(
                line_no,
                4,
                "unexpected extra tokens in opening declaration (expected: opening <amount> [COMMODITY])",
              ));
                        }

                        if decl.opening.is_some() {
                            diagnostics.push(diag(line_no, 4, "duplicate opening declaration"));
                            continue;
                        }

                        let amount: f64 = amount_text.parse().unwrap_or(0.0);
                        decl.opening = Some(CommodityAmount { commodity, amount });
                    }
                    "name" => {
                        let rest_str: Vec<&str> = parts.collect();
                        if rest_str.is_empty() {
                            diagnostics.push(diag(line_no, 4, "name missing value"));
                            continue;
                        }
                        if decl.name.is_some() {
                            diagnostics.push(diag(line_no, 4, "duplicate name declaration"));
                            continue;
                        }
                        decl.name = Some(rest_str.join(" "));
                    }
                    _ => {
                        diagnostics.push(diag(
                            line_no,
                            4,
                            format!("unknown account declaration entry: {kind}"),
                        ));
                    }
                }

                continue;
            };

            if current_account.is_some() {
                diagnostics.push(diag(
                    line_no,
                    0,
                    "posting is not allowed inside an account declaration",
                ));
                continue;
            }

            let mut parts = stripped.split_whitespace();
            let account = parts.next();
            let amount = parts.next();
            let commodity = parts.next();

            if account.is_none() {
                diagnostics.push(diag(line_no, 4, "missing account"));
                continue;
            }

            let account = account.unwrap();
            if !account_re.is_match(account) {
                diagnostics.push(diag(line_no, 4, format!("invalid account path: {account}")));
            }

            if amount.is_none() {
                diagnostics.push(diag(line_no, 4 + account.len() + 1, "missing amount"));
                continue;
            }

            let amount = amount.unwrap();
            if !amount_re.is_match(amount) {
                diagnostics.push(diag(
                    line_no,
                    4 + account.len() + 1,
                    format!("invalid amount: {amount}"),
                ));
            }

            if commodity.is_none() {
                diagnostics.push(diag(
                    line_no,
                    4 + account.len() + 1 + amount.len() + 1,
                    "missing commodity",
                ));
                continue;
            }

            let commodity = commodity.unwrap();
            if !commodity_re.is_match(commodity) {
                diagnostics.push(diag(
                    line_no,
                    4 + account.len() + 1 + amount.len() + 1,
                    format!("invalid commodity: {commodity}"),
                ));
            }

            let parsed_amount: f64 = amount.parse().unwrap_or(0.0);
            let remainder_parts: Vec<&str> = parts.collect();
            let remainder = if remainder_parts.is_empty() {
                None
            } else {
                Some(remainder_parts.join(" "))
            };

            let (cost, price) = remainder
                .as_deref()
                .map(parse_remainder)
                .unwrap_or((None, None));
            txn.postings.push(Posting {
                account: account.to_string(),
                amount: parsed_amount,
                amount_text: amount.to_string(),
                commodity: commodity.to_string(),
                remainder,
                cost,
                price,
            });

            continue;
        }

        // Header line
        if let Some(caps) = header_re.captures(line) {
            flush_transaction(&mut current, &mut diagnostics, &mut transactions);
            if let Some((_, decl)) = current_account.take() {
                account_declarations.push(decl);
            }

            let date = caps
                .name("date")
                .map(|m| m.as_str().to_string())
                .unwrap_or_else(|| "0000-00-00".to_string());
            let time = caps.name("time").map(|m| m.as_str()).unwrap_or("");
            let datetime = format!("{date}{time}");

            let datetime_span = caps.get(0).expect("datetime capture");
            let datetime_end = datetime_span.end();

            let (before_meta, meta) = match line.split_once(';') {
                Some((left, right)) => (left, Some(right.trim().to_string())),
                None => (line, None),
            };

            let header_after_datetime = before_meta.get(datetime_end..).unwrap_or("");
            if header_after_datetime.trim().is_empty() {
                diagnostics.push(diag(line_no, datetime_end, "missing transaction details"));
            }

            if !line.contains(';') {
                diagnostics.push(diag(line_no, 0, "missing meta comment (expected ';')"));
            }

            let (status, payee, narration) = parse_header_fields(header_after_datetime);
            // Extract source_payee / source_commodity from meta.
            // If source_payee exists, header payee is the display name and source_payee is the raw value.
            let (clean_meta, source_payee) = extract_meta_field(&meta, "source_payee");
            let (clean_meta, source_commodity) =
                extract_meta_field(&clean_meta, "source_commodity");
            let (raw_payee, display_payee) = if let Some(src) = source_payee {
                (Some(src), payee)
            } else {
                (payee, None)
            };
            current = Some((
                line_no,
                Transaction {
                    date,
                    datetime,
                    status,
                    payee: raw_payee,
                    narration,
                    meta: clean_meta,
                    postings: Vec::new(),
                    display_payee,
                    amount: 0.0,
                    amount_commodity: source_commodity.unwrap_or_default(),
                    display_amount_commodity: None,
                    fee: None,
                    fee_commodity: None,
                },
            ));
        } else {
            diagnostics.push(diag(
                line_no,
                0,
                "invalid line: expected transaction header, posting, or directive",
            ));
        }
    }

    flush_transaction(&mut current, &mut diagnostics, &mut transactions);
    if let Some((_, decl)) = current_account.take() {
        account_declarations.push(decl);
    }

    let mut balances_by_account: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    for txn in &transactions {
        for posting in &txn.postings {
            let entry = balances_by_account
                .entry(posting.account.clone())
                .or_default()
                .entry(posting.commodity.clone())
                .or_insert(0.0);
            *entry += posting.amount;
        }
    }

    for decl in &account_declarations {
        let _ = balances_by_account.entry(decl.account.clone()).or_default();
        if let Some(opening) = &decl.opening {
            let entry = balances_by_account
                .entry(decl.account.clone())
                .or_default()
                .entry(opening.commodity.clone())
                .or_insert(0.0);
            *entry += opening.amount;
        }
    }

    let balances: Vec<AccountBalance> = balances_by_account
        .into_iter()
        .map(|(account, by_commodity)| AccountBalance {
            account,
            totals: by_commodity
                .into_iter()
                .map(|(commodity, amount)| CommodityAmount { commodity, amount })
                .collect(),
        })
        .collect();

    let accounts_with_opening: Vec<String> = account_declarations
        .iter()
        .filter(|d| d.opening.is_some())
        .map(|d| d.account.clone())
        .collect();

    let mut account_properties: HashMap<String, AccountProperties> = HashMap::new();
    for decl in &account_declarations {
        if decl.name.is_some() {
            account_properties.insert(
                decl.account.clone(),
                AccountProperties {
                    name: decl.name.clone(),
                },
            );
        }
    }

    ParseResult {
        ok: diagnostics.is_empty(),
        diagnostics,
        transactions,
        balances,
        accounts_with_opening,
        account_properties,
    }
}

/// Parse cost and price annotations from a posting's remainder text.
///
/// Cost: `{ amount commodity, field, ... }` or `{{ amount commodity, field, ... }}`
/// Price: `@ amount commodity` or `@@ amount commodity`
fn parse_remainder(remainder: &str) -> (Option<CostAnnotation>, Option<PriceAnnotation>) {
    let mut cost: Option<CostAnnotation> = None;
    let mut price: Option<PriceAnnotation> = None;
    let mut text_after_cost = remainder;

    // --- Cost extraction ---
    // Look for {{ ... }} (total cost) or { ... } (per-unit cost)
    if let Some(start) = remainder.find("{{") {
        if let Some(end) = remainder[start + 2..].find("}}") {
            let body = &remainder[start + 2..start + 2 + end];
            cost = Some(parse_cost_body(body, true));
            text_after_cost = &remainder[start + 2 + end + 2..];
        }
    } else if let Some(start) = remainder.find('{') {
        if let Some(end) = remainder[start + 1..].find('}') {
            let body = &remainder[start + 1..start + 1 + end];
            cost = Some(parse_cost_body(body, false));
            text_after_cost = &remainder[start + 1 + end + 1..];
        }
    }

    // --- Price extraction ---
    // Look for @@ (total price) or @ (per-unit price)
    let trimmed = text_after_cost.trim();
    if let Some(pos) = trimmed.find("@@") {
        let after = trimmed[pos + 2..].trim();
        price = parse_price_tokens(after, true);
    } else if let Some(pos) = trimmed.find('@') {
        let after = trimmed[pos + 1..].trim();
        price = parse_price_tokens(after, false);
    }

    (cost, price)
}

fn parse_cost_body(body: &str, is_total: bool) -> CostAnnotation {
    let parts: Vec<&str> = body.split(',').collect();
    let mut amount: Option<f64> = None;
    let mut amount_text: Option<String> = None;
    let mut commodity: Option<String> = None;
    let mut fields: Vec<String> = Vec::new();

    let amount_re = amount_re();
    let commodity_re = commodity_re();

    if let Some(first) = parts.first() {
        let tokens: Vec<&str> = first.split_whitespace().collect();
        // Try to parse leading "amount commodity"
        if tokens.len() >= 2 && amount_re.is_match(tokens[0]) && commodity_re.is_match(tokens[1]) {
            amount = tokens[0].parse().ok();
            amount_text = Some(tokens[0].to_string());
            commodity = Some(tokens[1].to_string());
        } else {
            // Not a recognisable amount+commodity; treat whole first part as a field
            let trimmed = first.trim();
            if !trimmed.is_empty() {
                fields.push(trimmed.to_string());
            }
        }
    }

    // Remaining comma-separated parts are raw metadata fields
    for part in parts.iter().skip(1) {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            fields.push(trimmed.to_string());
        }
    }

    CostAnnotation {
        is_total,
        amount,
        amount_text,
        commodity,
        fields,
    }
}

fn parse_price_tokens(text: &str, is_total: bool) -> Option<PriceAnnotation> {
    let mut tokens = text.split_whitespace();
    let amount_str = tokens.next()?;
    let commodity = tokens.next()?;

    let amount_re = amount_re();
    let commodity_re = commodity_re();

    if !amount_re.is_match(amount_str) || !commodity_re.is_match(commodity) {
        return None;
    }

    let amount: f64 = amount_str.parse().ok()?;
    Some(PriceAnnotation {
        is_total,
        amount,
        amount_text: amount_str.to_string(),
        commodity: commodity.to_string(),
    })
}

/// Parse a `.prices` file containing market price directives.
///
/// Format (hledger-compatible):
/// ```text
/// P 2026-01-15 BTC 30000.00 USD
/// P 2026-01-15T10:05:03.123456Z BTC 30150.00 USD
/// ; comments and blank lines allowed
/// ```
pub fn parse_prices(contents: &str) -> PricesParseResult {
    let datetime_re = Regex::new(
        r"^\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|[+-]\d{2}:\d{2})?)?$",
    )
    .expect("price datetime regex");
    let amount_re = amount_re();
    let commodity_re = commodity_re();

    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut prices: Vec<PriceDirective> = Vec::new();

    for (idx, raw_line) in contents.lines().enumerate() {
        let line_no = idx + 1;
        let line = raw_line.trim_end_matches('\r').trim();

        if line.is_empty() || line.starts_with(';') {
            continue;
        }

        if !line.starts_with('P') {
            diagnostics.push(diag(
                line_no,
                0,
                format!("expected 'P' directive, got: {line}"),
            ));
            continue;
        }

        let after_p = line[1..].trim_start();
        let mut tokens = after_p.split_whitespace();

        let Some(datetime_str) = tokens.next() else {
            diagnostics.push(diag(line_no, 0, "P directive missing datetime"));
            continue;
        };

        if !datetime_re.is_match(datetime_str) {
            diagnostics.push(diag(
                line_no,
                0,
                format!("invalid datetime: {datetime_str}"),
            ));
            continue;
        }

        let Some(commodity) = tokens.next() else {
            diagnostics.push(diag(line_no, 0, "P directive missing commodity"));
            continue;
        };

        if !commodity_re.is_match(commodity) {
            diagnostics.push(diag(line_no, 0, format!("invalid commodity: {commodity}")));
            continue;
        }

        let Some(price_amount_str) = tokens.next() else {
            diagnostics.push(diag(line_no, 0, "P directive missing price amount"));
            continue;
        };

        if !amount_re.is_match(price_amount_str) {
            diagnostics.push(diag(
                line_no,
                0,
                format!("invalid price amount: {price_amount_str}"),
            ));
            continue;
        }

        let Some(quote_commodity) = tokens.next() else {
            diagnostics.push(diag(line_no, 0, "P directive missing quote commodity"));
            continue;
        };

        if !commodity_re.is_match(quote_commodity) {
            diagnostics.push(diag(
                line_no,
                0,
                format!("invalid quote commodity: {quote_commodity}"),
            ));
            continue;
        }

        let price_amount: f64 = price_amount_str.parse().unwrap_or(0.0);

        prices.push(PriceDirective {
            datetime: datetime_str.to_string(),
            commodity: commodity.to_string(),
            price_amount,
            price_amount_text: price_amount_str.to_string(),
            quote_commodity: quote_commodity.to_string(),
        });
    }

    PricesParseResult {
        ok: diagnostics.is_empty(),
        diagnostics,
        prices,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PriceLookupResult {
    pub price_amount: f64,
    pub price_amount_text: String,
    pub quote_commodity: String,
    pub datetime: String,
}

/// Parse a CSV file containing price data.
/// Expected columns (case-insensitive): Date, Commodity, Price, Currency
pub fn parse_prices_csv(contents: &str) -> PricesParseResult {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut prices: Vec<PriceDirective> = Vec::new();
    let amount_re = amount_re();

    let mut lines = contents.lines();
    let Some(header_line) = lines.next() else {
        return PricesParseResult {
            ok: false,
            diagnostics: vec![diag(1, 0, "empty CSV")],
            prices,
        };
    };

    let headers: Vec<String> = header_line
        .split(',')
        .map(|h| h.trim().to_lowercase())
        .collect();
    let date_idx = headers.iter().position(|h| h == "date");
    let commodity_idx = headers.iter().position(|h| h == "commodity");
    let price_idx = headers.iter().position(|h| h == "price");
    let currency_idx = headers.iter().position(|h| h == "currency");

    if date_idx.is_none()
        || commodity_idx.is_none()
        || price_idx.is_none()
        || currency_idx.is_none()
    {
        diagnostics.push(diag(
            1,
            0,
            "CSV header must contain Date, Commodity, Price, Currency columns",
        ));
        return PricesParseResult {
            ok: false,
            diagnostics,
            prices,
        };
    }

    let date_idx = date_idx.unwrap();
    let commodity_idx = commodity_idx.unwrap();
    let price_idx = price_idx.unwrap();
    let currency_idx = currency_idx.unwrap();

    let datetime_re = Regex::new(
        r"^\d{4}-\d{2}-\d{2}(?:T\d{2}:\d{2}:\d{2}(?:\.\d{1,6})?(?:Z|[+-]\d{2}:\d{2})?)?$",
    )
    .expect("csv price datetime regex");
    let commodity_re = commodity_re();

    for (idx, raw_line) in lines.enumerate() {
        let line_no = idx + 2; // 1-based, skip header
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }

        let fields: Vec<&str> = line.split(',').map(|f| f.trim()).collect();
        let max_idx = *[date_idx, commodity_idx, price_idx, currency_idx]
            .iter()
            .max()
            .unwrap();
        if fields.len() <= max_idx {
            diagnostics.push(diag(line_no, 0, format!("not enough columns: {line}")));
            continue;
        }

        let date_str = fields[date_idx];
        let commodity = fields[commodity_idx];
        let price_str = fields[price_idx];
        let currency = fields[currency_idx];

        if !datetime_re.is_match(date_str) {
            diagnostics.push(diag(line_no, 0, format!("invalid date: {date_str}")));
            continue;
        }
        if !commodity_re.is_match(commodity) {
            diagnostics.push(diag(line_no, 0, format!("invalid commodity: {commodity}")));
            continue;
        }
        if !amount_re.is_match(price_str) {
            diagnostics.push(diag(
                line_no,
                0,
                format!("invalid price amount: {price_str}"),
            ));
            continue;
        }
        if !commodity_re.is_match(currency) {
            diagnostics.push(diag(line_no, 0, format!("invalid currency: {currency}")));
            continue;
        }

        let price_amount: f64 = price_str.parse().unwrap_or(0.0);
        prices.push(PriceDirective {
            datetime: date_str.to_string(),
            commodity: commodity.to_string(),
            price_amount,
            price_amount_text: price_str.to_string(),
            quote_commodity: currency.to_string(),
        });
    }

    PricesParseResult {
        ok: diagnostics.is_empty(),
        diagnostics,
        prices,
    }
}

/// Look up the closest price for a commodity at a given datetime.
/// Finds the last directive with datetime <= target, or the earliest as fallback.
pub fn lookup_price(
    sources_dir: &Path,
    commodity: &str,
    target_datetime: &str,
) -> Option<PriceLookupResult> {
    let price_file = sources_dir.join("_prices").join(format!("{commodity}.txt"));
    let contents = std::fs::read_to_string(&price_file).ok()?;
    let result = parse_prices(&contents);
    if result.prices.is_empty() {
        return None;
    }

    let mut sorted = result.prices;
    sorted.sort_by(|a, b| a.datetime.cmp(&b.datetime));

    // Find last directive with datetime <= target
    let mut best: Option<&PriceDirective> = None;
    for p in &sorted {
        if p.datetime.as_str() <= target_datetime {
            best = Some(p);
        }
    }

    // If target is before all prices, return earliest as fallback
    let chosen = best.unwrap_or(&sorted[0]);

    Some(PriceLookupResult {
        price_amount: chosen.price_amount,
        price_amount_text: chosen.price_amount_text.clone(),
        quote_commodity: chosen.quote_commodity.clone(),
        datetime: chosen.datetime.clone(),
    })
}

/// Graph-based price converter that loads all price files once and supports
/// multi-step conversion (e.g. ETH→USD→AUD).
pub struct PriceGraph {
    /// commodity → list of (normalized-datetime-key, directive), sorted by key.
    /// Pre-normalising at construction means `lookup` does one normalise of the
    /// target and a binary search — no per-comparison allocation.
    prices: HashMap<String, Vec<(String, PriceDirective)>>,
}

impl PriceGraph {
    fn build(entries: Vec<PriceDirective>) -> Self {
        let mut prices: HashMap<String, Vec<(String, PriceDirective)>> = HashMap::new();
        for p in entries {
            let key = normalize_datetime_for_compare(&p.datetime);
            prices.entry(p.commodity.clone()).or_default().push((key, p));
        }
        for list in prices.values_mut() {
            list.sort_by(|a, b| a.0.cmp(&b.0));
        }
        PriceGraph { prices }
    }

    /// Load all `_prices/*.txt` files from the sources directory.
    pub fn load(sources_dir: &Path) -> Self {
        let mut all: Vec<PriceDirective> = Vec::new();
        let prices_dir = sources_dir.join("_prices");
        if let Ok(entries) = std::fs::read_dir(&prices_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("txt") {
                    continue;
                }
                if let Ok(contents) = std::fs::read_to_string(&path) {
                    all.extend(parse_prices(&contents).prices);
                }
            }
        }
        Self::build(all)
    }

    /// Construct a PriceGraph from a list of price directives (for testing).
    pub fn from_entries(entries: Vec<PriceDirective>) -> Self {
        Self::build(entries)
    }

    /// Look up the nearest-prior price for a commodity at a given datetime.
    ///
    /// Returns `None` when no entry exists at or before `target_datetime`.
    /// (The previous behaviour silently fell back to the earliest entry,
    /// which fabricated wildly wrong prices when timestamp formats didn't
    /// match — e.g. ISO `T...Z` entries vs space-separated txn datetimes.)
    pub fn lookup(&self, commodity: &str, target_datetime: &str) -> Option<&PriceDirective> {
        let sorted = self.prices.get(commodity)?;
        if sorted.is_empty() {
            return None;
        }
        let target_norm = normalize_datetime_for_compare(target_datetime);
        // partition_point returns the first index where key > target_norm;
        // the entry just before it is the nearest-prior price.
        let idx = sorted.partition_point(|(k, _)| k.as_str() <= target_norm.as_str());
        if idx == 0 {
            None
        } else {
            Some(&sorted[idx - 1].1)
        }
    }

    /// Most recent price for a commodity (last entry in the date-sorted vec).
    pub fn latest(&self, commodity: &str) -> Option<&PriceDirective> {
        self.prices.get(commodity)?.last().map(|(_, p)| p)
    }

    /// Convert using the most recent prices (latest direct + latest one-hop).
    /// Mirrors `convert_to_base` semantics but without a date cutoff.
    pub fn convert_to_base_latest(
        &self,
        commodity: &str,
        amount: f64,
        base_currency: &str,
    ) -> Option<f64> {
        if commodity == base_currency {
            return Some(amount);
        }
        let p = self.latest(commodity)?;
        if p.quote_commodity == base_currency {
            return Some(amount * p.price_amount);
        }
        let p2 = self.latest(&p.quote_commodity)?;
        if p2.quote_commodity == base_currency {
            return Some(amount * p.price_amount * p2.price_amount);
        }
        None
    }

    /// Convert an amount from `commodity` to `base_currency` at `datetime`.
    ///
    /// Supports:
    /// 1. Identity (commodity == base)
    /// 2. Direct: commodity→base
    /// 3. One-hop: commodity→intermediate→base
    pub fn convert_to_base(
        &self,
        commodity: &str,
        amount: f64,
        datetime: &str,
        base_currency: &str,
    ) -> Option<f64> {
        // Identity
        if commodity == base_currency {
            return Some(amount);
        }

        // Direct: commodity→base
        if let Some(p) = self.lookup(commodity, datetime) {
            if p.quote_commodity == base_currency {
                return Some(amount * p.price_amount);
            }

            // One-hop: commodity→intermediate→base
            let intermediate = &p.quote_commodity;
            if let Some(p2) = self.lookup(intermediate, datetime) {
                if p2.quote_commodity == base_currency {
                    return Some(amount * p.price_amount * p2.price_amount);
                }
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_fixture() {
        let input = r#"2026-01-15 * \"Binance\" \"Buy SOL\" ; txn:01J2N9R9, src:binance:order:999
    assets:exchange:binance:sol    10.000000 SOL {{ 230.00 USD, fee:0.10 USD, fee_to:expenses:fees:trading, venue:binance, note:\"maker fee\" }}
    assets:cash:usd              -230.10 USD
"#;

        let result = parse_transactions(input);
        assert!(
            result.ok,
            "expected ok, got diagnostics: {:?}",
            result.diagnostics
        );
    }

    #[test]
    fn parses_account_declaration_opening_with_default_commodity() {
        let input = r#"account assets:cash:bank:cba:smartaccess AUD
    opening 100.00
"#;

        let result = parse_transactions(input);
        assert!(
            result.ok,
            "expected ok, got diagnostics: {:?}",
            result.diagnostics
        );

        let balance = result
            .balances
            .iter()
            .find(|b| b.account == "assets:cash:bank:cba:smartaccess")
            .expect("expected declared account balance");
        assert_eq!(
            balance.totals,
            vec![CommodityAmount {
                commodity: "AUD".to_string(),
                amount: 100.0
            }]
        );
    }

    #[test]
    fn rejects_posting_without_amount() {
        let input = r#"2026-01-15 * \"Binance\" \"Buy SOL\" ; txn:01J2N9R9
    assets:exchange:binance:sol
"#;

        let result = parse_transactions(input);
        assert!(!result.ok);
        assert!(result
            .diagnostics
            .iter()
            .any(|d| d.message.contains("missing amount")));
    }

    #[test]
    fn take_token_preserves_unicode_in_quoted_strings() {
        // é is multi-byte UTF-8 (0xC3 0xA9)
        let (tok, rest) = take_token(r#""Café" rest"#).unwrap();
        assert_eq!(tok, "Café");
        assert_eq!(rest, " rest");
    }

    #[test]
    fn take_token_preserves_cjk_characters() {
        let (tok, rest) = take_token(r#""比特币交易" next"#).unwrap();
        assert_eq!(tok, "比特币交易");
        assert_eq!(rest, " next");
    }

    #[test]
    fn take_token_handles_escaped_char_after_unicode() {
        let (tok, _) = take_token(r#""Café \"Latte\"""#).unwrap();
        assert_eq!(tok, "Café \"Latte\"");
    }

    #[test]
    fn commodity_re_accepts_unicode_letters() {
        let re = commodity_re();
        assert!(re.is_match("比特币"));
        assert!(re.is_match("UЅDС")); // Cyrillic homoglyphs
        assert!(re.is_match("ETH")); // ASCII still works
        assert!(re.is_match("BTC_v2"));
        assert!(!re.is_match(""));
        assert!(!re.is_match(" ETH")); // leading space
        assert!(!re.is_match(".ETH")); // leading dot
    }

    #[test]
    fn account_re_accepts_unicode_segments() {
        let re = account_re();
        assert!(re.is_match("资产:银行:储蓄"));
        assert!(re.is_match("assets:café"));
        assert!(re.is_match("assets:bank:savings")); // ASCII still works
        assert!(!re.is_match("single_segment")); // needs colon
    }

    #[test]
    fn parses_transaction_with_unicode_commodity() {
        let input = "2026-01-15 * \"Binance\" \"Buy\" ; txn:test1\n    assets:wallet 10.00 比特币\n    expenses:unknown -10.00 比特币\n";
        let result = parse_transactions(input);
        assert!(
            result.ok,
            "expected ok, got diagnostics: {:?}",
            result.diagnostics
        );
        assert_eq!(result.transactions[0].postings[0].commodity, "比特币");
    }

    #[test]
    fn unicode_payee_round_trips_through_quote_and_parse() {
        use crate::generated_store::quote;
        let original = "Café Müller";
        let quoted = quote(original);
        let (parsed, _) = take_token(&quoted).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn price_graph_one_hop_conversion() {
        let graph = PriceGraph::from_entries(vec![
            PriceDirective {
                datetime: "2025-01-01".into(),
                commodity: "USDC".into(),
                price_amount: 1.0,
                price_amount_text: "1.00".into(),
                quote_commodity: "USD".into(),
            },
            PriceDirective {
                datetime: "2025-01-01".into(),
                commodity: "USD".into(),
                price_amount: 1.55,
                price_amount_text: "1.55".into(),
                quote_commodity: "AUD".into(),
            },
        ]);
        // One-hop: USDC → USD → AUD
        let val = graph.convert_to_base("USDC", 100.0, "2025-03-05T01:03:44", "AUD");
        assert!(val.is_some(), "one-hop conversion should succeed");
        assert!((val.unwrap() - 155.0).abs() < 0.01);
    }

    #[test]
    fn price_graph_returns_none_for_unknown_commodity() {
        let graph = PriceGraph::from_entries(vec![PriceDirective {
            datetime: "2025-01-01".into(),
            commodity: "USD".into(),
            price_amount: 1.55,
            price_amount_text: "1.55".into(),
            quote_commodity: "AUD".into(),
        }]);
        let val = graph.convert_to_base("CHEEMS", 100.0, "2025-03-05", "AUD");
        assert!(val.is_none(), "unknown commodity should return None");
    }

    #[test]
    fn price_graph_latest_returns_last_chronological_entry() {
        let graph = PriceGraph::from_entries(vec![
            PriceDirective {
                datetime: "2024-06-01".into(),
                commodity: "HNT".into(),
                price_amount: 4.00,
                price_amount_text: "4.00".into(),
                quote_commodity: "USD".into(),
            },
            PriceDirective {
                datetime: "2026-01-01".into(),
                commodity: "HNT".into(),
                price_amount: 6.50,
                price_amount_text: "6.50".into(),
                quote_commodity: "USD".into(),
            },
            PriceDirective {
                datetime: "2025-03-01".into(),
                commodity: "HNT".into(),
                price_amount: 5.25,
                price_amount_text: "5.25".into(),
                quote_commodity: "USD".into(),
            },
        ]);
        let latest = graph.latest("HNT").expect("HNT should be present");
        assert_eq!(latest.datetime, "2026-01-01");
        assert!((latest.price_amount - 6.50).abs() < f64::EPSILON);
        assert!(graph.latest("UNKNOWN").is_none());
    }
}
