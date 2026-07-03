use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn ledger_path(base_dir: &Path) -> PathBuf {
    base_dir.join("ledger.transactions")
}

fn archive_dir(base_dir: &Path) -> PathBuf {
    base_dir.join("archive")
}

fn archive_path(base_dir: &Path, yyyymm: &str) -> PathBuf {
    archive_dir(base_dir).join(format!("ledger-{yyyymm}.transactions"))
}

fn parse_yyyymm_from_contents(contents: &str) -> Option<String> {
    for raw_line in contents.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        // Expected header starts with YYYY-MM-DD...
        if line.len() >= 7 {
            let bytes = line.as_bytes();
            let is_date = bytes.first().is_some_and(|c| c.is_ascii_digit())
                && bytes.get(1).is_some_and(|c| c.is_ascii_digit())
                && bytes.get(2).is_some_and(|c| c.is_ascii_digit())
                && bytes.get(3).is_some_and(|c| c.is_ascii_digit())
                && bytes.get(4) == Some(&b'-')
                && bytes.get(5).is_some_and(|c| c.is_ascii_digit())
                && bytes.get(6).is_some_and(|c| c.is_ascii_digit());
            if is_date {
                let year = &line[0..4];
                let month = &line[5..7];
                return Some(format!("{year}{month}"));
            }
        }
        break;
    }
    None
}

fn unique_archive_path(desired: PathBuf) -> PathBuf {
    if !desired.exists() {
        return desired;
    }

    let file_name = desired
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("ledger-archive.transactions")
        .to_string();
    let stem = file_name
        .strip_suffix(".transactions")
        .unwrap_or(&file_name);

    let mut i = 2;
    loop {
        let candidate = desired
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(format!("{stem}-{i}.transactions"));
        if !candidate.exists() {
            return candidate;
        }
        i += 1;
    }
}

/// Rotates `ledger.transactions` into `archive/ledger-YYYYMM.transactions` when the ledger's
/// transaction month differs from `now_yyyymm`.
///
/// - `base_dir` is the root folder containing `ledger.transactions`.
/// - `now_yyyymm` is the current month string, e.g. `"202601"`.
pub fn rotate_ledger_if_needed(base_dir: &Path, now_yyyymm: &str) -> io::Result<()> {
    fs::create_dir_all(base_dir)?;

    let ledger = ledger_path(base_dir);
    if !ledger.exists() {
        return Ok(());
    }

    let contents = fs::read_to_string(&ledger)?;
    if contents.trim().is_empty() {
        return Ok(());
    }

    let ledger_yyyymm =
        parse_yyyymm_from_contents(&contents).unwrap_or_else(|| "unknown".to_string());
    if ledger_yyyymm == now_yyyymm {
        return Ok(());
    }

    fs::create_dir_all(archive_dir(base_dir))?;
    let archive = unique_archive_path(archive_path(base_dir, &ledger_yyyymm));

    fs::rename(&ledger, &archive)?;
    fs::write(&ledger, "")?;
    Ok(())
}
