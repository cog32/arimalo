use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Content-addressed storage (CAS).
///
/// Files are stored under `<cas_dir>/<prefix>/<rest_of_hash>` where
/// `prefix` is the first 2 hex characters of the SHA-256 hash.
#[derive(Debug)]
pub struct ContentStore {
    cas_dir: PathBuf,
}

/// Result of a verification check against the manifest.
#[derive(Debug, Clone, PartialEq)]
pub enum BlobStatus {
    Ok,
    Missing(String),
    Corrupted { hash: String, actual: String },
}

impl ContentStore {
    pub fn new(cas_dir: PathBuf) -> Self {
        Self { cas_dir }
    }

    pub fn cas_dir(&self) -> &Path {
        &self.cas_dir
    }

    /// Store bytes in CAS, returning the content hash.
    pub fn store(&self, content: &[u8]) -> Result<String, String> {
        let hash = sha256_hex(content);
        let blob_path = self.blob_path(&hash);

        if blob_path.exists() {
            return Ok(hash); // already stored (dedup)
        }

        if let Some(parent) = blob_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("CAS mkdir failed: {}", e))?;
        }
        fs::write(&blob_path, content).map_err(|e| format!("CAS write failed: {}", e))?;

        Ok(hash)
    }

    /// Store a file from disk into CAS, returning the content hash.
    pub fn store_file(&self, path: &Path) -> Result<String, String> {
        let content = fs::read(path).map_err(|e| format!("Read failed: {}", e))?;
        self.store(&content)
    }

    /// Retrieve blob bytes by hash.
    pub fn retrieve(&self, hash: &str) -> Result<Vec<u8>, String> {
        let blob_path = self.blob_path(hash);
        fs::read(&blob_path).map_err(|e| format!("CAS read failed for {}: {}", hash, e))
    }

    /// Check whether a blob exists.
    pub fn exists(&self, hash: &str) -> bool {
        self.blob_path(hash).exists()
    }

    /// Verify integrity of a stored blob.
    pub fn verify(&self, hash: &str) -> Result<BlobStatus, String> {
        let blob_path = self.blob_path(hash);
        if !blob_path.exists() {
            return Ok(BlobStatus::Missing(hash.to_string()));
        }
        let content = fs::read(&blob_path).map_err(|e| format!("CAS read failed: {}", e))?;
        let actual = sha256_hex(&content);
        if actual == hash {
            Ok(BlobStatus::Ok)
        } else {
            Ok(BlobStatus::Corrupted {
                hash: hash.to_string(),
                actual,
            })
        }
    }

    /// Count the number of blobs in the store.
    pub fn blob_count(&self) -> usize {
        if !self.cas_dir.exists() {
            return 0;
        }
        WalkDir::new(&self.cas_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| {
                let path = e.path();
                path.is_file()
                    && !path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with('.'))
                        .unwrap_or(false)
            })
            .count()
    }

    /// Verify all hashes in a list, returning statuses.
    pub fn verify_manifest(&self, hashes: &[String]) -> Vec<BlobStatus> {
        hashes
            .iter()
            .map(|h| self.verify(h).unwrap_or(BlobStatus::Missing(h.clone())))
            .collect()
    }

    /// Delete a blob by hash. Used for testing corruption/missing scenarios.
    pub fn delete_blob(&self, hash: &str) -> Result<(), String> {
        let blob_path = self.blob_path(hash);
        fs::remove_file(&blob_path).map_err(|e| format!("CAS delete failed: {}", e))
    }

    /// Corrupt a blob by overwriting with garbage. Used for testing.
    pub fn corrupt_blob(&self, hash: &str) -> Result<(), String> {
        let blob_path = self.blob_path(hash);
        fs::write(&blob_path, b"CORRUPTED").map_err(|e| format!("CAS corrupt failed: {}", e))
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        let prefix = &hash[..2];
        let rest = &hash[2..];
        self.cas_dir.join(prefix).join(rest)
    }
}

/// Ingest all relevant files from a sources directory into CAS.
/// Returns a list of (relative_path, content_hash, size_bytes) tuples.
pub fn ingest_sources_to_cas(
    sources_dir: &Path,
    cas: &ContentStore,
) -> Result<Vec<(String, String, u64)>, String> {
    let mut results = Vec::new();

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
        match ext {
            "csv" | "pdf" | "rhai" | "json" | "transactions" => {}
            _ => continue,
        }

        let content = fs::read(path).map_err(|e| format!("Read failed: {}", e))?;
        let size = content.len() as u64;
        let hash = cas.store(&content)?;

        let relative = path
            .strip_prefix(sources_dir)
            .map_err(|e| format!("Path error: {}", e))?
            .to_string_lossy()
            .to_string();

        results.push((relative, hash, size));
    }

    Ok(results)
}

pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicU64, Ordering};
    static TEST_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_cas() -> (PathBuf, ContentStore) {
        // PID + counter so each test gets a unique dir even under
        // process-per-test runners (e.g. nextest), where the counter
        // restarts at 0 in each process.
        let id = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let pid = std::process::id();
        let dir = std::env::temp_dir().join(format!("arimalo-cas-test-{}-{}", pid, id));
        let _ = fs::remove_dir_all(&dir); // clean any leftover from prior runs
        let store = ContentStore::new(dir.clone());
        (dir, store)
    }

    fn cleanup(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn test_store_and_retrieve() {
        let (dir, cas) = temp_cas();
        let hash = cas.store(b"hello world").unwrap();
        assert!(!hash.is_empty());
        let content = cas.retrieve(&hash).unwrap();
        assert_eq!(content, b"hello world");
        cleanup(&dir);
    }

    #[test]
    fn test_deduplication() {
        let (dir, cas) = temp_cas();
        let h1 = cas.store(b"same").unwrap();
        let h2 = cas.store(b"same").unwrap();
        assert_eq!(h1, h2);
        assert_eq!(cas.blob_count(), 1);
        cleanup(&dir);
    }

    #[test]
    fn test_different_content() {
        let (dir, cas) = temp_cas();
        let h1 = cas.store(b"aaa").unwrap();
        let h2 = cas.store(b"bbb").unwrap();
        assert_ne!(h1, h2);
        assert_eq!(cas.blob_count(), 2);
        cleanup(&dir);
    }

    #[test]
    fn test_verify_ok() {
        let (dir, cas) = temp_cas();
        let hash = cas.store(b"verify me").unwrap();
        assert_eq!(cas.verify(&hash).unwrap(), BlobStatus::Ok);
        cleanup(&dir);
    }

    #[test]
    fn test_verify_corrupted() {
        let (dir, cas) = temp_cas();
        let hash = cas.store(b"original").unwrap();
        cas.corrupt_blob(&hash).unwrap();
        match cas.verify(&hash).unwrap() {
            BlobStatus::Corrupted { .. } => {}
            other => panic!("expected Corrupted, got {:?}", other),
        }
        cleanup(&dir);
    }

    #[test]
    fn test_verify_missing() {
        let (dir, cas) = temp_cas();
        let hash = cas.store(b"will delete").unwrap();
        cas.delete_blob(&hash).unwrap();
        assert_eq!(cas.verify(&hash).unwrap(), BlobStatus::Missing(hash));
        cleanup(&dir);
    }
}
