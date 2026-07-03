//! Backfill missing `_rules.json` entries for existing trade links.
//!
//! Older versions of `save_trade_link` skipped writing rules whenever the two
//! linked legs shared a `txn_id` (the on-chain swap pattern, e.g. Solana DEX
//! swaps). The link was stored in metadata so the UI rendered the swap, but
//! the underlying ledger contras stayed `expenses:unknown`. Once the writer
//! was fixed only newly-created links benefit — pre-existing links remain
//! missing their rules. This module locates each link's two legs in the
//! per-folder ledgers and writes any missing `trade-{id}-sell` /
//! `trade-{id}-buy` rules into the right `_rules.json`.
//!
//! The logic lives in the library (rather than the bin) so it's covered by
//! unit tests.
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use crate::automerge_store::TradeLink;
use crate::ledger_parser::{parse_transactions, Transaction};
use crate::rules::{build_trade_link_rules, Rule, RulesFile};

fn extract_meta_anchor(meta: &Option<String>, prefix: &str) -> Option<String> {
    meta.as_ref().and_then(|m| {
        m.split(',')
            .map(|p| p.trim())
            .find(|p| p.starts_with(prefix))
            .map(String::from)
    })
}

/// Extract the `txn:` value from a transaction's `meta` field.
pub fn extract_txn_id(meta: &Option<String>) -> Option<String> {
    extract_meta_anchor(meta, "txn:")
}

/// Extract the `leg:` value from a transaction's `meta` field. Each leg of a
/// shared-`txn:` swap carries its own `leg:` id, so this is the per-leg anchor.
pub fn extract_leg_id(meta: &Option<String>) -> Option<String> {
    extract_meta_anchor(meta, "leg:")
}

/// True when the transaction's first `assets:*` posting is negative — the
/// disposal side of a trade.
pub fn is_sell_side(t: &Transaction) -> bool {
    t.postings
        .iter()
        .find(|p| p.account.starts_with("assets:"))
        .map(|p| p.amount < 0.0)
        .unwrap_or(false)
}

/// Find which folder owns a given txn_id and return all matching transactions
/// in that folder. When the same txn_id appears in multiple folders we take
/// the first match — trade links are folder-scoped via `_rules.json`, so
/// cross-folder duplicates would already be ambiguous.
pub fn locate_txn<'a>(
    by_folder: &'a HashMap<String, Vec<Transaction>>,
    txn_id: &str,
) -> Option<(&'a str, Vec<&'a Transaction>)> {
    for (folder, txns) in by_folder {
        let matches: Vec<&Transaction> = txns
            .iter()
            .filter(|t| extract_txn_id(&t.meta).as_deref() == Some(txn_id))
            .collect();
        if !matches.is_empty() {
            return Some((folder.as_str(), matches));
        }
    }
    None
}

/// Walk every per-folder `ledger.transactions` under `generated_dir` and group
/// transactions by source-folder path (relative to `sources_dir`). Only folders
/// that still exist under `sources_dir` are included.
pub fn load_per_folder_ledgers(
    generated_dir: &Path,
    sources_dir: &Path,
) -> HashMap<String, Vec<Transaction>> {
    let mut by_folder: HashMap<String, Vec<Transaction>> = HashMap::new();
    if !generated_dir.exists() {
        return by_folder;
    }
    for entry in walkdir::WalkDir::new(generated_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file()
            || path.file_name().and_then(|n| n.to_str()) != Some("ledger.transactions")
        {
            continue;
        }
        let Some(parent) = path.parent() else { continue };
        let Ok(rel) = parent.strip_prefix(generated_dir) else {
            continue;
        };
        if rel.as_os_str().is_empty() || !sources_dir.join(rel).exists() {
            continue;
        }
        let Ok(contents) = std::fs::read_to_string(path) else {
            continue;
        };
        by_folder.insert(
            rel.to_string_lossy().into_owned(),
            parse_transactions(&contents).transactions,
        );
    }
    by_folder
}

/// Outcome of analysing a single trade link.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkOutcome {
    /// Link already has its `trade-{id}-*` rules in `_rules.json`. No-op.
    AlreadyOk,
    /// Couldn't resolve the link (txns missing from ledger, legs in different
    /// folders, sign-detection failed, etc).
    Unresolved(String),
    /// Migration would add `count` rules to `<folder>/_rules.json`. The
    /// `sell_id` and `buy_id` are the resolved txn_ids passed to
    /// `build_trade_link_rules`.
    WouldAdd {
        folder: String,
        sell_id: String,
        buy_id: String,
        count: usize,
    },
}

/// Decide what (if anything) to do for a single link, without touching disk
/// for the actual write. Pure given the inputs — drives both dry-run and
/// write modes from the same code path.
pub fn classify_link(
    link: &TradeLink,
    by_folder: &HashMap<String, Vec<Transaction>>,
    sources_dir: &Path,
) -> LinkOutcome {
    let a = locate_txn(by_folder, &link.txn_id_a);
    let b = locate_txn(by_folder, &link.txn_id_b);
    let (Some((folder_a, txns_a)), Some((folder_b, txns_b))) = (a, b) else {
        return LinkOutcome::Unresolved(format!(
            "could not locate txn(s) {} / {}",
            link.txn_id_a, link.txn_id_b
        ));
    };
    if folder_a != folder_b {
        return LinkOutcome::Unresolved(format!(
            "legs are in different folders ({folder_a} vs {folder_b})"
        ));
    }

    let pool: Vec<&Transaction> = txns_a.iter().chain(txns_b.iter()).copied().collect();
    let sell = pool.iter().find(|t| is_sell_side(t));
    let buy = pool.iter().find(|t| !is_sell_side(t));
    let (Some(sell), Some(buy)) = (sell, buy) else {
        return LinkOutcome::Unresolved("could not identify sell/buy by sign".to_string());
    };
    let Some(sell_id) = extract_txn_id(&sell.meta) else {
        return LinkOutcome::Unresolved("sell leg has no txn_id meta".to_string());
    };
    let Some(buy_id) = extract_txn_id(&buy.meta) else {
        return LinkOutcome::Unresolved("buy leg has no txn_id meta".to_string());
    };

    let folder_path = sources_dir.join(folder_a);
    let rules = RulesFile::load(&folder_path);
    let prefix = format!("trade-{}-", link.id);
    if rules.rules.iter().any(|r| r.id.starts_with(&prefix)) {
        return LinkOutcome::AlreadyOk;
    }

    LinkOutcome::WouldAdd {
        folder: folder_a.to_string(),
        sell_id,
        buy_id,
        count: 2,
    }
}

#[derive(Debug, Default)]
pub struct Report {
    pub links_total: usize,
    pub links_already_ok: usize,
    pub links_unresolved: usize,
    pub rules_added: usize,
    pub folders_changed: BTreeSet<String>,
}

/// Run `classify_link` for every link, optionally writing the resulting rules.
/// Lines describing each decision are appended to `log` so callers can decide
/// how to surface them (CLI prints them, tests assert on them).
pub fn repair_links(
    links: &[TradeLink],
    by_folder: &HashMap<String, Vec<Transaction>>,
    sources_dir: &Path,
    write: bool,
    log: &mut Vec<String>,
) -> Report {
    let mut report = Report {
        links_total: links.len(),
        ..Report::default()
    };
    for link in links {
        match classify_link(link, by_folder, sources_dir) {
            LinkOutcome::AlreadyOk => {
                report.links_already_ok += 1;
            }
            LinkOutcome::Unresolved(reason) => {
                log.push(format!("  link {}: {reason} — skipping", link.id));
                report.links_unresolved += 1;
            }
            LinkOutcome::WouldAdd {
                folder,
                sell_id,
                buy_id,
                count,
            } => {
                log.push(format!(
                    "  link {}: + {count} rule(s) in {folder}/_rules.json (sell={sell_id}, buy={buy_id})",
                    link.id,
                ));
                report.rules_added += count;
                report.folders_changed.insert(folder.clone());
                if write {
                    let folder_path = sources_dir.join(&folder);
                    let mut rules = RulesFile::load(&folder_path);
                    rules.insert_rules(build_trade_link_rules(&link.id, &sell_id, &buy_id));
                    if let Err(e) = rules.save(&folder_path) {
                        log.push(format!("  link {}: FAILED to save rules: {e}", link.id));
                    }
                }
            }
        }
    }
    report
}

/// Decide which folder a trade link's rules belong in, and build the rule
/// pair: `(folder_rel, [sell_rule, buy_rule])`.
///
/// For an on-chain swap whose two legs share one `txn:` id, a plain
/// `txn:`-anchored rule is outranked by any pre-existing per-leg (`leg:`)
/// categorisation on those legs (`leg:` beats `txn:`), and — because rules are
/// gathered leaf-folder-first — a rule written to an ancestor folder also loses
/// the tie. So for a resolvable shared-txn swap we anchor each rule on its own
/// `leg:` id and target the leaf folder that owns the legs, so the trade
/// categorisation wins outright. For distinct txns (a deposit/withdrawal pair),
/// or when the legs can't be resolved, this is the legacy `txn:`-anchored
/// behavior against `default_folder`.
pub fn plan_trade_link_rules(
    link_id: &str,
    sell_txn_id: &str,
    buy_txn_id: &str,
    default_folder: &str,
    by_folder: &HashMap<String, Vec<Transaction>>,
) -> (String, [Rule; 2]) {
    if sell_txn_id == buy_txn_id {
        if let Some((folder, txns)) = locate_txn(by_folder, sell_txn_id) {
            let sell_leg = txns
                .iter()
                .find(|t| is_sell_side(t))
                .and_then(|t| extract_leg_id(&t.meta));
            let buy_leg = txns
                .iter()
                .find(|t| !is_sell_side(t))
                .and_then(|t| extract_leg_id(&t.meta));
            if let (Some(sell_leg), Some(buy_leg)) = (sell_leg, buy_leg) {
                if sell_leg != buy_leg {
                    return (
                        folder.to_string(),
                        build_trade_link_rules(link_id, &sell_leg, &buy_leg),
                    );
                }
            }
        }
    }
    (
        default_folder.to_string(),
        build_trade_link_rules(link_id, sell_txn_id, buy_txn_id),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger_parser::Posting;

    fn posting(account: &str, amount: f64, commodity: &str) -> Posting {
        Posting {
            account: account.to_string(),
            amount,
            amount_text: amount.to_string(),
            commodity: commodity.to_string(),
            remainder: None,
            cost: None,
            price: None,
        }
    }

    fn txn(meta: &str, postings: Vec<Posting>) -> Transaction {
        Transaction {
            date: "2024-05-01".to_string(),
            datetime: "2024-05-01 11:44:03".to_string(),
            payee: Some("test".to_string()),
            display_payee: None,
            narration: Some("token_transfer".to_string()),
            meta: Some(meta.to_string()),
            status: Some('*'),
            amount: 0.0,
            amount_commodity: String::new(),
            display_amount_commodity: None,
            postings,
            fee: None,
            fee_commodity: None,
        }
    }

    fn link(id: &str, a: &str, b: &str) -> TradeLink {
        TradeLink {
            id: id.to_string(),
            txn_id_a: a.to_string(),
            txn_id_b: b.to_string(),
            device_origin: "test".to_string(),
            created_at: 0,
        }
    }

    fn leg_rule(id: &str, pattern: &str, contra: &str) -> Rule {
        Rule {
            id: id.to_string(),
            pattern: pattern.to_string(),
            match_field: Some("meta".to_string()),
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,
            amount_account: Some(contra.to_string()),
            fee_account: None,
            postings: vec![],
        }
    }

    fn meta_only(meta: &str) -> crate::rules::MatchFields<'_> {
        crate::rules::MatchFields {
            payee: None,
            display_payee: None,
            narration: None,
            meta: Some(meta),
            commodity: None,
            display_commodity: None,
            amount: None,
            fee: None,
        }
    }

    /// Regression: the user-reported bug. Both legs of one on-chain swap share a
    /// `txn:` id but carry distinct `leg:` ids. The sell leg already has a
    /// per-leg rule routing it to `assets:crypto:lending` (a prior manual
    /// categorisation). Linking the pair as a trade must flip the leg to
    /// `equity:trading:sell` — but a plain `txn:`-anchored trade rule is
    /// outranked by the `leg:` rule (`leg:` beats `txn:`), so it never takes
    /// effect. The fix anchors the trade rule on the leg id, co-located in the
    /// leaf folder, so it wins.
    #[test]
    fn shared_txn_link_overrides_existing_per_leg_rule() {
        let sell = txn(
            "txn:onchain, leg:l-sell",
            vec![
                posting("assets:crypto:wallet:eth", -1.0, "ETH"),
                posting("equity:trading", 1.0, "ETH"),
            ],
        );
        let buy = txn(
            "txn:onchain, leg:l-buy",
            vec![
                posting("assets:crypto:wallet:eth", 1.0, "WETH"),
                posting("equity:trading", -1.0, "WETH"),
            ],
        );
        let mut by_folder = HashMap::new();
        by_folder.insert("wallet".to_string(), vec![sell, buy]);

        let (folder, rules) =
            plan_trade_link_rules("xyz", "txn:onchain", "txn:onchain", "wallet", &by_folder);
        assert_eq!(folder, "wallet", "rules must target the leaf folder owning the legs");

        // Pre-existing per-leg categorisation lives in that same leaf folder.
        let mut rf = RulesFile {
            rules: vec![leg_rule("old-sell", "leg:l-sell", "assets:crypto:lending")],
        };
        rf.insert_rules(rules);

        // The sell leg's meta carries both the shared txn id and its own leg id.
        let matched = rf.find_match_prioritized(&meta_only("txn:onchain, leg:l-sell"));
        assert_eq!(
            matched.and_then(|r| r.amount_account.as_deref()),
            Some("equity:trading:sell"),
            "trade-link must override the prior per-leg rule for the sell leg",
        );
    }

    #[test]
    fn extract_txn_id_picks_meta_field() {
        assert_eq!(
            extract_txn_id(&Some("txn:abc, rule:r1".to_string())),
            Some("txn:abc".to_string())
        );
        assert_eq!(extract_txn_id(&Some("rule:only".to_string())), None);
        assert_eq!(extract_txn_id(&None), None);
    }

    #[test]
    fn is_sell_side_uses_first_asset_posting() {
        let sell = txn(
            "txn:1",
            vec![
                posting("assets:wallet", -100.0, "USDC"),
                posting("expenses:unknown", 100.0, "USDC"),
            ],
        );
        let buy = txn(
            "txn:2",
            vec![
                posting("assets:wallet", 50.0, "SOL"),
                posting("expenses:unknown", -50.0, "SOL"),
            ],
        );
        let no_asset = txn(
            "txn:3",
            vec![posting("expenses:unknown", 1.0, "USD")],
        );
        assert!(is_sell_side(&sell));
        assert!(!is_sell_side(&buy));
        assert!(!is_sell_side(&no_asset));
    }

    #[test]
    fn classify_same_txn_id_link_proposes_rules() {
        let dir = tempfile::tempdir().unwrap();
        // Same txn_id, two postings (Solana on-chain pattern)
        let sell = txn(
            "txn:onchain",
            vec![
                posting("assets:wallet:sol", -1420.0, "USDC"),
                posting("expenses:unknown", 1420.0, "USDC"),
            ],
        );
        let buy = txn(
            "txn:onchain",
            vec![
                posting("assets:wallet:sol", 11.67, "SOL"),
                posting("expenses:unknown", -11.67, "SOL"),
            ],
        );
        let mut by_folder = HashMap::new();
        by_folder.insert("exchange".to_string(), vec![sell, buy]);

        let outcome = classify_link(
            &link("abc", "txn:onchain", "txn:onchain"),
            &by_folder,
            dir.path(),
        );

        match outcome {
            LinkOutcome::WouldAdd {
                folder,
                sell_id,
                buy_id,
                count,
            } => {
                assert_eq!(folder, "exchange");
                assert_eq!(sell_id, "txn:onchain");
                assert_eq!(buy_id, "txn:onchain");
                assert_eq!(count, 2);
            }
            other => panic!("expected WouldAdd, got {other:?}"),
        }
    }

    #[test]
    fn classify_distinct_txn_id_link_proposes_rules() {
        let dir = tempfile::tempdir().unwrap();
        let sell = txn(
            "txn:s1",
            vec![
                posting("assets:wallet:bybit", -0.5, "ETH"),
                posting("equity:trading", 0.5, "ETH"),
            ],
        );
        let buy = txn(
            "txn:b1",
            vec![
                posting("assets:wallet:bybit", 1500.0, "USDC"),
                posting("equity:trading", -1500.0, "USDC"),
            ],
        );
        let mut by_folder = HashMap::new();
        by_folder.insert("bybit".to_string(), vec![sell, buy]);

        let outcome = classify_link(
            &link("xyz", "txn:s1", "txn:b1"),
            &by_folder,
            dir.path(),
        );
        match outcome {
            LinkOutcome::WouldAdd {
                folder,
                sell_id,
                buy_id,
                ..
            } => {
                assert_eq!(folder, "bybit");
                assert_eq!(sell_id, "txn:s1");
                assert_eq!(buy_id, "txn:b1");
            }
            other => panic!("expected WouldAdd, got {other:?}"),
        }
    }

    #[test]
    fn classify_returns_already_ok_when_rules_exist() {
        let dir = tempfile::tempdir().unwrap();
        let folder_dir = dir.path().join("exchange");
        std::fs::create_dir_all(&folder_dir).unwrap();
        // Pre-existing trade-link rules
        let mut rf = RulesFile::default();
        rf.rules
            .extend(build_trade_link_rules("abc", "txn:s", "txn:b"));
        rf.save(&folder_dir).unwrap();

        let sell = txn(
            "txn:s",
            vec![
                posting("assets:wallet", -1.0, "ETH"),
                posting("equity:trading", 1.0, "ETH"),
            ],
        );
        let buy = txn(
            "txn:b",
            vec![
                posting("assets:wallet", 100.0, "USDC"),
                posting("equity:trading", -100.0, "USDC"),
            ],
        );
        let mut by_folder = HashMap::new();
        by_folder.insert("exchange".to_string(), vec![sell, buy]);

        let outcome = classify_link(&link("abc", "txn:s", "txn:b"), &by_folder, dir.path());
        assert_eq!(outcome, LinkOutcome::AlreadyOk);
    }

    #[test]
    fn classify_unresolved_when_txn_missing() {
        let dir = tempfile::tempdir().unwrap();
        let by_folder: HashMap<String, Vec<Transaction>> = HashMap::new();
        match classify_link(&link("z", "txn:gone-1", "txn:gone-2"), &by_folder, dir.path()) {
            LinkOutcome::Unresolved(_) => {}
            other => panic!("expected Unresolved, got {other:?}"),
        }
    }

    #[test]
    fn classify_unresolved_when_legs_in_different_folders() {
        let dir = tempfile::tempdir().unwrap();
        let sell = txn(
            "txn:s",
            vec![
                posting("assets:wallet", -1.0, "ETH"),
                posting("equity:trading", 1.0, "ETH"),
            ],
        );
        let buy = txn(
            "txn:b",
            vec![
                posting("assets:wallet", 100.0, "USDC"),
                posting("equity:trading", -100.0, "USDC"),
            ],
        );
        let mut by_folder = HashMap::new();
        by_folder.insert("folder-a".to_string(), vec![sell]);
        by_folder.insert("folder-b".to_string(), vec![buy]);

        match classify_link(&link("z", "txn:s", "txn:b"), &by_folder, dir.path()) {
            LinkOutcome::Unresolved(reason) => assert!(reason.contains("different folders")),
            other => panic!("expected Unresolved, got {other:?}"),
        }
    }

    #[test]
    fn classify_unresolved_when_no_buy_side() {
        let dir = tempfile::tempdir().unwrap();
        // Both legs are sells (e.g. liquidation-of-pair) — sign detection fails.
        let sell_a = txn(
            "txn:1",
            vec![
                posting("assets:wallet", -1.0, "ETH"),
                posting("equity:trading", 1.0, "ETH"),
            ],
        );
        let sell_b = txn(
            "txn:2",
            vec![
                posting("assets:wallet", -100.0, "USDC"),
                posting("equity:trading", 100.0, "USDC"),
            ],
        );
        let mut by_folder = HashMap::new();
        by_folder.insert("exchange".to_string(), vec![sell_a, sell_b]);

        match classify_link(&link("z", "txn:1", "txn:2"), &by_folder, dir.path()) {
            LinkOutcome::Unresolved(reason) => assert!(reason.contains("sign")),
            other => panic!("expected Unresolved, got {other:?}"),
        }
    }

    #[test]
    fn repair_links_writes_rules_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let sell = txn(
            "txn:onchain",
            vec![
                posting("assets:wallet:sol", -1.0, "USDC"),
                posting("expenses:unknown", 1.0, "USDC"),
            ],
        );
        let buy = txn(
            "txn:onchain",
            vec![
                posting("assets:wallet:sol", 0.5, "SOL"),
                posting("expenses:unknown", -0.5, "SOL"),
            ],
        );
        let mut by_folder = HashMap::new();
        by_folder.insert("solana".to_string(), vec![sell, buy]);
        let links = vec![link("xyz", "txn:onchain", "txn:onchain")];

        let mut log = Vec::new();
        let report = repair_links(&links, &by_folder, dir.path(), true, &mut log);
        assert_eq!(report.links_total, 1);
        assert_eq!(report.rules_added, 2);
        assert_eq!(report.folders_changed.len(), 1);
        assert_eq!(report.links_unresolved, 0);

        // Verify the rules actually landed on disk with amount_condition set.
        let written = RulesFile::load(&dir.path().join("solana"));
        let trade_rules: Vec<_> = written
            .rules
            .iter()
            .filter(|r| r.id.starts_with("trade-xyz-"))
            .collect();
        assert_eq!(trade_rules.len(), 2);
        let sell_rule = trade_rules.iter().find(|r| r.id.ends_with("-sell")).unwrap();
        let buy_rule = trade_rules.iter().find(|r| r.id.ends_with("-buy")).unwrap();
        assert_eq!(sell_rule.amount_condition.as_deref(), Some("<0"));
        assert_eq!(buy_rule.amount_condition.as_deref(), Some(">0"));
    }

    #[test]
    fn repair_links_dry_run_does_not_write() {
        let dir = tempfile::tempdir().unwrap();
        let sell = txn(
            "txn:s",
            vec![
                posting("assets:wallet", -1.0, "ETH"),
                posting("equity:trading", 1.0, "ETH"),
            ],
        );
        let buy = txn(
            "txn:b",
            vec![
                posting("assets:wallet", 100.0, "USDC"),
                posting("equity:trading", -100.0, "USDC"),
            ],
        );
        let mut by_folder = HashMap::new();
        by_folder.insert("exchange".to_string(), vec![sell, buy]);
        let links = vec![link("k", "txn:s", "txn:b")];

        let mut log = Vec::new();
        let report = repair_links(&links, &by_folder, dir.path(), false, &mut log);
        assert_eq!(report.rules_added, 2);
        // Nothing on disk
        let written = RulesFile::load(&dir.path().join("exchange"));
        assert!(!written.rules.iter().any(|r| r.id.starts_with("trade-")));
    }
}
