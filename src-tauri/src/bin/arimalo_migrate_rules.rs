#![deny(warnings)]
//! One-off migrations applied to every per-folder `_rules.json` under the
//! configured sources root:
//!
//! 1. Rewrite legacy `*txn:HASH*` patterns to bare `txn:HASH` form, and
//!    stable-partition the file so txn-anchored rules sit at the top.
//! 2. Dedupe `ai-*` rule ids: rewrite each id to its content-derived form
//!    (`ai_rule_id`) and drop true duplicates that collapse to the same id.
//!
//! Usage:
//!   arimalo-migrate-rules            # dry-run; reports what would change
//!   arimalo-migrate-rules --write    # apply the changes

use std::path::{Path, PathBuf};

use arimalo_covid::root_config;
use arimalo_covid::rules::RulesFile;

fn platform_app_data_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join("Library/Application Support/com.cog32.arimalocovid");
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(data) = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| format!("{h}/.local/share"))
        }) {
            return PathBuf::from(data).join("com.cog32.arimalocovid");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("com.cog32.arimalocovid");
        }
    }
    PathBuf::from(".")
}

fn resolve_sources_dir() -> PathBuf {
    let env_override = std::env::var("ARIMALO_SOURCES_DIR").ok();
    let app_dir = platform_app_data_dir();
    let config = root_config::load_root_config(&app_dir);
    root_config::resolve_sources(env_override.as_deref(), &config, &app_dir)
}

/// Walk `dir` recursively, calling `cb` for every directory that
/// contains a `_rules.json`.
fn walk_rules_folders(dir: &Path, cb: &mut dyn FnMut(&Path)) {
    if !dir.is_dir() {
        return;
    }
    if dir.join("_rules.json").is_file() {
        cb(dir);
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(it) => it,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            walk_rules_folders(&p, cb);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let write = args.iter().any(|a| a == "--write");
    let help = args.iter().any(|a| a == "--help" || a == "-h");
    if help {
        println!("Usage: arimalo-migrate-rules [--write]");
        println!("  Default mode is dry-run; pass --write to apply.");
        return;
    }

    let sources = resolve_sources_dir();
    if !sources.is_dir() {
        eprintln!("sources dir not found: {}", sources.display());
        std::process::exit(1);
    }
    eprintln!("sources: {}", sources.display());
    eprintln!("mode:    {}", if write { "WRITE" } else { "dry-run" });
    eprintln!();

    let mut total_files = 0usize;
    let mut changed_files = 0usize;
    let mut total_canonicalized = 0usize;
    let mut total_promoted = 0usize;
    let mut total_renamed = 0usize;
    let mut total_dropped = 0usize;

    walk_rules_folders(&sources, &mut |folder| {
        total_files += 1;
        let mut rules = RulesFile::load(folder);
        let anchor = rules.migrate_legacy_anchored();
        let dedupe = rules.dedupe_ai_ids();
        if !anchor.changed() && !dedupe.changed() {
            return;
        }
        let rel = folder
            .strip_prefix(&sources)
            .unwrap_or(folder)
            .display();
        println!(
            "{rel}: {} pattern(s) canonicalized, {} rule(s) promoted, {} ai-id(s) renamed, {} duplicate(s) dropped",
            anchor.patterns_canonicalized,
            anchor.rules_promoted,
            dedupe.rules_renamed,
            dedupe.duplicates_removed,
        );
        changed_files += 1;
        total_canonicalized += anchor.patterns_canonicalized;
        total_promoted += anchor.rules_promoted;
        total_renamed += dedupe.rules_renamed;
        total_dropped += dedupe.duplicates_removed;
        if write {
            if let Err(e) = rules.save(folder) {
                eprintln!("  FAILED to save {}: {e}", folder.display());
            }
        }
    });

    eprintln!();
    eprintln!(
        "{} of {} folder(s) changed; {} pattern(s) canonicalized, {} rule(s) promoted, {} ai-id(s) renamed, {} duplicate(s) dropped",
        changed_files, total_files, total_canonicalized, total_promoted, total_renamed, total_dropped
    );
    if !write && changed_files > 0 {
        eprintln!("Re-run with --write to apply.");
    }
}
