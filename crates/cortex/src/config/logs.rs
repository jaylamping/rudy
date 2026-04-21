//! SQLite-backed log store + runtime tracing-filter knobs.
//!
//! Defaults are sized for the single-Pi deployment: ~7 days of audit + tracing
//! events at the daemon's natural cadence comfortably fits under ~50 MiB,
//! well below the SD card's wear budget, and the 250 ms / 1k-row batch
//! matches the existing telemetry tick.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsConfig {
    /// Where the SQLite database file lives. Parent dir is created at
    /// startup. Defaults to `.cortex/logs.db`, next to `audit.jsonl` so all
    /// operator-state files share one backup target.
    #[serde(default = "default_logs_db_path")]
    pub db_path: PathBuf,

    /// Days of history retained. The purger task deletes anything older
    /// every `purge_interval_s` seconds.
    #[serde(default = "default_logs_retention_days")]
    pub retention_days: u32,

    /// Default `EnvFilter` directive string used when no `RUST_LOG` is set
    /// AND `.cortex/log_filter.txt` is absent. The `Reset to default` button
    /// in the UI reverts to this value.
    #[serde(default = "default_logs_default_filter")]
    pub default_filter: String,

    /// Max rows per batched insert transaction.
    #[serde(default = "default_logs_batch_max_rows")]
    pub batch_max_rows: usize,

    /// Max time to wait before flushing a partial batch, in milliseconds.
    #[serde(default = "default_logs_batch_flush_ms")]
    pub batch_flush_ms: u64,

    /// Cadence of the retention purger task, in seconds.
    #[serde(default = "default_logs_purge_interval_s")]
    pub purge_interval_s: u64,
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            db_path: default_logs_db_path(),
            retention_days: default_logs_retention_days(),
            default_filter: default_logs_default_filter(),
            batch_max_rows: default_logs_batch_max_rows(),
            batch_flush_ms: default_logs_batch_flush_ms(),
            purge_interval_s: default_logs_purge_interval_s(),
        }
    }
}

pub(crate) fn default_logs_db_path() -> PathBuf {
    // Intentionally relative so dev (cwd = repo root) lands at
    // `./.cortex/logs.db` next to `./.cortex/audit.jsonl`. On the Pi
    // `Config::load -> normalize_paths` rewrites this to live next to the
    // (absolute) audit log so it doesn't get pinned under the read-only
    // /opt/rudy by ProtectSystem=strict. See `normalize_paths` for the why.
    PathBuf::from(".cortex/logs.db")
}

fn default_logs_retention_days() -> u32 {
    7
}

fn default_logs_default_filter() -> String {
    "cortex=info,tower_http=info".to_string()
}

fn default_logs_batch_max_rows() -> usize {
    1000
}

fn default_logs_batch_flush_ms() -> u64 {
    250
}

fn default_logs_purge_interval_s() -> u64 {
    300
}
