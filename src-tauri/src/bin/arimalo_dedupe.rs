#![deny(warnings)]

//! arimalo-dedupe — remove duplicate transactions caused by overlapping
//! bank-statement exports. See `src-tauri/src/dedupe/mod.rs` for the strategy.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use arimalo_covid::dedupe::apply::{apply, archive_timestamp, write_report, Report};
use arimalo_covid::dedupe::csv as dcsv;
use arimalo_covid::dedupe::ofx as dofx;
use arimalo_covid::dedupe::plan::{DupeKind, FolderPlan, StripLocations};
use arimalo_covid::root_config;

fn print_usage() {
    eprintln!("Usage: arimalo-dedupe [OPTIONS]");
    eprintln!();
    eprintln!("Detect and remove duplicate transactions caused by overlapping");
    eprintln!("bank-statement exports. Within each folder, OFX files are deduped");
    eprintln!("by FITID and CSV files by whitespace-normalized row content. The");
    eprintln!("file with the widest date span keeps each record; narrower files");
    eprintln!("are rewritten with duplicates stripped. Dry-run by default.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --folder PATH      Scope to a subtree (relative to sources/ or absolute)");
    eprintln!("  --kinds ofx,csv    File types to dedupe (default: both)");
    eprintln!("  --apply            Rewrite files in place (default is dry-run)");
    eprintln!("  --verbose          Print per-key/per-row drop detail");
    eprintln!("  --sources-dir PATH Override sources directory");
    eprintln!("  --help             Show this help");
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

fn resolve_sources_dir(override_path: Option<&Path>) -> PathBuf {
    if let Some(p) = override_path {
        return p.to_path_buf();
    }
    let env_override = std::env::var("ARIMALO_SOURCES_DIR").ok();
    let app_dir = platform_app_data_dir();
    let config = root_config::load_root_config(&app_dir);
    root_config::resolve_sources(env_override.as_deref(), &config, &app_dir)
}

struct Args {
    folder: Option<PathBuf>,
    kinds: Vec<DupeKind>,
    apply: bool,
    verbose: bool,
    sources_dir: Option<PathBuf>,
}

fn parse_args() -> Result<Args, i32> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut out = Args {
        folder: None,
        kinds: vec![DupeKind::Ofx, DupeKind::Csv],
        apply: false,
        verbose: false,
        sources_dir: None,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_usage();
                return Err(0);
            }
            "--folder" => {
                i += 1;
                out.folder = Some(PathBuf::from(args.get(i).ok_or_else(|| {
                    eprintln!("--folder requires a value");
                    2
                })?));
            }
            "--kinds" => {
                i += 1;
                let raw = args.get(i).ok_or_else(|| {
                    eprintln!("--kinds requires a value");
                    2_i32
                })?;
                out.kinds = parse_kinds(raw).map_err(|e| {
                    eprintln!("{e}");
                    2_i32
                })?;
            }
            "--apply" => out.apply = true,
            "--verbose" | "-v" => out.verbose = true,
            "--sources-dir" => {
                i += 1;
                out.sources_dir = Some(PathBuf::from(args.get(i).ok_or_else(|| {
                    eprintln!("--sources-dir requires a value");
                    2_i32
                })?));
            }
            other => {
                eprintln!("unknown argument: {other}");
                print_usage();
                return Err(2);
            }
        }
        i += 1;
    }
    Ok(out)
}

fn parse_kinds(raw: &str) -> Result<Vec<DupeKind>, String> {
    let mut out = Vec::new();
    for part in raw.split(',') {
        match part.trim() {
            "ofx" => out.push(DupeKind::Ofx),
            "csv" => out.push(DupeKind::Csv),
            other => {
                return Err(format!(
                    "--kinds: unknown value '{other}' (expected ofx or csv)"
                ))
            }
        }
    }
    if out.is_empty() {
        return Err("--kinds: at least one of ofx,csv required".into());
    }
    Ok(out)
}

fn main() {
    match run() {
        Ok(()) => {}
        Err(code) => std::process::exit(code),
    }
}

fn run() -> Result<(), i32> {
    let args = parse_args()?;
    let sources_root = resolve_sources_dir(args.sources_dir.as_deref());
    let scan_root = match args.folder {
        Some(ref f) if f.is_absolute() => f.clone(),
        Some(ref f) => sources_root.join(f),
        None => sources_root.clone(),
    };
    if !scan_root.exists() {
        eprintln!("scan root does not exist: {}", scan_root.display());
        return Err(2);
    }

    let folders = group_files_by_folder(&scan_root, &args.kinds).map_err(|e| {
        eprintln!("{e}");
        1_i32
    })?;

    // Archive lives *outside* sources/ so the pipeline file watcher doesn't
    // re-ingest the archived OFX/CSV files on the next regenerate. Sibling of
    // sources/ by default; falls back to in-tree only if sources/ is the
    // filesystem root (effectively never).
    let archive_base = sources_root
        .parent()
        .map(|p| p.join("dedupe-archive"))
        .unwrap_or_else(|| sources_root.join(".dedupe-archive"))
        .join(archive_timestamp());
    let mut report = Report::default();
    let mut total_dropped = 0_usize;
    let mut touched_folders = 0_usize;

    for (folder, files) in &folders {
        for &kind in &args.kinds {
            let kind_files: Vec<&FileEntry> = files.iter().filter(|f| f.kind == kind).collect();
            if kind_files.len() < 2 {
                continue;
            }
            let plan = plan_for(kind, &kind_files).map_err(|e| {
                eprintln!("{e}");
                1_i32
            })?;
            if plan.is_empty() {
                continue;
            }
            touched_folders += 1;
            print_plan(folder, kind, &plan, args.verbose);
            for s in &plan.strips {
                total_dropped += s.dropped_keys.len();
            }
            if args.apply {
                apply(&plan, kind, &sources_root, &archive_base, &mut report).map_err(|e| {
                    eprintln!("{e}");
                    1_i32
                })?;
            }
        }
    }

    println!();
    if total_dropped == 0 {
        println!("No duplicates found.");
        return Ok(());
    }
    println!(
        "{}: {total_dropped} duplicate record(s) across {touched_folders} folder(s).",
        if args.apply { "Dropped" } else { "Would drop" }
    );
    if args.apply {
        let dest = write_report(&report, &archive_base).map_err(|e| {
            eprintln!("{e}");
            1_i32
        })?;
        println!("Report: {}", dest.display());
    } else {
        println!("Run with --apply to rewrite files.");
    }
    Ok(())
}

struct FileEntry {
    path: PathBuf,
    kind: DupeKind,
    content: String,
}

fn group_files_by_folder(
    root: &Path,
    kinds: &[DupeKind],
) -> Result<BTreeMap<PathBuf, Vec<FileEntry>>, String> {
    let want_ofx = kinds.contains(&DupeKind::Ofx);
    let want_csv = kinds.contains(&DupeKind::Csv);
    let mut out: BTreeMap<PathBuf, Vec<FileEntry>> = BTreeMap::new();
    walk(root, &mut |path: &Path| -> Result<(), String> {
        if !path.is_file() {
            return Ok(());
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase());
        let kind = match ext.as_deref() {
            Some("ofx") if want_ofx => DupeKind::Ofx,
            Some("csv") if want_csv => DupeKind::Csv,
            _ => return Ok(()),
        };
        let content =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let folder = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        out.entry(folder).or_default().push(FileEntry {
            path: path.to_path_buf(),
            kind,
            content,
        });
        Ok(())
    })?;
    Ok(out)
}

fn walk(root: &Path, f: &mut dyn FnMut(&Path) -> Result<(), String>) -> Result<(), String> {
    if root.is_file() {
        return f(root);
    }
    let entries =
        std::fs::read_dir(root).map_err(|e| format!("read_dir {}: {e}", root.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("dir entry under {}: {e}", root.display()))?;
        let path = entry.path();
        let name = entry.file_name();
        // Skip the archive folder so successive --apply runs are idempotent.
        if name == ".dedupe-archive" {
            continue;
        }
        if path.is_dir() {
            walk(&path, f)?;
        } else {
            f(&path)?;
        }
    }
    Ok(())
}

fn plan_for(kind: DupeKind, files: &[&FileEntry]) -> Result<FolderPlan, String> {
    match kind {
        DupeKind::Ofx => {
            let inputs: Vec<dofx::OfxInput> = files
                .iter()
                .map(|f| dofx::OfxInput {
                    path: f.path.clone(),
                    content: &f.content,
                })
                .collect();
            dofx::plan_folder(&inputs)
        }
        DupeKind::Csv => {
            let inputs: Vec<dcsv::CsvInput> = files
                .iter()
                .map(|f| dcsv::CsvInput {
                    path: f.path.clone(),
                    content: &f.content,
                })
                .collect();
            dcsv::plan_folder(&inputs)
        }
    }
}

fn print_plan(folder: &Path, kind: DupeKind, plan: &FolderPlan, verbose: bool) {
    println!();
    println!("{}/  ({})", folder.display(), kind.as_str());
    if let Some(c) = &plan.canonical {
        println!("  canonical: {}", file_name(c));
    }
    for s in &plan.strips {
        let archived = plan.archives.iter().any(|a| a == &s.path);
        let action = if archived { "archive" } else { "strip" };
        let unit = match &s.locations {
            StripLocations::Byte(_) => "STMTTRN block",
            StripLocations::Row(_) => "row",
        };
        let plural = if s.dropped_keys.len() == 1 { "" } else { "s" };
        println!(
            "  {} {}: {} duplicate {unit}{plural}",
            action,
            file_name(&s.path),
            s.dropped_keys.len()
        );
        if verbose {
            for k in &s.dropped_keys {
                println!("      - {}", k);
            }
        }
    }
}

fn file_name(p: &Path) -> String {
    p.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}
