use automerge::AutoCommit;
use autosurgeon::{Hydrate, Reconcile};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::ledger_parser::parse_transactions;
use crate::rules::RulesFile;

// ── Metadata schemas ────────────────────────────────────────────────

fn default_trade_links() -> HashMap<String, TradeLink> {
    HashMap::new()
}

#[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize, Default)]
pub struct ArimaloMetadata {
    pub transaction_refs: Vec<TxnRef>,
    pub accounts: HashMap<String, AccountMeta>,
    pub rules: HashMap<String, RuleMeta>,
    pub file_manifest: HashMap<String, FileEntry>,
    pub sync_log: Vec<SyncEvent>,
    pub devices: HashMap<String, DeviceInfo>,
    #[autosurgeon(missing = "default_trade_links")]
    pub trade_links: HashMap<String, TradeLink>,
}

#[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TxnRef {
    pub id: String,
    pub content_hash: String,
    pub file_path: String,
    pub datetime: String,
    pub device_origin: String,
    pub created_at: i64,
}

#[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct AccountMeta {
    pub path: String,
    pub default_commodity: Option<String>,
    pub device_origin: String,
}

#[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct RuleMeta {
    pub id: String,
    pub pattern: String,
    pub payee: Option<String>,
    pub postings: Vec<String>,
    pub account_folder: String,
    pub device_origin: String,
}

#[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct FileEntry {
    pub content_hash: String,
    pub relative_path: String,
    pub file_type: String,
    pub size_bytes: u64,
    pub device_origin: String,
    pub uploaded_at: i64,
}

#[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct SyncEvent {
    pub timestamp: i64,
    pub device_id: String,
    pub event_type: String,
    pub target_id: String,
    pub details: String,
}

#[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub device_name: String,
    pub last_seen: i64,
}

#[derive(Reconcile, Hydrate, Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct TradeLink {
    pub id: String,
    pub txn_id_a: String,
    pub txn_id_b: String,
    pub device_origin: String,
    pub created_at: i64,
}

// ── MetadataStore ───────────────────────────────────────────────────

pub struct MetadataStore {
    doc: AutoCommit,
    device_id: String,
    metadata_path: PathBuf,
}

impl std::fmt::Debug for MetadataStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetadataStore")
            .field("device_id", &self.device_id)
            .field("metadata_path", &self.metadata_path)
            .finish()
    }
}

impl MetadataStore {
    pub fn new(metadata_path: PathBuf) -> Result<Self, String> {
        let device_id = get_device_id();

        let doc = if metadata_path.exists() {
            let bytes =
                fs::read(&metadata_path).map_err(|e| format!("Failed to read metadata: {}", e))?;
            AutoCommit::load(&bytes).map_err(|e| format!("Failed to load Automerge doc: {}", e))?
        } else {
            let mut doc = AutoCommit::new();
            let empty = ArimaloMetadata::default();
            autosurgeon::reconcile(&mut doc, &empty)
                .map_err(|e| format!("Failed to initialize doc: {}", e))?;
            doc
        };

        Ok(Self {
            doc,
            device_id,
            metadata_path,
        })
    }

    pub fn device_id(&self) -> &str {
        &self.device_id
    }

    pub fn metadata_path(&self) -> &Path {
        &self.metadata_path
    }

    pub fn build_from_sources(&mut self, sources_dir: &Path) -> Result<(), String> {
        let mut metadata = ArimaloMetadata::default();

        self.scan_transactions(sources_dir, &mut metadata)?;
        self.scan_files(sources_dir, &mut metadata)?;
        self.scan_rules(sources_dir, &mut metadata)?;

        metadata.devices.insert(
            self.device_id.clone(),
            DeviceInfo {
                device_id: self.device_id.clone(),
                device_name: get_hostname(),
                last_seen: now(),
            },
        );

        autosurgeon::reconcile(&mut self.doc, &metadata)
            .map_err(|e| format!("Reconcile failed: {}", e))?;

        self.log_sync_event("metadata_built", "", "Built from sources")?;

        Ok(())
    }

    fn scan_transactions(
        &self,
        sources_dir: &Path,
        metadata: &mut ArimaloMetadata,
    ) -> Result<(), String> {
        // Collect all manual.transactions files recursively
        let mut manual_files: Vec<std::path::PathBuf> = Vec::new();
        for entry in WalkDir::new(sources_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_file()
                && path.file_name().and_then(|n| n.to_str()) == Some("manual.transactions")
            {
                manual_files.push(path.to_path_buf());
            }
        }

        for manual_path in &manual_files {
            let rel_path = manual_path
                .strip_prefix(sources_dir)
                .map(|p| format!("sources/{}", p.display()))
                .unwrap_or_else(|_| format!("{}", manual_path.display()));

            let content =
                fs::read_to_string(manual_path).map_err(|e| format!("Read failed: {}", e))?;

            let parse_result = parse_transactions(&content);

            for (idx, txn) in parse_result.transactions.iter().enumerate() {
                let txn_id = extract_txn_id(&txn.meta).unwrap_or_else(|| format!("manual-{}", idx));
                let txn_text = format!("{} {}", txn.datetime, txn.payee.as_deref().unwrap_or(""));
                metadata.transaction_refs.push(TxnRef {
                    id: txn_id,
                    content_hash: sha256_string(&txn_text),
                    file_path: rel_path.clone(),
                    datetime: txn.datetime.clone(),
                    device_origin: self.device_id.clone(),
                    created_at: now(),
                });
            }
        }
        Ok(())
    }

    fn scan_files(
        &self,
        sources_dir: &Path,
        metadata: &mut ArimaloMetadata,
    ) -> Result<(), String> {
        if !sources_dir.exists() {
            return Ok(());
        }

        for entry in WalkDir::new(sources_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("");
            let file_type = match ext {
                "csv" => "csv",
                "rhai" => "transform",
                "json" => "json",
                "transactions" => "transactions",
                "pdf" => "pdf",
                _ => continue,
            };

            let content = fs::read(path).map_err(|e| format!("Read failed: {}", e))?;
            let hash = sha256_bytes(&content);

            let relative = path
                .strip_prefix(sources_dir)
                .map_err(|e| format!("Path error: {}", e))?;

            metadata.file_manifest.insert(
                hash.clone(),
                FileEntry {
                    content_hash: hash,
                    relative_path: relative.to_string_lossy().to_string(),
                    file_type: file_type.to_string(),
                    size_bytes: content.len() as u64,
                    device_origin: self.device_id.clone(),
                    uploaded_at: now(),
                },
            );
        }

        Ok(())
    }

    fn scan_rules(
        &self,
        sources_dir: &Path,
        metadata: &mut ArimaloMetadata,
    ) -> Result<(), String> {
        if !sources_dir.exists() {
            return Ok(());
        }

        for entry in WalkDir::new(sources_dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.file_name().and_then(|n| n.to_str()) != Some("_rules.json") {
                continue;
            }

            let folder = path.parent().unwrap_or(sources_dir);
            let account_folder = folder
                .strip_prefix(sources_dir)
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .to_string();

            let rules_file = RulesFile::load(folder);
            for rule in &rules_file.rules {
                metadata.rules.insert(
                    rule.id.clone(),
                    RuleMeta {
                        id: rule.id.clone(),
                        pattern: rule.pattern.clone(),
                        payee: rule.payee.clone(),
                        postings: rule.postings.clone(),
                        account_folder: account_folder.clone(),
                        device_origin: self.device_id.clone(),
                    },
                );
            }
        }

        Ok(())
    }

    pub fn save_trade_link(&mut self, txn_id_a: &str, txn_id_b: &str) -> Result<String, String> {
        let mut metadata: ArimaloMetadata =
            autosurgeon::hydrate(&self.doc).map_err(|e| format!("Hydrate failed: {}", e))?;

        let id = trade_link_id(txn_id_a, txn_id_b);
        metadata.trade_links.insert(
            id.clone(),
            TradeLink {
                id: id.clone(),
                txn_id_a: txn_id_a.to_string(),
                txn_id_b: txn_id_b.to_string(),
                device_origin: self.device_id.clone(),
                created_at: now(),
            },
        );

        autosurgeon::reconcile(&mut self.doc, &metadata)
            .map_err(|e| format!("Reconcile failed: {}", e))?;
        Ok(id)
    }

    pub fn delete_trade_link(&mut self, link_id: &str) -> Result<(), String> {
        let mut metadata: ArimaloMetadata =
            autosurgeon::hydrate(&self.doc).map_err(|e| format!("Hydrate failed: {}", e))?;

        metadata.trade_links.remove(link_id);

        autosurgeon::reconcile(&mut self.doc, &metadata)
            .map_err(|e| format!("Reconcile failed: {}", e))?;
        Ok(())
    }

    pub fn get_trade_links(&self) -> Result<Vec<TradeLink>, String> {
        let metadata: ArimaloMetadata =
            autosurgeon::hydrate(&self.doc).map_err(|e| format!("Hydrate failed: {}", e))?;
        Ok(metadata.trade_links.values().cloned().collect())
    }

    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.metadata_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {}", e))?;
        }
        let bytes = self.doc.clone().save();
        fs::write(&self.metadata_path, bytes).map_err(|e| format!("Save failed: {}", e))?;
        Ok(())
    }

    pub fn merge_from_file(&mut self, remote_path: &Path) -> Result<(), String> {
        let remote_bytes =
            fs::read(remote_path).map_err(|e| format!("Read remote failed: {}", e))?;
        self.merge_from_bytes(&remote_bytes)?;
        self.log_sync_event(
            "merged_remote",
            "",
            &format!("Merged from {:?}", remote_path),
        )?;
        Ok(())
    }

    /// Merge from raw Automerge bytes. Tolerates "duplicate seq" errors
    /// which occur when merging a doc that already contains our own changes
    /// (e.g. when syncing through a relay that has the merged result).
    pub fn merge_from_bytes(&mut self, remote_bytes: &[u8]) -> Result<(), String> {
        let mut remote_doc =
            AutoCommit::load(remote_bytes).map_err(|e| format!("Load remote failed: {}", e))?;

        match self.doc.merge(&mut remote_doc) {
            Ok(_) => Ok(()),
            Err(e) => {
                let msg = e.to_string();
                if msg.contains("duplicate seq") {
                    // Benign: the remote doc contains changes we already have
                    Ok(())
                } else {
                    Err(format!("Merge failed: {}", e))
                }
            }
        }
    }

    pub fn get_metadata(&self) -> Result<ArimaloMetadata, String> {
        autosurgeon::hydrate(&self.doc).map_err(|e| format!("Hydrate failed: {}", e))
    }

    pub fn log_sync_event(
        &mut self,
        event_type: &str,
        target_id: &str,
        details: &str,
    ) -> Result<(), String> {
        let mut metadata: ArimaloMetadata =
            autosurgeon::hydrate(&self.doc).map_err(|e| format!("Hydrate failed: {}", e))?;

        metadata.sync_log.push(SyncEvent {
            timestamp: now(),
            device_id: self.device_id.clone(),
            event_type: event_type.to_string(),
            target_id: target_id.to_string(),
            details: details.to_string(),
        });

        autosurgeon::reconcile(&mut self.doc, &metadata)
            .map_err(|e| format!("Reconcile failed: {}", e))?;

        Ok(())
    }

    /// Register a file in the metadata file manifest.
    pub fn register_file(
        &mut self,
        content_hash: &str,
        relative_path: &str,
        file_type: &str,
        size_bytes: u64,
    ) -> Result<(), String> {
        let mut metadata: ArimaloMetadata =
            autosurgeon::hydrate(&self.doc).map_err(|e| format!("Hydrate failed: {}", e))?;

        metadata.file_manifest.insert(
            content_hash.to_string(),
            FileEntry {
                content_hash: content_hash.to_string(),
                relative_path: relative_path.to_string(),
                file_type: file_type.to_string(),
                size_bytes,
                device_origin: self.device_id.clone(),
                uploaded_at: now(),
            },
        );

        autosurgeon::reconcile(&mut self.doc, &metadata)
            .map_err(|e| format!("Reconcile failed: {}", e))?;

        Ok(())
    }

    /// Recover metadata entries that were lost during an Automerge merge due to
    /// conflicting map objects created independently by different devices.
    /// Returns true if any entries were recovered.
    pub fn recover_from_snapshot(&mut self, snapshot: &ArimaloMetadata) -> Result<bool, String> {
        let mut current: ArimaloMetadata =
            autosurgeon::hydrate(&self.doc).map_err(|e| format!("Hydrate failed: {}", e))?;

        let mut changed = false;

        // Recover file_manifest entries
        for (hash, entry) in &snapshot.file_manifest {
            if !current.file_manifest.contains_key(hash) {
                current.file_manifest.insert(hash.clone(), entry.clone());
                changed = true;
            }
        }

        // Recover sync_log entries (match by all fields to avoid false dedup)
        for event in &snapshot.sync_log {
            let exists = current.sync_log.iter().any(|e| {
                e.timestamp == event.timestamp
                    && e.device_id == event.device_id
                    && e.event_type == event.event_type
                    && e.target_id == event.target_id
                    && e.details == event.details
            });
            if !exists {
                current.sync_log.push(event.clone());
                changed = true;
            }
        }

        // Recover device entries
        for (id, info) in &snapshot.devices {
            if !current.devices.contains_key(id) {
                current.devices.insert(id.clone(), info.clone());
                changed = true;
            }
        }

        // Recover trade links
        for (id, link) in &snapshot.trade_links {
            if !current.trade_links.contains_key(id) {
                current.trade_links.insert(id.clone(), link.clone());
                changed = true;
            }
        }

        if changed {
            autosurgeon::reconcile(&mut self.doc, &current)
                .map_err(|e| format!("Reconcile failed: {}", e))?;
        }

        Ok(changed)
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

fn extract_txn_id(meta: &Option<String>) -> Option<String> {
    let meta = meta.as_ref()?;
    for part in meta.split(',') {
        let trimmed = part.trim();
        if trimmed.starts_with("txn:") {
            return Some(trimmed.to_string());
        }
    }
    None
}

fn extract_src_ref(meta: &Option<String>) -> Option<String> {
    let meta = meta.as_ref()?;
    for part in meta.split(',') {
        let trimmed = part.trim();
        if trimmed.starts_with("src:") {
            return Some(trimmed.to_string());
        }
    }
    None
}

pub fn get_device_id() -> String {
    machine_uid::get()
        .map(|id| {
            let short = if id.len() >= 8 { &id[..8] } else { &id };
            format!("device-{}", short)
        })
        .unwrap_or_else(|_| "device-unknown".to_string())
}

fn get_hostname() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn trade_link_id(txn_id_a: &str, txn_id_b: &str) -> String {
    let mut pair = [txn_id_a, txn_id_b];
    pair.sort();
    let mut hasher = Sha256::new();
    hasher.update(pair[0].as_bytes());
    hasher.update(pair[1].as_bytes());
    hex::encode(hasher.finalize())[..12].to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeSuggestion {
    pub txn_id_a: String,
    pub txn_id_b: String,
    pub summary: String,
}

/// Suggest trade link candidates from parsed transactions.
/// Looks for pairs on the same date/within 60 seconds that have different
/// commodities, opposite directions, and the same primary account.
/// Prefers pairs that also share the same payee (e.g. "Uniswap V3: …").
///
/// When `price_graph` and `base_currency` are provided, rejects pairs whose
/// base-currency values differ by more than 80% (min/max < 0.2).
pub fn suggest_trade_links(
    transactions: &[crate::ledger_parser::Transaction],
    existing_links: &[TradeLink],
    price_graph: Option<&crate::ledger_parser::PriceGraph>,
    base_currency: Option<&str>,
) -> Vec<TradeSuggestion> {
    use std::collections::HashSet;

    // Build set of already-linked txn IDs
    let linked: HashSet<&str> = existing_links
        .iter()
        .flat_map(|l| [l.txn_id_a.as_str(), l.txn_id_b.as_str()])
        .collect();

    struct TxnInfo {
        txn_id: String,
        datetime: String,
        account: String,
        commodity: String,
        amount: f64,
        payee: String,
        src_ref: Option<String>,
    }

    let mut infos: Vec<TxnInfo> = Vec::new();
    for txn in transactions {
        let txn_id = extract_txn_id(&txn.meta).unwrap_or_default();
        if txn_id.is_empty() || linked.contains(txn_id.as_str()) {
            continue;
        }
        if let Some(posting) = txn.postings.first() {
            infos.push(TxnInfo {
                txn_id,
                datetime: txn.datetime.clone(),
                account: posting.account.clone(),
                commodity: posting.commodity.clone(),
                amount: posting.amount,
                payee: txn.payee.clone().unwrap_or_default(),
                src_ref: extract_src_ref(&txn.meta),
            });
        }
    }

    // Sort by datetime
    infos.sort_by(|a, b| a.datetime.cmp(&b.datetime));

    let mut suggestions = Vec::new();
    let mut used: HashSet<usize> = HashSet::new();

    fn is_swap_candidate(a: &TxnInfo, b: &TxnInfo) -> bool {
        let basic = a.amount.abs() > 1e-9
            && b.amount.abs() > 1e-9
            && a.commodity != b.commodity
            && a.account == b.account
            && ((a.amount < 0.0 && b.amount > 0.0) || (a.amount > 0.0 && b.amount < 0.0));
        let shared_src =
            a.src_ref.is_some() && a.src_ref == b.src_ref && a.commodity != b.commodity;
        basic || shared_src
    }

    for i in 0..infos.len() {
        if used.contains(&i) {
            continue;
        }

        // Collect all same-day/60s candidates
        let mut best: Option<usize> = None;
        let mut best_has_payee_match = false;

        for j in (i + 1)..infos.len() {
            if used.contains(&j) {
                continue;
            }
            if !within_seconds(&infos[i].datetime, &infos[j].datetime, 60) {
                break;
            }
            if is_swap_candidate(&infos[i], &infos[j]) {
                let payee_match = !infos[i].payee.is_empty() && infos[i].payee == infos[j].payee;
                // Prefer payee match; otherwise take first available
                if payee_match && !best_has_payee_match {
                    best = Some(j);
                    best_has_payee_match = true;
                } else if best.is_none() {
                    best = Some(j);
                }
            }
        }

        if let Some(j) = best {
            // Value-ratio filter: reject pairs with wildly mismatched base values
            if let (Some(pg), Some(base)) = (price_graph, base_currency) {
                let val_a = pg.convert_to_base(
                    &infos[i].commodity,
                    infos[i].amount.abs(),
                    &infos[i].datetime,
                    base,
                );
                let val_b = pg.convert_to_base(
                    &infos[j].commodity,
                    infos[j].amount.abs(),
                    &infos[j].datetime,
                    base,
                );
                if let (Some(va), Some(vb)) = (val_a, val_b) {
                    let min = va.min(vb);
                    let max = va.max(vb);
                    if max > 0.0 && min / max < 0.2 {
                        continue; // values differ by more than 80%
                    }
                }
            }

            let (sell, buy) = if infos[i].amount < 0.0 {
                (&infos[i], &infos[j])
            } else {
                (&infos[j], &infos[i])
            };
            suggestions.push(TradeSuggestion {
                txn_id_a: sell.txn_id.clone(),
                txn_id_b: buy.txn_id.clone(),
                summary: format!(
                    "{} {} {} → {} {}",
                    sell.datetime.split('T').next().unwrap_or(&sell.datetime),
                    format!("{:.6}", sell.amount.abs())
                        .trim_end_matches('0')
                        .trim_end_matches('.'),
                    sell.commodity,
                    format!("{:.6}", buy.amount.abs())
                        .trim_end_matches('0')
                        .trim_end_matches('.'),
                    buy.commodity,
                ),
            });
            used.insert(i);
            used.insert(j);
        }
    }

    suggestions
}

/// Check if two datetime strings are within N seconds of each other.
/// Supports "YYYY-MM-DD HH:MM:SS", "YYYY-MM-DDTHH:MM:SS", and date-only "YYYY-MM-DD".
/// Date-only values are treated as same-day (diff = 0).
fn within_seconds(a: &str, b: &str, max_secs: i64) -> bool {
    fn parse_secs(s: &str) -> Option<i64> {
        // Normalize T separator to space
        let s = s.replace('T', " ");
        let parts: Vec<&str> = s.split(' ').collect();
        let date_parts: Vec<i64> = parts[0].split('-').filter_map(|p| p.parse().ok()).collect();
        if date_parts.len() < 3 {
            return None;
        }
        let base = date_parts[0] * 365 * 86400 + date_parts[1] * 30 * 86400 + date_parts[2] * 86400;
        if parts.len() >= 2 {
            let time_parts: Vec<i64> = parts[1].split(':').filter_map(|p| p.parse().ok()).collect();
            if time_parts.len() >= 3 {
                return Some(base + time_parts[0] * 3600 + time_parts[1] * 60 + time_parts[2]);
            }
        }
        Some(base)
    }
    match (parse_secs(a), parse_secs(b)) {
        (Some(a_s), Some(b_s)) => (a_s - b_s).abs() <= max_secs,
        _ => false,
    }
}

pub fn sha256_string(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn sha256_bytes(b: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_extract_txn_id() {
        assert_eq!(
            extract_txn_id(&Some("txn:abc123".to_string())),
            Some("txn:abc123".to_string())
        );
        assert_eq!(
            extract_txn_id(&Some("origin:device-1, txn:def456".to_string())),
            Some("txn:def456".to_string())
        );
        assert_eq!(extract_txn_id(&None), None);
        assert_eq!(extract_txn_id(&Some("no-txn".to_string())), None);
    }

    #[test]
    fn test_sha256_string_deterministic() {
        let a = sha256_string("hello");
        let b = sha256_string("hello");
        assert_eq!(a, b);
        assert_ne!(a, sha256_string("world"));
    }

    #[test]
    fn test_create_empty_store() {
        let tmp = std::env::temp_dir().join("arimalo-test-empty-store.automerge");
        let _ = fs::remove_file(&tmp);
        let store = MetadataStore::new(tmp.clone()).unwrap();
        let meta = store.get_metadata().unwrap();
        assert!(meta.transaction_refs.is_empty());
        assert!(meta.file_manifest.is_empty());
        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_save_and_reload() {
        let tmp = std::env::temp_dir().join("arimalo-test-save-reload.automerge");
        let _ = fs::remove_file(&tmp);

        {
            let mut store = MetadataStore::new(tmp.clone()).unwrap();
            store
                .log_sync_event("test_event", "target-1", "testing save")
                .unwrap();
            store.save().unwrap();
        }

        {
            let store = MetadataStore::new(tmp.clone()).unwrap();
            let meta = store.get_metadata().unwrap();
            assert!(!meta.sync_log.is_empty());
            assert_eq!(meta.sync_log[0].event_type, "test_event");
        }

        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_merge_two_docs() {
        let tmp_a = std::env::temp_dir().join("arimalo-test-merge-a.automerge");
        let tmp_b = std::env::temp_dir().join("arimalo-test-merge-b.automerge");
        let _ = fs::remove_file(&tmp_a);
        let _ = fs::remove_file(&tmp_b);

        // Device A creates the original doc
        {
            let mut store_a = MetadataStore::new(tmp_a.clone()).unwrap();
            store_a.log_sync_event("event_a", "", "from A").unwrap();
            store_a.save().unwrap();
        }

        // Device B loads from A's doc (shared origin), then adds its own event
        fs::copy(&tmp_a, &tmp_b).unwrap();
        {
            let mut store_b = MetadataStore::new(tmp_b.clone()).unwrap();
            store_b.log_sync_event("event_b", "", "from B").unwrap();
            store_b.save().unwrap();
        }

        // Device A adds more work concurrently
        {
            let mut store_a = MetadataStore::new(tmp_a.clone()).unwrap();
            store_a
                .log_sync_event("event_a2", "", "more from A")
                .unwrap();
            store_a.save().unwrap();
        }

        // Merge B into A
        {
            let mut store_a = MetadataStore::new(tmp_a.clone()).unwrap();
            store_a.merge_from_file(&tmp_b).unwrap();
            let meta = store_a.get_metadata().unwrap();
            let event_types: Vec<&str> = meta
                .sync_log
                .iter()
                .map(|e| e.event_type.as_str())
                .collect();
            assert!(
                event_types.contains(&"event_a"),
                "missing event_a; got: {:?}",
                event_types
            );
            assert!(
                event_types.contains(&"event_b"),
                "missing event_b; got: {:?}",
                event_types
            );
            assert!(
                event_types.contains(&"event_a2"),
                "missing event_a2; got: {:?}",
                event_types
            );
        }

        let _ = fs::remove_file(&tmp_a);
        let _ = fs::remove_file(&tmp_b);
    }

    #[test]
    fn test_suggest_trade_links_rejects_mismatched_values() {
        use crate::ledger_parser::{Posting, PriceDirective, PriceGraph, Transaction};

        fn test_posting(account: &str, amount: f64, commodity: &str) -> Posting {
            Posting {
                account: account.to_string(),
                amount,
                amount_text: format!("{}", amount),
                commodity: commodity.to_string(),
                remainder: None,
                cost: None,
                price: None,
            }
        }

        // Two transactions: sell 200 USDC, buy 0.002 ETH
        // At ETH=3000 USD, USDC=1 USD → $200 vs $6 → ratio 0.03 → rejected
        let txns = vec![
            Transaction {
                date: "2025-01-15".to_string(),
                datetime: "2025-01-15 10:00:00".to_string(),
                status: Some('*'),
                payee: Some("Swap".to_string()),
                narration: None,
                meta: Some("txn:sell-usdc".to_string()),
                postings: vec![test_posting("assets:exchange", -200.0, "USDC")],
                display_payee: None,
                amount: -200.0,
                amount_commodity: "USDC".to_string(),
                display_amount_commodity: None,
                fee: None,
                fee_commodity: None,
            },
            Transaction {
                date: "2025-01-15".to_string(),
                datetime: "2025-01-15 10:00:30".to_string(),
                status: Some('*'),
                payee: Some("Swap".to_string()),
                narration: None,
                meta: Some("txn:buy-eth".to_string()),
                postings: vec![test_posting("assets:exchange", 0.002, "ETH")],
                display_payee: None,
                display_amount_commodity: None,
                amount: 0.002,
                amount_commodity: "ETH".to_string(),
                fee: None,
                fee_commodity: None,
            },
        ];

        let pg = PriceGraph::from_entries(vec![
            PriceDirective {
                datetime: "2025-01-01".to_string(),
                commodity: "ETH".to_string(),
                price_amount: 3000.0,
                price_amount_text: "3000".to_string(),
                quote_commodity: "USD".to_string(),
            },
            PriceDirective {
                datetime: "2025-01-01".to_string(),
                commodity: "USDC".to_string(),
                price_amount: 1.0,
                price_amount_text: "1".to_string(),
                quote_commodity: "USD".to_string(),
            },
        ]);

        // With price data → should reject the mismatched pair
        let suggestions = suggest_trade_links(&txns, &[], Some(&pg), Some("USD"));
        assert!(
            suggestions.is_empty(),
            "expected no suggestions for mismatched values, got: {:?}",
            suggestions,
        );

        // Without price data → should still suggest the pair
        let suggestions_no_price = suggest_trade_links(&txns, &[], None, None);
        assert_eq!(
            suggestions_no_price.len(),
            1,
            "expected 1 suggestion without price data, got: {:?}",
            suggestions_no_price,
        );
    }

    #[test]
    fn test_suggest_trade_links_bybit_multi_pair() {
        use crate::ledger_parser::{Posting, Transaction};

        fn test_posting(account: &str, amount: f64, commodity: &str) -> Posting {
            Posting {
                account: account.to_string(),
                amount,
                amount_text: format!("{}", amount),
                commodity: commodity.to_string(),
                remainder: None,
                cost: None,
                price: None,
            }
        }

        // Reproduces: 5 HNT buys + 5 USDT sells at the same second on Bybit
        // All should pair up as trade suggestions
        let account = "assets:crypto:exchange:bybit:personal";
        let make_txn = |id: &str, amount: f64, commodity: &str, datetime: &str| -> Transaction {
            Transaction {
                date: datetime.split(' ').next().unwrap().to_string(),
                datetime: datetime.to_string(),
                status: Some('*'),
                payee: Some("Bybit".to_string()),
                narration: Some("BUY HNTUSDT".to_string()),
                meta: Some(format!("txn:{id}")),
                postings: vec![test_posting(account, amount, commodity)],
                display_payee: None,
                amount,
                amount_commodity: commodity.to_string(),
                display_amount_commodity: None,
                fee: None,
                fee_commodity: None,
            }
        };

        let txns = vec![
            // HNT buys (positive)
            make_txn("csv-hnt1", 26.75, "HNT", "2025-11-13 18:44:34"),
            make_txn("csv-hnt2", 9.96, "HNT", "2025-11-13 18:44:35"),
            make_txn("csv-hnt3", 3.19, "HNT", "2025-11-13 18:44:35"),
            make_txn("csv-hnt4", 7.20, "HNT", "2025-11-13 18:44:35"),
            make_txn("csv-hnt5", 6.39, "HNT", "2025-11-13 18:44:35"),
            // USDT sells (negative)
            make_txn("csv-usdt1", -59.76, "USDT", "2025-11-13 18:44:34"),
            make_txn("csv-usdt2", -22.15, "USDT", "2025-11-13 18:44:35"),
            make_txn("csv-usdt3", -14.22, "USDT", "2025-11-13 18:44:35"),
            make_txn("csv-usdt4", -7.09, "USDT", "2025-11-13 18:44:35"),
            make_txn("csv-usdt5", -16.03, "USDT", "2025-11-13 18:44:35"),
        ];

        let suggestions = suggest_trade_links(&txns, &[], None, None);
        assert_eq!(
            suggestions.len(),
            5,
            "expected 5 trade suggestions for 5 HNT/USDT pairs, got {}: {:?}",
            suggestions.len(),
            suggestions,
        );

        // Now test with a price graph where HNT has a known price
        use crate::ledger_parser::{PriceDirective, PriceGraph};
        let pg = PriceGraph::from_entries(vec![
            PriceDirective {
                datetime: "2025-11-01".to_string(),
                commodity: "HNT".to_string(),
                price_amount: 5.0,
                price_amount_text: "5".to_string(),
                quote_commodity: "AUD".to_string(),
            },
            PriceDirective {
                datetime: "2025-11-01".to_string(),
                commodity: "USDT".to_string(),
                price_amount: 1.5,
                price_amount_text: "1.5".to_string(),
                quote_commodity: "AUD".to_string(),
            },
        ]);
        let suggestions_priced = suggest_trade_links(&txns, &[], Some(&pg), Some("AUD"));
        assert_eq!(
            suggestions_priced.len(),
            5,
            "expected 5 suggestions even WITH price data (values should roughly match), got {}: {:?}",
            suggestions_priced.len(),
            suggestions_priced,
        );
    }
}
