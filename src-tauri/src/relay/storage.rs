use automerge::AutoCommit;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::automerge_store::ArimaloMetadata;

/// Relay-side storage for a sync group. Stores metadata (Automerge doc)
/// and blobs using the same CAS layout as ContentStore.
#[derive(Debug)]
pub struct GroupStorage {
    group_dir: PathBuf,
}

impl GroupStorage {
    pub fn new(data_dir: &Path, group_id: &str) -> Self {
        let group_dir = data_dir.join("groups").join(group_id);
        Self { group_dir }
    }

    pub fn ensure_dirs(&self) -> Result<(), String> {
        fs::create_dir_all(self.group_dir.join("blobs"))
            .map_err(|e| format!("Failed to create group dirs: {}", e))?;
        Ok(())
    }

    // ── Metadata ────────────────────────────────────────────────

    fn metadata_path(&self) -> PathBuf {
        self.group_dir.join("metadata.automerge")
    }

    /// Load metadata bytes. Returns None if no metadata exists yet.
    pub fn load_metadata(&self) -> Result<Option<Vec<u8>>, String> {
        let path = self.metadata_path();
        if !path.exists() {
            return Ok(None);
        }
        fs::read(&path)
            .map(Some)
            .map_err(|e| format!("Read metadata failed: {}", e))
    }

    /// Save metadata bytes (merging with existing if present).
    /// After the Automerge merge, recovers any metadata entries lost due to
    /// conflicting map objects created independently by different devices.
    pub fn merge_and_save_metadata(&self, incoming: &[u8]) -> Result<(), String> {
        self.ensure_dirs()?;
        let path = self.metadata_path();

        let mut incoming_doc = AutoCommit::load(incoming)
            .map_err(|e| format!("Load incoming metadata failed: {}", e))?;

        if path.exists() {
            let existing =
                fs::read(&path).map_err(|e| format!("Read existing metadata failed: {}", e))?;
            let mut existing_doc = AutoCommit::load(&existing)
                .map_err(|e| format!("Load existing metadata failed: {}", e))?;

            // Snapshot both sides before merge so we can recover lost entries
            let existing_meta: ArimaloMetadata =
                autosurgeon::hydrate(&existing_doc).unwrap_or_default();
            let incoming_meta: ArimaloMetadata =
                autosurgeon::hydrate(&incoming_doc).unwrap_or_default();

            match existing_doc.merge(&mut incoming_doc) {
                Ok(_) => {}
                Err(e) => {
                    let msg = e.to_string();
                    if !msg.contains("duplicate seq") {
                        return Err(format!("Merge metadata failed: {}", e));
                    }
                }
            }

            // Recover entries lost due to Automerge map conflicts.
            // When two devices independently create their docs, the file_manifest
            // (and other maps/vecs) conflict — only one side "wins" during hydration.
            let mut merged_meta: ArimaloMetadata =
                autosurgeon::hydrate(&existing_doc).unwrap_or_default();
            let mut recovered = false;

            for (hash, entry) in existing_meta
                .file_manifest
                .iter()
                .chain(incoming_meta.file_manifest.iter())
            {
                if !merged_meta.file_manifest.contains_key(hash) {
                    merged_meta
                        .file_manifest
                        .insert(hash.clone(), entry.clone());
                    recovered = true;
                }
            }

            for event in existing_meta
                .sync_log
                .iter()
                .chain(incoming_meta.sync_log.iter())
            {
                let exists = merged_meta.sync_log.iter().any(|e| {
                    e.timestamp == event.timestamp
                        && e.device_id == event.device_id
                        && e.event_type == event.event_type
                        && e.target_id == event.target_id
                        && e.details == event.details
                });
                if !exists {
                    merged_meta.sync_log.push(event.clone());
                    recovered = true;
                }
            }

            for (id, info) in existing_meta
                .devices
                .iter()
                .chain(incoming_meta.devices.iter())
            {
                if !merged_meta.devices.contains_key(id) {
                    merged_meta.devices.insert(id.clone(), info.clone());
                    recovered = true;
                }
            }

            if recovered {
                autosurgeon::reconcile(&mut existing_doc, &merged_meta)
                    .map_err(|e| format!("Reconcile recovery failed: {}", e))?;
            }

            let bytes = existing_doc.save();
            fs::write(&path, bytes).map_err(|e| format!("Write merged metadata failed: {}", e))?;
        } else {
            fs::write(&path, incoming).map_err(|e| format!("Write metadata failed: {}", e))?;
        }

        Ok(())
    }

    // ── Blobs ───────────────────────────────────────────────────

    fn blob_path(&self, hash: &str) -> PathBuf {
        let prefix = &hash[..2.min(hash.len())];
        let rest = &hash[2.min(hash.len())..];
        self.group_dir.join("blobs").join(prefix).join(rest)
    }

    pub fn has_blob(&self, hash: &str) -> bool {
        self.blob_path(hash).exists()
    }

    pub fn load_blob(&self, hash: &str) -> Result<Option<Vec<u8>>, String> {
        let path = self.blob_path(hash);
        if !path.exists() {
            return Ok(None);
        }
        fs::read(&path)
            .map(Some)
            .map_err(|e| format!("Read blob failed: {}", e))
    }

    pub fn store_blob(&self, hash: &str, content: &[u8]) -> Result<(), String> {
        let path = self.blob_path(hash);
        if path.exists() {
            return Ok(()); // dedup
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Blob mkdir failed: {}", e))?;
        }
        fs::write(&path, content).map_err(|e| format!("Write blob failed: {}", e))
    }

    pub fn list_blobs(&self) -> Result<Vec<String>, String> {
        let blobs_dir = self.group_dir.join("blobs");
        if !blobs_dir.exists() {
            return Ok(Vec::new());
        }

        let mut hashes = Vec::new();
        for entry in WalkDir::new(&blobs_dir).into_iter().filter_map(|e| e.ok()) {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            // Skip hidden files
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with('.'))
                .unwrap_or(false)
            {
                continue;
            }

            // Reconstruct hash from prefix/rest path structure
            if let Ok(relative) = path.strip_prefix(&blobs_dir) {
                let parts: Vec<_> = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().to_string())
                    .collect();
                if parts.len() == 2 {
                    hashes.push(format!("{}{}", parts[0], parts[1]));
                }
            }
        }

        Ok(hashes)
    }

    /// List blobs as a HashSet for efficient lookup.
    pub fn list_blobs_set(&self) -> Result<HashSet<String>, String> {
        self.list_blobs().map(|v| v.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        // Tests run in parallel — include process id and an atomic
        // counter so two tests that hit the same nanosecond don't
        // collide on the temp directory.
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "relay-storage-test-{}-{}-{}",
            std::process::id(),
            nanos,
            n,
        ))
    }

    #[test]
    fn test_metadata_roundtrip() {
        let dir = temp_dir();
        let storage = GroupStorage::new(&dir, "test-group");
        storage.ensure_dirs().unwrap();

        // No metadata initially
        assert!(storage.load_metadata().unwrap().is_none());

        // Create and store a minimal Automerge doc
        let mut doc = AutoCommit::new();
        let bytes = doc.save();
        storage.merge_and_save_metadata(&bytes).unwrap();

        // Should now exist
        assert!(storage.load_metadata().unwrap().is_some());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_blob_roundtrip() {
        let dir = temp_dir();
        let storage = GroupStorage::new(&dir, "test-group");
        storage.ensure_dirs().unwrap();

        let hash = "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890";
        assert!(!storage.has_blob(hash));

        storage.store_blob(hash, b"test content").unwrap();
        assert!(storage.has_blob(hash));

        let content = storage.load_blob(hash).unwrap().unwrap();
        assert_eq!(content, b"test content");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_list_blobs() {
        let dir = temp_dir();
        let storage = GroupStorage::new(&dir, "test-group");
        storage.ensure_dirs().unwrap();

        let h1 = "aa11111111111111111111111111111111111111111111111111111111111111";
        let h2 = "bb22222222222222222222222222222222222222222222222222222222222222";

        storage.store_blob(h1, b"one").unwrap();
        storage.store_blob(h2, b"two").unwrap();

        let blobs = storage.list_blobs().unwrap();
        assert_eq!(blobs.len(), 2);
        assert!(blobs.contains(&h1.to_string()));
        assert!(blobs.contains(&h2.to_string()));

        let _ = fs::remove_dir_all(&dir);
    }
}
