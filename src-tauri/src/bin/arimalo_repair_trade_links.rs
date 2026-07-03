#![deny(warnings)]
//! One-off migration: backfill missing _rules.json entries for existing
//! trade links in the metadata store. The bulk of the logic lives in
//! `arimalo_covid::trade_link_repair` so it can be unit-tested.
//!
//! Usage:
//!   arimalo-repair-trade-links            # dry-run; reports what would change
//!   arimalo-repair-trade-links --write    # apply the changes

use std::path::PathBuf;

use arimalo_covid::automerge_store::MetadataStore;
use arimalo_covid::root_config;
use arimalo_covid::trade_link_repair::{load_per_folder_ledgers, repair_links};

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

fn resolve_dirs() -> (PathBuf, PathBuf) {
    let sources_override = std::env::var("ARIMALO_SOURCES_DIR").ok();
    let generated_override = std::env::var("ARIMALO_GENERATED_DIR").ok();
    let app_dir = platform_app_data_dir();
    let config = root_config::load_root_config(&app_dir);
    (
        root_config::resolve_sources(sources_override.as_deref(), &config, &app_dir),
        root_config::resolve_generated(generated_override.as_deref(), &config, &app_dir),
    )
}

fn main() -> Result<(), String> {
    let write = std::env::args().any(|a| a == "--write");
    let (sources_dir, generated_dir) = resolve_dirs();
    println!("sources:   {}", sources_dir.display());
    println!("generated: {}", generated_dir.display());
    println!("mode:      {}", if write { "WRITE" } else { "dry-run" });
    println!();

    let metadata_path = sources_dir.join("arimalo-metadata.automerge");
    if !metadata_path.exists() {
        return Err(format!(
            "metadata store not found at {}",
            metadata_path.display()
        ));
    }
    let store = MetadataStore::new(metadata_path)?;
    let links = store.get_trade_links()?;
    println!("loaded {} trade link(s) from metadata store", links.len());

    let by_folder = load_per_folder_ledgers(&generated_dir, &sources_dir);
    println!("scanned {} folder ledger(s)\n", by_folder.len());

    let mut log = Vec::new();
    let report = repair_links(&links, &by_folder, &sources_dir, write, &mut log);
    for line in &log {
        println!("{line}");
    }

    println!();
    println!("== summary ==");
    println!("  links total:        {}", report.links_total);
    println!("  links already ok:   {}", report.links_already_ok);
    println!("  links unresolved:   {}", report.links_unresolved);
    println!("  rules to add:       {}", report.rules_added);
    println!("  folders to change:  {}", report.folders_changed.len());
    if !write && report.rules_added > 0 {
        println!();
        println!("(dry-run — re-run with --write to apply)");
    }
    Ok(())
}
