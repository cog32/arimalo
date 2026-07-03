//! Edge-layer deduplication for overlapping bank statement exports.
//!
//! Banks routinely re-export overlapping date ranges (e.g. CBA SmartAccess
//! producing three OFX files whose ranges cover the same window). The pipeline
//! would otherwise ingest the same transaction multiple times.
//!
//! This module produces *plans* — pure data describing which records to strip
//! from which files — without performing any I/O. The CLI binary (`arimalo-dedupe`)
//! reads files into memory, asks these planners what to do, and applies the result.
//!
//! Strategy: within a folder, identify duplicate records (OFX: by FITID;
//! CSV: by whitespace-normalized row content). The file with the widest date
//! span keeps each record; narrower files have it stripped. Ties are broken by
//! file size, then alphabetical filename.

pub mod apply;
pub mod csv;
pub mod ofx;
pub mod plan;
