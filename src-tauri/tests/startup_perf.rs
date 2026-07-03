//! Wall-clock regression test for the warm-cache pipeline run.
//!
//! When `sources/` hasn't changed since the last run, `run_pipeline` must hit
//! the `cache.inputs_hash == global_fp` early-exit branch in
//! `processing_pipeline.rs` and return promptly. This is the path the app
//! re-enters on every boot, so a regression here directly translates to a
//! slower startup for every launch.
//!
//! The frontend startup tests in `src/startup.test.ts` enforce call counts
//! against mocked invokes — they cannot catch a regression where this path
//! starts doing real work, nor a slowdown in `load_cache` itself.

use arimalo_covid::processing_pipeline::{run_pipeline, PipelineConfig};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn tmp(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("{prefix}-{pid}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_config(sources: &PathBuf, generated: &PathBuf) -> PipelineConfig {
    PipelineConfig {
        sources_dir: sources.clone(),
        generated_dir: generated.clone(),
        now_yyyymm: "202501".to_string(),
        force: false,
        default_expense_account: arimalo_covid::FALLBACK_EXPENSE_ACCOUNT.to_string(),
        changed_folder_hint: None,
    }
}

/// One trivial folder with a CSV + transform — minimum to give the pipeline
/// real work to do on the cold run so it has something to cache.
fn write_minimal_sources(sources: &PathBuf) {
    let folder = sources.join("richard/wallet/sample");
    fs::create_dir_all(&folder).unwrap();
    fs::write(
        folder.join("accounts.transactions"),
        "account assets:crypto:wallet:sample\n  asset_class: \"crypto\"\n",
    )
    .unwrap();
    fs::write(
        folder.join("sample.csv"),
        "Date,Description,Amount\n2025-01-01,test,1.00\n",
    )
    .unwrap();
    fs::write(
        folder.join("_transform.rhai"),
        r##"#{
  date: row["Date"],
  payee: row["Description"],
  narration: "imported",
  amount: row["Amount"],
  commodity: "AUD",
  status: "*"
}"##,
    )
    .unwrap();
}

#[test]
fn warm_cache_pipeline_run_finishes_under_500ms() {
    let sources = tmp("arimalo-perf-srcs");
    let generated = tmp("arimalo-perf-gen");
    write_minimal_sources(&sources);
    let config = make_config(&sources, &generated);

    let cold = run_pipeline(&config).expect("cold pipeline run");
    assert!(
        !cold.early_exit,
        "cold run should do real work (cache was empty)"
    );

    let start = Instant::now();
    let warm = run_pipeline(&config).expect("warm pipeline run");
    let elapsed = start.elapsed();

    assert!(
        warm.early_exit,
        "warm run should hit the global_fingerprint early-exit branch \
         (sources unchanged); instead it ran the full pipeline"
    );
    let ceiling = Duration::from_millis(500);
    assert!(
        elapsed < ceiling,
        "warm-cache pipeline run took {elapsed:?}; ceiling is {ceiling:?}. \
         The app re-enters this path on every boot — keep it fast."
    );
}
