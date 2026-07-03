use crate::automerge_store::MetadataStore;
use crate::content_store::ContentStore;
use std::path::Path;

/// Result of a two-phase sync operation.
#[derive(Debug, Clone)]
pub struct SyncResult {
    pub files_transferred: usize,
    pub metadata_merged: bool,
    pub missing_from_local: Vec<String>,
    pub missing_from_remote: Vec<String>,
}

/// Compare two metadata stores and find file hashes missing from each side.
pub fn diff_manifests(
    local: &MetadataStore,
    remote: &MetadataStore,
) -> Result<(Vec<String>, Vec<String>), String> {
    let local_meta = local.get_metadata()?;
    let remote_meta = remote.get_metadata()?;

    let local_hashes: std::collections::HashSet<&str> = local_meta
        .file_manifest
        .keys()
        .map(|k| k.as_str())
        .collect();
    let remote_hashes: std::collections::HashSet<&str> = remote_meta
        .file_manifest
        .keys()
        .map(|k| k.as_str())
        .collect();

    let missing_from_local: Vec<String> = remote_hashes
        .difference(&local_hashes)
        .map(|h| h.to_string())
        .collect();
    let missing_from_remote: Vec<String> = local_hashes
        .difference(&remote_hashes)
        .map(|h| h.to_string())
        .collect();

    Ok((missing_from_local, missing_from_remote))
}

/// Phase 1: Sync Automerge metadata between local and remote.
pub fn sync_metadata(local: &mut MetadataStore, remote_metadata_path: &Path) -> Result<(), String> {
    local.merge_from_file(remote_metadata_path)?;
    Ok(())
}

/// Phase 2: Transfer missing CAS blobs from remote to local.
pub fn sync_files(
    local_cas: &ContentStore,
    remote_cas: &ContentStore,
    missing_hashes: &[String],
) -> Result<usize, String> {
    let mut transferred = 0;
    for hash in missing_hashes {
        if local_cas.exists(hash) {
            continue; // already have it
        }
        let content = remote_cas.retrieve(hash)?;
        local_cas.store(&content)?;
        transferred += 1;
    }
    Ok(transferred)
}

/// Full two-phase sync: metadata first, then files.
pub fn full_sync(
    local_store: &mut MetadataStore,
    local_cas: &ContentStore,
    remote_metadata_path: &Path,
    remote_cas: &ContentStore,
) -> Result<SyncResult, String> {
    // Phase 1: Merge metadata
    sync_metadata(local_store, remote_metadata_path)?;

    // After merge, get the combined manifest to find what we're missing
    let merged_meta = local_store.get_metadata()?;
    let all_hashes: Vec<String> = merged_meta.file_manifest.keys().cloned().collect();

    // Find hashes missing from local CAS
    let missing: Vec<String> = all_hashes
        .iter()
        .filter(|h| !local_cas.exists(h))
        .cloned()
        .collect();

    // Find hashes missing from remote CAS (for reporting)
    let missing_from_remote: Vec<String> = all_hashes
        .iter()
        .filter(|h| !remote_cas.exists(h))
        .cloned()
        .collect();

    // Phase 2: Transfer missing files
    let transferred = sync_files(local_cas, remote_cas, &missing)?;

    // Log the sync event
    local_store.log_sync_event(
        "full_sync",
        "",
        &format!("Transferred {} files", transferred),
    )?;

    Ok(SyncResult {
        files_transferred: transferred,
        metadata_merged: true,
        missing_from_local: missing,
        missing_from_remote,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automerge_store::MetadataStore;
    use crate::content_store::ContentStore;
    use std::fs;

    fn temp_dir(prefix: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("{}-{}", prefix, nanos))
    }

    #[test]
    fn test_full_sync_transfers_files() {
        let dir_a = temp_dir("sync-a");
        let dir_b = temp_dir("sync-b");
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();

        let cas_a = ContentStore::new(dir_a.join("cas"));
        let cas_b = ContentStore::new(dir_b.join("cas"));

        // Device A creates metadata, stores a file, and registers it
        let meta_a_path = dir_a.join("metadata.automerge");
        let mut store_a = MetadataStore::new(meta_a_path.clone()).unwrap();
        let hash_a = cas_a.store(b"file from A").unwrap();
        store_a
            .register_file(&hash_a, "file_a.csv", "csv", 11)
            .unwrap();
        store_a.save().unwrap();

        // Device B loads A's metadata and stores a different file
        let meta_b_path = dir_b.join("metadata.automerge");
        fs::copy(&meta_a_path, &meta_b_path).unwrap();
        let mut store_b = MetadataStore::new(meta_b_path.clone()).unwrap();
        let hash_b = cas_b.store(b"file from B").unwrap();
        store_b
            .register_file(&hash_b, "file_b.csv", "csv", 11)
            .unwrap();
        store_b.save().unwrap();

        // Sync B into A
        let result = full_sync(&mut store_a, &cas_a, &meta_b_path, &cas_b).unwrap();

        assert!(result.metadata_merged);
        // A should now have B's file
        assert!(cas_a.exists(&hash_b), "A should have B's blob");
        assert!(cas_a.exists(&hash_a), "A should still have its own blob");

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }

    #[test]
    fn test_sync_no_transfer_when_same() {
        let dir_a = temp_dir("sync-same-a");
        let dir_b = temp_dir("sync-same-b");
        fs::create_dir_all(&dir_a).unwrap();
        fs::create_dir_all(&dir_b).unwrap();

        let cas_a = ContentStore::new(dir_a.join("cas"));
        let cas_b = ContentStore::new(dir_b.join("cas"));

        // Both have the same file
        cas_a.store(b"shared content").unwrap();
        cas_b.store(b"shared content").unwrap();

        let meta_a_path = dir_a.join("metadata.automerge");
        let mut store_a = MetadataStore::new(meta_a_path.clone()).unwrap();
        store_a.save().unwrap();

        let meta_b_path = dir_b.join("metadata.automerge");
        fs::copy(&meta_a_path, &meta_b_path).unwrap();

        let result = full_sync(&mut store_a, &cas_a, &meta_b_path, &cas_b).unwrap();
        assert_eq!(result.files_transferred, 0);

        let _ = fs::remove_dir_all(&dir_a);
        let _ = fs::remove_dir_all(&dir_b);
    }
}
