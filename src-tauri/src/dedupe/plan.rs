//! Shared plan types used by both OFX and CSV dedup planners.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DupeKind {
    Ofx,
    Csv,
}

impl DupeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DupeKind::Ofx => "ofx",
            DupeKind::Csv => "csv",
        }
    }
}

/// Where in a file the duplicate records live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StripLocations {
    /// Byte ranges in the original file content (start inclusive, end exclusive).
    /// Sorted ascending and non-overlapping.
    Byte(Vec<(usize, usize)>),
    /// Zero-based data-row indices, *excluding* the header row.
    /// Sorted ascending.
    Row(Vec<usize>),
}

#[derive(Debug, Clone)]
pub struct StripAction {
    pub path: PathBuf,
    pub locations: StripLocations,
    /// Human-readable keys (FITIDs for OFX, normalized rows for CSV) for reporting.
    pub dropped_keys: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct FolderPlan {
    /// The file the planner considers the "canonical" copy for the folder:
    /// the widest-span file that has zero records stripped. None if no single
    /// file qualifies (e.g. all files share the same span and contribute unique
    /// records).
    pub canonical: Option<PathBuf>,
    pub strips: Vec<StripAction>,
    /// Files that, after stripping, contain zero records and should be archived.
    pub archives: Vec<PathBuf>,
}

impl FolderPlan {
    /// True if the plan would modify any file.
    pub fn is_empty(&self) -> bool {
        self.strips.is_empty() && self.archives.is_empty()
    }
}

/// Metadata used to rank candidate files for "which copy keeps the record".
///
/// Wider span wins; ties broken by larger size; final tiebreak alphabetical
/// (lexicographically earliest filename wins, so the result is deterministic).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRank {
    /// Span width in days (or 0 if the file has no parseable date range).
    pub span_days: i64,
    /// Size in bytes *after stripping CR characters*. Two files that differ only
    /// by line endings (LF vs CRLF) must rank equally on this axis — otherwise
    /// the CRLF copy wins the tiebreaker by 2 bytes per line for the same data.
    pub size: u64,
    pub filename: String,
}

/// Size in bytes used by [`FileRank`] — same content, ignoring CR characters.
pub fn rank_size(content: &str) -> u64 {
    content.bytes().filter(|b| *b != b'\r').count() as u64
}

impl FileRank {
    /// Returns `Ordering::Greater` when `self` is the better candidate.
    pub fn cmp_winner(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match self.span_days.cmp(&other.span_days) {
            Ordering::Equal => {}
            ord => return ord,
        }
        match self.size.cmp(&other.size) {
            Ordering::Equal => {}
            ord => return ord,
        }
        // Alphabetically earlier filename wins → flip the natural ordering.
        other.filename.cmp(&self.filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(span: i64, size: u64, name: &str) -> FileRank {
        FileRank {
            span_days: span,
            size,
            filename: name.to_string(),
        }
    }

    #[test]
    fn wider_span_wins() {
        assert!(r(100, 1, "z.ofx").cmp_winner(&r(50, 999, "a.ofx")).is_gt());
    }

    #[test]
    fn equal_span_larger_size_wins() {
        assert!(r(100, 999, "z.ofx")
            .cmp_winner(&r(100, 100, "a.ofx"))
            .is_gt());
    }

    #[test]
    fn equal_span_and_size_alphabetical_wins() {
        assert!(r(100, 500, "a.ofx")
            .cmp_winner(&r(100, 500, "b.ofx"))
            .is_gt());
    }
}
