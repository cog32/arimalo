use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;

use crate::automerge_store::MetadataStore;
use crate::content_store::ContentStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    pub relay_url: String,
    pub group_id: String,
}

#[derive(Debug, Deserialize)]
struct PairInitiateResponse {
    group_id: String,
    pairing_code: String,
    expires_in: u64,
}

#[derive(Debug, Deserialize)]
struct PairJoinResponse {
    group_id: String,
}

#[derive(Debug, Deserialize)]
struct BlobListResponse {
    hashes: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct PairResult {
    pub group_id: String,
    pub pairing_code: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone)]
pub struct RelaySyncResult {
    pub metadata_merged: bool,
    pub blobs_uploaded: usize,
    pub blobs_downloaded: usize,
}

/// Initiate pairing with the relay server. Returns group_id and pairing code.
pub fn pair_initiate(relay_url: &str) -> Result<PairResult, String> {
    let url = format!("{}/pair/initiate", relay_url.trim_end_matches('/'));
    let resp: PairInitiateResponse = ureq::post(&url)
        .call()
        .map_err(|e| format!("Pair initiate failed: {}", e))?
        .into_json()
        .map_err(|e| format!("Parse response failed: {}", e))?;

    Ok(PairResult {
        group_id: resp.group_id,
        pairing_code: resp.pairing_code,
        expires_in: resp.expires_in,
    })
}

/// Join a group using a pairing code. Returns the group_id.
pub fn pair_join(relay_url: &str, pairing_code: &str) -> Result<String, String> {
    let url = format!("{}/pair/join", relay_url.trim_end_matches('/'));
    let body = serde_json::json!({ "pairing_code": pairing_code });
    let resp: PairJoinResponse = ureq::post(&url)
        .send_json(&body)
        .map_err(|e| format!("Pair join failed: {}", e))?
        .into_json()
        .map_err(|e| format!("Parse response failed: {}", e))?;

    Ok(resp.group_id)
}

/// Download metadata from relay. Returns raw bytes or None if 404.
fn download_metadata(relay_url: &str, group_id: &str) -> Result<Option<Vec<u8>>, String> {
    let url = format!("{}/metadata/{}", relay_url.trim_end_matches('/'), group_id);
    match ureq::get(&url).call() {
        Ok(resp) => {
            let mut bytes = Vec::new();
            resp.into_reader()
                .read_to_end(&mut bytes)
                .map_err(|e| format!("Read metadata failed: {}", e))?;
            Ok(Some(bytes))
        }
        Err(ureq::Error::Status(404, _)) => Ok(None),
        Err(e) => Err(format!("Download metadata failed: {}", e)),
    }
}

/// Upload metadata to relay.
fn upload_metadata(relay_url: &str, group_id: &str, bytes: &[u8]) -> Result<(), String> {
    let url = format!("{}/metadata/{}", relay_url.trim_end_matches('/'), group_id);
    ureq::post(&url)
        .set("Content-Type", "application/octet-stream")
        .send_bytes(bytes)
        .map_err(|e| format!("Upload metadata failed: {}", e))?;
    Ok(())
}

/// List remote blob hashes.
fn list_remote_blobs(relay_url: &str, group_id: &str) -> Result<HashSet<String>, String> {
    let url = format!(
        "{}/blobs/{}/list",
        relay_url.trim_end_matches('/'),
        group_id
    );
    let resp: BlobListResponse = ureq::get(&url)
        .call()
        .map_err(|e| format!("List blobs failed: {}", e))?
        .into_json()
        .map_err(|e| format!("Parse blob list failed: {}", e))?;
    Ok(resp.hashes.into_iter().collect())
}

/// Download a blob from relay.
fn download_blob(relay_url: &str, group_id: &str, hash: &str) -> Result<Vec<u8>, String> {
    let url = format!(
        "{}/blobs/{}/{}",
        relay_url.trim_end_matches('/'),
        group_id,
        hash
    );
    let resp = ureq::get(&url)
        .call()
        .map_err(|e| format!("Download blob failed: {}", e))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Read blob failed: {}", e))?;
    Ok(bytes)
}

/// Upload a blob to relay.
fn upload_blob(relay_url: &str, group_id: &str, hash: &str, content: &[u8]) -> Result<(), String> {
    let url = format!(
        "{}/blobs/{}/{}",
        relay_url.trim_end_matches('/'),
        group_id,
        hash
    );
    ureq::post(&url)
        .set("Content-Type", "application/octet-stream")
        .send_bytes(content)
        .map_err(|e| format!("Upload blob failed: {}", e))?;
    Ok(())
}

/// Full sync with relay server:
/// 1. Upload local metadata (relay merges server-side)
/// 2. Download the relay's merged metadata and reload
/// 3. Download missing blobs from relay
/// 4. Upload missing blobs to relay
/// 5. Log sync event
pub fn sync_with_relay(
    store: &mut MetadataStore,
    local_cas: &ContentStore,
    config: &RelayConfig,
) -> Result<RelaySyncResult, String> {
    let relay_url = &config.relay_url;
    let group_id = &config.group_id;

    // 1. Snapshot local metadata before uploading (needed to recover
    //    manifest entries that may be lost during Automerge conflict
    //    resolution when two devices independently create the file_manifest map)
    let pre_sync_meta = store.get_metadata()?;

    // 2. Upload local metadata (relay does server-side merge)
    store.save()?;
    let local_bytes = std::fs::read(store.metadata_path())
        .map_err(|e| format!("Read local metadata failed: {}", e))?;
    upload_metadata(relay_url, group_id, &local_bytes)?;

    // 3. Download the relay's merged result and reload locally
    let metadata_merged = if let Some(merged_bytes) = download_metadata(relay_url, group_id)? {
        std::fs::write(store.metadata_path(), &merged_bytes)
            .map_err(|e| format!("Write merged metadata failed: {}", e))?;
        *store = MetadataStore::new(store.metadata_path().to_path_buf())?;
        true
    } else {
        false
    };

    // 4. Recover metadata entries lost during Automerge merge.
    //    When two devices independently create their Automerge docs, the
    //    CRDT map objects conflict and only one "wins" during merge.
    //    This recovers file_manifest, sync_log, and device entries from
    //    the pre-sync snapshot.
    let recovered = store.recover_from_snapshot(&pre_sync_meta)?;

    // 4b. If we recovered entries, re-upload so the relay also has the complete metadata
    if recovered {
        store.save()?;
        let fixed_bytes = std::fs::read(store.metadata_path())
            .map_err(|e| format!("Read fixed metadata failed: {}", e))?;
        upload_metadata(relay_url, group_id, &fixed_bytes)?;
    }

    // 5. Get list of remote blobs
    let remote_hashes = list_remote_blobs(relay_url, group_id)?;

    // 6. Build combined local hashes from manifest (now includes recovered entries)
    let meta = store.get_metadata()?;
    let local_hashes: HashSet<String> = meta.file_manifest.keys().cloned().collect();

    // Download missing blobs (remote has, local doesn't)
    let mut blobs_downloaded = 0;
    for hash in &remote_hashes {
        if !local_cas.exists(hash) {
            let content = download_blob(relay_url, group_id, hash)?;
            local_cas.store(&content)?;
            blobs_downloaded += 1;
        }
    }

    // Upload missing blobs (local has, remote doesn't)
    let mut blobs_uploaded = 0;
    for hash in &local_hashes {
        if !remote_hashes.contains(hash) && local_cas.exists(hash) {
            let content = local_cas.retrieve(hash)?;
            upload_blob(relay_url, group_id, hash, &content)?;
            blobs_uploaded += 1;
        }
    }

    // 5. Log sync event
    store.log_sync_event(
        "relay_sync",
        group_id,
        &format!(
            "Synced with relay: {} up, {} down",
            blobs_uploaded, blobs_downloaded
        ),
    )?;
    store.save()?;

    Ok(RelaySyncResult {
        metadata_merged,
        blobs_uploaded,
        blobs_downloaded,
    })
}

/// Save relay config to a JSON file in sources_dir.
pub fn save_relay_config(sources_dir: &Path, config: &RelayConfig) -> Result<(), String> {
    let path = sources_dir.join("relay-config.json");
    let json = crate::to_sorted_json_pretty(config)
        .map_err(|e| format!("Serialize config failed: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Write config failed: {}", e))?;
    Ok(())
}

/// Load relay config from sources_dir. Returns None if file doesn't exist.
pub fn load_relay_config(sources_dir: &Path) -> Result<Option<RelayConfig>, String> {
    let path = sources_dir.join("relay-config.json");
    if !path.exists() {
        return Ok(None);
    }
    let json = std::fs::read_to_string(&path).map_err(|e| format!("Read config failed: {}", e))?;
    let config: RelayConfig =
        serde_json::from_str(&json).map_err(|e| format!("Parse config failed: {}", e))?;
    Ok(Some(config))
}

use std::io::Read;
