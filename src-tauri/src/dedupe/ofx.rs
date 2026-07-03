//! OFX-file deduplication planner.
//!
//! Groups all `.ofx` files in a folder, keys each transaction by its FITID,
//! and decides which file keeps each duplicated FITID. See [`plan_folder`].

use super::plan::{rank_size, DupeKind, FileRank, FolderPlan, StripAction, StripLocations};
use crate::ofx_parser::{parse_ofx, OfxFile};
use crate::parse_date_to_iso;
use chrono::NaiveDate;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One OFX file ready for planning: its path and raw text content.
pub struct OfxInput<'a> {
    pub path: PathBuf,
    pub content: &'a str,
}

struct ParsedFile {
    path: PathBuf,
    parsed: OfxFile,
    rank: FileRank,
    fitid_count: usize,
}

/// One occurrence of a FITID: (file index, transaction block range).
type FitidOccurrence = (usize, (usize, usize));

/// Plan dedup for all OFX files in a folder.
pub fn plan_folder(files: &[OfxInput<'_>]) -> Result<FolderPlan, String> {
    if files.len() < 2 {
        return Ok(FolderPlan::default());
    }

    let mut parsed_files: Vec<ParsedFile> = Vec::with_capacity(files.len());
    for f in files {
        let parsed = parse_ofx(f.content)
            .map_err(|e| format!("failed to parse OFX {}: {e}", f.path.display()))?;
        let span_days = span_days_from_dt(&parsed.dtstart, &parsed.dtend, &parsed.transactions);
        let filename = file_name_string(&f.path);
        let rank = FileRank {
            span_days,
            size: rank_size(f.content),
            filename,
        };
        let fitid_count = parsed.transactions.len();
        parsed_files.push(ParsedFile {
            path: f.path.clone(),
            parsed,
            rank,
            fitid_count,
        });
    }

    // For each FITID, list every (file_idx, block_range, fitid).
    let mut occurrences: HashMap<String, Vec<FitidOccurrence>> = HashMap::new();
    for (i, pf) in parsed_files.iter().enumerate() {
        for txn in &pf.parsed.transactions {
            occurrences
                .entry(txn.fitid.clone())
                .or_default()
                .push((i, txn.block_range));
        }
    }

    // For every duplicated FITID, pick a winner; everyone else gets stripped.
    let mut strip_ranges: Vec<Vec<(usize, usize)>> = vec![Vec::new(); parsed_files.len()];
    let mut dropped_keys: Vec<Vec<String>> = vec![Vec::new(); parsed_files.len()];

    for (fitid, occs) in &occurrences {
        if occs.len() < 2 {
            continue;
        }
        let winner_idx = occs
            .iter()
            .max_by(|a, b| parsed_files[a.0].rank.cmp_winner(&parsed_files[b.0].rank))
            .map(|(idx, _)| *idx)
            .expect("non-empty");
        for (idx, range) in occs {
            if *idx == winner_idx {
                continue;
            }
            strip_ranges[*idx].push(*range);
            dropped_keys[*idx].push(fitid.clone());
        }
    }

    // Assemble FolderPlan.
    let mut plan = FolderPlan::default();
    for (i, mut ranges) in strip_ranges.into_iter().enumerate() {
        if ranges.is_empty() {
            continue;
        }
        ranges.sort_by_key(|(s, _)| *s);
        let stripped_count = ranges.len();
        let mut keys = std::mem::take(&mut dropped_keys[i]);
        keys.sort();
        let action = StripAction {
            path: parsed_files[i].path.clone(),
            locations: StripLocations::Byte(ranges),
            dropped_keys: keys,
        };
        plan.strips.push(action);
        if stripped_count == parsed_files[i].fitid_count {
            plan.archives.push(parsed_files[i].path.clone());
        }
    }

    // Canonical = widest-span file that has zero strips.
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

    // Sort outputs for stable reporting.
    plan.strips.sort_by(|a, b| a.path.cmp(&b.path));
    plan.archives.sort();

    let _kind = DupeKind::Ofx;
    Ok(plan)
}

fn file_name_string(p: &Path) -> String {
    p.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string()
}

/// Compute span in days. Prefer explicit DTSTART/DTEND; fall back to min/max
/// DTPOSTED across the transactions. Returns 0 if no usable dates exist.
fn span_days_from_dt(
    dtstart: &str,
    dtend: &str,
    txns: &[crate::ofx_parser::OfxTransaction],
) -> i64 {
    let parse = |s: &str| -> Option<NaiveDate> {
        let iso = parse_date_to_iso(s).ok()?;
        NaiveDate::parse_from_str(&iso, "%Y-%m-%d").ok()
    };
    let (start, end) = match (parse(dtstart), parse(dtend)) {
        (Some(s), Some(e)) => (s, e),
        _ => {
            let dates: Vec<NaiveDate> = txns.iter().filter_map(|t| parse(&t.dtposted)).collect();
            if dates.is_empty() {
                return 0;
            }
            let s = *dates.iter().min().unwrap();
            let e = *dates.iter().max().unwrap();
            (s, e)
        }
    };
    (end - start).num_days().max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ofx(dtstart: &str, dtend: &str, fitids: &[&str]) -> String {
        let mut s = format!(
            "OFXHEADER:100\nDATA:OFXSGML\n\n<OFX>\n<BANKMSGSRSV1>\n<STMTTRNRS>\n<STMTRS>\n<CURDEF>AUD\n<BANKTRANLIST>\n<DTSTART>{dtstart}\n<DTEND>{dtend}\n"
        );
        for (i, fid) in fitids.iter().enumerate() {
            s.push_str(&format!(
                "<STMTTRN>\n<TRNTYPE>DEBIT\n<DTPOSTED>{dtstart}\n<TRNAMT>-{}.00\n<FITID>{fid}\n<MEMO>tx{fid}\n</STMTTRN>\n",
                i + 1
            ));
        }
        s.push_str("</BANKTRANLIST>\n</STMTRS>\n</STMTTRNRS>\n</BANKMSGSRSV1>\n</OFX>\n");
        s
    }

    #[test]
    fn triple_overlap_picks_widest_canonical() {
        let wide = ofx("20240101", "20251231", &["A", "B", "C", "D"]);
        let mid = ofx("20240401", "20250617", &["B", "C"]);
        let narrow = ofx("20250101", "20250228", &["C"]);

        let inputs = vec![
            OfxInput {
                path: PathBuf::from("wide.ofx"),
                content: &wide,
            },
            OfxInput {
                path: PathBuf::from("mid.ofx"),
                content: &mid,
            },
            OfxInput {
                path: PathBuf::from("narrow.ofx"),
                content: &narrow,
            },
        ];

        let plan = plan_folder(&inputs).unwrap();
        assert_eq!(plan.canonical, Some(PathBuf::from("wide.ofx")));
        assert_eq!(plan.strips.len(), 2);

        let mid_strip = plan
            .strips
            .iter()
            .find(|s| s.path == PathBuf::from("mid.ofx"))
            .unwrap();
        assert_eq!(
            mid_strip.dropped_keys,
            vec!["B".to_string(), "C".to_string()]
        );

        let narrow_strip = plan
            .strips
            .iter()
            .find(|s| s.path == PathBuf::from("narrow.ofx"))
            .unwrap();
        assert_eq!(narrow_strip.dropped_keys, vec!["C".to_string()]);

        // mid (B,C) and narrow (C) lose every FITID to wide → both archived.
        assert_eq!(
            plan.archives,
            vec![PathBuf::from("mid.ofx"), PathBuf::from("narrow.ofx")]
        );
    }

    #[test]
    fn equal_span_size_tiebreak() {
        let small = ofx("20240101", "20240630", &["X", "Y"]);
        let big = ofx("20240101", "20240630", &["X", "Y", "Z", "W", "Q"]);

        let inputs = vec![
            OfxInput {
                path: PathBuf::from("small.ofx"),
                content: &small,
            },
            OfxInput {
                path: PathBuf::from("big.ofx"),
                content: &big,
            },
        ];

        let plan = plan_folder(&inputs).unwrap();
        assert_eq!(plan.canonical, Some(PathBuf::from("big.ofx")));
        assert_eq!(plan.archives, vec![PathBuf::from("small.ofx")]);
    }

    #[test]
    fn crlf_and_lf_copies_of_same_content_tie_on_size() {
        // Two byte-for-byte identical exports modulo line endings must NOT
        // give the CRLF copy a size advantage in the tiebreaker — otherwise
        // we silently prefer the CRLF download over the LF one.
        let lf = ofx("20240101", "20240630", &["A", "B", "C"]);
        let crlf = lf.replace('\n', "\r\n");

        let inputs = vec![
            OfxInput {
                path: PathBuf::from("a.ofx"),
                content: &lf,
            },
            OfxInput {
                path: PathBuf::from("b.ofx"),
                content: &crlf,
            },
        ];
        let plan = plan_folder(&inputs).unwrap();
        // Tie broken alphabetically — "a.ofx" wins, "b.ofx" archived.
        assert_eq!(plan.canonical, Some(PathBuf::from("a.ofx")));
        assert_eq!(plan.archives, vec![PathBuf::from("b.ofx")]);
    }

    #[test]
    fn no_duplicates_returns_empty_plan() {
        let a = ofx("20240101", "20240131", &["A1"]);
        let b = ofx("20240201", "20240228", &["B1"]);
        let inputs = vec![
            OfxInput {
                path: PathBuf::from("a.ofx"),
                content: &a,
            },
            OfxInput {
                path: PathBuf::from("b.ofx"),
                content: &b,
            },
        ];
        let plan = plan_folder(&inputs).unwrap();
        assert!(plan.is_empty());
    }

    #[test]
    fn single_file_no_plan() {
        let a = ofx("20240101", "20240131", &["A1", "A2"]);
        let inputs = vec![OfxInput {
            path: PathBuf::from("a.ofx"),
            content: &a,
        }];
        let plan = plan_folder(&inputs).unwrap();
        assert!(plan.is_empty());
    }

    #[test]
    fn byte_ranges_strip_only_duplicate_blocks() {
        let wide = ofx("20240101", "20251231", &["A", "B"]);
        let narrow = ofx("20250101", "20250228", &["B"]);
        let inputs = vec![
            OfxInput {
                path: PathBuf::from("wide.ofx"),
                content: &wide,
            },
            OfxInput {
                path: PathBuf::from("narrow.ofx"),
                content: &narrow,
            },
        ];

        let plan = plan_folder(&inputs).unwrap();
        let strip = &plan.strips[0];
        assert_eq!(strip.path, PathBuf::from("narrow.ofx"));
        if let StripLocations::Byte(ranges) = &strip.locations {
            assert_eq!(ranges.len(), 1);
            // The removed substring must be the <STMTTRN> block for FITID B.
            let (s, e) = ranges[0];
            let block = &narrow[s..e];
            assert!(block.contains("<FITID>B"));
            assert!(block.starts_with("<STMTTRN>"));
            assert!(block.ends_with("</STMTTRN>"));
        } else {
            panic!("expected byte ranges");
        }
    }
}
