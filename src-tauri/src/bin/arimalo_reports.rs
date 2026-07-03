#![deny(warnings)]

use std::path::{Path, PathBuf};

use arimalo_covid::report_templates::{self, ReportFormat, ALL_FORMATS};
use arimalo_covid::root_config;

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

/// Scan `generated_dir` for top-level subdirs that look like account sets —
/// anything containing at least one `ledger.transactions` file at any depth.
/// An empty list means single-set mode (reports written directly under
/// `generated_dir`), matching `generate_all_reports`'s contract.
fn discover_account_sets(generated_dir: &Path) -> Vec<String> {
    let mut sets = Vec::new();
    let Ok(rd) = std::fs::read_dir(generated_dir) else {
        return sets;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let has_ledger = walkdir::WalkDir::new(&path)
            .into_iter()
            .filter_map(|e| e.ok())
            .any(|e| {
                e.file_type().is_file()
                    && e.file_name() == std::ffi::OsStr::new("ledger.transactions")
            });
        if has_ledger {
            sets.push(name.to_string());
        }
    }
    sets.sort();
    sets
}

fn print_usage() {
    eprintln!("Usage: arimalo-reports [OPTIONS]");
    eprintln!();
    eprintln!("Regenerate report artifacts for every account set in the vault.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --sources-dir PATH    Override sources directory");
    eprintln!("  --generated-dir PATH  Override generated directory");
    eprintln!("  --set NAME            Limit to one account set (repeatable)");
    eprintln!("  --format FMT          Output format: json | md | csv | all (default: all)");
    eprintln!("                        Repeatable; e.g. --format json --format csv");
    eprintln!("  --help                Show this help");
}

fn parse_format(arg: &str) -> Result<&'static [ReportFormat], String> {
    match arg {
        "json" => Ok(&[ReportFormat::Json]),
        "md" | "markdown" => Ok(&[ReportFormat::Md]),
        "csv" => Ok(&[ReportFormat::Csv]),
        "all" => Ok(ALL_FORMATS),
        other => Err(format!(
            "unknown --format value '{other}' (expected json, md, csv, or all)"
        )),
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    let mut sources_dir: Option<PathBuf> = None;
    let mut generated_dir: Option<PathBuf> = None;
    let mut explicit_sets: Vec<String> = Vec::new();
    let mut formats: Vec<ReportFormat> = Vec::new();

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
            "--set" => {
                i += 1;
                explicit_sets.push(
                    args.get(i)
                        .unwrap_or_else(|| {
                            eprintln!("--set requires a value");
                            std::process::exit(2);
                        })
                        .clone(),
                );
            }
            "--format" => {
                i += 1;
                let val = args.get(i).unwrap_or_else(|| {
                    eprintln!("--format requires a value");
                    std::process::exit(2);
                });
                match parse_format(val) {
                    Ok(fmts) => {
                        for f in fmts {
                            if !formats.contains(f) {
                                formats.push(*f);
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(2);
                    }
                }
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

    let sources = sources_dir.unwrap_or_else(resolve_sources_dir);
    let generated = generated_dir.unwrap_or_else(resolve_generated_dir);

    let sets = if !explicit_sets.is_empty() {
        explicit_sets
    } else {
        discover_account_sets(&generated)
    };

    let formats: &[ReportFormat] = if formats.is_empty() {
        ALL_FORMATS
    } else {
        formats.as_slice()
    };

    let root_cfg = root_config::load_root_config(&platform_app_data_dir());
    if let Err(e) = report_templates::generate_all_reports(
        &sources,
        &generated,
        &sets,
        formats,
        &root_cfg.extra_primary_account_prefixes,
    ) {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    if sets.is_empty() {
        eprintln!("Reports regenerated for single-set vault at {}", generated.display());
    } else {
        eprintln!("Reports regenerated for {} account set(s): {}", sets.len(), sets.join(", "));
    }
}
