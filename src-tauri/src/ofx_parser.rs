use crate::csv_transform::apply_rules;
use crate::ledger_parser::{Posting, Transaction};
use crate::rules::RulesFile;
use crate::{parse_date_to_iso, FALLBACK_EXPENSE_ACCOUNT};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct OfxTransaction {
    pub trntype: String,
    pub dtposted: String,
    pub trnamt: String,
    pub fitid: String,
    pub memo: String,
    /// Byte range of the entire `<STMTTRN>...</STMTTRN>` block in the source content.
    /// Used by the dedupe tool to strip blocks without re-serializing the file.
    pub block_range: (usize, usize),
}

#[derive(Debug, Clone)]
pub struct OfxFile {
    pub curdef: String,
    pub transactions: Vec<OfxTransaction>,
    /// Statement period start, as the raw DTSTART value (e.g. "20240403"). Empty if missing.
    pub dtstart: String,
    /// Statement period end, as the raw DTEND value. Empty if missing.
    pub dtend: String,
}

/// Parse OFX v1 SGML content into an OfxFile.
pub fn parse_ofx(content: &str) -> Result<OfxFile, String> {
    // OFX v1 is ASCII-based. Sanitize non-ASCII to avoid byte-position
    // mismatch between uppercase search indices and original content indexing.
    let content: std::borrow::Cow<str> = if content.is_ascii() {
        std::borrow::Cow::Borrowed(content)
    } else {
        std::borrow::Cow::Owned(content.replace(|c: char| !c.is_ascii(), "?"))
    };

    let curdef = extract_tag_value(&content, "CURDEF").unwrap_or_else(|| "USD".to_string());
    let dtstart = extract_tag_value(&content, "DTSTART").unwrap_or_default();
    let dtend = extract_tag_value(&content, "DTEND").unwrap_or_default();

    let mut transactions = Vec::new();
    let upper = content.to_uppercase();
    let mut search_from = 0;
    // Counts of synthesized keys, so genuinely-separate rows that share
    // date+amount+memo within one file still get distinct ids.
    let mut synth_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    while let Some(pos) = upper[search_from..].find("<STMTTRN>") {
        let start = search_from + pos;
        let end = match upper[start..].find("</STMTTRN>") {
            Some(pos) => start + pos + "</STMTTRN>".len(),
            None => break,
        };

        let block = &content[start..end];

        let trntype = extract_tag_value(block, "TRNTYPE").unwrap_or_default();
        let dtposted = extract_tag_value(block, "DTPOSTED").unwrap_or_default();
        let trnamt = extract_tag_value(block, "TRNAMT").unwrap_or_default();
        let fitid = extract_tag_value(block, "FITID").unwrap_or_default();
        let memo = extract_tag_value(block, "MEMO")
            .or_else(|| extract_tag_value(block, "NAME"))
            .unwrap_or_default();

        // Some banks (e.g. CBA savings) emit an empty <FITID> on interest rows.
        // Synthesize a deterministic key from the row's own fields so it still
        // imports and dedupes consistently — both `ofx_txn_id` and the
        // OFX-vs-OFX dedupe planner key off this `fitid` field.
        let fitid = if fitid.is_empty() {
            synth_fitid(&dtposted, &trnamt, &memo, &mut synth_counts)
        } else {
            fitid
        };

        transactions.push(OfxTransaction {
            trntype,
            dtposted,
            trnamt,
            fitid,
            memo,
            block_range: (start, end),
        });

        search_from = end;
    }

    Ok(OfxFile {
        curdef,
        transactions,
        dtstart,
        dtend,
    })
}

/// Synthesize a stable FITID for rows whose `<FITID>` is empty.
///
/// Keyed on (date, amount, memo) so the same physical row in two overlapping
/// exports synthesizes identically (each file's first occurrence → count 0 →
/// same key → still dedupes). The per-file `counts` map disambiguates genuinely
/// separate rows that share all three fields within one file. The `syn-` prefix
/// marks the value as synthetic and avoids any clash with a real FITID.
fn synth_fitid(
    dtposted: &str,
    trnamt: &str,
    memo: &str,
    counts: &mut std::collections::HashMap<String, usize>,
) -> String {
    let base = format!("{dtposted}|{trnamt}|{memo}");
    let n = *counts.entry(base.clone()).or_insert(0);
    *counts.get_mut(&base).expect("just inserted") += 1;
    let keyed = if n == 0 { base } else { format!("{base}#{n}") };
    let mut hasher = Sha256::new();
    hasher.update(keyed.as_bytes());
    format!("syn-{}", &hex::encode(hasher.finalize())[..12])
}

/// Deterministic ID: SHA-256 of (relative_path + ":" + fitid), truncated.
pub fn ofx_txn_id(relative_path: &str, fitid: &str) -> String {
    let input = format!("{relative_path}:{fitid}");
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    let hex_str = hex::encode(result);
    format!("ofx-{}", &hex_str[..12])
}

/// Convert OFX transactions to ledger Transactions.
pub fn ofx_to_transactions(
    ofx: &OfxFile,
    account_name: &str,
    relative_path: &str,
    rules: &RulesFile,
) -> Result<Vec<Transaction>, String> {
    ofx_to_transactions_with_default(
        ofx,
        account_name,
        relative_path,
        rules,
        FALLBACK_EXPENSE_ACCOUNT,
    )
}

pub fn ofx_to_transactions_with_default(
    ofx: &OfxFile,
    account_name: &str,
    relative_path: &str,
    rules: &RulesFile,
    default_expense_account: &str,
) -> Result<Vec<Transaction>, String> {
    let mut transactions = Vec::new();

    for otxn in &ofx.transactions {
        let date = parse_date_to_iso(&otxn.dtposted)?;
        let amount: f64 = otxn
            .trnamt
            .trim()
            .parse()
            .map_err(|e| format!("invalid OFX amount '{}': {e}", otxn.trnamt))?;

        let txn_id = ofx_txn_id(relative_path, &otxn.fitid);
        let meta = format!("txn:{txn_id}");
        let neg_amount = -amount;

        let mut txn = Transaction {
            date: date.clone(),
            datetime: date,
            status: Some('*'),
            // The bank memo is the row's payee for rule purposes — mirror the
            // CSV transform (which sets payee from the description column) so
            // `payee_condition` rules match OFX rows too. Without this, OFX rows
            // with empty payee silently fall through to the default account.
            payee: Some(otxn.memo.clone()),
            narration: Some(otxn.memo.clone()),
            meta: Some(meta),
            display_payee: None,
            display_amount_commodity: None,
            postings: vec![
                Posting {
                    account: account_name.to_string(),
                    amount,
                    amount_text: otxn.trnamt.trim().to_string(),
                    commodity: ofx.curdef.clone(),
                    remainder: None,
                    cost: None,
                    price: None,
                },
                Posting {
                    account: default_expense_account.to_string(),
                    amount: neg_amount,
                    amount_text: format!("{neg_amount}"),
                    commodity: ofx.curdef.clone(),
                    remainder: None,
                    cost: None,
                    price: None,
                },
            ],
            amount,
            amount_commodity: ofx.curdef.clone(),
            fee: None,
            fee_commodity: None,
        };

        apply_rules(&mut txn, rules);
        transactions.push(txn);
    }

    Ok(transactions)
}

/// Extract the value of a simple SGML tag like `<TAG>value`.
/// OFX v1 uses `<TAG>value` (no closing tag for simple values).
fn extract_tag_value(content: &str, tag: &str) -> Option<String> {
    let upper_content = content.to_uppercase();
    let needle = format!("<{}>", tag.to_uppercase());
    let pos = upper_content.find(&needle)?;
    let start = pos + needle.len();
    let rest = &content[start..];
    // Value ends at newline or next '<'
    let end = rest
        .find(['<', '\n', '\r'])
        .unwrap_or(rest.len());
    let value = rest[..end].trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_OFX: &str = r#"OFXHEADER:100
DATA:OFXSGML

<OFX>
<BANKMSGSRSV1>
<STMTTRNRS>
<STMTRS>
<CURDEF>AUD
<BANKTRANLIST>
<STMTTRN>
<TRNTYPE>DEBIT
<DTPOSTED>20250115
<TRNAMT>-4.50
<FITID>TXN001
<MEMO>Coffee Shop
</STMTTRN>
<STMTTRN>
<TRNTYPE>CREDIT
<DTPOSTED>20250116120000
<TRNAMT>3500.00
<FITID>TXN002
<MEMO>Salary
</STMTTRN>
</BANKTRANLIST>
</STMTRS>
</STMTTRNRS>
</BANKMSGSRSV1>
</OFX>"#;

    #[test]
    fn test_parse_ofx_basic() {
        let ofx = parse_ofx(SAMPLE_OFX).unwrap();
        assert_eq!(ofx.curdef, "AUD");
        assert_eq!(ofx.transactions.len(), 2);

        let t0 = &ofx.transactions[0];
        assert_eq!(t0.trntype, "DEBIT");
        assert_eq!(t0.dtposted, "20250115");
        assert_eq!(t0.trnamt, "-4.50");
        assert_eq!(t0.fitid, "TXN001");
        assert_eq!(t0.memo, "Coffee Shop");

        let t1 = &ofx.transactions[1];
        assert_eq!(t1.trntype, "CREDIT");
        assert_eq!(t1.dtposted, "20250116120000");
        assert_eq!(t1.trnamt, "3500.00");
        assert_eq!(t1.fitid, "TXN002");
        assert_eq!(t1.memo, "Salary");
    }

    #[test]
    fn test_parse_date_8digit() {
        assert_eq!(parse_date_to_iso("20250115").unwrap(), "2025-01-15");
    }

    #[test]
    fn test_parse_date_14digit() {
        assert_eq!(parse_date_to_iso("20250116120000").unwrap(), "2025-01-16");
    }

    #[test]
    fn test_parse_date_too_short() {
        assert!(parse_date_to_iso("2025").is_err());
    }

    #[test]
    fn test_parse_date_iso_format() {
        assert_eq!(parse_date_to_iso("2025-01-15").unwrap(), "2025-01-15");
    }

    #[test]
    fn test_parse_date_iso_with_time() {
        assert_eq!(
            parse_date_to_iso("2025-01-15 12:30:00").unwrap(),
            "2025-01-15"
        );
    }

    #[test]
    fn test_ofx_txn_id_deterministic() {
        let id1 = ofx_txn_id("bank/savings/statement.ofx", "TXN001");
        let id2 = ofx_txn_id("bank/savings/statement.ofx", "TXN001");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("ofx-"));
        assert_eq!(id1.len(), 4 + 12); // "ofx-" + 12 hex chars
    }

    #[test]
    fn test_ofx_txn_id_different_fitids() {
        let id1 = ofx_txn_id("path.ofx", "A");
        let id2 = ofx_txn_id("path.ofx", "B");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_ofx_to_transactions() {
        let ofx = parse_ofx(SAMPLE_OFX).unwrap();
        let rules = RulesFile::default();
        let txns = ofx_to_transactions(
            &ofx,
            "assets:bank:savings",
            "bank/savings/statement.ofx",
            &rules,
        )
        .unwrap();
        assert_eq!(txns.len(), 2);

        let t0 = &txns[0];
        assert_eq!(t0.date, "2025-01-15");
        assert_eq!(t0.narration.as_deref(), Some("Coffee Shop"));
        assert_eq!(t0.postings[0].account, "assets:bank:savings");
        assert_eq!(t0.postings[0].commodity, "AUD");
        assert!((t0.postings[0].amount - (-4.50)).abs() < 1e-9);
        assert_eq!(t0.postings[1].account, "expenses:unknown");
        assert!(t0.meta.as_ref().unwrap().contains("ofx-"));

        let t1 = &txns[1];
        assert_eq!(t1.date, "2025-01-16");
        assert_eq!(t1.narration.as_deref(), Some("Salary"));
    }

    #[test]
    fn test_ofx_payee_condition_rule_matches_memo() {
        // OFX carries a row's description in MEMO, not a separate payee field.
        // A categorization rule gated on `payee_condition` must still match, so
        // interest-style rules route OFX rows correctly without needing a
        // separate payee-rename rule. Regression: empty-payee OFX rows used to
        // fall through to the default expense account.
        let ofx = parse_ofx(SAMPLE_OFX).unwrap();
        let rules = RulesFile {
            rules: vec![crate::rules::Rule {
                id: "r1".to_string(),
                pattern: "*Salary*".to_string(),
                match_field: None,
                payee: None,
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some("income:salary".to_string()),
                fee_account: None,
                payee_condition: Some("*Salary*".to_string()),
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,
                postings: vec![],
            }],
        };
        let txns =
            ofx_to_transactions(&ofx, "assets:bank:savings", "p.ofx", &rules).unwrap();
        // SAMPLE_OFX t1 is the "Salary" credit.
        assert_eq!(txns[1].postings[1].account, "income:salary");
    }

    #[test]
    fn test_ofx_to_transactions_with_rules() {
        let ofx = parse_ofx(SAMPLE_OFX).unwrap();
        let rules = RulesFile {
            rules: vec![crate::rules::Rule {
                id: "r1".to_string(),
                pattern: "Coffee*".to_string(),
                match_field: None,
                payee: Some("Cafe".to_string()),
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some("expenses:food".to_string()),
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            }],
        };
        let txns = ofx_to_transactions(
            &ofx,
            "assets:bank:savings",
            "bank/savings/statement.ofx",
            &rules,
        )
        .unwrap();
        assert_eq!(txns[0].display_payee.as_deref(), Some("Cafe"));
        assert_eq!(txns[0].postings[1].account, "expenses:food");
    }

    #[test]
    fn test_parse_ofx_empty_fitid_synthesized() {
        // Some banks (e.g. CBA savings) emit an empty <FITID> on interest rows.
        // The parser must synthesize a deterministic key instead of erroring,
        // otherwise a single such file aborts the whole pipeline rebuild.
        let doc = r#"<OFX><STMTTRN>
<TRNTYPE>CREDIT
<DTPOSTED>20250301
<TRNAMT>539.72
<FITID>
<MEMO>Credit Interest
</STMTTRN></OFX>"#;
        let ofx = parse_ofx(doc).unwrap();
        assert_eq!(ofx.transactions.len(), 1);
        assert!(
            ofx.transactions[0].fitid.starts_with("syn-"),
            "empty FITID should be synthesized, got {:?}",
            ofx.transactions[0].fitid
        );
    }

    #[test]
    fn test_synth_fitid_distinct_within_file() {
        // Two genuinely separate rows that share date+amount+memo within one
        // file must get distinct keys so they are not collapsed into one.
        let doc = r#"<OFX>
<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20250301<TRNAMT>5.00<FITID><MEMO>Credit Interest</STMTTRN>
<STMTTRN><TRNTYPE>CREDIT<DTPOSTED>20250301<TRNAMT>5.00<FITID><MEMO>Credit Interest</STMTTRN>
</OFX>"#;
        let ofx = parse_ofx(doc).unwrap();
        assert_eq!(ofx.transactions.len(), 2);
        assert_ne!(ofx.transactions[0].fitid, ofx.transactions[1].fitid);
    }

    #[test]
    fn test_synth_fitid_stable_across_files() {
        // The same physical row appearing in two overlapping exports must
        // synthesize the same key, so OFX-vs-OFX dedupe (which keys on FITID)
        // still collapses it.
        let row = r#"<OFX><STMTTRN>
<TRNTYPE>CREDIT
<DTPOSTED>20250301
<TRNAMT>539.72
<FITID>
<MEMO>Credit Interest
</STMTTRN></OFX>"#;
        let a = parse_ofx(row).unwrap();
        let b = parse_ofx(row).unwrap();
        assert_eq!(a.transactions[0].fitid, b.transactions[0].fitid);
    }

    #[test]
    fn test_parse_ofx_empty() {
        let ofx = parse_ofx("no transactions here").unwrap();
        assert_eq!(ofx.transactions.len(), 0);
    }

    #[test]
    fn test_extract_tag_value_case_insensitive() {
        let content = "<curdef>AUD\n";
        assert_eq!(
            extract_tag_value(content, "CURDEF"),
            Some("AUD".to_string())
        );
    }

    #[test]
    fn test_parse_ofx_non_ascii_memo() {
        let ofx = "<OFX><CURDEF>AUD<STMTTRN>\n\
            <TRNTYPE>DEBIT\n\
            <DTPOSTED>20250115\n\
            <TRNAMT>-10.00\n\
            <FITID>TXN99\n\
            <MEMO>Caf\u{00e9} Purchase\n\
            </STMTTRN></OFX>";
        let result = parse_ofx(ofx).unwrap();
        assert_eq!(result.transactions.len(), 1);
        assert_eq!(result.transactions[0].fitid, "TXN99");
        // Non-ASCII 'é' is replaced with '?' during sanitization
        assert!(result.transactions[0].memo.contains("Caf"));
    }
}
