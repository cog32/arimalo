use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;

// --- Manifest types ---

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub config: HashMap<String, ConfigField>,
    #[serde(default)]
    pub secrets: HashMap<String, ConfigField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub description: Option<String>,
    pub script: String,
    /// When true, this plugin is part of the automatic "daily" backfill that
    /// the app can run on startup. Daily plugins must be incremental and
    /// idempotent (fetch only what's missing). Defaults to false.
    #[serde(default)]
    pub daily: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigField {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub default: Option<toml::Value>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
}

// --- Runtime types ---

#[derive(Debug, Clone, Serialize)]
pub struct PluginInfo {
    pub dir_name: String,
    pub dir: PathBuf,
    pub manifest: PluginManifest,
    pub last_run: Option<String>,
    pub last_status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginRunResult {
    /// True only on a clean exit 0. Exit 2 (partial success) has `success:
    /// false` but `exit_code: Some(2)` — inspect `exit_code` to tell partial
    /// success apart from outright failure.
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    /// The process exit code, if the process ran to completion. `None` when the
    /// plugin could not be spawned/awaited at all.
    pub exit_code: Option<i32>,
}

/// Outcome of one plugin within a daily batch run.
#[derive(Debug, Clone, Serialize)]
pub struct DailyPluginOutcome {
    pub dir_name: String,
    pub name: String,
    /// True only on a clean exit 0. Exit 2 (partial success) is `false` here but
    /// `exit_code: Some(2)`.
    pub success: bool,
    pub duration_ms: u64,
    /// True if the plugin was skipped because it already succeeded today.
    pub skipped_ran_today: bool,
    /// The plugin's process exit code (`None` if skipped or un-spawnable).
    /// 0 = updated, 2 = updated with warnings, anything else = failed.
    pub exit_code: Option<i32>,
}

/// Aggregate result of running the daily-flagged plugins.
#[derive(Debug, Clone, Serialize)]
pub struct DailyRunSummary {
    pub outcomes: Vec<DailyPluginOutcome>,
}

impl DailyRunSummary {
    /// True if every plugin either succeeded (exit 0), partially succeeded
    /// (exit 2 — wrote data with warnings), or was skipped. No hard failures.
    pub fn all_ok(&self) -> bool {
        self.outcomes
            .iter()
            .all(|o| o.success || o.exit_code == Some(2))
    }

    /// True if at least one plugin actually ran (not skipped). The caller uses
    /// this to decide whether a pipeline rebuild is worth doing.
    pub fn any_ran(&self) -> bool {
        self.outcomes.iter().any(|o| !o.skipped_ran_today)
    }
}

// --- Discovery ---

pub fn discover_plugins(root_dir: &Path) -> Vec<PluginInfo> {
    let plugins_dir = root_dir.join("plugins");
    discover_plugins_in(root_dir, &plugins_dir)
}

pub fn discover_plugins_in(_root_dir: &Path, plugins_dir: &Path) -> Vec<PluginInfo> {
    if !plugins_dir.is_dir() {
        return Vec::new();
    }

    let mut plugins = Vec::new();
    let entries = match std::fs::read_dir(plugins_dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let manifest_path = path.join("plugin.toml");
        if !manifest_path.exists() {
            continue;
        }
        let content = match std::fs::read_to_string(&manifest_path) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let manifest: PluginManifest = match toml::from_str(&content) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let dir_name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // Load last run status from .data/last_run.json
        let data_dir = path.join(".data");
        let (last_run, last_status) = load_last_run(&data_dir);

        plugins.push(PluginInfo {
            dir_name,
            dir: path,
            manifest,
            last_run,
            last_status,
        });
    }

    plugins.sort_by(|a, b| a.dir_name.cmp(&b.dir_name));
    plugins
}

fn load_last_run(data_dir: &Path) -> (Option<String>, Option<String>) {
    let last_run_path = data_dir.join("last_run.json");
    if let Ok(content) = std::fs::read_to_string(&last_run_path) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&content) {
            let run = v
                .get("timestamp")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string());
            let status = v
                .get("status")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            return (run, status);
        }
    }
    (None, None)
}

fn save_last_run(data_dir: &Path, success: bool) {
    let _ = std::fs::create_dir_all(data_dir);
    let timestamp = chrono::Utc::now().to_rfc3339();
    let status = if success { "success" } else { "failed" };
    let v = serde_json::json!({ "timestamp": timestamp, "status": status });
    let _ = std::fs::write(
        data_dir.join("last_run.json"),
        crate::to_sorted_json_pretty(&v).unwrap_or_default(),
    );
}

// --- Execution ---

/// Resolve the directory holding sibling CLI binaries (arimalo-query, ...).
/// Tests and non-Tauri callers can override via `ARIMALO_BIN_DIR`.
fn resolve_bin_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("ARIMALO_BIN_DIR") {
        let p = PathBuf::from(dir);
        if p.is_dir() {
            return Some(p);
        }
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|p| p.to_path_buf()))
}

pub fn run_plugin(
    plugin_dir: &Path,
    sources_dir: &Path,
    config: &serde_json::Value,
    secrets: &serde_json::Value,
) -> PluginRunResult {
    run_plugin_with_logger(plugin_dir, sources_dir, config, secrets, |_, _| {})
}

/// Like `run_plugin`, but invokes `on_line(stream, line)` for each line of
/// stdout/stderr as it arrives. `stream` is "stdout" or "stderr".
pub fn run_plugin_with_logger<F>(
    plugin_dir: &Path,
    sources_dir: &Path,
    config: &serde_json::Value,
    secrets: &serde_json::Value,
    on_line: F,
) -> PluginRunResult
where
    F: Fn(&str, &str) + Send + Sync + 'static,
{
    let manifest_path = plugin_dir.join("plugin.toml");
    let manifest_content = match std::fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(e) => {
            return PluginRunResult {
                success: false,
                stdout: String::new(),
                stderr: format!("Failed to read plugin.toml: {e}"),
                duration_ms: 0,
                exit_code: None,
            };
        }
    };
    let manifest: PluginManifest = match toml::from_str(&manifest_content) {
        Ok(m) => m,
        Err(e) => {
            return PluginRunResult {
                success: false,
                stdout: String::new(),
                stderr: format!("Failed to parse plugin.toml: {e}"),
                duration_ms: 0,
                exit_code: None,
            };
        }
    };

    let data_dir = plugin_dir.join(".data");
    let _ = std::fs::create_dir_all(&data_dir);

    let script_path = plugin_dir.join(&manifest.plugin.script);
    if !script_path.exists() {
        return PluginRunResult {
            success: false,
            stdout: String::new(),
            stderr: format!("Script not found: {}", manifest.plugin.script),
            duration_ms: 0,
            exit_code: None,
        };
    }

    let bin_dir = resolve_bin_dir();
    let arimalo_query_bin = bin_dir.as_ref().map(|d| d.join("arimalo-query"));

    // Build stdin context
    let context = serde_json::json!({
        "sources_dir": sources_dir.to_string_lossy(),
        "plugin_dir": plugin_dir.to_string_lossy(),
        "data_dir": data_dir.to_string_lossy(),
        "config": config,
        "secrets": secrets,
        "bin": {
            "arimalo_query": arimalo_query_bin.as_ref().map(|p| p.to_string_lossy().to_string()),
        },
    });
    let stdin_json = serde_json::to_string(&context).unwrap_or_default();

    // PEP 723 inline-metadata aware launcher.
    //
    // If `uv` is on PATH and the script begins with a `# /// script` block,
    // run via `uv run --script` so dependencies declared in the header are
    // provisioned in an ephemeral, hardlinked venv. Otherwise fall back to
    // bare `python3` for stdlib-only scripts and existing setups.
    let has_pep723 = script_has_pep723(&script_path);
    let uv_available = has_pep723 && which_command("uv").is_some();

    let start = Instant::now();
    let mut cmd = if uv_available {
        let mut c = Command::new("uv");
        c.arg("run").arg("--script").arg(&script_path);
        c
    } else {
        let mut c = Command::new("python3");
        c.arg(&script_path);
        c
    };
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env(
            "ARIMALO_SOURCES_DIR",
            sources_dir.to_string_lossy().as_ref(),
        )
        .env("ARIMALO_PLUGIN_DIR", plugin_dir.to_string_lossy().as_ref())
        .env(
            "ARIMALO_PLUGIN_DATA_DIR",
            data_dir.to_string_lossy().as_ref(),
        );

    if let Some(ref qbin) = arimalo_query_bin {
        cmd.env("ARIMALO_QUERY_BIN", qbin);
    }
    if let Some(ref dir) = bin_dir {
        // Prepend bin_dir to PATH so plugins can invoke `arimalo-query` directly.
        let sep = if cfg!(windows) { ";" } else { ":" };
        let existing_path = std::env::var("PATH").unwrap_or_default();
        cmd.env(
            "PATH",
            format!("{}{sep}{existing_path}", dir.to_string_lossy()),
        );
    }

    let mut child = match cmd.spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return PluginRunResult {
                success: false,
                stdout: String::new(),
                stderr: format!(
                    "Failed to spawn plugin script: {e}. Ensure {} is installed.",
                    if uv_available { "`uv`" } else { "Python 3" }
                ),
                duration_ms: 0,
                exit_code: None,
            };
        }
    };

    // Write stdin
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(stdin_json.as_bytes());
    }

    // Stream stdout/stderr line-by-line so the UI can render progress live
    // (Claude classification loops can run for many minutes).
    let on_line = Arc::new(on_line);
    let stdout_buf = Arc::new(Mutex::new(String::new()));
    let stderr_buf = Arc::new(Mutex::new(String::new()));

    let stdout_handle = child.stdout.take().map(|out| {
        let buf = Arc::clone(&stdout_buf);
        let cb = Arc::clone(&on_line);
        thread::spawn(move || {
            let reader = BufReader::new(out);
            for line in reader.lines().map_while(Result::ok) {
                cb("stdout", &line);
                let mut b = buf.lock().unwrap();
                b.push_str(&line);
                b.push('\n');
            }
        })
    });
    let stderr_handle = child.stderr.take().map(|err| {
        let buf = Arc::clone(&stderr_buf);
        let cb = Arc::clone(&on_line);
        thread::spawn(move || {
            let reader = BufReader::new(err);
            for line in reader.lines().map_while(Result::ok) {
                cb("stderr", &line);
                let mut b = buf.lock().unwrap();
                b.push_str(&line);
                b.push('\n');
            }
        })
    });

    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => {
            return PluginRunResult {
                success: false,
                stdout: String::new(),
                stderr: format!("Failed to wait for plugin: {e}"),
                duration_ms: start.elapsed().as_millis() as u64,
                exit_code: None,
            };
        }
    };
    if let Some(h) = stdout_handle {
        let _ = h.join();
    }
    if let Some(h) = stderr_handle {
        let _ = h.join();
    }

    let duration_ms = start.elapsed().as_millis() as u64;
    let stdout = stdout_buf.lock().unwrap().clone();
    let stderr = stderr_buf.lock().unwrap().clone();
    let exit_code = status.code();
    let success = status.success();

    // Exit 2 is "partial success" per the plugin contract (data written, some
    // warnings). Count it as a completed run for the once-per-day guard so a
    // partial run isn't repeated on every relaunch the same day.
    save_last_run(&data_dir, matches!(exit_code, Some(0) | Some(2)));

    PluginRunResult {
        success,
        stdout,
        stderr,
        duration_ms,
        exit_code,
    }
}

// --- Daily batch run ---

/// True if `last_run.json` in `data_dir` records a successful run whose
/// timestamp falls on today's local date.
fn succeeded_today(data_dir: &Path) -> bool {
    let (timestamp, status) = load_last_run(data_dir);
    if status.as_deref() != Some("success") {
        return false;
    }
    let Some(ts) = timestamp else {
        return false;
    };
    match chrono::DateTime::parse_from_rfc3339(&ts) {
        Ok(dt) => {
            dt.with_timezone(&chrono::Local).date_naive() == chrono::Local::now().date_naive()
        }
        Err(_) => false,
    }
}

/// Run every plugin whose manifest has `daily = true`, sequentially, in
/// discovery (dir-name) order. Continues past individual failures. When
/// `skip_if_succeeded_today` is true, a plugin that already succeeded today is
/// skipped without running. `on_line(dir_name, stream, line)` receives each
/// line of plugin output as it arrives.
///
/// Does NOT rebuild the pipeline — callers do that once after the batch.
pub fn run_daily_plugins<F>(
    plugins_dir: &Path,
    sources_dir: &Path,
    skip_if_succeeded_today: bool,
    on_line: F,
) -> DailyRunSummary
where
    F: Fn(&str, &str, &str) + Send + Sync + 'static,
{
    let on_line = Arc::new(on_line);
    let mut outcomes = Vec::new();

    for plugin in discover_plugins_in(plugins_dir, plugins_dir)
        .into_iter()
        .filter(|p| p.manifest.plugin.daily)
    {
        let dir_name = plugin.dir_name.clone();
        let name = plugin.manifest.plugin.name.clone();

        if skip_if_succeeded_today && succeeded_today(&plugin.dir.join(".data")) {
            outcomes.push(DailyPluginOutcome {
                dir_name,
                name,
                success: true,
                duration_ms: 0,
                skipped_ran_today: true,
                exit_code: None,
            });
            continue;
        }

        let config = load_plugin_config(&plugin.dir);
        let secrets = load_plugin_secrets(&plugin.dir);

        // Tag each output line with the plugin's dir name so a shared logger
        // can attribute lines to the right plugin.
        let cb = Arc::clone(&on_line);
        let tag = dir_name.clone();
        let result = run_plugin_with_logger(
            &plugin.dir,
            sources_dir,
            &config,
            &secrets,
            move |stream, line| cb(&tag, stream, line),
        );

        outcomes.push(DailyPluginOutcome {
            dir_name,
            name,
            success: result.success,
            duration_ms: result.duration_ms,
            skipped_ran_today: false,
            exit_code: result.exit_code,
        });
    }

    DailyRunSummary { outcomes }
}

// --- Config / Secrets persistence ---

pub fn load_plugin_config(plugin_dir: &Path) -> serde_json::Value {
    let path = plugin_dir.join(".data").join("config.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(Default::default()))
        }
        Err(_) => serde_json::Value::Object(Default::default()),
    }
}

pub fn save_plugin_config(plugin_dir: &Path, config: &serde_json::Value) -> Result<(), String> {
    let data_dir = plugin_dir.join(".data");
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("Failed to create .data dir: {e}"))?;
    let content = crate::to_sorted_json_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {e}"))?;
    std::fs::write(data_dir.join("config.json"), content)
        .map_err(|e| format!("Failed to write config: {e}"))
}

pub fn load_plugin_secrets(plugin_dir: &Path) -> serde_json::Value {
    let path = plugin_dir.join(".data").join("secrets.json");
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(Default::default()))
        }
        Err(_) => serde_json::Value::Object(Default::default()),
    }
}

pub fn save_plugin_secrets(plugin_dir: &Path, secrets: &serde_json::Value) -> Result<(), String> {
    let data_dir = plugin_dir.join(".data");
    std::fs::create_dir_all(&data_dir).map_err(|e| format!("Failed to create .data dir: {e}"))?;
    let content = crate::to_sorted_json_pretty(secrets)
        .map_err(|e| format!("Failed to serialize secrets: {e}"))?;
    std::fs::write(data_dir.join("secrets.json"), content)
        .map_err(|e| format!("Failed to write secrets: {e}"))
}

/// True if the script has a PEP 723 inline-metadata block in the first ~64 lines.
/// Recognised marker: a line of exactly `# /// script` followed somewhere by `# ///`.
fn script_has_pep723(script_path: &Path) -> bool {
    let Ok(content) = std::fs::read_to_string(script_path) else {
        return false;
    };
    let mut seen_open = false;
    for line in content.lines().take(64) {
        let trimmed = line.trim_end();
        if trimmed == "# /// script" {
            seen_open = true;
        } else if seen_open && trimmed == "# ///" {
            return true;
        }
    }
    false
}

/// Lightweight `which`: returns the resolved binary path if `name` is on PATH.
fn which_command(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
