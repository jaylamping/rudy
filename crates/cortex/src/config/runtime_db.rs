//! SQLite-backed runtime state (safety/telemetry, optional inventory).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeDbConfig {
    /// If false, `cortex` uses only `cortex.toml` for safety/telemetry and YAML for inventory
    /// (useful for minimal tests; production keeps this true on the Pi).
    #[serde(default = "default_runtime_enabled")]
    pub enabled: bool,
    /// Single SQLite file for `settings_kv`, `meta`, and optional `inventory_doc`.
    #[serde(default = "default_runtime_db_path")]
    pub db_path: PathBuf,
    /// When true, any inventory change persisted to the DB is also written to
    /// `paths.inventory` as a best-effort mirror for diffs/backup.
    #[serde(default = "default_true")]
    pub inventory_yaml_mirror: bool,
}

impl Default for RuntimeDbConfig {
    fn default() -> Self {
        Self {
            enabled: default_runtime_enabled(),
            db_path: default_runtime_db_path(),
            inventory_yaml_mirror: default_true(),
        }
    }
}

fn default_runtime_enabled() -> bool {
    // Tests build `Config` without `[runtime]`; opt in via `cortex.toml` on the Pi.
    false
}

fn default_true() -> bool {
    true
}

pub(crate) fn default_runtime_db_path() -> PathBuf {
    PathBuf::from(".cortex/runtime.db")
}
