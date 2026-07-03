#![deny(warnings)]

use std::path::PathBuf;

use arimalo_covid::processing_pipeline::{run_pipeline, PipelineConfig};
use arimalo_covid::root_config;

fn now_yyyymm() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let days = secs / 86400;
    let year = 1970 + (days * 400 / 146097) as i32;
    let remaining = days
        - ((year - 1970) as i64 * 365 + ((year - 1970) as i64 / 4) - ((year - 1970) as i64 / 100)
            + ((year - 1970) as i64 / 400));
    let month = (remaining / 30).clamp(0, 11) as u32 + 1;
    format!("{year}{month:02}")
}

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

fn resolve_generated_dir() -> PathBuf {
    let env_override = std::env::var("ARIMALO_GENERATED_DIR").ok();
    let app_dir = platform_app_data_dir();
    let config = root_config::load_root_config(&app_dir);
    root_config::resolve_generated(env_override.as_deref(), &config, &app_dir)
}

fn print_usage() {
    eprintln!("Usage: arimalo-regenerate [OPTIONS]");
    eprintln!();
    eprintln!("Regenerate the ledger from sources (incremental via build cache).");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --sources-dir PATH   Override sources directory");
    eprintln!("  --generated-dir PATH Override generated directory");
    eprintln!("  --now YYYYMM         Override current month (default: auto-detected)");
    eprintln!("  --help               Show this help");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut sources_dir: Option<PathBuf> = None;
    let mut generated_dir: Option<PathBuf> = None;
    let mut now: Option<String> = None;

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
                eprintln!("Unexpected argument: {}", args[i]);
                print_usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    let app_data_dir = platform_app_data_dir();
    let root_cfg = root_config::load_root_config(&app_data_dir);
    let config = PipelineConfig {
        sources_dir: sources_dir.unwrap_or_else(resolve_sources_dir),
        generated_dir: generated_dir.unwrap_or_else(resolve_generated_dir),
        now_yyyymm: now.unwrap_or_else(now_yyyymm),
        force: false,
        default_expense_account: root_cfg.default_expense_account.clone(),
        changed_folder_hint: None,
    };

    match run_pipeline(&config) {
        Ok(result) => {
            eprintln!(
                "Pipeline: {} transformed, {} cached, {} manual, {} total written",
                result.csv_transformed,
                result.csv_cached,
                result.manual_count,
                result.total_written,
            );
            if result.output_files_written > 0 {
                let mut account_sets: Vec<String> = result.owner_accounts.keys().cloned().collect();
                account_sets.sort();
                if let Err(e) = arimalo_covid::report_templates::generate_all_reports(
                    &config.sources_dir,
                    &config.generated_dir,
                    &account_sets,
                    arimalo_covid::report_templates::ALL_FORMATS,
                    &root_cfg.extra_primary_account_prefixes,
                ) {
                    eprintln!("Report generation error: {e}");
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}
