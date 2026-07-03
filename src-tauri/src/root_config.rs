use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RootConfig {
    pub current_root: Option<String>,
    #[serde(default)]
    pub known_roots: Vec<String>,
    #[serde(default = "default_expense_account")]
    pub default_expense_account: String,
    /// Optional path to an override prompt file for `arimalo-classify`.
    /// If unset, the embedded default is used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classify_prompt_path: Option<String>,
    /// When true, the app runs the daily price-backfill plugins on startup.
    #[serde(default)]
    pub update_prices_on_startup: bool,
    /// Extra account-name prefixes unioned into the primary-accounts allowlist
    /// used by the Balances / Performance / Tax-Savings reports. Lets nominal
    /// accounts (e.g. `assets:staking`, `assets:lending`) and sub-accounts of
    /// folder accounts count toward holdings. Each entry matches itself and any
    /// sub-account; empty list = today's folder-accounts-only behaviour.
    #[serde(default)]
    pub extra_primary_account_prefixes: Vec<String>,
}

fn default_expense_account() -> String {
    crate::FALLBACK_EXPENSE_ACCOUNT.to_string()
}

impl Default for RootConfig {
    fn default() -> Self {
        Self {
            current_root: None,
            known_roots: Vec::new(),
            default_expense_account: default_expense_account(),
            classify_prompt_path: None,
            update_prices_on_startup: false,
            extra_primary_account_prefixes: Vec::new(),
        }
    }
}

/// Load root config from `config.json` inside `app_data_dir`.
/// Returns a default (empty) config if the file doesn't exist or can't be parsed.
pub fn load_root_config(app_data_dir: &Path) -> RootConfig {
    let config_path = app_data_dir.join("config.json");
    match std::fs::read_to_string(&config_path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => RootConfig::default(),
    }
}

/// Persist root config to `config.json` inside `app_data_dir`.
pub fn save_root_config(app_data_dir: &Path, config: &RootConfig) -> Result<(), String> {
    std::fs::create_dir_all(app_data_dir)
        .map_err(|e| format!("failed to create app data dir: {e}"))?;
    let config_path = app_data_dir.join("config.json");
    let json = crate::to_sorted_json_pretty(config)
        .map_err(|e| format!("failed to serialize config: {e}"))?;
    std::fs::write(&config_path, json).map_err(|e| format!("failed to write config: {e}"))?;
    Ok(())
}

/// Set the current root, add it to known_roots, and create the directory structure.
/// Returns the updated config.
pub fn set_root(app_data_dir: &Path, root_path: &Path) -> Result<RootConfig, String> {
    // Canonicalize to prevent duplicates from symlinks or trailing slashes.
    // Fall back to the original path if canonicalization fails (e.g. path doesn't exist yet).
    let canonical = root_path
        .canonicalize()
        .unwrap_or_else(|_| root_path.to_path_buf());
    let root_str = canonical.to_string_lossy().to_string();
    let mut config = load_root_config(app_data_dir);
    config.current_root = Some(root_str.clone());
    if !config.known_roots.contains(&root_str) {
        config.known_roots.push(root_str);
    }

    // Create sources/ and generated/ under the root
    let sources = canonical.join("sources");
    let generated = canonical.join("generated");
    std::fs::create_dir_all(&sources).map_err(|e| format!("failed to create sources dir: {e}"))?;
    std::fs::create_dir_all(&generated)
        .map_err(|e| format!("failed to create generated dir: {e}"))?;

    // Write .gitignore in generated/
    let gitignore_path = generated.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, "*\n!.gitignore\n")
            .map_err(|e| format!("failed to write .gitignore: {e}"))?;
    }

    save_root_config(app_data_dir, &config)?;
    Ok(config)
}

/// Resolve sources directory: env var > config root > app_data_dir fallback.
pub fn resolve_sources(
    env_override: Option<&str>,
    config: &RootConfig,
    app_data_dir: &Path,
) -> PathBuf {
    if let Some(custom) = env_override {
        return PathBuf::from(custom);
    }
    if let Some(ref root) = config.current_root {
        return PathBuf::from(root).join("sources");
    }
    app_data_dir.join("sources")
}

/// Resolve generated directory: env var > config root > app_data_dir fallback.
pub fn resolve_generated(
    env_override: Option<&str>,
    config: &RootConfig,
    app_data_dir: &Path,
) -> PathBuf {
    if let Some(custom) = env_override {
        return PathBuf::from(custom);
    }
    if let Some(ref root) = config.current_root {
        return PathBuf::from(root).join("generated");
    }
    app_data_dir.join("generated")
}
