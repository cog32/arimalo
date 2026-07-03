use crate::build_cache::string_hash;
use crate::ledger_parser::{Posting, Transaction};
use crate::rules::{wildcard_match, LabelsFile, MatchFields, RulesFile};
use crate::{parse_date_to_iso, FALLBACK_ASSET_ACCOUNT, FALLBACK_EXPENSE_ACCOUNT};
use rhai::{Engine, Scope, AST};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

const TRANSFORM_FILENAME: &str = "_transform.rhai";

#[derive(Clone)]
pub struct TransformChain {
    pub script: String,
    pub hash: String,
    pub labels: LabelsFile,
    pub rules: RulesFile,
    pub combined_hash: String,
}

/// Resolve the effective transform for a CSV file by walking up the
/// directory tree from the CSV's parent to sources_dir root.
/// Returns the nearest _transform.rhai content, or error if none found.
pub fn resolve_transform(csv_path: &Path, sources_dir: &Path) -> Result<TransformChain, String> {
    let parent = csv_path
        .parent()
        .ok_or_else(|| format!("CSV has no parent directory: {}", csv_path.display()))?;

    let sources_canonical = sources_dir
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize sources dir: {e}"))?;
    let parent_canonical = parent
        .canonicalize()
        .map_err(|e| format!("cannot canonicalize CSV parent: {e}"))?;

    // Gather labels and rules from all folders: CSV parent up to sources_dir.
    // Closer (more specific) entries come first for priority.
    let mut all_labels = Vec::new();
    let mut all_rules = Vec::new();
    let mut hash_parts = Vec::new();
    {
        let mut walk = parent_canonical.as_path();
        loop {
            let folder_labels = LabelsFile::load(walk);
            if !folder_labels.labels.is_empty() {
                hash_parts.push(folder_labels.hash());
                all_labels.extend(folder_labels.labels);
            }
            let folder_rules = RulesFile::load(walk);
            if !folder_rules.rules.is_empty() {
                hash_parts.push(folder_rules.hash());
                all_rules.extend(folder_rules.rules);
            }
            if walk == sources_canonical {
                break;
            }
            match walk.parent() {
                Some(p) => walk = p,
                None => break,
            }
        }
    }
    let merged_labels = LabelsFile { labels: all_labels };
    let merged_rules = RulesFile { rules: all_rules };
    let merged_hash = if hash_parts.is_empty() {
        string_hash(&format!("{}{}", merged_labels.hash(), merged_rules.hash()))
    } else {
        string_hash(&hash_parts.join(":"))
    };

    let mut current = parent_canonical.as_path();
    loop {
        let candidate = current.join(TRANSFORM_FILENAME);
        if candidate.exists() {
            let script = fs::read_to_string(&candidate)
                .map_err(|e| format!("failed to read {}: {e}", candidate.display()))?;
            let hash = string_hash(&script);
            let combined_hash = string_hash(&format!(
                "{hash}{merged_hash}:m={}",
                crate::rules::MATCHER_VERSION
            ));
            return Ok(TransformChain {
                script,
                hash,
                labels: merged_labels,
                rules: merged_rules,
                combined_hash,
            });
        }

        if current == sources_canonical {
            break;
        }
        current = match current.parent() {
            Some(p) => p,
            None => break,
        };
    }

    Err(format!(
        "no {} found for CSV {} (searched up to {})",
        TRANSFORM_FILENAME,
        csv_path.display(),
        sources_dir.display()
    ))
}

/// Deterministic ID: SHA-256 of (relative_path + ":" + row_index), truncated.
pub fn csv_txn_id(relative_path: &str, row_index: usize) -> String {
    let input = format!("{relative_path}:{row_index}");
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    let hex_str = hex::encode(result);
    format!("csv-{}", &hex_str[..12])
}

/// Value of the `txn:` segment in a comma-separated meta string, if present
/// (the on-chain hash for crypto, shared by every leg of one transaction).
fn txn_id_from_meta(meta: Option<&str>) -> Option<String> {
    meta?
        .split(',')
        .map(str::trim)
        .find_map(|seg| seg.strip_prefix("txn:").map(str::to_string))
}

/// Content-derived per-leg id, returned as the `l-<hash12>` value (no `leg:`
/// prefix). It depends only on the leg's on-chain txn id, primary commodity,
/// primary amount, and an ordinal that disambiguates byte-identical sibling
/// legs — never on row position — so it survives CSV re-exports that reorder
/// rows (crypto-sync rewrites the file sorted by timestamp).
fn leg_id(txn_id: &str, commodity: &str, amount_text: &str, ordinal: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(format!("{txn_id}|{commodity}|{amount_text}|{ordinal}").as_bytes());
    format!("l-{}", &hex::encode(hasher.finalize())[..12])
}

/// Stamp a stable `leg:<id>` onto every transaction that shares its `txn:<id>`
/// with at least one sibling — the swap / wrap / multi-hop case where one
/// on-chain transaction expands into several legs that all carry the same
/// hash. Singletons (bank/OFX rows, single-transfer crypto rows) are left
/// untouched so they keep matching on `txn:` alone. Must run BEFORE rule
/// application so `leg:`-anchored rules can match.
fn stamp_leg_ids(transactions: &mut [Transaction]) {
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for txn in transactions.iter() {
        if let Some(id) = txn_id_from_meta(txn.meta.as_deref()) {
            *counts.entry(id).or_insert(0) += 1;
        }
    }
    let mut ordinals: HashMap<String, usize> = HashMap::new();
    for txn in transactions.iter_mut() {
        let Some(txn_id) = txn_id_from_meta(txn.meta.as_deref()) else {
            continue;
        };
        if counts.get(&txn_id).copied().unwrap_or(0) < 2 {
            continue;
        }
        let amount_text = txn
            .postings
            .first()
            .map(|p| p.amount_text.as_str())
            .unwrap_or("");
        let content_key = format!("{txn_id}|{}|{amount_text}", txn.amount_commodity);
        let ordinal = {
            let c = ordinals.entry(content_key).or_insert(0);
            let v = *c;
            *c += 1;
            v
        };
        let leg_tag = format!("leg:{}", leg_id(&txn_id, &txn.amount_commodity, amount_text, ordinal));
        txn.meta = Some(match txn.meta.take() {
            Some(existing) => format!("{existing}, {leg_tag}"),
            None => leg_tag,
        });
    }
}

/// Parse a CSV file and run each row through the Rhai transform.
/// Returns a list of Transactions with stable txn: IDs.
pub fn transform_csv(
    csv_path: &Path,
    sources_dir: &Path,
    chain: &TransformChain,
) -> Result<Vec<Transaction>, String> {
    transform_csv_with_default(csv_path, sources_dir, chain, FALLBACK_EXPENSE_ACCOUNT)
}

/// Engine configured with the extra helper functions exposed to transform
/// scripts. Centralised so the live pipeline and the suggestion run-check
/// (`transform_suggest::rhai_run_check`) evaluate scripts identically.
pub fn new_transform_engine() -> Engine {
    let mut engine = Engine::new();
    // Expose Rust's Unicode-aware char::is_alphabetic() to Rhai scripts.
    engine.register_fn("is_alphabetic", |c: char| c.is_alphabetic());
    engine
}

/// Build the `row` map a transform script sees for one CSV record. Keeps the
/// injected keys (raw columns by header, `_cols`, `_row_index`, `_source_path`,
/// `_account`) in one place so the live pipeline and the run-check agree on the
/// shape of `row`.
pub fn build_row_map(
    headers: &[String],
    cols: &[String],
    row_idx: usize,
    source_path: &str,
    account: &str,
) -> rhai::Map {
    let mut row_map = rhai::Map::new();
    for (i, header) in headers.iter().enumerate() {
        let value = cols.get(i).map(|s| s.as_str()).unwrap_or("");
        row_map.insert(header.clone().into(), value.to_string().into());
    }
    // Expose every raw column positionally (regardless of the header row), so a
    // transform can read layouts the header doesn't describe.
    let arr: rhai::Array = cols.iter().map(|f| f.clone().into()).collect();
    row_map.insert("_cols".into(), arr.into());
    row_map.insert("_row_index".into(), (row_idx as rhai::INT).into());
    row_map.insert("_source_path".into(), source_path.to_string().into());
    row_map.insert("_account".into(), account.to_string().into());
    row_map
}

/// Like `transform_csv` but with a configurable default expense account.
pub fn transform_csv_with_default(
    csv_path: &Path,
    sources_dir: &Path,
    chain: &TransformChain,
    default_expense_account: &str,
) -> Result<Vec<Transaction>, String> {
    let relative_path = csv_path
        .strip_prefix(sources_dir)
        .map_err(|_| {
            format!(
                "CSV {} is not under sources dir {}",
                csv_path.display(),
                sources_dir.display()
            )
        })?
        .to_string_lossy()
        .to_string();

    // flexible(true): don't reject rows whose column count differs from the
    // header. Combined with `row._cols` below, this lets a transform handle
    // exports the header row doesn't describe (preamble / multi-section files).
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_path(csv_path)
        .map_err(|e| format!("failed to open CSV {}: {e}", csv_path.display()))?;

    let headers: Vec<String> = reader
        .headers()
        .map_err(|e| format!("failed to read CSV headers: {e}"))?
        .iter()
        .map(|h| h.to_string())
        .collect();

    let engine = new_transform_engine();
    let ast: AST = engine
        .compile(&chain.script)
        .map_err(|e| format!("failed to compile Rhai transform: {e}"))?;

    let mut transactions = Vec::new();

    for (row_idx, record) in reader.records().enumerate() {
        let record = record.map_err(|e| format!("CSV row {} error: {e}", row_idx + 1))?;

        // Folder-derived account name (e.g. "assets:crypto:wallet:ethereum:0xabc…");
        // needed both for the `row` map and for map_to_transaction below.
        let folder_rel = csv_path
            .parent()
            .and_then(|p| p.strip_prefix(sources_dir).ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let account_name = if folder_rel.is_empty() {
            FALLBACK_ASSET_ACCOUNT.to_string()
        } else {
            crate::processing_pipeline::folder_to_account_name(&folder_rel)
        };

        let cols: Vec<String> = record.iter().map(|f| f.to_string()).collect();
        let row_map = build_row_map(&headers, &cols, row_idx, &relative_path, &account_name);

        let mut scope = Scope::new();
        scope.push("row", row_map);

        let result: rhai::Map = engine
            .eval_ast_with_scope(&mut scope, &ast)
            .map_err(|e| format!("Rhai transform error at row {}: {e}", row_idx + 1))?;

        // Transform may emit `skip: true` to drop a row before it enters the
        // ledger (e.g. phishing-token denylist applied to commodity).
        if result
            .get("skip")
            .and_then(|v| v.clone().as_bool().ok())
            .unwrap_or(false)
        {
            continue;
        }

        let txn = map_to_transaction(
            &result,
            &relative_path,
            row_idx,
            &account_name,
            default_expense_account,
        )?;
        transactions.push(txn);
    }

    // Stamp per-leg ids on shared-txn groups (swap / wrap / multi-hop) before
    // rules run, so a `leg:`-anchored rule can target a single leg.
    stamp_leg_ids(&mut transactions);

    for txn in &mut transactions {
        apply_labels_and_rules(txn, &chain.labels, &chain.rules);
    }

    Ok(transactions)
}

pub fn apply_rules(txn: &mut Transaction, rules: &RulesFile) {
    apply_labels_and_rules(txn, &LabelsFile::default(), rules);
}

pub fn apply_labels_and_rules(txn: &mut Transaction, labels: &LabelsFile, rules: &RulesFile) {
    apply_labels_and_rules_with_accounts(txn, labels, rules, &[]);
}

/// Apply rules and auto-detect self-transfers from declared accounts.
/// If no explicit rule matches but the transaction text contains an identifier
/// from a declared account, auto-categorise as a self-transfer.
pub fn apply_rules_with_accounts(
    txn: &mut Transaction,
    rules: &RulesFile,
    declared_accounts: &[String],
) {
    apply_labels_and_rules_with_accounts(txn, &LabelsFile::default(), rules, declared_accounts);
}

/// Apply labels (pre-pass transforms) then rules (categorization).
/// Labels from `_labels.json` handle payee/commodity normalization.
/// Rules from `_rules.json` handle categorization using the normalized values.
pub fn apply_labels_and_rules_with_accounts(
    txn: &mut Transaction,
    labels: &LabelsFile,
    rules: &RulesFile,
    declared_accounts: &[String],
) {
    // txn.payee and txn.amount_commodity are IMMUTABLE source data.
    // Transforms set display_payee / display_amount_commodity instead.

    // --- Pre-pass 1: apply all matching commodity-rename labels ---
    // Sources: _labels.json, plus legacy commodity renames in _rules.json.
    let pre_fields = MatchFields {
        payee: txn.payee.as_deref(),
        display_payee: None,
        narration: txn.narration.as_deref(),
        meta: txn.meta.as_deref(),
        commodity: Some(txn.amount_commodity.as_str()),
        display_commodity: None,
        amount: Some(txn.amount),
        fee: txn.fee,
    };
    let label_renames = labels.find_commodity_renames(&pre_fields);
    let legacy_renames = rules.find_commodity_renames(&pre_fields);
    for rule in label_renames.iter().chain(legacy_renames.iter()) {
        if let Some(ref new_commodity) = rule.commodity {
            let old_commodity = rule.pattern.clone();
            // Set display commodity (raw amount_commodity stays immutable)
            if txn.display_amount_commodity.is_none()
                && wildcard_match(&old_commodity, &txn.amount_commodity)
            {
                txn.display_amount_commodity = Some(new_commodity.clone());
            }
            // Posting commodities are effective output — rename in place
            for posting in txn.postings.iter_mut() {
                if wildcard_match(&old_commodity, &posting.commodity) {
                    posting.commodity = new_commodity.clone();
                }
            }
        }
    }

    // --- Pre-pass 2: apply first matching payee-rename label ---
    // Sources: _labels.json, plus legacy payee transforms in _rules.json.
    let payee_fields = MatchFields {
        payee: txn.payee.as_deref(),
        display_payee: None,
        narration: txn.narration.as_deref(),
        meta: txn.meta.as_deref(),
        commodity: Some(txn.amount_commodity.as_str()),
        display_commodity: txn.display_amount_commodity.as_deref(),
        amount: Some(txn.amount),
        fee: txn.fee,
    };
    let payee_label = labels
        .find_payee_rename(&payee_fields)
        .or_else(|| rules.find_payee_transform(&payee_fields));
    if let Some(rule) = payee_label {
        if let Some(ref payee) = rule.payee {
            txn.display_payee = Some(payee.clone());
        }
    }

    // --- Main pass: match using both raw and display values ---
    let fields = MatchFields {
        payee: txn.payee.as_deref(),
        display_payee: txn.display_payee.as_deref(),
        narration: txn.narration.as_deref(),
        meta: txn.meta.as_deref(),
        commodity: Some(txn.amount_commodity.as_str()),
        display_commodity: txn.display_amount_commodity.as_deref(),
        amount: Some(txn.amount),
        fee: txn.fee,
    };
    if let Some(rule) = rules.find_match_prioritized(&fields) {
        if let Some(ref payee) = rule.payee {
            txn.display_payee = Some(payee.clone());
        }
        if rule.amount_account.is_some() || rule.fee_account.is_some() {
            apply_rule_accounts(
                txn,
                rule.amount_account.as_deref(),
                rule.fee_account.as_deref(),
            );
        }
        if let Some(ref new_commodity) = rule.commodity {
            let old_commodity = rule.pattern.clone();
            if txn.display_amount_commodity.is_none()
                && wildcard_match(&old_commodity, &txn.amount_commodity)
            {
                txn.display_amount_commodity = Some(new_commodity.clone());
            }
            for posting in txn.postings.iter_mut() {
                if wildcard_match(&old_commodity, &posting.commodity) {
                    posting.commodity = new_commodity.clone();
                }
            }
        }
        let tag = format!("rule:{}", rule.id);
        txn.meta = Some(match txn.meta.take() {
            Some(existing) => format!("{existing}, {tag}"),
            None => tag,
        });
        return;
    }

    // Auto self-transfer: check if transaction text mentions a declared account identifier.
    // Uses raw payee (immutable, always the address).
    if !declared_accounts.is_empty() && txn.postings.len() >= 2 {
        let haystack = format!(
            "{} {} {}",
            txn.payee.as_deref().unwrap_or(""),
            txn.narration.as_deref().unwrap_or(""),
            txn.meta.as_deref().unwrap_or("")
        )
        .to_lowercase();

        for account in declared_accounts {
            let identifier = account.rsplit(':').next().unwrap_or("");
            if identifier.is_empty() || identifier.len() < 4 {
                continue;
            }
            if haystack.contains(&identifier.to_lowercase()) {
                txn.display_payee = Some("Self Transfer".to_string());
                txn.postings[1].account = "assets:transfer".to_string();
                return;
            }
        }
    }
}

/// Apply rule accounts to a transaction.
/// Replaces contra/fee account names on existing postings.
/// Zero-amount handling: if asset amount ≈ 0 and there's a 3-posting
/// fee transaction, the zero-amount contra is dropped and the asset
/// posting rebalances against the remaining fee posting.
///
/// `ignore:*` semantics: when amount_account routes to an `ignore:*`
/// account, the wallet/asset leg (posting[0]) is also rewritten to
/// the same ignore account. Otherwise spam/airdrop tokens persist as
/// non-zero balances on the wallet despite the offsetting leg being
/// ignored.
fn apply_rule_accounts(
    txn: &mut Transaction,
    amount_account: Option<&str>,
    fee_account: Option<&str>,
) {
    // Replace contra account name if specified
    if let Some(acct) = amount_account {
        if txn.postings.len() >= 2 {
            txn.postings[1].account = acct.to_string();
            if acct == "ignore" || acct.starts_with("ignore:") {
                txn.postings[0].account = acct.to_string();
            }
        }
    }

    // Replace fee posting account name if specified
    if let Some(acct) = fee_account {
        if txn.postings.len() >= 3 {
            txn.postings[2].account = acct.to_string();
        }
    }

    // Zero-amount handling: drop the zero-amount contra posting
    if txn.postings.len() >= 3 && txn.postings[1].amount.abs() < 1e-9 {
        txn.postings.remove(1);
    }
}

fn get_string(map: &rhai::Map, key: &str) -> Option<String> {
    map.get(key).and_then(|v| {
        let s = v.clone().into_string().ok()?;
        if s.is_empty() {
            None
        } else {
            Some(s)
        }
    })
}

fn map_to_transaction(
    map: &rhai::Map,
    relative_path: &str,
    row_index: usize,
    folder_account: &str,
    default_expense_account: &str,
) -> Result<Transaction, String> {
    let date = get_string(map, "date")
        .ok_or_else(|| format!("transform row {row_index}: missing 'date'"))?;
    let datetime = get_string(map, "datetime").unwrap_or_else(|| date.clone());
    let payee = get_string(map, "payee");
    let narration = get_string(map, "narration");
    let status = get_string(map, "status").and_then(|s| s.chars().next());
    // Account is always derived from the folder path, not from the transform script.
    let account = folder_account.to_string();
    let contra = get_string(map, "contra");
    let commodity = get_string(map, "commodity").unwrap_or_else(|| "USD".to_string());

    let txn_id = match get_string(map, "txn_id") {
        Some(native_id) => native_id,
        None => csv_txn_id(relative_path, row_index),
    };
    let meta = if let Some(extra) = get_string(map, "meta_extra") {
        format!("txn:{txn_id}, {extra}")
    } else {
        format!("txn:{txn_id}")
    };

    // Check for explicit multi-leg postings (no top-level amount required)
    if let Some(postings_val) = map.get("postings") {
        if let Ok(postings_arr) = postings_val.clone().into_typed_array::<rhai::Map>() {
            let mut postings = Vec::new();
            for p in &postings_arr {
                let p_account = get_string(p, "account").ok_or_else(|| {
                    format!("transform row {row_index}: posting missing 'account'")
                })?;
                let p_amount_str = get_string(p, "amount").ok_or_else(|| {
                    format!("transform row {row_index}: posting missing 'amount'")
                })?;
                let p_commodity = get_string(p, "commodity").unwrap_or_else(|| commodity.clone());
                let p_amount: f64 = p_amount_str.parse().map_err(|e| {
                    format!(
                        "transform row {row_index}: invalid posting amount '{p_amount_str}': {e}"
                    )
                })?;
                let p_remainder = get_string(p, "remainder");
                postings.push(Posting {
                    account: p_account,
                    amount: p_amount,
                    amount_text: p_amount_str,
                    commodity: p_commodity,
                    remainder: p_remainder,
                    cost: None,
                    price: None,
                });
            }
            let parsed_date =
                parse_date_to_iso(&date).map_err(|e| format!("transform row {row_index}: {e}"))?;
            // Infer amount/fee from explicit postings
            let p_amount = postings.first().map(|p| p.amount).unwrap_or(0.0);
            let p_amount_commodity = postings
                .first()
                .map(|p| p.commodity.clone())
                .unwrap_or_default();
            let (p_fee, p_fee_commodity) = if postings.len() >= 3 {
                if let Some(last) = postings.last() {
                    if last.account.starts_with("expenses:fees")
                        || last.account == "income:trading:fees"
                    {
                        (Some(last.amount), Some(last.commodity.clone()))
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            };
            return Ok(Transaction {
                date: parsed_date,
                datetime,
                status,
                payee,
                narration,
                meta: Some(meta),
                postings,
                display_payee: None,
                amount: p_amount,
                amount_commodity: p_amount_commodity,
                display_amount_commodity: None,
                fee: p_fee,
                fee_commodity: p_fee_commodity,
            });
        }
    }

    // Simple two-leg mode requires top-level amount
    let amount_str = get_string(map, "amount")
        .ok_or_else(|| format!("transform row {row_index}: missing 'amount'"))?;
    let amount: f64 = amount_str
        .parse()
        .map_err(|e| format!("transform row {row_index}: invalid amount '{amount_str}': {e}"))?;

    // Parse optional fee field
    let (fee_amount, fee_commodity) = if let Some(fee_str) = get_string(map, "fee") {
        // Try "amount commodity" format (split on last space)
        if let Some(pos) = fee_str.rfind(' ') {
            let (amt_part, comm_part) = fee_str.split_at(pos);
            let comm_part = comm_part.trim();
            if let Ok(f) = amt_part.trim().parse::<f64>() {
                (f, comm_part.to_string())
            } else if let Ok(f) = fee_str.parse::<f64>() {
                (f, commodity.clone())
            } else {
                (0.0, commodity.clone())
            }
        } else if let Ok(f) = fee_str.parse::<f64>() {
            (f, commodity.clone())
        } else {
            (0.0, commodity.clone())
        }
    } else {
        (0.0, commodity.clone())
    };
    let has_fee = fee_amount.abs() > 1e-9;

    // Simple two-leg transaction (+ optional fee leg)
    // When fee is in the same commodity, deduct it from the asset so the
    // contra reflects the gross amount and the asset reflects the net.
    let contra_account = contra.unwrap_or_else(|| default_expense_account.to_string());
    let asset_amount = if has_fee && fee_commodity == commodity {
        amount - fee_amount
    } else {
        amount
    };
    let contra_amount = -amount;

    let parsed_date =
        parse_date_to_iso(&date).map_err(|e| format!("transform row {row_index}: {e}"))?;

    let mut postings = vec![
        Posting {
            account: account.clone(),
            amount: asset_amount,
            amount_text: format!("{asset_amount}"),
            commodity: commodity.clone(),
            remainder: None,
            cost: None,
            price: None,
        },
        Posting {
            account: contra_account,
            amount: contra_amount,
            amount_text: format!("{contra_amount}"),
            commodity: commodity.clone(),
            remainder: None,
            cost: None,
            price: None,
        },
    ];

    let txn_fee = if has_fee { Some(fee_amount) } else { None };
    let txn_fee_commodity = if has_fee {
        Some(fee_commodity.clone())
    } else {
        None
    };

    if has_fee {
        postings.push(Posting {
            account: "income:trading:fees".to_string(),
            amount: fee_amount,
            amount_text: format!("{fee_amount}"),
            commodity: fee_commodity,
            remainder: None,
            cost: None,
            price: None,
        });
    }
    Ok(Transaction {
        date: parsed_date,
        datetime,
        status,
        payee,
        narration,
        meta: Some(meta),
        display_payee: None,
        postings,
        amount,
        amount_commodity: commodity,
        display_amount_commodity: None,
        fee: txn_fee,
        fee_commodity: txn_fee_commodity,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger_parser::{Posting, Transaction};
    use crate::rules::Rule;

    fn make_txn(payee: &str, narration: &str, commodity: &str) -> Transaction {
        Transaction {
            date: "2024-11-07".to_string(),
            datetime: "2024-11-07T10:00:00".to_string(),
            status: None,
            payee: Some(payee.to_string()),
            narration: Some(narration.to_string()),
            meta: None,
            postings: vec![
                Posting {
                    account: "assets:ethereum".to_string(),
                    amount: 100.0,
                    amount_text: "100".to_string(),
                    commodity: commodity.to_string(),
                    remainder: None,
                    cost: None,
                    price: None,
                },
                Posting {
                    account: "expenses:unknown".to_string(),
                    amount: -100.0,
                    amount_text: "-100".to_string(),
                    commodity: commodity.to_string(),
                    remainder: None,
                    cost: None,
                    price: None,
                },
            ],
            display_payee: None,
            amount: 100.0,
            amount_commodity: commodity.to_string(),
            display_amount_commodity: None,
            fee: None,
            fee_commodity: None,
        }
    }

    fn make_rule(
        id: &str,
        pattern: &str,
        match_field: Option<&str>,
        payee: Option<&str>,
        amount_account: Option<&str>,
    ) -> Rule {
        Rule {
            id: id.to_string(),
            pattern: pattern.to_string(),
            match_field: match_field.map(|s| s.to_string()),
            payee: payee.map(|s| s.to_string()),
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            amount_account: amount_account.map(|s| s.to_string()),
            fee_account: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,            postings: vec![],
        }
    }

    fn with_meta(mut t: Transaction, meta: &str) -> Transaction {
        t.meta = Some(meta.to_string());
        t
    }

    fn leg_seg(meta: &str) -> Option<String> {
        meta.split(',')
            .map(str::trim)
            .find_map(|s| s.strip_prefix("leg:").map(str::to_string))
    }

    #[test]
    fn stamp_leg_ids_tags_shared_txn_group_distinctly() {
        // One on-chain tx split into two legs (sell ETH / buy DYDX) sharing txn:H.
        let mut txns = vec![
            with_meta(make_txn("a", "trade ETH", "ETH"), "txn:H"),
            with_meta(make_txn("b", "trade DYDX", "DYDX"), "txn:H"),
        ];
        stamp_leg_ids(&mut txns);
        let a = leg_seg(txns[0].meta.as_deref().unwrap());
        let b = leg_seg(txns[1].meta.as_deref().unwrap());
        assert!(a.is_some() && b.is_some(), "both shared legs get a leg: tag");
        assert_ne!(a, b, "distinct legs get distinct leg ids");
        assert!(
            txns[0].meta.as_deref().unwrap().contains("txn:H"),
            "the shared txn: id is preserved"
        );
    }

    #[test]
    fn stamp_leg_ids_leaves_singletons_untouched() {
        let mut txns = vec![with_meta(make_txn("a", "send", "ETH"), "txn:SOLO")];
        stamp_leg_ids(&mut txns);
        assert_eq!(
            txns[0].meta.as_deref(),
            Some("txn:SOLO"),
            "a single-leg transaction keeps txn-only meta"
        );
    }

    #[test]
    fn stamp_leg_ids_disambiguates_byte_identical_legs() {
        // Two legs sharing txn id, commodity and amount (the Raydium duplicate-leg case).
        let mut txns = vec![
            with_meta(make_txn("a", "trade MNGO", "MNGO"), "txn:rad-001"),
            with_meta(make_txn("b", "trade MNGO", "MNGO"), "txn:rad-001"),
        ];
        stamp_leg_ids(&mut txns);
        let a = leg_seg(txns[0].meta.as_deref().unwrap()).unwrap();
        let b = leg_seg(txns[1].meta.as_deref().unwrap()).unwrap();
        assert_ne!(a, b, "byte-identical legs are disambiguated by ordinal");
    }

    #[test]
    fn shared_hash_legs_route_independently_via_leg_rules() {
        // End-to-end: one on-chain swap (hash H) imported as two rows — sell 8 ETH,
        // buy 1688 DYDX — that share `txn:H`. The user allocates each leg
        // independently (as the inline category edit does) with a `leg:`-anchored
        // rule; each leg must route to its own contra, unlike a `txn:` rule which
        // would bleed across both.
        let dir = std::env::temp_dir().join("arimalo_leg_routing_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let csv_path = dir.join("swap.csv");
        fs::write(
            &csv_path,
            "hash,commodity,amount\n\
             H,ETH,-8\n\
             H,DYDX,1688.649359870722\n",
        )
        .unwrap();

        // Transform sets txn_id to the shared on-chain hash, so both legs carry txn:H.
        let script = r#"#{ date: "2021-09-22", payee: "swap", narration: "trade " + row["_cols"][1],
            status: "*", txn_id: row["_cols"][0], amount: row["_cols"][2], commodity: row["_cols"][1] }"#;
        let chain = TransformChain {
            script: script.to_string(),
            hash: "h".to_string(),
            labels: LabelsFile::default(),
            rules: RulesFile::default(),
            combined_hash: "c".to_string(),
        };

        let mut txns = transform_csv(&csv_path, &dir, &chain).unwrap();
        assert_eq!(txns.len(), 2);

        let leg_of = |commodity: &str| -> String {
            let t = txns.iter().find(|t| t.amount_commodity == commodity).unwrap();
            assert!(
                t.meta.as_deref().unwrap().contains("txn:H"),
                "shared txn id preserved on {commodity} leg"
            );
            leg_seg(t.meta.as_deref().unwrap())
                .unwrap_or_else(|| panic!("no leg: tag on {commodity} leg: {:?}", t.meta))
        };
        let eth_leg = leg_of("ETH");
        let dydx_leg = leg_of("DYDX");
        assert_ne!(eth_leg, dydx_leg, "the two legs get distinct leg ids");

        let rules = RulesFile {
            rules: vec![
                make_rule("r-sell", &format!("leg:{eth_leg}"), Some("meta"), None, Some("equity:trading:sell")),
                make_rule("r-buy", &format!("leg:{dydx_leg}"), Some("meta"), None, Some("equity:trading:buy")),
            ],
        };
        for txn in &mut txns {
            apply_rules(txn, &rules);
        }
        let _ = fs::remove_dir_all(&dir);

        let routes_to = |commodity: &str, account: &str| {
            let t = txns.iter().find(|t| t.amount_commodity == commodity).unwrap();
            assert!(
                t.postings.iter().any(|p| p.account == account),
                "{commodity} leg should route to {account}; postings: {:?}",
                t.postings
            );
        };
        routes_to("ETH", "equity:trading:sell");
        routes_to("DYDX", "equity:trading:buy");
    }

    #[test]
    fn test_cols_exposes_all_columns_and_reads_ragged_rows() {
        // Generic primitive: the engine exposes every raw column via row._cols
        // (regardless of the header) and reads ragged rows without erroring, so a
        // transform can parse layouts the header row doesn't describe.
        let dir = std::env::temp_dir().join("arimalo_cols_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let csv_path = dir.join("data.csv");
        // 2-column header, but the data row carries 4 columns (ragged).
        fs::write(&csv_path, "A,B\nx,y,2024-06-28,12.50\n").unwrap();

        let chain = TransformChain {
            script: "#{ date: row[\"_cols\"][2], payee: \"P\", narration: \"n\", \
                     amount: row[\"_cols\"][3], commodity: \"USD\", status: \"*\" }"
                .to_string(),
            hash: "h".to_string(),
            labels: LabelsFile::default(),
            rules: RulesFile::default(),
            combined_hash: "c".to_string(),
        };
        let txns = transform_csv(&csv_path, &dir, &chain).unwrap();
        let _ = fs::remove_dir_all(&dir);

        assert_eq!(txns.len(), 1);
        assert_eq!(txns[0].date, "2024-06-28");
        assert!((txns[0].amount - 12.50).abs() < 1e-9);
    }

    #[test]
    fn test_ibkr_multisection_transform_skips_preamble_and_routes_by_type() {
        // End-to-end regression guard for raw IBKR Transaction History imports.
        // The download is multi-section: Statement/Summary metadata sit above the
        // real table. This must parse via the generic row._cols primitive —
        // preamble + section-header rows skipped, transaction rows read
        // positionally and routed by Transaction Type. Mirrors the logic in
        // sources/.../equity/broker/ibkr/personal/_transform.rhai.
        let dir = std::env::temp_dir().join("arimalo_ibkr_multisection_test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let csv_path = dir.join("ibkr.csv");
        fs::write(
            &csv_path,
            "Statement,Header,Field Name,Field Value\n\
             Statement,Data,Title,Transaction History\n\
             Summary,Header,Field Name,Field Value\n\
             Summary,Data,Base Currency,USD\n\
             Transaction History,Header,Date,Account,Description,Transaction Type,Symbol,Quantity,Price,Price Currency,Gross Amount ,Commission,Net Amount\n\
             Transaction History,Data,2024-07-02,U5240,Buy SGOV,Buy,SGOV,487.0,100.525,USD,-48955.675,-2.435,-48958.11\n\
             Transaction History,Data,2024-07-08,U5240,VWOB Dividend,Dividend,VWOB,-,-,-,295.22,-,295.22\n\
             Transaction History,Data,2024-07-08,U5240,VWOB US Tax,Foreign Tax Withholding,VWOB,-,-,-,-44.28,-,-44.28\n",
        )
        .unwrap();

        let script = r#"
            fn nums(s){let r="" + s; r.replace(",",""); if r==""||r=="-"{r="0";} r}
            fn abss(s){let r=nums(s); if r.starts_with("-"){r=r.sub_string(1,r.len()-1);} r}
            fn neg(s){let r=nums(s); if r.starts_with("-"){r=r.sub_string(1,r.len()-1);}else{r="-"+r;} r}
            fn sym(s){let r="" + s; r.replace(" ",""); r}
            let cols = row["_cols"];
            let n = cols.len();
            let skip_row=false; let ttype=""; let symbol=""; let qty=""; let gross=""; let comm=""; let net=""; let desc=""; let date="";
            if n>=13 && ("" + cols[0])=="Transaction History" {
                if ("" + cols[1])!="Data" { skip_row=true; }
                else { date=""+cols[2]; desc=""+cols[4]; ttype=""+cols[5]; symbol=""+cols[6]; qty=""+cols[7]; gross=""+cols[10]; comm=""+cols[11]; net=""+cols[12]; }
            } else if n>=1 && (("" + cols[0])=="Statement" || ("" + cols[0])=="Summary") { skip_row=true; }
            let acct = row["_account"];
            let cash = acct + ":cash";
            let result = #{};
            if skip_row || date=="" { result = #{ skip: true }; }
            else if ttype=="Buy" || ttype=="Sell" {
                result = #{ date: date, payee: "IBKR", narration: ttype, status: "*",
                    postings: [
                        #{ account: acct, amount: nums(qty), commodity: sym(symbol), remainder: "@@ " + abss(gross) + " USD" },
                        #{ account: cash, amount: nums(net), commodity: "USD" },
                        #{ account: "expenses:fees:brokerage", amount: abss(comm), commodity: "USD" }
                    ] };
            } else {
                let contra = "expenses:unknown";
                if ttype=="Dividend" { contra="income:dividends:ibkr"; }
                else if ttype=="Foreign Tax Withholding" { contra="expenses:tax:foreign-withholding:ibkr"; }
                result = #{ date: date, payee: "IBKR", narration: desc, status: "*",
                    postings: [
                        #{ account: cash, amount: nums(net), commodity: "USD" },
                        #{ account: contra, amount: neg(net), commodity: "USD" }
                    ] };
            }
            result
        "#;

        let chain = TransformChain {
            script: script.to_string(),
            hash: "h".to_string(),
            labels: LabelsFile::default(),
            rules: RulesFile::default(),
            combined_hash: "c".to_string(),
        };
        let txns = transform_csv(&csv_path, &dir, &chain).unwrap();
        let _ = fs::remove_dir_all(&dir);

        // 4 preamble/section-header rows skipped → exactly the 3 transactions.
        assert_eq!(txns.len(), 3, "preamble + section header must be skipped");

        // Buy → 3 legs, balanced (security @@ gross, cash Net, brokerage fee).
        let buy = &txns[0];
        assert_eq!(buy.date, "2024-07-02");
        assert_eq!(buy.postings.len(), 3);
        assert_eq!(buy.postings[0].commodity, "SGOV");
        assert!((buy.postings[0].amount - 487.0).abs() < 1e-9);
        assert_eq!(buy.postings[0].account, FALLBACK_ASSET_ACCOUNT);
        assert_eq!(buy.postings[1].account, format!("{FALLBACK_ASSET_ACCOUNT}:cash"));
        assert!((buy.postings[1].amount - (-48958.11)).abs() < 1e-6);
        assert_eq!(buy.postings[2].account, "expenses:fees:brokerage");
        assert!((buy.postings[2].amount - 2.435).abs() < 1e-9);

        // Dividend → income:dividends:ibkr.
        let div = &txns[1];
        assert_eq!(div.date, "2024-07-08");
        assert_eq!(div.postings.len(), 2);
        assert_eq!(div.postings[1].account, "income:dividends:ibkr");
        assert!((div.postings[1].amount - (-295.22)).abs() < 1e-6);

        // Foreign Tax Withholding → withholding expense.
        let tax = &txns[2];
        assert_eq!(tax.postings[1].account, "expenses:tax:foreign-withholding:ibkr");
        assert!((tax.postings[1].amount - 44.28).abs() < 1e-6);
    }

    #[test]
    fn test_payee_field_rule_does_not_match_narration() {
        // Rule: match_field:"payee", pattern:"*Swell Network*"
        // Should NOT match a transaction where only the narration contains relevant text
        let rules = RulesFile {
            rules: vec![make_rule(
                "eth-swell-1",
                "*Swell Network*",
                Some("payee"),
                Some("Swell Network"),
                Some("expenses:crypto:defi:swell"),
            )],
        };

        let mut txn = make_txn("0xabcdef1234567890", "token_received SWELL", "SWELL");
        apply_rules(&mut txn, &rules);

        assert!(
            txn.meta.is_none() || !txn.meta.as_ref().unwrap().contains("eth-swell-1"),
            "rule with match_field:payee must not match on narration"
        );
        assert!(
            txn.display_payee.is_none(),
            "must not rename payee when the rule shouldn't have matched"
        );
    }

    #[test]
    fn test_payee_field_rule_matches_correct_payee() {
        let rules = RulesFile {
            rules: vec![make_rule(
                "eth-swell-1",
                "*Swell Network*",
                Some("payee"),
                Some("Swell Network"),
                Some("expenses:crypto:defi:swell"),
            )],
        };

        let mut txn = make_txn("Swell Network", "token_transfer SWELL", "SWELL");
        apply_rules(&mut txn, &rules);

        assert!(
            txn.meta.as_ref().unwrap().contains("eth-swell-1"),
            "rule should match when payee matches"
        );
        assert_eq!(txn.display_payee.as_deref(), Some("Swell Network"));
    }

    #[test]
    fn test_payee_transform_with_field_does_not_match_narration() {
        // A payee-only rule (no amount_account) with match_field:"payee"
        // must only set display_payee for transactions matching on payee
        let rules = RulesFile {
            rules: vec![make_rule(
                "rename-swell",
                "*Swell*",
                Some("payee"),
                Some("Swell Network"),
                None,
            )],
        };

        // Transaction with "Swell" in narration but NOT in payee
        let mut txn = make_txn("0xabcdef", "Swell token_received", "ETH");
        apply_rules(&mut txn, &rules);

        assert!(
            txn.display_payee.is_none(),
            "payee transform with match_field:payee must not match narration"
        );
    }

    #[test]
    fn test_no_match_field_matches_all_fields() {
        // Rule with no match_field should match across all fields (current behavior, regression guard)
        let rules = RulesFile {
            rules: vec![make_rule(
                "broad",
                "*token_transfer*",
                None,
                None,
                Some("expenses:transfers"),
            )],
        };

        let mut txn = make_txn("0xabcdef", "token_transfer SWELL", "SWELL");
        apply_rules(&mut txn, &rules);

        assert!(
            txn.meta.as_ref().unwrap().contains("broad"),
            "rule with no match_field should match narration"
        );
    }

    #[test]
    fn test_ignore_rule_rewrites_both_legs() {
        // When a rule routes amount_account to an `ignore:*` account,
        // BOTH legs of the transaction (wallet + contra) must be rewritten
        // to that ignore account. Otherwise the wallet retains a non-zero
        // balance of the spam/airdrop commodity even though the offsetting
        // leg is ignored.
        let rules = RulesFile {
            rules: vec![make_rule(
                "spam-jupdrop",
                "JUPDROP",
                Some("commodity"),
                None,
                Some("ignore:spam"),
            )],
        };

        let mut txn = make_txn("SCAMMER", "token_transfer:mint JUPDROP", "JUPDROP");
        apply_rules(&mut txn, &rules);

        assert_eq!(
            txn.postings[0].account, "ignore:spam",
            "wallet leg must be rewritten to ignore:spam, got {:?}",
            txn.postings[0].account
        );
        assert_eq!(txn.postings[1].account, "ignore:spam");
    }

    #[test]
    fn test_non_ignore_rule_leaves_wallet_leg_untouched() {
        // Sanity: regular rules (not ignore:*) only rewrite the contra leg.
        let rules = RulesFile {
            rules: vec![make_rule(
                "swap-buy",
                "*token_transfer*",
                None,
                None,
                Some("equity:trading:buy"),
            )],
        };

        let mut txn = make_txn("Uniswap", "token_transfer ETH", "ETH");
        apply_rules(&mut txn, &rules);

        assert_eq!(txn.postings[0].account, "assets:ethereum");
        assert_eq!(txn.postings[1].account, "equity:trading:buy");
    }

    #[test]
    fn test_payee_condition_prevents_broad_match() {
        // The user's exact scenario: rule has pattern matching narration + payee_condition.
        // Must NOT match transactions where the payee doesn't match.
        let rules = RulesFile {
            rules: vec![Rule {
                id: "eth-swell-1".to_string(),
                pattern: "*token_transfer*".to_string(),
                match_field: None,
                payee: None,
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some("income:crypto:airdrop".to_string()),
                fee_account: None,
                payee_condition: Some("*Swell Network*".to_string()),
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,
                postings: vec![],
            }],
        };

        // Swell Network transaction — should match
        let mut txn_swell = make_txn("Swell Network", "token_transfer SWELL", "SWELL");
        apply_rules(&mut txn_swell, &rules);
        assert!(
            txn_swell.meta.as_ref().unwrap().contains("eth-swell-1"),
            "should match when payee_condition matches"
        );

        // Uniswap transaction — same narration pattern but different payee — must NOT match
        let mut txn_uni = make_txn("Uniswap V3", "token_transfer ETH", "ETH");
        apply_rules(&mut txn_uni, &rules);
        assert!(
            txn_uni.meta.is_none() || !txn_uni.meta.as_ref().unwrap().contains("eth-swell-1"),
            "must not match when payee_condition does not match"
        );
    }

    #[test]
    fn test_label_sets_payee_before_rule_payee_condition() {
        // Label in _labels.json renames the address to "Swell Network".
        // Rule in _rules.json has payee_condition:"*Swell Network*" + narration pattern.
        // The label pre-pass sets display_payee, then the rule matches via payee_condition.
        let labels = LabelsFile {
            labels: vec![Rule {
                id: "label-swell".to_string(),
                pattern: "*0x342f*".to_string(),
                match_field: Some("payee".to_string()),
                payee: Some("Swell Network".to_string()),
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
            }],
        };
        let rules = RulesFile {
            rules: vec![make_rule(
                "eth-swell-1",
                "*token_transfer:receive*",
                None,
                None,
                Some("income:crypto:airdrops"),
            )],
        };
        // Add payee_condition to the rule
        let mut rule = rules.rules[0].clone();
        rule.payee_condition = Some("*Swell Network*".to_string());
        let rules = RulesFile { rules: vec![rule] };

        // Transaction with address payee and matching narration
        let mut txn = make_txn(
            "0x342f0d375ba986a65204750a4aece3b39f739d75",
            "token_transfer:receive SWELL",
            "SWELL",
        );
        apply_labels_and_rules(&mut txn, &labels, &rules);

        assert_eq!(
            txn.display_payee.as_deref(),
            Some("Swell Network"),
            "label should set display_payee in pre-pass"
        );
        assert!(
            txn.meta.as_ref().unwrap().contains("eth-swell-1"),
            "rule should match via payee_condition after label set display_payee"
        );

        // Transaction with different payee — label doesn't match, rule shouldn't either
        let mut txn_other = make_txn("0xabcdef1234567890", "token_transfer:receive ETH", "ETH");
        apply_labels_and_rules(&mut txn_other, &labels, &rules);

        assert!(
            txn_other.display_payee.is_none(),
            "label should not match different address"
        );
        assert!(
            txn_other.meta.is_none() || !txn_other.meta.as_ref().unwrap().contains("eth-swell-1"),
            "rule should not match without payee_condition satisfied"
        );
    }
}
