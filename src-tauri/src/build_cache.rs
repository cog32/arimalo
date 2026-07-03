use crate::ledger_parser::Transaction;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedTransaction {
    pub text: String,
    /// Parsed Transaction struct, stored to avoid re-parsing on cache load.
    /// None for old cache entries (pre-migration); falls back to parse_transactions.
    #[serde(default)]
    pub txn: Option<Transaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub content_hash: String,
    pub transform_hash: String,
    pub transactions: Vec<CachedTransaction>,
}

// --- Per-folder cache file (one per source folder) ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FolderCacheFile {
    pub fingerprint: String,
    /// Per-CSV/OFX file entries within this folder (key: relative path from sources_dir)
    #[serde(default)]
    pub entries: HashMap<String, CacheEntry>,
    /// Manual transactions content hash for this folder
    #[serde(default)]
    pub manual_hash: Option<String>,
    /// Accounts file hash + cached text for this folder
    #[serde(default)]
    pub accounts_hash: Option<String>,
    #[serde(default)]
    pub accounts_text: Option<String>,
}

// --- Lightweight global manifest (always loaded) ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CacheManifest {
    pub inputs_hash: Option<String>,
    /// Quick lookup: folder relative path → fingerprint hash
    #[serde(default)]
    pub folder_fingerprints: HashMap<String, String>,
    /// Layer 4: SHA256 of each generated output file
    #[serde(default)]
    pub output_hashes: HashMap<String, String>,
    /// Root-level _ignored.txt
    #[serde(default)]
    pub ignored_hash: Option<String>,
    #[serde(default)]
    pub ignored_ids: Vec<String>,
    /// Root-level accounts.transactions (key "" for root)
    #[serde(default)]
    pub root_accounts_hash: Option<String>,
    #[serde(default)]
    pub root_accounts_text: Option<String>,
}

// --- In-memory working struct (hydrated from manifest + folder files) ---

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BuildCache {
    /// Per-file cache entries (Layer 3: content_hash + transform_hash → skip re-transform)
    pub entries: HashMap<String, CacheEntry>,
    #[serde(default)]
    pub inputs_hash: Option<String>,
    #[serde(default)]
    pub folder_hashes: HashMap<String, String>,
    #[serde(default)]
    pub output_hashes: HashMap<String, String>,
    #[serde(default)]
    pub manual_hashes: HashMap<String, String>,
    #[serde(default)]
    pub accounts_hashes: HashMap<String, String>,
    #[serde(default)]
    pub accounts_cached: HashMap<String, String>,
    #[serde(default)]
    pub ignored_hash: Option<String>,
    #[serde(default)]
    pub ignored_ids: Vec<String>,
}

// --- Path helpers ---

fn cache_dir(generated_dir: &Path) -> PathBuf {
    generated_dir.join(".cache")
}

fn manifest_path(generated_dir: &Path) -> PathBuf {
    cache_dir(generated_dir).join("manifest.json")
}

fn folder_cache_filename(folder_rel: &str) -> String {
    format!("{}.json", folder_rel.replace('/', "--"))
}

fn folder_cache_path(generated_dir: &Path, folder_rel: &str) -> PathBuf {
    cache_dir(generated_dir).join(folder_cache_filename(folder_rel))
}

fn legacy_cache_path(generated_dir: &Path) -> PathBuf {
    generated_dir.join("build-cache.json")
}

// --- Load ---

/// Load full cache (all folders). Used by file watcher and full rebuilds.
pub fn load_cache(generated_dir: &Path) -> BuildCache {
    load_cache_filtered(generated_dir, None)
}

/// Load cache with an optional folder filter.
/// When `only_folders` is Some, only those folders' cache files are loaded.
/// The manifest (fingerprints, output hashes, etc.) is always loaded in full.
pub fn load_cache_filtered(
    generated_dir: &Path,
    only_folders: Option<&HashSet<String>>,
) -> BuildCache {
    let manifest_p = manifest_path(generated_dir);
    if manifest_p.exists() {
        return load_from_folder_cache(generated_dir, only_folders);
    }
    // Fall back to legacy monolithic build-cache.json (migration path)
    let legacy_p = legacy_cache_path(generated_dir);
    if legacy_p.exists() {
        let contents = match fs::read_to_string(&legacy_p) {
            Ok(c) => c,
            Err(_) => return BuildCache::default(),
        };
        return serde_json::from_str(&contents).unwrap_or_default();
    }
    BuildCache::default()
}

fn load_from_folder_cache(
    generated_dir: &Path,
    only_folders: Option<&HashSet<String>>,
) -> BuildCache {
    let manifest: CacheManifest = match fs::read_to_string(manifest_path(generated_dir)) {
        Ok(c) => serde_json::from_str(&c).unwrap_or_default(),
        Err(_) => return BuildCache::default(),
    };

    let mut cache = BuildCache {
        inputs_hash: manifest.inputs_hash,
        folder_hashes: manifest.folder_fingerprints.clone(),
        output_hashes: manifest.output_hashes,
        ignored_hash: manifest.ignored_hash,
        ignored_ids: manifest.ignored_ids,
        ..Default::default()
    };

    // Root-level accounts
    if let Some(h) = manifest.root_accounts_hash {
        cache.accounts_hashes.insert(String::new(), h);
    }
    if let Some(t) = manifest.root_accounts_text {
        cache.accounts_cached.insert(String::new(), t);
    }

    // Determine which folder cache files to load.
    let mut folder_rels: Vec<String> = manifest.folder_fingerprints.keys().cloned().collect();
    // Also scan .cache/ dir for orphaned cache files (fingerprints may have been cleared)
    if only_folders.is_none() {
        if let Ok(dir_entries) = fs::read_dir(cache_dir(generated_dir)) {
            for entry in dir_entries.filter_map(|e| e.ok()) {
                let name = entry.file_name().to_string_lossy().to_string();
                if name == "manifest.json" {
                    continue;
                }
                if let Some(stem) = name
                    .strip_suffix(".json")
                    .or_else(|| name.strip_suffix(".bin"))
                {
                    let rel = stem.replace("--", "/");
                    if !folder_rels.contains(&rel) {
                        folder_rels.push(rel);
                    }
                }
            }
        }
    }

    // Load folder cache files (filtered if requested)
    for folder_rel in &folder_rels {
        if let Some(filter) = only_folders {
            if !filter.contains(folder_rel) {
                continue;
            }
        }
        hydrate_folder_cache(&mut cache, generated_dir, folder_rel);
    }

    cache
}

/// Load a single folder's cache file and merge into the BuildCache.
/// Can be called after initial load to lazily load additional folders.
pub fn hydrate_folder_cache(cache: &mut BuildCache, generated_dir: &Path, folder_rel: &str) {
    let path = folder_cache_path(generated_dir, folder_rel);
    let fc: FolderCacheFile = match fs::read_to_string(&path) {
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => return,
    };
    for (k, v) in fc.entries {
        cache.entries.insert(k, v);
    }
    if let Some(h) = fc.manual_hash {
        let manual_key = if folder_rel.is_empty() {
            FILE_MANUAL.to_string()
        } else {
            format!("{folder_rel}/{FILE_MANUAL}")
        };
        cache.manual_hashes.insert(manual_key, h);
    }
    if let Some(h) = fc.accounts_hash {
        cache.accounts_hashes.insert(folder_rel.to_string(), h);
    }
    if let Some(t) = fc.accounts_text {
        cache.accounts_cached.insert(folder_rel.to_string(), t);
    }
}

const FILE_MANUAL: &str = "manual.transactions";

// --- Save ---

/// Save cache to per-folder files + manifest.
/// Only writes folder cache files for folders in `changed_folders`.
/// Pass None to write all folders (e.g. first run or migration).
pub fn save_cache(
    generated_dir: &Path,
    cache: &BuildCache,
    changed_folders: Option<&HashSet<String>>,
) -> Result<(), String> {
    let dir = cache_dir(generated_dir);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    // Build manifest
    let manifest = CacheManifest {
        inputs_hash: cache.inputs_hash.clone(),
        folder_fingerprints: cache.folder_hashes.clone(),
        output_hashes: cache.output_hashes.clone(),
        ignored_hash: cache.ignored_hash.clone(),
        ignored_ids: cache.ignored_ids.clone(),
        root_accounts_hash: cache.accounts_hashes.get("").cloned(),
        root_accounts_text: cache.accounts_cached.get("").cloned(),
    };

    let manifest_json = crate::to_sorted_json_pretty(&manifest).map_err(|e| e.to_string())?;
    fs::write(manifest_path(generated_dir), manifest_json).map_err(|e| e.to_string())?;

    // Determine which folders to write
    let folders_to_write: HashSet<String> = match changed_folders {
        Some(changed) => cache
            .folder_hashes
            .keys()
            .filter(|k| changed.contains(*k))
            .cloned()
            .collect(),
        None => {
            // Write all folders: collect from all sources (entries, folder_hashes, etc.)
            let mut all: HashSet<String> = cache.folder_hashes.keys().cloned().collect();
            // Also collect folders from per-file entries
            for key in cache.entries.keys() {
                if let Some(pos) = key.rfind('/') {
                    all.insert(key[..pos].to_string());
                }
            }
            all
        }
    };

    for folder_rel in &folders_to_write {
        let fc = extract_folder_cache(cache, folder_rel);
        let json = serde_json::to_string(&fc).map_err(|e| e.to_string())?;
        fs::write(folder_cache_path(generated_dir, folder_rel), json).map_err(|e| e.to_string())?;
    }

    // Prune stale folder cache files (folders that no longer exist on disk).
    //
    // Critically, "stale" is "folder is gone" — NOT "folder wasn't changed
    // this run". With `changed_folders = Some({a,b,c})`, `folders_to_write`
    // only contains the changed folders. Pruning against that set deletes
    // every OTHER folder's cache file on every partial regen, dropping the
    // cache from ~24 files to a handful. Each subsequent regen then has to
    // re-transform those folders from scratch (cache miss → CSV re-runs).
    //
    // The "exists" set is the union of: (1) folder_hashes (every folder
    // we've ever fingerprinted), (2) parent folders of cache.entries keys
    // (every folder with a cached CSV/OFX). A cache file outside that set
    // is one we wrote in a prior run for a folder that has since been
    // deleted from sources/ — those are safe to remove.
    let known_folders: HashSet<String> = {
        let mut s: HashSet<String> = cache.folder_hashes.keys().cloned().collect();
        for key in cache.entries.keys() {
            if let Some(pos) = key.rfind('/') {
                s.insert(key[..pos].to_string());
            }
        }
        s
    };
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "manifest.json" {
                continue;
            }
            // Reverse the filename → folder_rel mapping
            if let Some(folder_rel) = name
                .strip_suffix(".bin")
                .or_else(|| name.strip_suffix(".json"))
            {
                let folder_rel = folder_rel.replace("--", "/");
                if !known_folders.contains(&folder_rel) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    }

    // Remove legacy cache if it exists (migration complete)
    let legacy = legacy_cache_path(generated_dir);
    if legacy.exists() {
        let _ = fs::remove_file(&legacy);
    }

    Ok(())
}

/// Extract a FolderCacheFile from the in-memory BuildCache for a given folder.
fn extract_folder_cache(cache: &BuildCache, folder_rel: &str) -> FolderCacheFile {
    // Collect entries belonging to this folder (entries whose path starts with folder_rel/)
    let prefix = if folder_rel.is_empty() {
        String::new()
    } else {
        format!("{folder_rel}/")
    };
    let entries: HashMap<String, CacheEntry> = cache
        .entries
        .iter()
        .filter(|(k, _)| {
            if folder_rel.is_empty() {
                !k.contains('/')
            } else {
                k.starts_with(&prefix) && !k[prefix.len()..].contains('/')
            }
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();

    let manual_key = if folder_rel.is_empty() {
        FILE_MANUAL.to_string()
    } else {
        format!("{folder_rel}/{FILE_MANUAL}")
    };

    FolderCacheFile {
        fingerprint: cache
            .folder_hashes
            .get(folder_rel)
            .cloned()
            .unwrap_or_default(),
        entries,
        manual_hash: cache.manual_hashes.get(&manual_key).cloned(),
        accounts_hash: cache.accounts_hashes.get(folder_rel).cloned(),
        accounts_text: cache.accounts_cached.get(folder_rel).cloned(),
    }
}

pub fn lookup<'a>(
    cache: &'a BuildCache,
    relative_path: &str,
    content_hash: &str,
    transform_hash: &str,
) -> Option<&'a [CachedTransaction]> {
    let entry = cache.entries.get(relative_path)?;
    if entry.content_hash == content_hash && entry.transform_hash == transform_hash {
        Some(&entry.transactions)
    } else {
        None
    }
}

pub fn file_hash(path: &Path) -> Result<String, String> {
    let contents = fs::read(path).map_err(|e| format!("failed to read {}: {e}", path.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&contents);
    let result = hasher.finalize();
    Ok(hex::encode(result))
}

pub fn string_hash(s: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(s.as_bytes());
    let result = hasher.finalize();
    hex::encode(result)
}

/// Git-style tree fingerprint from file metadata (no content reads).
/// Hash of sorted (relative_path, mtime_secs, size) tuples.
pub fn dir_fingerprint(dir: &Path) -> Result<String, String> {
    if !dir.exists() {
        return Ok(String::new());
    }
    let mut entries: Vec<(String, u64, u64)> = Vec::new();
    for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Skip hidden files (e.g. .DS_Store)
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
        }
        let relative = path
            .strip_prefix(dir)
            .map_err(|e| format!("strip_prefix failed: {e}"))?
            .to_string_lossy()
            .to_string();
        let meta = fs::metadata(path)
            .map_err(|e| format!("metadata failed for {}: {e}", path.display()))?;
        let mtime = meta
            .modified()
            .map_err(|e| format!("mtime failed for {}: {e}", path.display()))?
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let size = meta.len();
        entries.push((relative, mtime, size));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut hasher = Sha256::new();
    for (name, mtime, size) in &entries {
        hasher.update(name.as_bytes());
        hasher.update(b"\0");
        hasher.update(mtime.to_le_bytes());
        hasher.update(size.to_le_bytes());
    }
    Ok(hex::encode(hasher.finalize()))
}
