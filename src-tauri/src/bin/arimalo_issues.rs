#![deny(warnings)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use arimalo_covid::issues::{self, Category, CollectFilter, IssueGroup, IssueSeverity};
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

fn resolve_dirs() -> (PathBuf, PathBuf) {
    let env_sources = std::env::var("ARIMALO_SOURCES_DIR").ok();
    let env_generated = std::env::var("ARIMALO_GENERATED_DIR").ok();
    let app_dir = platform_app_data_dir();
    let config = root_config::load_root_config(&app_dir);
    let sources = root_config::resolve_sources(env_sources.as_deref(), &config, &app_dir);
    let generated = root_config::resolve_generated(env_generated.as_deref(), &config, &app_dir);
    (sources, generated)
}

fn print_usage() {
    eprintln!("Usage: arimalo-issues [CATEGORY...] [OPTIONS]");
    eprintln!();
    eprintln!("Diagnostic checks for the ledger.");
    eprintln!();
    eprintln!("Categories (any combination; default: --all):");
    eprintln!("  --all                List every category");
    eprintln!("  --parse-errors       Ledger parse diagnostics");
    eprintln!("  --uncategorised      Transactions lacking a second posting or using expenses:unknown");
    eprintln!("  --pipeline-warnings  Warnings emitted by the import/transform pipeline");
    eprintln!("  --gaps               Accounts with missing months");
    eprintln!("  --unverified         assets:* accounts lacking an opening balance");
    eprintln!("  --trade-suggestions  Auto-detected trade link candidates");
    eprintln!("  --unpriced           Commodities missing price data for conversion to base currency");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --account NAME        Restrict per-account categories to this account");
    eprintln!("  --format FORMAT       Output format: text | json (default: text)");
    eprintln!("  --sources-dir PATH    Override sources directory");
    eprintln!("  --generated-dir PATH  Override generated directory");
    eprintln!("  --help                Show this help");
}

#[derive(Debug)]
struct CliArgs {
    categories: BTreeSet<Category>,
    account: Option<String>,
    format: String,
    sources_dir: Option<PathBuf>,
    generated_dir: Option<PathBuf>,
}

fn parse_args(args: &[String]) -> Result<CliArgs, String> {
    let mut categories: BTreeSet<Category> = BTreeSet::new();
    let mut account: Option<String> = None;
    let mut format = "text".to_string();
    let mut sources_dir: Option<PathBuf> = None;
    let mut generated_dir: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        let needs_value = |name: &str| -> Result<String, String> {
            args.get(i + 1)
                .cloned()
                .ok_or_else(|| format!("{name} requires a value"))
        };

        match arg {
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            "--all" => categories.extend(Category::ALL.iter().copied()),
            "--parse-errors" => {
                categories.insert(Category::ParseErrors);
            }
            "--uncategorised" => {
                categories.insert(Category::Uncategorised);
            }
            "--pipeline-warnings" => {
                categories.insert(Category::PipelineWarnings);
            }
            "--gaps" => {
                categories.insert(Category::AccountGaps);
            }
            "--unverified" => {
                categories.insert(Category::UnverifiedBalances);
            }
            "--trade-suggestions" => {
                categories.insert(Category::TradeSuggestions);
            }
            "--unpriced" => {
                categories.insert(Category::Unpriced);
            }
            "--account" => {
                account = Some(needs_value("--account")?);
                i += 1;
            }
            "--format" => {
                format = needs_value("--format")?;
                i += 1;
            }
            "--sources-dir" => {
                sources_dir = Some(PathBuf::from(needs_value("--sources-dir")?));
                i += 1;
            }
            "--generated-dir" => {
                generated_dir = Some(PathBuf::from(needs_value("--generated-dir")?));
                i += 1;
            }
            other => return Err(format!("Unknown argument: {other}")),
        }
        i += 1;
    }

    if categories.is_empty() {
        categories.extend(Category::ALL.iter().copied());
    }

    Ok(CliArgs {
        categories,
        account,
        format,
        sources_dir,
        generated_dir,
    })
}

fn severity_label(s: IssueSeverity) -> &'static str {
    match s {
        IssueSeverity::Error => "ERROR",
        IssueSeverity::Warning => "WARN",
        IssueSeverity::Info => "INFO",
    }
}

fn print_text(groups: &[IssueGroup]) {
    if groups.is_empty() {
        println!("No issues found.");
        return;
    }
    let mut total = 0usize;
    for group in groups {
        println!(
            "[{}] {} ({} issue{})",
            severity_label(group.severity),
            group.label,
            group.issues.len(),
            if group.issues.len() == 1 { "" } else { "s" }
        );
        for issue in &group.issues {
            println!("  - {}", issue.message);
            total += 1;
        }
    }
    println!();
    println!("{} groups, {} issues total", groups.len(), total);
}

fn main() {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let parsed = match parse_args(&raw) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            print_usage();
            std::process::exit(2);
        }
    };

    let (resolved_sources, resolved_generated) = resolve_dirs();
    let sources = parsed.sources_dir.unwrap_or(resolved_sources);
    let generated = parsed.generated_dir.unwrap_or(resolved_generated);

    let filter = CollectFilter {
        categories: parsed.categories,
        account: parsed.account,
    };

    let collected = match issues::collect_all(&sources, &generated, &filter) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error collecting issues: {e}");
            std::process::exit(1);
        }
    };
    let groups = collected.groups;

    match parsed.format.as_str() {
        "json" => match serde_json::to_string_pretty(&groups) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("JSON serialization failed: {e}");
                std::process::exit(1);
            }
        },
        "text" => print_text(&groups),
        other => {
            eprintln!("Unknown format: {other}. Use 'json' or 'text'.");
            std::process::exit(2);
        }
    }
}
