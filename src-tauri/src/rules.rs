use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Bump when rule-matching semantics change so cached pipeline output is invalidated.
/// v2: tier-sorted prioritized matching (meta > payee+narration > payee > narration > other > general).
/// v3: per-leg `leg:` id anchoring + within-meta-tier order (leg > txn > other meta);
///     the CSV transform now stamps `leg:` ids on shared-`txn:` legs.
pub const MATCHER_VERSION: &str = "v3";

const RULES_FILENAME: &str = "_rules.json";
const LABELS_FILENAME: &str = "_labels.json";
const CONFIG_FILENAME: &str = "_config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: String,
    pub pattern: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commodity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_condition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_condition: Option<String>,
    /// Additional payee match constraint — transaction payee must match this pattern.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payee_condition: Option<String>,
    /// Additional narration match constraint — ANDed with `pattern` at match time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub narration_condition: Option<String>,
    /// Additional commodity match constraint — ANDed with `pattern` at match time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commodity_condition: Option<String>,
    /// Additional meta match constraint — ANDed with `pattern` at match time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta_condition: Option<String>,
    /// Contra (amount) account — replaces the default expense account.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount_account: Option<String>,
    /// Fee account — used when the transaction has a non-zero fee.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fee_account: Option<String>,
    /// Legacy field: read from old files for migration, never written back.
    #[serde(default, skip_serializing)]
    pub postings: Vec<String>,
}

impl Rule {
    /// A pure commodity-rename rule: match_field is "commodity" and the effect is renaming.
    pub fn is_commodity_rename(&self) -> bool {
        self.match_field.as_deref() == Some("commodity") && self.commodity.is_some()
    }

    /// A payee-only transform: sets payee but no account categorization or commodity rename.
    pub fn is_payee_transform(&self) -> bool {
        self.payee.is_some()
            && self.amount_account.is_none()
            && self.fee_account.is_none()
            && self.commodity.is_none()
    }

    /// True if this rule is a transform (data normalization) rather than categorization.
    pub fn is_transform(&self) -> bool {
        self.is_commodity_rename() || self.is_payee_transform()
    }

    fn has_meta_condition(&self) -> bool {
        self.match_field.as_deref() == Some("meta") || self.meta_condition.is_some()
    }

    fn has_payee_condition(&self) -> bool {
        self.match_field.as_deref() == Some("payee") || self.payee_condition.is_some()
    }

    fn has_narration_condition(&self) -> bool {
        self.match_field.as_deref() == Some("narration") || self.narration_condition.is_some()
    }

    /// Specificity tier for prioritized matching. Lower = more specific = higher precedence.
    ///   0: has meta condition (exact id-style match)
    ///   1: has both payee and narration constraints
    ///   2: has payee constraint only
    ///   3: has narration constraint only
    ///   4: other field-specific (commodity / amount / fee only)
    ///   5: general fallback (no payee/narration/meta constraint)
    pub fn specificity_tier(&self) -> u8 {
        if self.has_meta_condition() {
            return 0;
        }
        let p = self.has_payee_condition();
        let n = self.has_narration_condition();
        match (p, n) {
            (true, true) => 1,
            (true, false) => 2,
            (false, true) => 3,
            (false, false) => {
                if self.match_field.is_some()
                    || self.commodity_condition.is_some()
                    || self.amount_condition.is_some()
                    || self.fee_condition.is_some()
                {
                    4
                } else {
                    5
                }
            }
        }
    }

    /// Sub-ordering *within* the meta tier (tier 0): a per-leg anchor (`leg:`)
    /// is more specific than a shared-transaction anchor (`txn:`), which is more
    /// specific than any other meta rule (e.g. a `meta_condition` glob). Returns
    /// the lowest sub-priority (2) for every non-`meta` rule, so it has no effect
    /// outside tier 0. Mirrored in `find_match_prioritized` and
    /// `sort_by_specificity` so the on-disk order matches evaluation order.
    fn meta_anchor_rank(&self) -> u8 {
        if self.match_field.as_deref() == Some("meta") {
            match id_anchor_prefix(&self.pattern) {
                Some("leg:") => 0,
                Some("txn:") => 1,
                _ => 2,
            }
        } else {
            2
        }
    }
}

pub struct MatchFields<'a> {
    pub payee: Option<&'a str>,
    pub display_payee: Option<&'a str>,
    pub narration: Option<&'a str>,
    pub meta: Option<&'a str>,
    pub commodity: Option<&'a str>,
    pub display_commodity: Option<&'a str>,
    pub amount: Option<f64>,
    pub fee: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AmountCondition {
    Gt(f64),
    Gte(f64),
    Lt(f64),
    Lte(f64),
    Eq(f64),
    Range(f64, f64),
}

pub fn parse_amount_condition(s: &str) -> Option<AmountCondition> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    // Range: "10..500"
    if let Some((lo, hi)) = s.split_once("..") {
        let lo: f64 = lo.trim().parse().ok()?;
        let hi: f64 = hi.trim().parse().ok()?;
        return Some(AmountCondition::Range(lo, hi));
    }
    if let Some(rest) = s.strip_prefix(">=") {
        return rest.trim().parse().ok().map(AmountCondition::Gte);
    }
    if let Some(rest) = s.strip_prefix('>') {
        return rest.trim().parse().ok().map(AmountCondition::Gt);
    }
    if let Some(rest) = s.strip_prefix("<=") {
        return rest.trim().parse().ok().map(AmountCondition::Lte);
    }
    if let Some(rest) = s.strip_prefix('<') {
        return rest.trim().parse().ok().map(AmountCondition::Lt);
    }
    if let Some(rest) = s.strip_prefix('=') {
        return rest.trim().parse().ok().map(AmountCondition::Eq);
    }
    None
}

pub fn amount_matches(condition: &AmountCondition, amount: f64) -> bool {
    match condition {
        AmountCondition::Gt(v) => amount > *v,
        AmountCondition::Gte(v) => amount >= *v,
        AmountCondition::Lt(v) => amount < *v,
        AmountCondition::Lte(v) => amount <= *v,
        AmountCondition::Eq(v) => (amount - *v).abs() < 1e-9,
        AmountCondition::Range(lo, hi) => amount >= *lo && amount <= *hi,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RulesFile {
    pub rules: Vec<Rule>,
}

impl RulesFile {
    pub fn load(folder: &Path) -> Self {
        let path = folder.join(RULES_FILENAME);
        if !path.exists() {
            return Self::default();
        }
        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to read {}: {e}", path.display());
                return Self::default();
            }
        };
        let mut rules_file: Self = match serde_json::from_str(&contents) {
            Ok(rules) => rules,
            Err(e) => {
                eprintln!("warning: failed to parse {}: {e}", path.display());
                return Self::default();
            }
        };
        // Migrate legacy postings → amount_account / fee_account
        for rule in &mut rules_file.rules {
            if rule.postings.is_empty()
                || rule.amount_account.is_some()
                || rule.fee_account.is_some()
            {
                continue;
            }
            let first = &rule.postings[0];
            let first_account = first.split_whitespace().next().unwrap_or("").to_string();
            if first.contains("${fee}") {
                rule.fee_account = Some(first_account);
            } else {
                rule.amount_account = Some(first_account);
            }
            if rule.postings.len() >= 2 {
                let second = &rule.postings[1];
                let second_account = second.split_whitespace().next().unwrap_or("").to_string();
                rule.fee_account = Some(second_account);
            }
            rule.postings.clear();
        }
        rules_file
    }

    /// Stable-sort rules by specificity tier (most specific first), then by the
    /// within-meta-tier anchor rank (`leg:` before `txn:` before other meta),
    /// preserving existing intra-group order. Mirrors the runtime sort in
    /// `find_match_prioritized` so the on-disk file reflects evaluation order.
    pub fn sort_by_specificity(&mut self) {
        self.rules
            .sort_by_key(|r| (r.specificity_tier(), r.meta_anchor_rank()));
    }

    pub fn save(&self, folder: &Path) -> Result<(), String> {
        // Filter out no-op rules (rules whose only effect is assigning the default
        // expense account) so they never get persisted.
        let mut filtered = RulesFile {
            rules: self
                .rules
                .iter()
                .filter(|r| !rule_is_noop(r, crate::FALLBACK_EXPENSE_ACCOUNT))
                .cloned()
                .collect(),
        };
        filtered.sort_by_specificity();
        let path = folder.join(RULES_FILENAME);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let contents = crate::to_sorted_json_pretty(&filtered).map_err(|e| e.to_string())?;
        fs::write(&path, contents).map_err(|e| e.to_string())
    }

    pub fn hash(&self) -> String {
        let v = serde_json::to_value(&self.rules).unwrap_or_default();
        let serialized = serde_json::to_string(&v).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Insert a rule, honouring the id top-anchor invariant.
    /// Id-anchored rules (bare `txn:<hash>` or `leg:<id>` pattern with
    /// `match_field == meta`) land at index 0; everything else appends. Caller
    /// still owns dedup and the `save()` call.
    pub fn insert_rule(&mut self, rule: Rule) {
        if rule.match_field.as_deref() == Some("meta") && is_id_anchored(&rule.pattern) {
            self.rules.insert(0, rule);
        } else {
            self.rules.push(rule);
        }
    }

    /// Bulk variant. Each item is dispatched via `insert_rule`, so a
    /// trade-link `[sell, buy]` pair lands at the top of the file with
    /// `buy` at index 0 and `sell` at index 1.
    pub fn insert_rules<I: IntoIterator<Item = Rule>>(&mut self, new_rules: I) {
        for r in new_rules {
            self.insert_rule(r);
        }
    }

    /// One-shot migration: rewrite legacy `*txn:HASH*` patterns to the
    /// bare `txn:HASH` form, then stable-partition so all txn-anchored
    /// rules sit at the top in their original relative order. Idempotent
    /// — re-running on an already-migrated file is a no-op.
    pub fn migrate_legacy_anchored(&mut self) -> MigrationStats {
        let mut stats = MigrationStats::default();
        for r in &mut self.rules {
            if r.match_field.as_deref() != Some("meta") {
                continue;
            }
            if let Some(bare) = canonicalize_legacy_anchored(&r.pattern) {
                r.pattern = bare;
                stats.patterns_canonicalized += 1;
            }
        }
        let mut anchored: Vec<Rule> = Vec::new();
        let mut rest: Vec<Rule> = Vec::new();
        for r in self.rules.drain(..) {
            let is_anchored =
                r.match_field.as_deref() == Some("meta") && is_id_anchored(&r.pattern);
            if is_anchored {
                // "Promoted" = preceded by at least one non-anchored rule
                // in the original sequence (i.e. the rule moves up).
                if !rest.is_empty() {
                    stats.rules_promoted += 1;
                }
                anchored.push(r);
            } else {
                rest.push(r);
            }
        }
        self.rules = anchored;
        self.rules.append(&mut rest);
        stats
    }

    /// One-shot migration: rewrite the id of every `ai-*` rule to its
    /// content-derived form (`ai_rule_id`). If two ai-rules collapse to
    /// the same id (true duplicates), the first is kept and the rest
    /// dropped. Non-ai rules pass through untouched — even if they share
    /// ids with each other, that's a separate problem with a different
    /// resolution. Idempotent.
    pub fn dedupe_ai_ids(&mut self) -> AiDedupeStats {
        let mut stats = AiDedupeStats::default();
        let mut seen_ai_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut kept: Vec<Rule> = Vec::with_capacity(self.rules.len());
        for mut r in self.rules.drain(..) {
            if !r.id.starts_with("ai-") {
                kept.push(r);
                continue;
            }
            let new_id = ai_rule_id(&r);
            if new_id != r.id {
                stats.rules_renamed += 1;
            }
            r.id = new_id;
            if !seen_ai_ids.insert(r.id.clone()) {
                stats.duplicates_removed += 1;
                continue;
            }
            kept.push(r);
        }
        self.rules = kept;
        stats
    }

    pub fn find_match(&self, narration: &str) -> Option<&Rule> {
        self.rules
            .iter()
            .find(|r| wildcard_match(&r.pattern, narration))
    }

    pub fn find_match_any(&self, fields: &[&str]) -> Option<&Rule> {
        self.rules.iter().find(|r| {
            r.match_field.is_none() && fields.iter().any(|f| wildcard_match(&r.pattern, f))
        })
    }

    /// Return all commodity-rename rules that match the given fields.
    pub fn find_commodity_renames<'a>(&'a self, fields: &MatchFields) -> Vec<&'a Rule> {
        self.rules
            .iter()
            .filter(|r| {
                r.is_commodity_rename()
                    && fields
                        .commodity
                        .is_some_and(|c| wildcard_match(&r.pattern, c))
                    && rule_amount_matches(r, fields)
            })
            .collect()
    }

    /// Find the first matching payee-transform rule.
    pub fn find_payee_transform(&self, fields: &MatchFields) -> Option<&Rule> {
        let all_fields: Vec<&str> = [fields.payee, fields.narration, fields.meta]
            .into_iter()
            .flatten()
            .collect();
        self.rules.iter().find(|r| {
            r.is_payee_transform()
                && if let Some(ref field_name) = r.match_field {
                    let target = match field_name.as_str() {
                        "meta" => fields.meta,
                        "payee" => fields.payee,
                        "narration" => fields.narration,
                        _ => None,
                    };
                    if field_name == "meta" && is_id_anchored(&r.pattern) {
                        target.is_some_and(|t| meta_segment_match(&r.pattern, t))
                    } else {
                        target.is_some_and(|t| wildcard_match(&r.pattern, t))
                    }
                } else {
                    all_fields.iter().any(|f| wildcard_match(&r.pattern, f))
                }
                && rule_amount_matches(r, fields)
                && rule_payee_condition_matches(r, fields)
                && rule_other_conditions_match(r, fields)
        })
    }

    /// Prioritized matching: rules are evaluated in specificity order
    /// (meta > payee+narration > payee > narration > other field-specific > general).
    /// Within a tier, original array order is preserved (stable sort), so per-folder
    /// rules continue to beat root rules and earlier file entries beat later ones.
    /// Labels (payee/commodity transforms) live in `_labels.json` and are applied in
    /// the pre-pass; legacy transform rules in `_rules.json` are skipped here.
    /// Matches against both raw and display values for payee and commodity.
    pub fn find_match_prioritized(&self, fields: &MatchFields) -> Option<&Rule> {
        let mut indexed: Vec<(usize, &Rule)> = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| !r.is_transform())
            .collect();
        indexed.sort_by_key(|(idx, r)| (r.specificity_tier(), r.meta_anchor_rank(), *idx));
        indexed
            .into_iter()
            .find(|(_, r)| rule_matches(r, fields))
            .map(|(_, r)| r)
    }
}

fn rule_matches(rule: &Rule, fields: &MatchFields) -> bool {
    let pattern_matched = if let Some(ref field_name) = rule.match_field {
        let (target, fallback) = match field_name.as_str() {
            "meta" => (fields.meta, None),
            "payee" => (fields.payee, fields.display_payee),
            "narration" => (fields.narration, None),
            "commodity" => (fields.commodity, fields.display_commodity),
            _ => (None, None),
        };
        if field_name == "meta" && is_id_anchored(&rule.pattern) {
            target.is_some_and(|t| meta_segment_match(&rule.pattern, t))
        } else {
            target.is_some_and(|t| wildcard_match(&rule.pattern, t))
                || fallback.is_some_and(|t| wildcard_match(&rule.pattern, t))
        }
    } else {
        let all_fields: [Option<&str>; 4] = [
            fields.payee,
            fields.display_payee,
            fields.narration,
            fields.meta,
        ];
        all_fields
            .iter()
            .flatten()
            .any(|f| wildcard_match(&rule.pattern, f))
    };
    pattern_matched
        && rule_amount_matches(rule, fields)
        && rule_payee_condition_matches(rule, fields)
        && rule_other_conditions_match(rule, fields)
}

/// Labels file — payee renames and commodity renames applied in the pre-pass
/// before categorization rules. Stored in `_labels.json` per source folder.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LabelsFile {
    pub labels: Vec<Rule>,
}

impl LabelsFile {
    pub fn load(folder: &Path) -> Self {
        let path = folder.join(LABELS_FILENAME);
        if !path.exists() {
            return Self::default();
        }
        let contents = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("warning: failed to read {}: {e}", path.display());
                return Self::default();
            }
        };
        match serde_json::from_str(&contents) {
            Ok(labels) => labels,
            Err(e) => {
                eprintln!("warning: failed to parse {}: {e}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self, folder: &Path) -> Result<(), String> {
        let path = folder.join(LABELS_FILENAME);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let contents = crate::to_sorted_json_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, contents).map_err(|e| e.to_string())
    }

    pub fn hash(&self) -> String {
        let v = serde_json::to_value(&self.labels).unwrap_or_default();
        let serialized = serde_json::to_string(&v).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Return all commodity-rename labels that match the given fields.
    pub fn find_commodity_renames<'a>(&'a self, fields: &MatchFields) -> Vec<&'a Rule> {
        self.labels
            .iter()
            .filter(|r| {
                r.commodity.is_some()
                    && fields
                        .commodity
                        .is_some_and(|c| wildcard_match(&r.pattern, c))
                    && rule_amount_matches(r, fields)
            })
            .collect()
    }

    /// Find the first matching payee-rename label.
    pub fn find_payee_rename(&self, fields: &MatchFields) -> Option<&Rule> {
        let all_fields: Vec<&str> = [fields.payee, fields.narration, fields.meta]
            .into_iter()
            .flatten()
            .collect();
        self.labels.iter().find(|r| {
            r.payee.is_some()
                && if let Some(ref field_name) = r.match_field {
                    let target = match field_name.as_str() {
                        "meta" => fields.meta,
                        "payee" => fields.payee,
                        "narration" => fields.narration,
                        _ => None,
                    };
                    if field_name == "meta" && is_id_anchored(&r.pattern) {
                        target.is_some_and(|t| meta_segment_match(&r.pattern, t))
                    } else {
                        target.is_some_and(|t| wildcard_match(&r.pattern, t))
                    }
                } else {
                    all_fields.iter().any(|f| wildcard_match(&r.pattern, f))
                }
                && rule_amount_matches(r, fields)
        })
    }
}

/// A rule is a no-op if its only effect is assigning the default expense account.
/// Such rules can be stripped from _rules.json files since the pipeline applies
/// the default_expense_account automatically when no rule matches.
pub fn rule_is_noop(rule: &Rule, default_expense_account: &str) -> bool {
    let accounts_is_default = match &rule.amount_account {
        Some(acct) => acct == default_expense_account,
        None => true,
    } && rule.fee_account.is_none();
    accounts_is_default && rule.payee.is_none() && rule.commodity.is_none()
}

/// Strip no-op rules from a rules file and save if any were removed.
/// Returns the number of rules removed.
pub fn strip_noop_rules(folder: &Path, default_expense_account: &str) -> usize {
    let mut rules_file = RulesFile::load(folder);
    let before = rules_file.rules.len();
    rules_file
        .rules
        .retain(|r| !rule_is_noop(r, default_expense_account));
    let removed = before - rules_file.rules.len();
    if removed > 0 {
        if let Err(e) = rules_file.save(folder) {
            eprintln!(
                "warning: failed to save stripped rules in {}: {e}",
                folder.display()
            );
        }
    }
    removed
}

/// Check if a rule's amount/fee conditions (if any) match the fields.
fn rule_amount_matches(rule: &Rule, fields: &MatchFields) -> bool {
    if let Some(ref cond_str) = rule.amount_condition {
        if let Some(cond) = parse_amount_condition(cond_str) {
            if let Some(amt) = fields.amount {
                if !amount_matches(&cond, amt) {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    if let Some(ref cond_str) = rule.fee_condition {
        if let Some(cond) = parse_amount_condition(cond_str) {
            if let Some(fee) = fields.fee {
                if !amount_matches(&cond, fee) {
                    return false;
                }
            } else {
                return false;
            }
        }
    }
    true
}

/// Check if a rule's payee_condition (if any) matches the fields.
fn rule_payee_condition_matches(rule: &Rule, fields: &MatchFields) -> bool {
    if let Some(ref pattern) = rule.payee_condition {
        let matched = fields.payee.is_some_and(|p| wildcard_match(pattern, p))
            || fields
                .display_payee
                .is_some_and(|p| wildcard_match(pattern, p));
        return matched;
    }
    true
}

/// Check the additional narration/commodity/meta conditions (each ANDed
/// with the rule's main pattern). Each condition is None when not used.
fn rule_other_conditions_match(rule: &Rule, fields: &MatchFields) -> bool {
    if let Some(ref pattern) = rule.narration_condition {
        if !fields.narration.is_some_and(|n| wildcard_match(pattern, n)) {
            return false;
        }
    }
    if let Some(ref pattern) = rule.commodity_condition {
        let matched = fields.commodity.is_some_and(|c| wildcard_match(pattern, c))
            || fields
                .display_commodity
                .is_some_and(|c| wildcard_match(pattern, c));
        if !matched {
            return false;
        }
    }
    if let Some(ref pattern) = rule.meta_condition {
        if !fields.meta.is_some_and(|m| wildcard_match(pattern, m)) {
            return false;
        }
    }
    true
}

/// Create a trade link rule targeting a specific transaction by its meta field.
/// `amount_condition` is required when both legs of the link share a txn_id
/// (on-chain swap pattern): without it, a meta-only match can't tell sell from
/// buy and the first matching rule would win for both legs. Pass `None` when
/// the two legs already have distinct txn_ids.
pub fn trade_link_rule(
    link_id: &str,
    txn_id: &str,
    contra: &str,
    side: &str,
    amount_condition: Option<&str>,
) -> Rule {
    Rule {
        id: format!("trade-{link_id}-{side}"),
        pattern: txn_id.to_string(),
        match_field: Some("meta".to_string()),
        payee: None,
        commodity: None,
        comment: Some("auto:trade-link".to_string()),
        amount_condition: amount_condition.map(String::from),
        fee_condition: None,
        amount_account: Some(contra.to_string()),
        fee_account: None,
        payee_condition: None,
        narration_condition: None,
        commodity_condition: None,
        meta_condition: None,        postings: vec![],
    }
}

/// Build the pair of rules for a trade link. When sell and buy share a txn_id
/// (on-chain swap pattern) the rules are disambiguated by amount sign so each
/// rule only matches its own leg.
pub fn build_trade_link_rules(link_id: &str, sell_txn_id: &str, buy_txn_id: &str) -> [Rule; 2] {
    let same_id = sell_txn_id == buy_txn_id;
    let sell_cond = if same_id { Some("<0") } else { None };
    let buy_cond = if same_id { Some(">0") } else { None };
    [
        trade_link_rule(link_id, sell_txn_id, "equity:trading:sell", "sell", sell_cond),
        trade_link_rule(link_id, buy_txn_id, "equity:trading:buy", "buy", buy_cond),
    ]
}

/// Remove all rules generated for a trade link (matching `trade-{link_id}-*` IDs).
pub fn remove_trade_link_rules(rules_file: &mut RulesFile, link_id: &str) {
    let prefix = format!("trade-{link_id}-");
    rules_file.rules.retain(|r| !r.id.starts_with(&prefix));
}

/// Case-insensitive glob matching with `*` wildcards and `|` OR alternatives.
/// e.g. `*0xabc*|*0xdef*` matches if either sub-pattern matches.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    if pattern.contains('|') {
        return pattern
            .split('|')
            .any(|sub| wildcard_match_single(sub.trim(), text));
    }
    wildcard_match_single(pattern, text)
}

fn wildcard_match_single(pattern: &str, text: &str) -> bool {
    let p = pattern.to_lowercase();
    let t = text.to_lowercase();

    if !p.contains('*') {
        return p == t;
    }

    let parts: Vec<&str> = p.split('*').collect();

    if parts.len() == 2 {
        let prefix = parts[0];
        let suffix = parts[1];
        if prefix.is_empty() && suffix.is_empty() {
            return true; // pattern is just "*"
        }
        if prefix.is_empty() {
            return t.ends_with(suffix);
        }
        if suffix.is_empty() {
            return t.starts_with(prefix);
        }
        return t.starts_with(prefix)
            && t.ends_with(suffix)
            && t.len() >= prefix.len() + suffix.len();
    }

    // General case: all parts must appear in order
    let mut remaining = t.as_str();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            if !remaining.ends_with(part) {
                return false;
            }
        } else {
            match remaining.find(part) {
                Some(pos) => remaining = &remaining[pos + part.len()..],
                None => return false,
            }
        }
    }
    true
}

/// One-shot migration result for a single `_rules.json`.
#[derive(Debug, Default, Clone, Copy)]
pub struct MigrationStats {
    /// `*txn:HASH*` patterns rewritten to bare `txn:HASH`.
    pub patterns_canonicalized: usize,
    /// Txn-anchored rules that moved up to the contiguous top block.
    pub rules_promoted: usize,
}

impl MigrationStats {
    pub fn changed(&self) -> bool {
        self.patterns_canonicalized > 0 || self.rules_promoted > 0
    }
}

/// Result of a single `dedupe_ai_ids` pass on a `_rules.json`.
#[derive(Debug, Default, Clone, Copy)]
pub struct AiDedupeStats {
    /// AI rules whose id was rewritten to its content-derived form.
    pub rules_renamed: usize,
    /// True duplicate AI rules dropped (same content collapsed to one rule).
    pub duplicates_removed: usize,
}

impl AiDedupeStats {
    pub fn changed(&self) -> bool {
        self.rules_renamed > 0 || self.duplicates_removed > 0
    }
}

/// Strip leading/trailing `*` from a `*txn:HASH*` pattern that wraps a
/// txn-anchored hash and nothing else. Returns `Some(bare)` only when
/// the rewrite is safe (no `|`, no internal `*`, non-empty hash).
fn canonicalize_legacy_anchored(pattern: &str) -> Option<String> {
    let p = pattern.trim();
    let inner = p.strip_prefix('*')?.strip_suffix('*')?;
    if inner.contains('*') || inner.contains('|') || inner.contains(',') {
        return None;
    }
    if !is_id_anchored(inner) {
        return None;
    }
    Some(inner.to_string())
}

/// If `pattern` is a bare exact id-anchor, return the matched prefix
/// (`"txn:"` or `"leg:"`); otherwise `None`. "Bare" means no `*`, `|`,
/// comma, or whitespace and a non-empty id.
///
/// `txn:<hash>` anchors one on-chain transaction — shared by every leg of a
/// swap/multi-hop. `leg:<id>` anchors a single leg of such a transaction, so
/// a per-leg category override binds to exactly one posting even when siblings
/// share the on-chain hash. Both are content-addressed unique pointers matched
/// by segment-exact equality. See
/// `features/architecture/txn_id_rule_priority.feature`.
pub fn id_anchor_prefix(pattern: &str) -> Option<&'static str> {
    let p = pattern.trim();
    for prefix in ["leg:", "txn:"] {
        if let Some(rest) = p.strip_prefix(prefix) {
            if !rest.is_empty()
                && !rest.contains('*')
                && !rest.contains('|')
                && !rest.contains(',')
                && !rest.chars().any(char::is_whitespace)
            {
                return Some(prefix);
            }
        }
    }
    None
}

/// True iff `pattern` is a bare exact id-anchor (`txn:<hash>` or `leg:<id>`).
pub fn is_id_anchored(pattern: &str) -> bool {
    id_anchor_prefix(pattern).is_some()
}

/// Match a txn-anchored pattern against a transaction's meta string by
/// segment-exact equality (case-insensitive). The meta is split on `,`
/// and each segment trimmed; the rule matches iff one segment equals
/// the pattern.
///
/// This is the matching strategy for txn-id rules — substring matching
/// (the legacy `wildcard_match` path) is reserved for genuinely
/// glob-shaped patterns.
pub fn meta_segment_match(pattern: &str, meta: &str) -> bool {
    let p = pattern.trim().to_lowercase();
    if p.is_empty() {
        return false;
    }
    meta.split(',').any(|seg| seg.trim().to_lowercase() == p)
}

pub fn generate_rule_id(pattern: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let input = format!("{pattern}{nanos}");
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("rule-{}", &hex::encode(result)[..8])
}

/// Content-derived id for AI-classified rules. Same rule content → same id,
/// so re-running `arimalo-classify` cannot mint a fresh id for an identical
/// rule, and two genuinely-different rules can never collide on `ai-XXXX`.
///
/// `comment` is intentionally excluded from the hash so editing the
/// explanation text doesn't change the rule's identity.
pub fn ai_rule_id(rule: &Rule) -> String {
    // ASCII unit separator between fields, record separator between
    // posting entries — neither character occurs in any real rule field,
    // so the canonical form is unambiguous.
    let postings = rule.postings.join("\x1e");
    let parts = [
        rule.pattern.as_str(),
        rule.match_field.as_deref().unwrap_or(""),
        rule.payee.as_deref().unwrap_or(""),
        rule.commodity.as_deref().unwrap_or(""),
        rule.amount_account.as_deref().unwrap_or(""),
        rule.fee_account.as_deref().unwrap_or(""),
        rule.amount_condition.as_deref().unwrap_or(""),
        rule.fee_condition.as_deref().unwrap_or(""),
        rule.payee_condition.as_deref().unwrap_or(""),
        rule.narration_condition.as_deref().unwrap_or(""),
        rule.commodity_condition.as_deref().unwrap_or(""),
        rule.meta_condition.as_deref().unwrap_or(""),
        postings.as_str(),
    ];
    let input = parts.join("\x1f");
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    format!("ai-{}", &hex::encode(result)[..8])
}

/// Per-account-folder configuration. Stored in `_config.json`.
/// Uses override semantics: the nearest config walking up from the
/// account folder to sources root wins (like `_transform.rhai`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountConfig {
    /// Explorer URL template. Use `{txn_id}` as placeholder for the transaction hash.
    /// Example: `https://solscan.io/tx/{txn_id}`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explorer_url: Option<String>,
}

impl AccountConfig {
    /// Resolve the effective config for an account folder by walking up
    /// the directory tree from `folder` to `sources_dir`.
    /// Returns the first `_config.json` found (override semantics).
    pub fn resolve(folder: &Path, sources_dir: &Path) -> Self {
        let sources_canonical = match sources_dir.canonicalize() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };
        let abs_folder = sources_dir.join(folder);
        let folder_canonical = match abs_folder.canonicalize() {
            Ok(p) => p,
            Err(_) => return Self::default(),
        };

        let mut current = folder_canonical.as_path();
        loop {
            let candidate = current.join(CONFIG_FILENAME);
            if candidate.exists() {
                if let Ok(contents) = fs::read_to_string(&candidate) {
                    if let Ok(config) = serde_json::from_str::<AccountConfig>(&contents) {
                        return config;
                    }
                }
            }
            if current == sources_canonical {
                break;
            }
            current = match current.parent() {
                Some(p) => p,
                None => break,
            };
        }
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_match() {
        assert!(wildcard_match("hello", "Hello"));
        assert!(!wildcard_match("hello", "world"));
    }

    #[test]
    fn test_prefix_wildcard() {
        assert!(wildcard_match("hello*", "Hello World"));
        assert!(!wildcard_match("hello*", "World Hello"));
    }

    #[test]
    fn test_suffix_wildcard() {
        assert!(wildcard_match("*world", "Hello World"));
        assert!(!wildcard_match("*world", "World Hello"));
    }

    #[test]
    fn test_contains_wildcard() {
        assert!(wildcard_match("*ello*", "Hello World"));
        assert!(!wildcard_match("*xyz*", "Hello World"));
    }

    #[test]
    fn test_star_only() {
        assert!(wildcard_match("*", "anything"));
    }

    #[test]
    fn test_or_pattern() {
        assert!(wildcard_match("*0xabc*|*0xdef*", "tx to 0xabc123"));
        assert!(wildcard_match("*0xabc*|*0xdef*", "tx to 0xdef456"));
        assert!(!wildcard_match("*0xabc*|*0xdef*", "tx to 0x999999"));
    }

    #[test]
    fn test_or_pattern_with_spaces() {
        assert!(wildcard_match("*abc* | *def*", "has abc here"));
        assert!(wildcard_match("*abc* | *def*", "has def here"));
    }

    #[test]
    fn test_rules_hash_changes() {
        let r1 = RulesFile { rules: vec![] };
        let r2 = RulesFile {
            rules: vec![Rule {
                id: "rule-1".to_string(),
                pattern: "test".to_string(),
                match_field: None,
                payee: Some("Test".to_string()),
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
        assert_ne!(r1.hash(), r2.hash());
    }

    #[test]
    fn test_find_match() {
        let rules = RulesFile {
            rules: vec![Rule {
                id: "rule-1".to_string(),
                pattern: "Coffee*".to_string(),
                match_field: None,
                payee: Some("Cafe".to_string()),
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
        assert!(rules.find_match("Coffee Shop").is_some());
        assert!(rules.find_match("Tea Shop").is_none());
    }

    #[test]
    fn test_field_specific_rule_matches_target_field() {
        let rules = RulesFile {
            rules: vec![Rule {
                id: "r1".to_string(),
                pattern: "txn:csv-*".to_string(),
                match_field: Some("meta".to_string()),
                payee: None,
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some("equity:trading:sell".to_string()),
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            }],
        };
        let fields = MatchFields {
            payee: Some("Groceries"),
            display_payee: None,
            narration: Some("imported"),
            meta: Some("txn:csv-abc123"),
            commodity: None,
            display_commodity: None,
            amount: None,
            fee: None,
        };
        let m = rules.find_match_prioritized(&fields);
        assert!(m.is_some());
        assert_eq!(
            m.unwrap().amount_account.as_deref(),
            Some("equity:trading:sell")
        );
    }

    #[test]
    fn test_field_specific_rule_does_not_match_wrong_field() {
        let rules = RulesFile {
            rules: vec![Rule {
                id: "r1".to_string(),
                pattern: "Groceries".to_string(),
                match_field: Some("meta".to_string()),
                payee: None,
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
        let fields = MatchFields {
            payee: Some("Groceries"),
            display_payee: None,
            narration: None,
            meta: Some("txn:csv-abc123"),
            commodity: None,
            display_commodity: None,
            amount: None,
            fee: None,
        };
        let m = rules.find_match_prioritized(&fields);
        assert!(
            m.is_none(),
            "should not match payee when match_field is meta"
        );
    }

    #[test]
    fn test_field_specific_rules_take_priority() {
        let rules = RulesFile {
            rules: vec![
                Rule {
                    id: "general".to_string(),
                    pattern: "imported".to_string(),
                    match_field: None,
                    payee: None,
                    commodity: None,
                    comment: None,
                    amount_condition: None,
                    fee_condition: None,
                    amount_account: Some("expenses:food".to_string()),
                    fee_account: None,
                    payee_condition: None,
                    narration_condition: None,
                    commodity_condition: None,
                    meta_condition: None,                    postings: vec![],
                },
                Rule {
                    id: "specific".to_string(),
                    pattern: "txn:csv-*".to_string(),
                    match_field: Some("meta".to_string()),
                    payee: None,
                    commodity: None,
                    comment: None,
                    amount_condition: None,
                    fee_condition: None,
                    amount_account: Some("equity:trading:sell".to_string()),
                    fee_account: None,
                    payee_condition: None,
                    narration_condition: None,
                    commodity_condition: None,
                    meta_condition: None,                    postings: vec![],
                },
            ],
        };
        let fields = MatchFields {
            payee: None,
            display_payee: None,
            narration: Some("imported"),
            meta: Some("txn:csv-abc123"),
            commodity: None,
            display_commodity: None,
            amount: None,
            fee: None,
        };
        let m = rules.find_match_prioritized(&fields).unwrap();
        assert_eq!(m.amount_account.as_deref(), Some("equity:trading:sell"));
    }

    #[test]
    fn test_trade_link_rule_helpers() {
        let mut rf = RulesFile { rules: vec![] };
        rf.rules
            .extend(build_trade_link_rules("abc123", "txn:csv-001", "txn:csv-002"));
        assert_eq!(rf.rules.len(), 2);
        assert_eq!(rf.rules[0].id, "trade-abc123-sell");
        assert_eq!(rf.rules[0].match_field, Some("meta".to_string()));
        // Distinct txn IDs — pattern alone disambiguates, no amount_condition needed.
        assert_eq!(rf.rules[0].amount_condition, None);
        assert_eq!(rf.rules[1].amount_condition, None);

        remove_trade_link_rules(&mut rf, "abc123");
        assert_eq!(rf.rules.len(), 0);
    }

    #[test]
    fn test_build_trade_link_rules_shared_txn_id() {
        let rules = build_trade_link_rules("xyz", "txn:onchain-001", "txn:onchain-001");
        assert_eq!(rules[0].id, "trade-xyz-sell");
        assert_eq!(rules[0].amount_account.as_deref(), Some("equity:trading:sell"));
        assert_eq!(rules[0].amount_condition.as_deref(), Some("<0"));
        assert_eq!(rules[1].id, "trade-xyz-buy");
        assert_eq!(rules[1].amount_account.as_deref(), Some("equity:trading:buy"));
        assert_eq!(rules[1].amount_condition.as_deref(), Some(">0"));
    }

    #[test]
    fn test_parse_amount_condition() {
        assert_eq!(
            parse_amount_condition(">100"),
            Some(AmountCondition::Gt(100.0))
        );
        assert_eq!(
            parse_amount_condition(">=50.5"),
            Some(AmountCondition::Gte(50.5))
        );
        assert_eq!(parse_amount_condition("<0"), Some(AmountCondition::Lt(0.0)));
        assert_eq!(
            parse_amount_condition("<=50"),
            Some(AmountCondition::Lte(50.0))
        );
        assert_eq!(
            parse_amount_condition("=100"),
            Some(AmountCondition::Eq(100.0))
        );
        assert_eq!(
            parse_amount_condition("10..500"),
            Some(AmountCondition::Range(10.0, 500.0))
        );
        assert_eq!(parse_amount_condition(""), None);
        assert_eq!(parse_amount_condition("abc"), None);
        assert_eq!(parse_amount_condition(">abc"), None);
    }

    #[test]
    fn test_amount_matches() {
        // Uses raw signed value, not abs
        assert!(amount_matches(&AmountCondition::Gt(100.0), 150.0));
        assert!(!amount_matches(&AmountCondition::Gt(100.0), -150.0)); // negative is not > 100
        assert!(!amount_matches(&AmountCondition::Gt(100.0), 50.0));
        assert!(amount_matches(&AmountCondition::Gte(100.0), 100.0));
        assert!(!amount_matches(&AmountCondition::Gte(100.0), 99.9));
        assert!(amount_matches(&AmountCondition::Lt(0.0), -50.0)); // outflows
        assert!(!amount_matches(&AmountCondition::Lt(0.0), 50.0));
        assert!(amount_matches(&AmountCondition::Lt(100.0), 50.0));
        assert!(!amount_matches(&AmountCondition::Lt(100.0), 150.0));
        assert!(amount_matches(&AmountCondition::Lte(50.0), 50.0));
        assert!(amount_matches(&AmountCondition::Eq(100.0), 100.0));
        assert!(!amount_matches(&AmountCondition::Eq(100.0), -100.0)); // signed, not abs
        assert!(!amount_matches(&AmountCondition::Eq(100.0), 100.1));
        assert!(amount_matches(&AmountCondition::Range(10.0, 500.0), 150.0));
        assert!(!amount_matches(
            &AmountCondition::Range(10.0, 500.0),
            -150.0
        )); // signed
        assert!(!amount_matches(&AmountCondition::Range(10.0, 500.0), 5.0));
        assert!(!amount_matches(&AmountCondition::Range(10.0, 500.0), 600.0));
        // Negative ranges for outflows
        assert!(amount_matches(
            &AmountCondition::Range(-500.0, -10.0),
            -150.0
        ));
        assert!(!amount_matches(
            &AmountCondition::Range(-500.0, -10.0),
            150.0
        ));
    }

    #[test]
    fn test_find_match_prioritized_with_amount_condition() {
        let rules = RulesFile {
            rules: vec![Rule {
                id: "r1".to_string(),
                pattern: "*Payment*".to_string(),
                match_field: None,
                payee: None,
                commodity: None,
                comment: None,
                amount_condition: Some("<-100".to_string()),
                fee_condition: None,
                amount_account: Some("expenses:large".to_string()),
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            }],
        };
        // Amount -150 → matches (outflow > 100)
        let fields = MatchFields {
            payee: None,
            display_payee: None,
            narration: Some("Payment Big"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(-150.0),
            fee: None,
        };
        assert!(rules.find_match_prioritized(&fields).is_some());

        // Amount -50 → does not match (outflow only 50)
        let fields = MatchFields {
            payee: None,
            display_payee: None,
            narration: Some("Payment Small"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(-50.0),
            fee: None,
        };
        assert!(rules.find_match_prioritized(&fields).is_none());
    }

    #[test]
    fn test_payee_field_rule_only_matches_payee() {
        let rules = RulesFile {
            rules: vec![Rule {
                id: "eth-swell-1".to_string(),
                pattern: "*Swell Network*".to_string(),
                match_field: Some("payee".to_string()),
                payee: Some("Swell Network".to_string()),
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some("expenses:crypto:defi:swell".to_string()),
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            }],
        };
        // Should match when payee contains "Swell Network"
        let fields = MatchFields {
            payee: Some("Swell Network"),
            display_payee: None,
            narration: Some("token_transfer SWELL"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(66805.0),
            fee: None,
        };
        assert!(
            rules.find_match_prioritized(&fields).is_some(),
            "should match on payee"
        );

        // Must NOT match when payee is different, even if narration contains unrelated text
        let fields_wrong_payee = MatchFields {
            payee: Some("0xabcdef1234567890"),
            display_payee: None,
            narration: Some("token_received ETH"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(1.5),
            fee: None,
        };
        assert!(
            rules.find_match_prioritized(&fields_wrong_payee).is_none(),
            "must not match narration when match_field is payee"
        );
    }

    #[test]
    fn test_payee_field_rule_matches_display_payee() {
        let rules = RulesFile {
            rules: vec![Rule {
                id: "r1".to_string(),
                pattern: "*Swell*".to_string(),
                match_field: Some("payee".to_string()),
                payee: None,
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some("expenses:defi".to_string()),
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            }],
        };
        // Raw payee is an address, but display_payee matches
        let fields = MatchFields {
            payee: Some("0xabcdef"),
            display_payee: Some("Swell Network"),
            narration: Some("token_transfer"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(100.0),
            fee: None,
        };
        assert!(
            rules.find_match_prioritized(&fields).is_some(),
            "should match on display_payee"
        );
    }

    #[test]
    fn test_narration_field_rule_only_matches_narration() {
        let rules = RulesFile {
            rules: vec![Rule {
                id: "r1".to_string(),
                pattern: "*token_transfer*".to_string(),
                match_field: Some("narration".to_string()),
                payee: None,
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some("expenses:transfers".to_string()),
                fee_account: None,
                payee_condition: None,
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,                postings: vec![],
            }],
        };
        // Should match narration
        let fields = MatchFields {
            payee: Some("0xabc"),
            display_payee: None,
            narration: Some("token_transfer SWELL"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(100.0),
            fee: None,
        };
        assert!(rules.find_match_prioritized(&fields).is_some());

        // Must NOT match when only payee matches the pattern
        let fields_no_narr = MatchFields {
            payee: Some("token_transfer_contract"),
            display_payee: None,
            narration: Some("swap executed"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(100.0),
            fee: None,
        };
        assert!(
            rules.find_match_prioritized(&fields_no_narr).is_none(),
            "must not match payee when match_field is narration"
        );
    }

    #[test]
    fn test_payee_transform_with_field_only_matches_that_field() {
        // A payee-only rule (no amount_account) with match_field:"payee"
        // should only match on the payee field, not narration
        let rules = RulesFile {
            rules: vec![Rule {
                id: "r1".to_string(),
                pattern: "*Swell*".to_string(),
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
        // This is a payee_transform — test find_payee_transform
        let fields_match = MatchFields {
            payee: Some("Swell Protocol"),
            display_payee: None,
            narration: Some("token_received ETH"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(1.0),
            fee: None,
        };
        assert!(
            rules.find_payee_transform(&fields_match).is_some(),
            "should match on payee"
        );

        let fields_no_match = MatchFields {
            payee: Some("0xabcdef"),
            display_payee: None,
            narration: Some("Swell token_received"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(1.0),
            fee: None,
        };
        assert!(
            rules.find_payee_transform(&fields_no_match).is_none(),
            "must not match narration when match_field is payee"
        );
    }

    #[test]
    fn test_payee_condition_restricts_match() {
        // Rule: pattern matches narration, but payee_condition restricts to specific payee.
        // This is the user scenario: payee:"Swell Network" *token_receive:transfer*
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

        // Transaction WITH matching payee — should match
        let fields_match = MatchFields {
            payee: Some("Swell Network"),
            display_payee: None,
            narration: Some("token_transfer SWELL"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(66805.0),
            fee: None,
        };
        assert!(
            rules.find_match_prioritized(&fields_match).is_some(),
            "should match when both narration and payee_condition match"
        );

        // Transaction WITHOUT matching payee — must NOT match
        let fields_wrong_payee = MatchFields {
            payee: Some("Uniswap V3"),
            display_payee: None,
            narration: Some("token_transfer ETH"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(1.5),
            fee: None,
        };
        assert!(
            rules.find_match_prioritized(&fields_wrong_payee).is_none(),
            "must not match when payee_condition does not match"
        );
    }

    #[test]
    fn test_payee_condition_matches_display_payee() {
        let rules = RulesFile {
            rules: vec![Rule {
                id: "r1".to_string(),
                pattern: "*token_transfer*".to_string(),
                match_field: None,
                payee: None,
                commodity: None,
                comment: None,
                amount_condition: None,
                fee_condition: None,
                amount_account: Some("expenses:defi".to_string()),
                fee_account: None,
                payee_condition: Some("*Swell*".to_string()),
                narration_condition: None,
                commodity_condition: None,
                meta_condition: None,
                postings: vec![],
            }],
        };
        // Raw payee is an address, display_payee matches
        let fields = MatchFields {
            payee: Some("0xabcdef"),
            display_payee: Some("Swell Network"),
            narration: Some("token_transfer SWELL"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: Some(100.0),
            fee: None,
        };
        assert!(
            rules.find_match_prioritized(&fields).is_some(),
            "payee_condition should match on display_payee"
        );
    }

    fn meta_rule(id: &str, pattern: &str) -> Rule {
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
            amount_account: Some("expenses:test".to_string()),
            fee_account: None,
            postings: vec![],
        }
    }

    #[test]
    fn anchored_bare_txn_id() {
        assert!(is_id_anchored("txn:abc"));
    }

    #[test]
    fn anchored_bare_leg_id() {
        assert!(is_id_anchored("leg:l-abc123"));
    }

    #[test]
    fn id_anchor_prefix_distinguishes_kinds() {
        assert_eq!(id_anchor_prefix("txn:abc"), Some("txn:"));
        assert_eq!(id_anchor_prefix("leg:l-abc"), Some("leg:"));
        assert_eq!(id_anchor_prefix("payee:foo"), None);
        assert_eq!(id_anchor_prefix("leg:"), None);
        assert_eq!(id_anchor_prefix("*leg:abc*"), None);
    }

    #[test]
    fn anchored_with_hyphens() {
        assert!(is_id_anchored("txn:ofx-5e3ec4f08130"));
    }

    #[test]
    fn anchored_long_base58() {
        assert!(is_id_anchored(
            "txn:CUpfayqaoQjyNTrGUfkWspet1X1DmV8YuSY9pvGbAdMY"
        ));
    }

    #[test]
    fn not_anchored_empty_hash() {
        assert!(!is_id_anchored("txn:"));
        assert!(!is_id_anchored("leg:"));
    }

    #[test]
    fn not_anchored_wildcard_form() {
        assert!(!is_id_anchored("*txn:abc*"));
        assert!(!is_id_anchored("txn:abc*"));
        assert!(!is_id_anchored("*txn:abc"));
    }

    #[test]
    fn not_anchored_alternation() {
        assert!(!is_id_anchored("txn:a|txn:b"));
    }

    #[test]
    fn not_anchored_comma_or_whitespace() {
        assert!(!is_id_anchored("txn:a,b"));
        assert!(!is_id_anchored("txn:a b"));
    }

    #[test]
    fn not_anchored_other_field_kind() {
        assert!(!is_id_anchored("ofx_id:abc"));
        assert!(!is_id_anchored("payee:foo"));
        assert!(!is_id_anchored(""));
    }

    fn meta_fields(meta: &str) -> MatchFields<'_> {
        MatchFields {
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

    #[test]
    fn leg_anchored_rule_matches_only_its_own_leg() {
        // One on-chain tx (hash H) split into two legs that share `txn:H` but
        // carry distinct `leg:` ids. A per-leg rule must bind to exactly one leg.
        let rules = RulesFile {
            rules: vec![meta_rule("r-sell", "leg:l-aaa"), meta_rule("r-buy", "leg:l-bbb")],
        };
        assert_eq!(
            rules.find_match_prioritized(&meta_fields("txn:H, leg:l-aaa")).map(|r| r.id.as_str()),
            Some("r-sell"),
        );
        assert_eq!(
            rules.find_match_prioritized(&meta_fields("txn:H, leg:l-bbb")).map(|r| r.id.as_str()),
            Some("r-buy"),
        );
    }

    #[test]
    fn leg_anchored_outranks_shared_txn_anchored() {
        // A shared-txn rule (bleeds to every leg) listed BEFORE the per-leg rule;
        // the per-leg anchor must still win for its own leg regardless of order.
        let rules = RulesFile {
            rules: vec![meta_rule("r-shared-txn", "txn:H"), meta_rule("r-leg", "leg:l-bbb")],
        };
        assert_eq!(
            rules.find_match_prioritized(&meta_fields("txn:H, leg:l-bbb")).map(|r| r.id.as_str()),
            Some("r-leg"),
        );
    }

    #[test]
    fn segment_match_single() {
        assert!(meta_segment_match("txn:abc", "txn:abc"));
    }

    #[test]
    fn segment_match_first_of_many() {
        assert!(meta_segment_match("txn:abc", "txn:abc, rule:r1"));
    }

    #[test]
    fn segment_match_last_of_many() {
        assert!(meta_segment_match(
            "txn:abc",
            "rule:r1, source_payee:foo, txn:abc"
        ));
    }

    #[test]
    fn segment_match_rejects_prefix() {
        assert!(!meta_segment_match("txn:abc", "txn:abcd"));
    }

    #[test]
    fn segment_match_rejects_embedded() {
        // The whole segment is `source_payee:txn:abc`, which doesn't
        // equal `txn:abc` after split-on-comma.
        assert!(!meta_segment_match("txn:abc", "source_payee:txn:abc"));
    }

    #[test]
    fn segment_match_rejects_empty_meta() {
        assert!(!meta_segment_match("txn:abc", ""));
    }

    #[test]
    fn segment_match_tolerates_whitespace_around_segments() {
        assert!(meta_segment_match("txn:abc", "txn:abc , rule:r1"));
        assert!(meta_segment_match("txn:abc", "rule:r1 ,  txn:abc"));
    }

    #[test]
    fn segment_match_is_case_insensitive() {
        assert!(meta_segment_match("txn:Abc", "TXN:abc"));
    }

    #[test]
    fn insert_anchored_rule_lands_at_index_zero() {
        let mut rf = RulesFile { rules: vec![] };
        rf.rules.push(meta_rule("a", "*token_transfer*"));
        rf.rules.push(meta_rule("b", "*ofx_id:bank-X*"));
        rf.insert_rule(meta_rule("hide", "txn:abc"));
        assert_eq!(rf.rules[0].id, "hide");
        assert_eq!(rf.rules.len(), 3);
    }

    #[test]
    fn insert_non_anchored_rule_appends() {
        let mut rf = RulesFile { rules: vec![] };
        rf.insert_rule(meta_rule("hide", "txn:abc"));
        rf.insert_rule(meta_rule("broad", "*token_transfer*"));
        assert_eq!(rf.rules[0].id, "hide");
        assert_eq!(rf.rules[1].id, "broad");
    }

    #[test]
    fn insert_rule_skips_anchor_when_match_field_is_not_meta() {
        // A bare `txn:abc` pattern with a non-meta `match_field` is a
        // human authoring mistake — don't promote it. (The matcher
        // wouldn't match it as a txn id anyway.)
        let mut rf = RulesFile { rules: vec![] };
        rf.insert_rule(meta_rule("first", "*token_transfer*"));
        let mut weird = meta_rule("weird", "txn:abc");
        weird.match_field = Some("payee".to_string());
        rf.insert_rule(weird);
        assert_eq!(rf.rules[0].id, "first");
        assert_eq!(rf.rules[1].id, "weird");
    }

    #[test]
    fn canonicalize_strips_stars_around_bare_txn_id() {
        assert_eq!(
            canonicalize_legacy_anchored("*txn:abc*"),
            Some("txn:abc".to_string())
        );
        assert_eq!(
            canonicalize_legacy_anchored("*txn:ofx-abc-123*"),
            Some("txn:ofx-abc-123".to_string())
        );
    }

    #[test]
    fn canonicalize_rejects_non_txn() {
        assert_eq!(canonicalize_legacy_anchored("*token_transfer*"), None);
        assert_eq!(canonicalize_legacy_anchored("*ofx_id:bank-X*"), None);
    }

    #[test]
    fn canonicalize_rejects_partial_or_already_bare() {
        // Only one star: not a wrapped form.
        assert_eq!(canonicalize_legacy_anchored("*txn:abc"), None);
        assert_eq!(canonicalize_legacy_anchored("txn:abc*"), None);
        // Already bare → not legacy.
        assert_eq!(canonicalize_legacy_anchored("txn:abc"), None);
        // Star inside the hash is a malformed input we don't touch.
        assert_eq!(canonicalize_legacy_anchored("*txn:a*b*"), None);
    }

    #[test]
    fn migrate_canonicalizes_and_promotes() {
        let mut rf = RulesFile { rules: vec![] };
        rf.rules.push(meta_rule("broad", "*token_transfer*"));
        rf.rules.push(meta_rule("legacy_hide", "*txn:abc*"));
        rf.rules.push(meta_rule("trade", "txn:def"));
        let stats = rf.migrate_legacy_anchored();
        assert_eq!(stats.patterns_canonicalized, 1);
        assert_eq!(stats.rules_promoted, 2);
        assert_eq!(rf.rules[0].pattern, "txn:abc");
        assert_eq!(rf.rules[1].pattern, "txn:def");
        assert_eq!(rf.rules[2].pattern, "*token_transfer*");
    }

    #[test]
    fn migrate_is_idempotent_on_already_migrated_file() {
        let mut rf = RulesFile { rules: vec![] };
        rf.rules.push(meta_rule("hide", "txn:abc"));
        rf.rules.push(meta_rule("trade", "txn:def"));
        rf.rules.push(meta_rule("broad", "*token_transfer*"));
        let stats = rf.migrate_legacy_anchored();
        assert_eq!(stats.patterns_canonicalized, 0);
        assert_eq!(stats.rules_promoted, 0);
        assert_eq!(rf.rules[0].pattern, "txn:abc");
        assert_eq!(rf.rules[1].pattern, "txn:def");
        assert_eq!(rf.rules[2].pattern, "*token_transfer*");
    }

    #[test]
    fn migrate_preserves_relative_order_within_each_block() {
        let mut rf = RulesFile { rules: vec![] };
        rf.rules.push(meta_rule("broad-a", "*token_transfer*"));
        rf.rules.push(meta_rule("hide-1", "*txn:111*"));
        rf.rules.push(meta_rule("broad-b", "*ofx_id:bank-X*"));
        rf.rules.push(meta_rule("hide-2", "*txn:222*"));
        rf.migrate_legacy_anchored();
        assert_eq!(rf.rules[0].id, "hide-1");
        assert_eq!(rf.rules[1].id, "hide-2");
        assert_eq!(rf.rules[2].id, "broad-a");
        assert_eq!(rf.rules[3].id, "broad-b");
    }

    #[test]
    fn insert_rules_pair_lands_at_top() {
        let mut rf = RulesFile { rules: vec![] };
        rf.insert_rule(meta_rule("existing", "*token_transfer*"));
        rf.insert_rules(build_trade_link_rules("link1", "txn:s", "txn:b"));
        // insert_rule promotes each anchored entry to index 0, so the
        // last anchored rule processed (`buy`) ends up nearest the top.
        assert_eq!(rf.rules[0].id, "trade-link1-buy");
        assert_eq!(rf.rules[1].id, "trade-link1-sell");
        assert_eq!(rf.rules[2].id, "existing");
    }

    fn rule_with_all_conditions() -> Rule {
        Rule {
            id: "rule-test".into(),
            pattern: "*token_transfer:*".into(),
            match_field: Some("narration".into()),
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: Some("-0.003..0".into()),
            fee_condition: None,
            payee_condition: Some("*Kraken*".into()),
            narration_condition: Some("*swap*".into()),
            commodity_condition: Some("SOL".into()),
            meta_condition: Some("*csv-abc*".into()),
            amount_account: Some("expenses:crypto:fees".into()),
            fee_account: None,
            postings: vec![],
        }
    }

    /// Round-trip: a Rule with all the new condition fields populated must
    /// serialize to JSON, deserialize back, and produce an identical struct.
    /// Catches any serde/typo regression that would silently drop new fields
    /// (the failure mode the user hit in the live app).
    #[test]
    fn rule_with_new_conditions_round_trips_through_json() {
        let r = rule_with_all_conditions();
        let json = serde_json::to_string_pretty(&r).expect("serialize");
        // The JSON must contain the new keys verbatim — Tauri's IPC bridge
        // serializes the struct, so missing keys here = missing fields on disk.
        assert!(
            json.contains("commodity_condition") && json.contains("\"SOL\""),
            "missing commodity_condition in JSON: {json}",
        );
        assert!(
            json.contains("narration_condition") && json.contains("*swap*"),
            "missing narration_condition in JSON: {json}",
        );
        assert!(
            json.contains("meta_condition") && json.contains("*csv-abc*"),
            "missing meta_condition in JSON: {json}",
        );
        let parsed: Rule = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.commodity_condition.as_deref(), Some("SOL"));
        assert_eq!(parsed.narration_condition.as_deref(), Some("*swap*"));
        assert_eq!(parsed.meta_condition.as_deref(), Some("*csv-abc*"));
    }

    /// RulesFile::save → load round-trip writes the new conditions to disk
    /// in a way the loader picks up. This is the test equivalent of what
    /// happens after Tauri save_rule writes the file. If it fails, the
    /// frontend → IPC → save_rule chain can't possibly produce the right
    /// JSON either.
    #[test]
    fn rules_file_persists_new_conditions_to_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rf = RulesFile {
            rules: vec![rule_with_all_conditions()],
        };
        rf.save(dir.path()).expect("save");
        let on_disk = std::fs::read_to_string(dir.path().join(RULES_FILENAME)).expect("read");
        assert!(
            on_disk.contains("commodity_condition") && on_disk.contains("\"SOL\""),
            "_rules.json missing commodity_condition: {on_disk}",
        );
        let reloaded = RulesFile::load(dir.path());
        assert_eq!(reloaded.rules.len(), 1);
        assert_eq!(reloaded.rules[0].commodity_condition.as_deref(), Some("SOL"));
        assert_eq!(reloaded.rules[0].narration_condition.as_deref(), Some("*swap*"));
        assert_eq!(reloaded.rules[0].meta_condition.as_deref(), Some("*csv-abc*"));
    }

    /// New conditions actually constrain matching: a rule with
    /// commodity_condition: "SOL" must NOT match a transaction whose
    /// commodity is USDC, even when its narration matches the pattern.
    #[test]
    fn commodity_condition_filters_match_at_engine_layer() {
        let r = rule_with_all_conditions();
        // Strip the unrelated conditions so we isolate commodity behaviour.
        let r = Rule {
            payee_condition: None,
            narration_condition: None,
            meta_condition: None,
            amount_condition: None,
            ..r
        };
        let rf = RulesFile { rules: vec![r] };
        let sol_fields = MatchFields {
            payee: None,
            display_payee: None,
            narration: Some("token_transfer:send SOL"),
            meta: None,
            commodity: Some("SOL"),
            display_commodity: None,
            amount: None,
            fee: None,
        };
        let usdc_fields = MatchFields {
            commodity: Some("USDC"),
            ..sol_fields
        };
        assert!(rf.find_match_prioritized(&sol_fields).is_some(), "should match SOL");
        assert!(
            rf.find_match_prioritized(&usdc_fields).is_none(),
            "must NOT match USDC when commodity_condition is SOL",
        );
    }

    fn tier_rule(id: &str) -> Rule {
        Rule {
            id: id.to_string(),
            pattern: "*".to_string(),
            match_field: None,
            payee: None,
            commodity: None,
            comment: None,
            amount_condition: None,
            fee_condition: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,
            amount_account: Some("expenses:misc".to_string()),
            fee_account: None,
            postings: vec![],
        }
    }

    #[test]
    fn specificity_tier_meta_is_most_specific() {
        let mut r = tier_rule("m1");
        r.meta_condition = Some("txn:0xabc".to_string());
        r.payee_condition = Some("Foo".to_string());
        r.narration_condition = Some("bar".to_string());
        assert_eq!(r.specificity_tier(), 0);

        let mut r2 = tier_rule("m2");
        r2.match_field = Some("meta".to_string());
        r2.pattern = "txn:0xabc".to_string();
        assert_eq!(r2.specificity_tier(), 0);
    }

    #[test]
    fn specificity_tier_payee_and_narration() {
        let mut r = tier_rule("pn");
        r.payee_condition = Some("Foo".to_string());
        r.narration_condition = Some("bar".to_string());
        assert_eq!(r.specificity_tier(), 1);
    }

    #[test]
    fn specificity_tier_payee_only() {
        let mut r = tier_rule("p");
        r.payee_condition = Some("Foo".to_string());
        assert_eq!(r.specificity_tier(), 2);

        let mut r2 = tier_rule("p2");
        r2.match_field = Some("payee".to_string());
        r2.pattern = "Foo".to_string();
        assert_eq!(r2.specificity_tier(), 2);
    }

    #[test]
    fn specificity_tier_narration_only() {
        let mut r = tier_rule("n");
        r.narration_condition = Some("bar".to_string());
        assert_eq!(r.specificity_tier(), 3);

        let mut r2 = tier_rule("n2");
        r2.match_field = Some("narration".to_string());
        r2.pattern = "bar".to_string();
        assert_eq!(r2.specificity_tier(), 3);
    }

    #[test]
    fn specificity_tier_other_field_specific() {
        let mut r = tier_rule("c");
        r.match_field = Some("commodity".to_string());
        r.pattern = "BTC".to_string();
        assert_eq!(r.specificity_tier(), 4);

        let mut r2 = tier_rule("a");
        r2.amount_condition = Some(">100".to_string());
        assert_eq!(r2.specificity_tier(), 4);
    }

    #[test]
    fn specificity_tier_general_fallback() {
        let r = tier_rule("g");
        assert_eq!(r.specificity_tier(), 5);
    }

    #[test]
    fn payee_rule_beats_earlier_narration_rule() {
        let mut narration = tier_rule("narr");
        narration.match_field = Some("narration".to_string());
        narration.pattern = "coffee".to_string();
        narration.amount_account = Some("expenses:food".to_string());

        let mut payee = tier_rule("payee");
        payee.payee_condition = Some("Starbucks".to_string());
        payee.pattern = "*".to_string();
        payee.amount_account = Some("expenses:coffee".to_string());

        let rf = RulesFile {
            rules: vec![narration, payee],
        };
        let fields = MatchFields {
            payee: Some("Starbucks"),
            display_payee: None,
            narration: Some("coffee"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: None,
            fee: None,
        };
        let m = rf.find_match_prioritized(&fields).unwrap();
        assert_eq!(m.id, "payee", "payee-conditioned rule must beat earlier narration rule");
    }

    #[test]
    fn intra_tier_order_preserved() {
        let mut first = tier_rule("first");
        first.payee_condition = Some("Foo*".to_string());
        first.amount_account = Some("expenses:a".to_string());

        let mut second = tier_rule("second");
        second.payee_condition = Some("Foo*".to_string());
        second.amount_account = Some("expenses:b".to_string());

        let rf = RulesFile {
            rules: vec![first, second],
        };
        let fields = MatchFields {
            payee: Some("FooBar"),
            display_payee: None,
            narration: None,
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: None,
            fee: None,
        };
        assert_eq!(rf.find_match_prioritized(&fields).unwrap().id, "first");
    }

    #[test]
    fn payee_ignore_rule_beats_narration_rule() {
        // Regression: protects 9209e7e fix. An airdrop with both a narration rule
        // and a payee-conditioned ignore:* rule must route to ignore:*.
        let mut narration = tier_rule("narr");
        narration.match_field = Some("narration".to_string());
        narration.pattern = "*airdrop*".to_string();
        narration.amount_account = Some("income:airdrop".to_string());

        let mut ignore = tier_rule("ignore");
        ignore.payee_condition = Some("ScamToken".to_string());
        ignore.amount_account = Some("ignore:noop".to_string());

        let rf = RulesFile {
            rules: vec![narration, ignore],
        };
        let fields = MatchFields {
            payee: Some("ScamToken"),
            display_payee: None,
            narration: Some("airdrop"),
            meta: None,
            commodity: None,
            display_commodity: None,
            amount: None,
            fee: None,
        };
        let m = rf.find_match_prioritized(&fields).unwrap();
        assert_eq!(m.amount_account.as_deref(), Some("ignore:noop"));
    }

    #[test]
    fn meta_rule_beats_payee_rule() {
        let mut payee = tier_rule("payee");
        payee.payee_condition = Some("Foo".to_string());
        payee.amount_account = Some("expenses:foo".to_string());

        let mut meta = tier_rule("meta");
        meta.match_field = Some("meta".to_string());
        meta.pattern = "txn:csv-*".to_string();
        meta.amount_account = Some("equity:trading:sell".to_string());

        let rf = RulesFile {
            rules: vec![payee, meta],
        };
        let fields = MatchFields {
            payee: Some("Foo"),
            display_payee: None,
            narration: None,
            meta: Some("txn:csv-001"),
            commodity: None,
            display_commodity: None,
            amount: None,
            fee: None,
        };
        assert_eq!(rf.find_match_prioritized(&fields).unwrap().id, "meta");
    }

    #[test]
    fn sort_by_specificity_orders_by_tier_stable() {
        let mut narration = tier_rule("narr");
        narration.match_field = Some("narration".to_string());
        narration.pattern = "x".to_string();

        let mut payee_a = tier_rule("payee_a");
        payee_a.payee_condition = Some("A".to_string());

        let general = tier_rule("gen");

        let mut meta = tier_rule("meta");
        meta.meta_condition = Some("txn:abc".to_string());

        let mut payee_b = tier_rule("payee_b");
        payee_b.payee_condition = Some("B".to_string());

        let mut rf = RulesFile {
            rules: vec![narration, payee_a, general, meta, payee_b],
        };
        rf.sort_by_specificity();
        let ids: Vec<&str> = rf.rules.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["meta", "payee_a", "payee_b", "narr", "gen"]);
    }

    #[test]
    fn save_writes_rules_in_specificity_order() {
        let mut narration = tier_rule("narr");
        narration.match_field = Some("narration".to_string());
        narration.pattern = "x".to_string();
        narration.amount_account = Some("expenses:misc".to_string());

        let mut meta = tier_rule("meta");
        meta.meta_condition = Some("txn:abc".to_string());
        meta.amount_account = Some("ignore:hidden".to_string());

        let rf = RulesFile {
            rules: vec![narration, meta],
        };
        let dir = tempfile::tempdir().unwrap();
        rf.save(dir.path()).unwrap();
        let on_disk = std::fs::read_to_string(dir.path().join(RULES_FILENAME)).unwrap();
        let pos_meta = on_disk.find("\"meta\"").expect("meta rule on disk");
        let pos_narr = on_disk.find("\"narr\"").expect("narr rule on disk");
        assert!(pos_meta < pos_narr, "meta rule must precede narration rule on disk");
    }

    #[test]
    fn transforms_still_skipped_in_prioritized_match() {
        let mut transform = tier_rule("transform");
        transform.match_field = Some("commodity".to_string());
        transform.pattern = "btc".to_string();
        transform.commodity = Some("BTC".to_string());
        transform.amount_account = None;

        let mut categorize = tier_rule("cat");
        categorize.payee_condition = Some("Foo".to_string());
        categorize.amount_account = Some("expenses:foo".to_string());

        let rf = RulesFile {
            rules: vec![transform, categorize],
        };
        let fields = MatchFields {
            payee: Some("Foo"),
            display_payee: None,
            narration: None,
            meta: None,
            commodity: Some("btc"),
            display_commodity: None,
            amount: None,
            fee: None,
        };
        assert_eq!(rf.find_match_prioritized(&fields).unwrap().id, "cat");
    }

    // ---- ai_rule_id / dedupe_ai_ids ----------------------------------------
    //
    // Regression: rules generated by `arimalo-classify` previously used a
    // nanosecond-low-bits id, which collided in batch runs. The user hit this
    // when clicking "Edit Rule" on an `equity:trading:sell` row opened the
    // editor for an unrelated `*normal:approve*` rule that happened to share
    // the same `ai-507dda08` id in the same `_rules.json`. The frontend's
    // `allRules.find(r => r.id === ruleId)` returned the first match, which
    // was the wrong rule.

    fn ai_classified_rule(pattern: &str, account: &str) -> Rule {
        Rule {
            id: "ai-collide".into(),
            pattern: pattern.into(),
            match_field: Some("narration".into()),
            payee: None,
            commodity: None,
            comment: Some("ai: example".into()),
            amount_condition: None,
            fee_condition: None,
            payee_condition: None,
            narration_condition: None,
            commodity_condition: None,
            meta_condition: None,
            amount_account: Some(account.into()),
            fee_account: None,
            postings: vec![],
        }
    }

    #[test]
    fn ai_rule_id_is_stable_for_same_content() {
        let a = ai_classified_rule("*token_transfer:trade*", "equity:trading:sell");
        let b = ai_classified_rule("*token_transfer:trade*", "equity:trading:sell");
        assert_eq!(ai_rule_id(&a), ai_rule_id(&b));
        assert!(ai_rule_id(&a).starts_with("ai-"));
    }

    #[test]
    fn ai_rule_id_differs_for_different_content() {
        let a = ai_classified_rule("*token_transfer:trade*", "equity:trading:sell");
        let b = ai_classified_rule("*normal:approve*", "ignore:noop");
        assert_ne!(ai_rule_id(&a), ai_rule_id(&b));
    }

    #[test]
    fn ai_rule_id_ignores_comment() {
        let mut a = ai_classified_rule("*token_transfer:trade*", "equity:trading:sell");
        let mut b = ai_classified_rule("*token_transfer:trade*", "equity:trading:sell");
        a.comment = Some("ai: original explanation".into());
        b.comment = Some("ai: edited explanation".into());
        assert_eq!(ai_rule_id(&a), ai_rule_id(&b));
    }

    #[test]
    fn dedupe_ai_ids_separates_distinct_rules_that_shared_an_id() {
        // The exact shape of the user-reported bug: two distinct AI rules
        // both stamped with `ai-507dda08`. After dedupe, both rules must
        // remain (different content) but with distinct content-derived ids.
        let approve = ai_classified_rule("*normal:approve*", "ignore:noop");
        let trade = ai_classified_rule("*token_transfer:trade*", "equity:trading:sell");
        let mut rf = RulesFile {
            rules: vec![approve.clone(), trade.clone()],
        };
        let stats = rf.dedupe_ai_ids();
        assert_eq!(rf.rules.len(), 2, "no duplicates should be removed");
        assert_eq!(stats.duplicates_removed, 0);
        assert_eq!(stats.rules_renamed, 2);
        assert_ne!(rf.rules[0].id, rf.rules[1].id);
        assert_eq!(rf.rules[0].id, ai_rule_id(&approve));
        assert_eq!(rf.rules[1].id, ai_rule_id(&trade));
    }

    #[test]
    fn dedupe_ai_ids_collapses_true_duplicates() {
        // Two rules with the same content (and same shared id) collapse to a
        // single rule.
        let r = ai_classified_rule("*normal:approve*", "ignore:noop");
        let mut rf = RulesFile {
            rules: vec![r.clone(), r.clone()],
        };
        let stats = rf.dedupe_ai_ids();
        assert_eq!(rf.rules.len(), 1);
        assert_eq!(stats.duplicates_removed, 1);
    }

    #[test]
    fn dedupe_ai_ids_is_idempotent() {
        let r1 = ai_classified_rule("*normal:approve*", "ignore:noop");
        let r2 = ai_classified_rule("*token_transfer:trade*", "equity:trading:sell");
        let mut rf = RulesFile {
            rules: vec![r1, r2],
        };
        let _ = rf.dedupe_ai_ids();
        let snapshot: Vec<String> = rf.rules.iter().map(|r| r.id.clone()).collect();
        let stats2 = rf.dedupe_ai_ids();
        let after: Vec<String> = rf.rules.iter().map(|r| r.id.clone()).collect();
        assert_eq!(snapshot, after);
        assert!(!stats2.changed());
    }

    #[test]
    fn dedupe_ai_ids_ignores_non_ai_rules() {
        // Manual rules (id prefix `rule-` or anything else) must not be
        // rewritten — their ids are user-curated.
        let mut manual = ai_classified_rule("*coffee*", "expenses:food");
        manual.id = "rule-manual".into();
        let mut rf = RulesFile {
            rules: vec![manual.clone()],
        };
        let stats = rf.dedupe_ai_ids();
        assert!(!stats.changed());
        assert_eq!(rf.rules[0].id, "rule-manual");
    }

    #[test]
    fn dedupe_ai_ids_preserves_non_ai_rules_even_when_they_share_ids() {
        // Real-world case from the live vault: four `auto-eth-0x00000000`
        // rules share an id but have distinct patterns. Dedupe must not
        // drop them — only ai- rules get the content-hash treatment.
        let mut a = ai_classified_rule("*0x0000000000000000000000000000000000001004*", "expenses:unknown");
        let mut b = ai_classified_rule("*0x0000000000000000000000000000000000001010*", "expenses:unknown");
        a.id = "auto-eth-0x00000000".into();
        b.id = "auto-eth-0x00000000".into();
        let mut rf = RulesFile {
            rules: vec![a, b],
        };
        let stats = rf.dedupe_ai_ids();
        assert_eq!(rf.rules.len(), 2, "non-ai rules with shared ids must be preserved");
        assert_eq!(stats.duplicates_removed, 0);
        assert_eq!(stats.rules_renamed, 0);
    }
}
