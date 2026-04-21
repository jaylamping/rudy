//! Operator daemon configuration loaded from `config/cortex.toml` by default.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

mod can;
mod http;
mod logs;
mod paths;
mod safety;
mod telemetry;
mod webtransport;

pub use can::{CanBusConfig, CanConfig};
pub use http::HttpConfig;
pub use logs::LogsConfig;
pub use paths::PathsConfig;
pub use safety::SafetyConfig;
pub use telemetry::TelemetryConfig;
pub use webtransport::WebTransportConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub http: HttpConfig,
    pub webtransport: WebTransportConfig,
    pub paths: PathsConfig,
    pub can: CanConfig,
    pub telemetry: TelemetryConfig,
    pub safety: SafetyConfig,
    #[serde(default)]
    pub logs: LogsConfig,
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&contents)
            .with_context(|| format!("parsing TOML in {}", path.display()))?;
        cfg.normalize_paths();
        Ok(cfg)
    }

    /// Rewrite paths that we know are unsafe at their default to live next to
    /// a path the operator/render script *did* set explicitly. Only touches
    /// `logs.db_path` today.
    ///
    /// Background: `default_logs_db_path()` is `.cortex/logs.db`. On the Pi the
    /// daemon runs with `WorkingDirectory=/opt/rudy` + `ProtectSystem=strict`,
    /// so a relative default resolves to `/opt/rudy/.cortex/logs.db` and
    /// `LogStore::open`'s `create_dir_all` blows up with EROFS — taking the
    /// HTTP listener with it (caught the hard way: `tailscale serve` returns
    /// 502, the operator console is unreachable). The render script *does*
    /// emit a `db_path = "/var/lib/rudy/logs.db"` line, but if any release
    /// ever ships without that change applied (or someone hand-edits the
    /// config) we don't want to brick the Pi.
    ///
    /// Heuristic: if `logs.db_path` is relative AND `paths.audit_log` is
    /// rooted (the operator pinned it to a known directory), anchor the log
    /// DB next to it. They're already supposed to share a backup target per
    /// the comment on `audit_log`, and the systemd unit's `ReadWritePaths`
    /// always covers that directory. On dev the audit log default is
    /// `./.cortex/audit.jsonl`, so this is a no-op — the log DB stays at
    /// `./.cortex/logs.db` next to it.
    ///
    /// We use `has_root()` rather than `is_absolute()` because production is
    /// always Linux and we only care about "did the operator give us an
    /// anchored path"; `Path::is_absolute()` on Windows would refuse a
    /// rendered config like `/var/lib/rudy/audit.jsonl` since it has no
    /// drive letter, which would silently turn the unit tests for this
    /// function into platform-conditional code.
    fn normalize_paths(&mut self) {
        if !self.logs.db_path.has_root() && self.paths.audit_log.has_root() {
            if let Some(parent) = self.paths.audit_log.parent() {
                let file_name = self
                    .logs
                    .db_path
                    .file_name()
                    .map(PathBuf::from)
                    .unwrap_or_else(|| PathBuf::from("logs.db"));
                self.logs.db_path = parent.join(file_name);
            }
        }
    }
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
