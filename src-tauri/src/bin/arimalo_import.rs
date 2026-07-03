#![deny(warnings)]

use std::path::PathBuf;

use arimalo_covid::processing_pipeline::{process_all_imports, process_imports, PipelineConfig};
use arimalo_covid::FALLBACK_EXPENSE_ACCOUNT;

fn now_yyyymm() -> String {
    let now = chrono_free_now();
    format!("{}{:02}", now.0, now.1)
}

/// Get (year, month) without pulling in chrono.
fn chrono_free_now() -> (i32, u32) {
    // Use UNIX timestamp arithmetic
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let days = secs / 86400;
    // Approximate: good enough for year/month
    let year = 1970 + (days * 400 / 146097) as i32;
    let remaining = days
        - ((year - 1970) as i64 * 365 + ((year - 1970) as i64 / 4) - ((year - 1970) as i64 / 100)
            + ((year - 1970) as i64 / 400));
    let month = (remaining / 30).clamp(0, 11) as u32 + 1;
    (year, month)
}

fn resolve_sources_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("ARIMALO_SOURCES_DIR") {
        return PathBuf::from(custom);
    }

    // Platform default (matches Tauri app data dir convention)
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join("Library/Application Support/com.arimalo.app/sources");
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(data) = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| format!("{h}/.local/share"))
        }) {
            return PathBuf::from(data).join("com.arimalo.app/sources");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("com.arimalo.app/sources");
        }
    }

    PathBuf::from("sources")
}

fn resolve_generated_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("ARIMALO_GENERATED_DIR") {
        return PathBuf::from(custom);
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home)
                .join("Library/Application Support/com.arimalo.app/generated");
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(data) = std::env::var("XDG_DATA_HOME").ok().or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| format!("{h}/.local/share"))
        }) {
            return PathBuf::from(data).join("com.arimalo.app/generated");
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("com.arimalo.app/generated");
        }
    }

    PathBuf::from("generated")
}

fn print_usage() {
    eprintln!("Usage: arimalo-import [OPTIONS] [FILE...]");
    eprintln!();
    eprintln!("Process pending CSV imports for Arimalo accounting.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --sources-dir PATH   Override sources directory");
    eprintln!("  --generated-dir PATH Override generated directory");
    eprintln!("  --account FOLDER     Process imports for a single account folder");
    eprintln!("  --now YYYYMM         Override current month (default: auto-detected)");
    eprintln!("  --help               Show this help");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  arimalo-import                              # Process all pending imports");
    eprintln!("  arimalo-import --account richard-savings     # Process one account");
    eprintln!("  arimalo-import --account richard-savings f.csv  # Copy file then process");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut sources_dir: Option<PathBuf> = None;
    let mut generated_dir: Option<PathBuf> = None;
    let mut account: Option<String> = None;
    let mut now: Option<String> = None;
    let mut files: Vec<PathBuf> = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--sources-dir" => {
                i += 1;
                sources_dir = Some(PathBuf::from(args.get(i).unwrap_or_else(|| {
                    eprintln!("--sources-dir requires a value");
                    std::process::exit(2);
                })));
            }
            "--generated-dir" => {
                i += 1;
                generated_dir = Some(PathBuf::from(args.get(i).unwrap_or_else(|| {
                    eprintln!("--generated-dir requires a value");
                    std::process::exit(2);
                })));
            }
            "--account" => {
                i += 1;
                account = Some(
                    args.get(i)
                        .unwrap_or_else(|| {
                            eprintln!("--account requires a value");
                            std::process::exit(2);
                        })
                        .clone(),
                );
            }
            "--now" => {
                i += 1;
                now = Some(
                    args.get(i)
                        .unwrap_or_else(|| {
                            eprintln!("--now requires a value");
                            std::process::exit(2);
                        })
                        .clone(),
                );
            }
            arg if arg.starts_with('-') => {
                eprintln!("Unknown option: {arg}");
                print_usage();
                std::process::exit(2);
            }
            _ => {
                files.push(PathBuf::from(&args[i]));
            }
        }
        i += 1;
    }

    let sources_dir = sources_dir.unwrap_or_else(resolve_sources_dir);
    let generated_dir = generated_dir.unwrap_or_else(resolve_generated_dir);
    let now_yyyymm = now.unwrap_or_else(now_yyyymm);

    let config = PipelineConfig {
        sources_dir: sources_dir.clone(),
        generated_dir,
        now_yyyymm,
        force: false,
        default_expense_account: FALLBACK_EXPENSE_ACCOUNT.to_string(),
        changed_folder_hint: None,
    };

    // If files are provided, copy them into the account's imports/ dir first
    if !files.is_empty() {
        let account_folder = account.as_deref().unwrap_or_else(|| {
            eprintln!("--account is required when specifying files");
            std::process::exit(2);
        });

        let imports_dir = sources_dir.join(account_folder).join("imports");
        if let Err(e) = std::fs::create_dir_all(&imports_dir) {
            eprintln!("Failed to create imports dir: {e}");
            std::process::exit(1);
        }

        for file in &files {
            if !file.exists() {
                eprintln!("File not found: {}", file.display());
                std::process::exit(1);
            }
            let filename = file.file_name().unwrap_or_default();
            let dest = imports_dir.join(filename);
            if let Err(e) = std::fs::copy(file, &dest) {
                eprintln!("Failed to copy {} to imports: {e}", file.display());
                std::process::exit(1);
            }
            eprintln!("Copied {} -> {}", file.display(), dest.display());
        }
    }

    // Process imports
    let result = if let Some(ref folder) = account {
        process_imports(&config, folder)
    } else {
        process_all_imports(&config)
    };

    match result {
        Ok((import_result, pipeline_result)) => {
            if import_result.files_processed.is_empty() {
                eprintln!("No pending imports found.");
            } else {
                eprintln!("Processed {} file(s):", import_result.files_processed.len());
                for f in &import_result.files_processed {
                    eprintln!("  {f}");
                }
            }
            for w in &import_result.warnings {
                eprintln!("Warning: {w}");
            }
            if !import_result.files_skipped.is_empty() {
                eprintln!("Skipped: {}", import_result.files_skipped.join(", "));
            }
            eprintln!(
                "Pipeline: {} CSV transformed, {} CSV cached, {} OFX transformed, {} OFX cached, {} manual, {} total written",
                pipeline_result.csv_transformed,
                pipeline_result.csv_cached,
                pipeline_result.ofx_transformed,
                pipeline_result.ofx_cached,
                pipeline_result.manual_count,
                pipeline_result.total_written,
            );
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
