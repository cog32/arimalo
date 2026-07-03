//! CSV-file deduplication planner.
//!
//! Groups all `.csv` files in a folder, keys each *data row* by its
//! whitespace-normalized content (trim outer whitespace, collapse internal
//! whitespace runs to a single space), and decides which file keeps each
//! duplicated row. The first line of each file is treated as a header and
//! never deduped.

use super::plan::{rank_size, DupeKind, FileRank, FolderPlan, StripAction, StripLocations};
use crate::parse_date_to_iso;
use chrono::NaiveDate;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct CsvInput<'a> {
    pub path: PathBuf,
    pub content: &'a str,
}

struct ParsedFile {
    path: PathBuf,
    /// Normalized representation of each non-header data row (in source order).
    rows: Vec<String>,
    rank: FileRank,
}

/// Plan dedup for all CSV files in a folder.
pub fn plan_folder(files: &[CsvInput<'_>]) -> Result<FolderPlan, String> {
    if files.len() < 2 {
        return Ok(FolderPlan::default());
    }

    let mut parsed_files: Vec<ParsedFile> = Vec::with_capacity(files.len());
    for f in files {
        let rows = data_rows(f.content);
        let span_days = span_days_from_rows(&rows);
        parsed_files.push(ParsedFile {
            path: f.path.clone(),
            rows: rows.iter().map(|r| normalize(r)).collect(),
            rank: FileRank {
                span_days,
                size: rank_size(f.content),
                filename: file_name_string(&f.path),
            },
        });
    }

    // Map normalized-row → list of (file_idx, row_idx).
    let mut occurrences: HashMap<String, Vec<(usize, usize)>> = HashMap::new();
    for (i, pf) in parsed_files.iter().enumerate() {
        for (r, row) in pf.rows.iter().enumerate() {
            if row.is_empty() {
                continue;
            }
            occurrences.entry(row.clone()).or_default().push((i, r));
        }
    }

    let mut strip_rows: Vec<Vec<usize>> = vec![Vec::new(); parsed_files.len()];
    let mut dropped_keys: Vec<Vec<String>> = vec![Vec::new(); parsed_files.len()];

    for (key, occs) in &occurrences {
        if occs.len() < 2 {
            continue;
        }
        let winner_idx = occs
            .iter()
            .max_by(|a, b| parsed_files[a.0].rank.cmp_winner(&parsed_files[b.0].rank))
            .map(|(idx, _)| *idx)
            .expect("non-empty");
        for (idx, row) in occs {
            if *idx == winner_idx {
                continue;
            }
            strip_rows[*idx].push(*row);
            dropped_keys[*idx].push(key.clone());
        }
    }

    let mut plan = FolderPlan::default();
    for (i, mut rows) in strip_rows.into_iter().enumerate() {
        if rows.is_empty() {
            continue;
        }
        rows.sort_unstable();
        rows.dedup();
        let total_rows_with_data = parsed_files[i]
            .rows
            .iter()
            .filter(|r| !r.is_empty())
            .count();
        let stripped_count = rows.len();
        let mut keys = std::mem::take(&mut dropped_keys[i]);
        keys.sort();
        plan.strips.push(StripAction {
            path: parsed_files[i].path.clone(),
            locations: StripLocations::Row(rows),
            dropped_keys: keys,
        });
        if stripped_count == total_rows_with_data {
            plan.archives.push(parsed_files[i].path.clone());
        }
    }

    let untouched: Vec<&ParsedFile> = parsed_files
        .iter()
        .filter(|pf| !plan.strips.iter().any(|s| s.path == pf.path))
        .collect();
    if !untouched.is_empty() {
        let winner = untouched
            .iter()
            .max_by(|a, b| a.rank.cmp_winner(&b.rank))
            .unwrap();
        plan.canonical = Some(winner.path.clone());
    }

    plan.strips.sort_by(|a, b| a.path.cmp(&b.path));
    plan.archives.sort();

    let _kind = DupeKind::Csv;
    Ok(plan)
}

/// Split file content into data rows (everything after the first line).
/// Preserves order and per-line raw text. The header line is discarded.
fn data_rows(content: &str) -> Vec<String> {
    let mut lines = content.lines();
    lines.next(); // header
    lines.map(|l| l.to_string()).collect()
}

/// Field-aware whitespace normalization. Split on commas, trim each cell, and
/// collapse internal whitespace runs to single spaces, then rejoin. This makes
/// `2024-10-01,100.00,Coffee  Shop` and `2024-10-01, 100.00 , Coffee Shop`
/// hash to the same key.
///
/// Note: this is a naive split that does not understand quoted fields with
/// embedded commas. Bank CSV exports overwhelmingly avoid embedded commas, and
/// the worst case is a missed dedup (still safe), not data loss.
fn normalize(s: &str) -> String {
    let trimmed = s.trim();
    let cells: Vec<String> = trimmed.split(',').map(normalize_cell).collect();
    cells.join(",")
}

fn normalize_cell(s: &str) -> String {
    let t = s.trim();
    let mut out = String::with_capacity(t.len());
    let mut in_ws = false;
    for c in t.chars() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

fn file_name_string(p: &Path) -> String {
    p.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Best-effort date span: scan each cell of every row, try to parse it as a
/// date, take min/max. Returns 0 if no cell parses.
fn span_days_from_rows(rows: &[String]) -> i64 {
    let mut min: Option<NaiveDate> = None;
    let mut max: Option<NaiveDate> = None;
    for row in rows {
        for cell in row.split(',') {
            let cell = cell.trim().trim_matches('"');
            if let Ok(iso) = parse_date_to_iso(cell) {
                if let Ok(d) = NaiveDate::parse_from_str(&iso, "%Y-%m-%d") {
                    min = Some(min.map_or(d, |m| m.min(d)));
                    max = Some(max.map_or(d, |m| m.max(d)));
                }
            }
        }
    }
    match (min, max) {
        (Some(s), Some(e)) => (e - s).num_days().max(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitespace_normalized_duplicate_detected() {
        let wide = "Date,Amount,Memo\n\
                    2024-10-01,100.00,Coffee  Shop\n\
                    2024-10-02,50.00,Bookshop\n\
                    2024-11-15,200.00,Groceries\n";
        let narrow = "Date,Amount,Memo\n\
                      2024-10-01, 100.00 , Coffee Shop\n\
                      2024-10-15,75.00,Cinema\n";
        let inputs = vec![
            CsvInput {
                path: PathBuf::from("wide.csv"),
                content: wide,
            },
            CsvInput {
                path: PathBuf::from("narrow.csv"),
                content: narrow,
            },
        ];
        let plan = plan_folder(&inputs).unwrap();
        assert_eq!(plan.canonical, Some(PathBuf::from("wide.csv")));
        assert_eq!(plan.strips.len(), 1);
        let strip = &plan.strips[0];
        assert_eq!(strip.path, PathBuf::from("narrow.csv"));
        if let StripLocations::Row(rows) = &strip.locations {
            assert_eq!(rows, &vec![0_usize]);
        } else {
            panic!("expected row strip");
        }
        assert!(plan.archives.is_empty());
    }

    #[test]
    fn header_is_never_deduped() {
        let a = "X,Y\n1,2\n";
        let b = "X,Y\n3,4\n";
        // Identical headers but different data rows → no strips.
        let inputs = vec![
            CsvInput {
                path: PathBuf::from("a.csv"),
                content: a,
            },
            CsvInput {
                path: PathBuf::from("b.csv"),
                content: b,
            },
        ];
        let plan = plan_folder(&inputs).unwrap();
        assert!(plan.is_empty());
    }

    #[test]
    fn full_overlap_archives_narrow() {
        let wide = "Date,Amount\n2024-01-01,10\n2024-06-30,20\n";
        let narrow = "Date,Amount\n2024-01-01,10\n";
        let inputs = vec![
            CsvInput {
                path: PathBuf::from("wide.csv"),
                content: wide,
            },
            CsvInput {
                path: PathBuf::from("narrow.csv"),
                content: narrow,
            },
        ];
        let plan = plan_folder(&inputs).unwrap();
        assert_eq!(plan.archives, vec![PathBuf::from("narrow.csv")]);
    }
}
