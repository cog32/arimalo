//! Apply a [`FolderPlan`] to the filesystem.
//!
//! Performs three actions per plan: rewrite stripped files, move archive-bound
//! files into a timestamped archive folder, and accumulate report entries that
//! the binary writes out as `dedupe-report.json` at the end of a run.

use super::plan::{DupeKind, FolderPlan, StripLocations};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct ReportEntry {
    pub path: String,
    pub kind: String,
    pub action: String,
    pub keys_dropped: usize,
    pub dropped_keys: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct Report {
    pub entries: Vec<ReportEntry>,
}

impl Report {
    pub fn total_keys_dropped(&self, kind: DupeKind) -> usize {
        self.entries
            .iter()
            .filter(|e| e.kind == kind.as_str())
            .map(|e| e.keys_dropped)
            .sum()
    }
}

/// Apply the OFX byte-range strip and CSV row strip rewrites in `plan` to disk.
///
/// `sources_root` is the root of the sources tree; archive paths are stored
/// relative to it. `archive_base` is the `<.dedupe-archive>/<timestamp>` folder
/// that has already been created (or will be lazily created on first archive).
pub fn apply(
    plan: &FolderPlan,
    kind: DupeKind,
    sources_root: &Path,
    archive_base: &Path,
    report: &mut Report,
) -> Result<(), String> {
    for strip in &plan.strips {
        let archived = plan.archives.iter().any(|a| a == &strip.path);
        if archived {
            archive_file(&strip.path, sources_root, archive_base)?;
            report.entries.push(ReportEntry {
                path: rel_string(&strip.path, sources_root),
                kind: kind.as_str().to_string(),
                action: "archived".to_string(),
                keys_dropped: strip.dropped_keys.len(),
                dropped_keys: strip.dropped_keys.clone(),
            });
            continue;
        }

        let original = fs::read_to_string(&strip.path)
            .map_err(|e| format!("read {}: {e}", strip.path.display()))?;
        let new_content = match &strip.locations {
            StripLocations::Byte(ranges) => strip_byte_ranges(&original, ranges),
            StripLocations::Row(rows) => strip_rows(&original, rows),
        };
        fs::write(&strip.path, &new_content)
            .map_err(|e| format!("write {}: {e}", strip.path.display()))?;
        report.entries.push(ReportEntry {
            path: rel_string(&strip.path, sources_root),
            kind: kind.as_str().to_string(),
            action: "stripped".to_string(),
            keys_dropped: strip.dropped_keys.len(),
            dropped_keys: strip.dropped_keys.clone(),
        });
    }
    Ok(())
}

/// Remove byte ranges from `content`. Ranges must be sorted ascending and
/// non-overlapping (the planner guarantees this).
fn strip_byte_ranges(content: &str, ranges: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(content.len());
    let mut cursor = 0;
    for &(s, e) in ranges {
        if s > cursor {
            out.push_str(&content[cursor..s]);
        }
        cursor = e;
    }
    if cursor < content.len() {
        out.push_str(&content[cursor..]);
    }
    out
}

/// Remove data rows from CSV content. `rows` are zero-based indices into the
/// data section (header row is index "before 0" and always kept).
fn strip_rows(content: &str, rows_to_drop: &[usize]) -> String {
    let mut out = String::with_capacity(content.len());
    let mut lines = content.split_inclusive('\n');
    if let Some(header) = lines.next() {
        out.push_str(header);
    }
    let drop_set: std::collections::HashSet<usize> = rows_to_drop.iter().copied().collect();
    for (i, line) in lines.enumerate() {
        if !drop_set.contains(&i) {
            out.push_str(line);
        }
    }
    out
}

fn archive_file(path: &Path, sources_root: &Path, archive_base: &Path) -> Result<(), String> {
    let rel = path
        .strip_prefix(sources_root)
        .map_err(|e| format!("{} not under sources root: {e}", path.display()))?;
    let dest = archive_base.join(rel);
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    fs::rename(path, &dest)
        .map_err(|e| format!("move {} → {}: {e}", path.display(), dest.display()))?;
    Ok(())
}

fn rel_string(path: &Path, sources_root: &Path) -> String {
    path.strip_prefix(sources_root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

/// ISO-8601 UTC timestamp suitable for an archive folder name, e.g.
/// `2026-05-28T12-34-56Z`. Colons are replaced with hyphens so the path is
/// portable to Windows.
pub fn archive_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H-%M-%SZ").to_string()
}

pub fn write_report(report: &Report, archive_base: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(archive_base)
        .map_err(|e| format!("mkdir {}: {e}", archive_base.display()))?;
    let dest = archive_base.join("dedupe-report.json");
    let json =
        crate::to_sorted_json_pretty(report).map_err(|e| format!("serialize report: {e}"))?;
    fs::write(&dest, json).map_err(|e| format!("write {}: {e}", dest.display()))?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_byte_ranges_removes_only_specified_slices() {
        let s = "AAAA<BLOCK1>BBBB</BLOCK1>CCCC<BLOCK2>DDDD</BLOCK2>EEEE";
        let r1 = (
            s.find("<BLOCK1>").unwrap(),
            s.find("</BLOCK1>").unwrap() + "</BLOCK1>".len(),
        );
        let r2 = (
            s.find("<BLOCK2>").unwrap(),
            s.find("</BLOCK2>").unwrap() + "</BLOCK2>".len(),
        );
        assert_eq!(strip_byte_ranges(s, &[r1, r2]), "AAAACCCCEEEE");
    }

    #[test]
    fn strip_rows_keeps_header_and_skips_indices() {
        let csv = "H1,H2\nrow0\nrow1\nrow2\n";
        let out = strip_rows(csv, &[0, 2]);
        assert_eq!(out, "H1,H2\nrow1\n");
    }
}
