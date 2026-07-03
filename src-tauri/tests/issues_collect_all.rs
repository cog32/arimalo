//! Integration tests for `issues::collect_all`.
//!
//! These exercise the on-disk loading path (warnings.json, pipeline metadata,
//! account set discovery) that the unit tests in `issues.rs` intentionally skip.

use std::fs;
use std::path::PathBuf;

use arimalo_covid::issues::{self, Category, CollectFilter};

fn tmp() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "arimalo-issues-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(path: &std::path::Path, s: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, s).unwrap();
}

fn minimal_ledger() -> &'static str {
    // Two-posting, balanced; not uncategorised. Uses assets:eth so the
    // unverified-balance collector has something to flag.
    "2024-01-01 * \"Coffee\" \"\"\n  assets:eth  1.0 ETH\n  expenses:food  -1.0 ETH\n"
}

fn uncategorised_ledger() -> &'static str {
    // Single-posting → flagged as uncategorised.
    "2024-01-02 * \"Single\" \"\"\n  assets:eth  2.0 ETH\n"
}

#[test]
fn collects_pipeline_warnings_from_sidecar() {
    let root = tmp();
    let generated = root.join("generated");
    let sources = root.join("sources");
    fs::create_dir_all(&sources).unwrap();

    // Minimal account set
    let set = generated.join("set1");
    write(&set.join("ledger.transactions"), minimal_ledger());

    // warnings.json sidecar
    write(
        &generated.join("warnings.json"),
        r#"{ "warnings": ["bank: something wrong"] }"#,
    );

    let filter = CollectFilter {
        categories: [Category::PipelineWarnings].into_iter().collect(),
        account: None,
    };
    let groups = issues::collect_all(&sources, &generated, &filter).unwrap().groups;

    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "bank");
    assert_eq!(groups[0].issues[0].message, "something wrong");
}

#[test]
fn missing_warnings_json_is_silently_empty() {
    let root = tmp();
    let generated = root.join("generated");
    let sources = root.join("sources");
    fs::create_dir_all(&sources).unwrap();
    let set = generated.join("set1");
    write(&set.join("ledger.transactions"), minimal_ledger());

    let filter = CollectFilter {
        categories: [Category::PipelineWarnings].into_iter().collect(),
        account: None,
    };
    let groups = issues::collect_all(&sources, &generated, &filter).unwrap().groups;
    assert!(groups.is_empty());
}

#[test]
fn collects_uncategorised_across_account_sets() {
    let root = tmp();
    let generated = root.join("generated");
    let sources = root.join("sources");
    fs::create_dir_all(&sources).unwrap();
    write(
        &generated.join("set1/ledger.transactions"),
        uncategorised_ledger(),
    );

    let filter = CollectFilter {
        categories: [Category::Uncategorised].into_iter().collect(),
        account: None,
    };
    let groups = issues::collect_all(&sources, &generated, &filter).unwrap().groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].label, "Uncategorised");
    // First issue is the summary line; detail lines follow.
    assert_eq!(groups[0].issues[0].message, "1 uncategorised transaction");
}

#[test]
fn category_filter_excludes_other_categories() {
    let root = tmp();
    let generated = root.join("generated");
    let sources = root.join("sources");
    fs::create_dir_all(&sources).unwrap();
    write(
        &generated.join("set1/ledger.transactions"),
        uncategorised_ledger(),
    );
    write(
        &generated.join("warnings.json"),
        r#"{ "warnings": ["bank: oops"] }"#,
    );

    // Ask only for pipeline warnings: uncategorised group must not appear.
    let filter = CollectFilter {
        categories: [Category::PipelineWarnings].into_iter().collect(),
        account: None,
    };
    let groups = issues::collect_all(&sources, &generated, &filter).unwrap().groups;
    assert!(groups.iter().all(|g| g.label != "Uncategorised"));
    assert!(groups.iter().any(|g| g.label == "bank"));
}

#[test]
fn skips_non_account_set_directories() {
    let root = tmp();
    let generated = root.join("generated");
    let sources = root.join("sources");
    fs::create_dir_all(&sources).unwrap();
    // An account set (has ledger.transactions)
    write(
        &generated.join("real/ledger.transactions"),
        uncategorised_ledger(),
    );
    // A decoy subdir without a ledger — must be ignored
    fs::create_dir_all(generated.join("prices")).unwrap();

    let filter = CollectFilter {
        categories: [Category::Uncategorised].into_iter().collect(),
        account: None,
    };
    let groups = issues::collect_all(&sources, &generated, &filter).unwrap().groups;
    assert_eq!(groups.len(), 1);
}
