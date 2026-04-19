//! Operator daemon configuration loaded from `config/rudyd.toml` by default.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    /// Address rudydae binds for the REST + SPA listener.
    ///
    /// Always plaintext HTTP. On the Pi this is `127.0.0.1:8443`, fronted by
    /// `tailscale serve` which terminates TLS using the auto-managed Tailscale
    /// Let's Encrypt cert. See `deploy/pi5/tailscale-cert.md` and ADR-0004
    /// addendum for the rationale (shrinks rudydae, kills the manual cert
    /// renewal dance).
    pub bind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebTransportConfig {
    pub bind: String,
    pub enabled: bool,
    /// PEM cert for the WebTransport (HTTP/3) listener.
    ///
    /// `tailscale serve` does not proxy HTTP/3, so the WebTransport endpoint
    /// continues to terminate TLS itself with the same Tailscale-issued cert.
    /// Required when `enabled = true`.
    #[serde(default)]
    pub cert_path: Option<PathBuf>,
    /// PEM private key paired with `cert_path`.
    #[serde(default)]
    pub key_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    pub actuator_spec: PathBuf,
    /// Path rudydae reads AND writes the live inventory from. Must live on a
    /// writable path that the daemon's user can mutate (e.g. `/var/lib/rudy/`
    /// on the Pi, where systemd `ProtectSystem=strict` permits writes via
    /// `ReadWritePaths`). Editing is via PUT endpoints (`travel_limits`,
    /// `verified`, `rename`); never hand-edit while the daemon is running.
    pub inventory: PathBuf,
    /// Optional read-only seed path. When set and `inventory` does not exist
    /// on disk at startup, rudydae copies `inventory_seed` → `inventory`
    /// once. Used on the Pi where `/opt/rudy/config/actuators/inventory.yaml`
    /// ships with the release tarball as the baseline, and `/var/lib/rudy/
    /// inventory.yaml` is the operator-mutable copy that survives upgrades.
    /// Leave unset for dev workflows where `inventory` is in-tree and edited
    /// in your editor.
    #[serde(default)]
    pub inventory_seed: Option<PathBuf>,
    pub audit_log: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanConfig {
    #[serde(default)]
    pub mock: bool,
    #[serde(default)]
    pub buses: Vec<CanBusConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanBusConfig {
    pub iface: String,
    #[serde(default = "default_bitrate")]
    pub bitrate: u32,
    /// Optional per-bus CPU pin for the dedicated I/O worker thread.
    /// When `Some(n)`, the worker calls `core_affinity::set_for_current`
    /// to pin itself to CPU `n` after spawn (Linux only; silent no-op
    /// on dev hosts).
    ///
    /// When `None`, the supervisor auto-assigns from cores `1..N`
    /// round-robin in the order `[[can.buses]]` is declared, leaving
    /// core 0 free for the kernel + tokio runtime + axum / WebTransport.
    /// On the Pi 5 (4 cores, no SMT), the auto-assignment puts one
    /// limb's bus on each of cores 1, 2, 3.
    ///
    /// Out-of-range values fall back to "unpinned" (logged at debug).
    #[serde(default)]
    pub cpu_pin: Option<usize>,
}

fn default_bitrate() -> u32 {
    1_000_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_poll_ms")]
    pub poll_interval_ms: u64,
}

fn default_poll_ms() -> u64 {
    100
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SafetyConfig {
    #[serde(default = "default_true")]
    pub require_verified: bool,

    /// Per-step angular ceiling enforced on every command path while
    /// `BootState != Homed`. Default 5 deg ~= 0.087 rad. Catches large
    /// position commands that bypass the homer (or buggy clients).
    #[serde(default = "default_boot_max_step_rad")]
    pub boot_max_step_rad: f32,

    /// Layer 6 budget. If a motor boots up further than this from the
    /// nearest band edge, auto-recovery refuses to start and the operator
    /// must move it physically. Default 90 deg ~= 1.5708 rad — chosen so
    /// "settled overnight" recoveries work but "operator clearly moved
    /// the joint by hand" does not.
    #[serde(default = "default_auto_recovery_max_rad")]
    pub auto_recovery_max_rad: f32,

    /// Margin INSIDE the band edge where auto-recovery aims to land.
    /// Avoids bouncing right on the boundary. Default 5 deg.
    #[serde(default = "default_recovery_margin_rad")]
    pub recovery_margin_rad: f32,

    /// Per-tick step size for both the slow-ramp homer and Layer 6
    /// auto-recovery. Default 0.02 rad ~= 1.1 deg.
    #[serde(default = "default_step_size_rad")]
    pub step_size_rad: f32,

    /// Tick interval for the slow-ramp loops, in milliseconds. Default 50
    /// ms; combined with `step_size_rad` gives ~22 deg/s effective speed.
    #[serde(default = "default_tick_interval_ms")]
    pub tick_interval_ms: u32,

    /// Maximum allowed `|setpoint - measured|` during a slow-ramp move.
    /// Exceeding this aborts the move (motor is bound up, or external
    /// force fighting it). Default 0.05 rad ~= 2.9 deg.
    #[serde(default = "default_tracking_error_max_rad")]
    pub tracking_error_max_rad: f32,

    /// Tolerance for "we have arrived at the target." Default 0.005 rad
    /// ~= 0.3 deg.
    #[serde(default = "default_target_tolerance_rad")]
    pub target_tolerance_rad: f32,

    /// Hard timeout on the slow-ramp loops, in milliseconds. Default 30 s.
    #[serde(default = "default_homer_timeout_ms")]
    pub homer_timeout_ms: u32,

    /// Master switch for Layer 6. With `false` the daemon never spawns
    /// auto-recovery; OutOfBand motors stay OutOfBand until manual rescue.
    /// Useful when bringing up a new joint or in paranoid environments.
    #[serde(default = "default_true")]
    pub auto_recovery_enabled: bool,

    /// Maximum tolerated age of cached telemetry, in ms, on the jog path.
    /// If `state.latest[role]` is missing or older than this, the daemon
    /// refuses the jog with `409 stale_telemetry`. This is the fail-closed
    /// half of the "Sweep travel limits" safety hole: when bus contention
    /// or backoff freezes `state.latest`, the position-projection check
    /// would otherwise approve every subsequent jog forever.
    ///
    /// Default 250 ms. The original 100 ms target matched the type-2
    /// hot-path cadence (~16 ms at 60 Hz), but on a real bus with N
    /// idle motors the type-17 fallback round-robin sits at roughly
    /// `poll_interval_ms × N + slack` per role — easily 100-200 ms when
    /// the motor isn't actively emitting type-2 frames. 250 ms absorbs
    /// that worst-case fallback gap (still ~15 missed 60 Hz frames) so
    /// the very first jog out of idle isn't a guaranteed false positive,
    /// while staying tight enough that a true mid-sweep type-2 stall
    /// fails closed within ~4 SPA tick budgets. The SPA mirror in
    /// `motion-tests-card.tsx` uses the same threshold so the client
    /// stops sending before the server refuses.
    #[serde(default = "default_max_feedback_age_ms")]
    pub max_feedback_age_ms: u64,

    /// Tolerance for the boot orchestrator's add_offset readback check.
    /// On every boot the orchestrator reads `add_offset` (0x702B) over
    /// CAN and compares it against the `commissioned_zero_offset`
    /// recorded in `inventory.yaml`; a mismatch larger than this lands
    /// the motor in `BootState::OffsetChanged` and refuses motion until
    /// the operator either re-commissions or restores. Default 1e-3 rad
    /// (~0.057°): tight enough to catch a deliberate set_zero from the
    /// bench tool, loose enough to ignore the usual firmware-side
    /// rounding when the same float survives a flash round-trip.
    #[serde(default = "default_commission_readback_tolerance_rad")]
    pub commission_readback_tolerance_rad: f32,

    /// Master switch for the boot orchestrator's auto-home flow.
    /// With `true` (the operator-confirmed default), every commissioned
    /// motor whose first valid telemetry lands `InBand` is automatically
    /// driven to its `predefined_home_rad` via the slow-ramp homer; the
    /// operator never has to click "Verify & Home" on every boot.
    /// With `false` the orchestrator never spawns an auto-home —
    /// commissioned motors then need the manual `Verify & Home` flow,
    /// exactly like uncommissioned motors. Useful as an escape hatch
    /// during a hardware investigation; the operator can flip this off
    /// and restart the daemon without losing any commissioning state.
    #[serde(default = "default_true")]
    pub auto_home_on_boot: bool,
}

fn default_true() -> bool {
    true
}

fn default_boot_max_step_rad() -> f32 {
    0.087
}

fn default_commission_readback_tolerance_rad() -> f32 {
    1e-3
}

fn default_auto_recovery_max_rad() -> f32 {
    std::f32::consts::FRAC_PI_2
}

fn default_recovery_margin_rad() -> f32 {
    0.087
}

fn default_step_size_rad() -> f32 {
    0.02
}

fn default_tick_interval_ms() -> u32 {
    50
}

fn default_tracking_error_max_rad() -> f32 {
    0.05
}

fn default_target_tolerance_rad() -> f32 {
    0.005
}

fn default_homer_timeout_ms() -> u32 {
    30_000
}

fn default_max_feedback_age_ms() -> u64 {
    250
}

/// SQLite-backed log store + runtime tracing-filter knobs.
///
/// Defaults are sized for the single-Pi deployment: ~7 days of audit + tracing
/// events at the daemon's natural cadence comfortably fits under ~50 MiB,
/// well below the SD card's wear budget, and the 250 ms / 1k-row batch
/// matches the existing telemetry tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsConfig {
    /// Where the SQLite database file lives. Parent dir is created at
    /// startup. Defaults to `.rudyd/logs.db`, next to `audit.jsonl` so all
    /// operator-state files share one backup target.
    #[serde(default = "default_logs_db_path")]
    pub db_path: PathBuf,

    /// Days of history retained. The purger task deletes anything older
    /// every `purge_interval_s` seconds.
    #[serde(default = "default_logs_retention_days")]
    pub retention_days: u32,

    /// Default `EnvFilter` directive string used when no `RUST_LOG` is set
    /// AND `.rudyd/log_filter.txt` is absent. The `Reset to default` button
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

fn default_logs_db_path() -> PathBuf {
    // Intentionally relative so dev (cwd = repo root) lands at
    // `./.rudyd/logs.db` next to `./.rudyd/audit.jsonl`. On the Pi
    // `Config::load -> normalize_paths` rewrites this to live next to the
    // (absolute) audit log so it doesn't get pinned under the read-only
    // /opt/rudy by ProtectSystem=strict. See `normalize_paths` for the why.
    PathBuf::from(".rudyd/logs.db")
}

fn default_logs_retention_days() -> u32 {
    7
}

fn default_logs_default_filter() -> String {
    "rudydae=info,tower_http=info".to_string()
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
    /// Background: `default_logs_db_path()` is `.rudyd/logs.db`. On the Pi the
    /// daemon runs with `WorkingDirectory=/opt/rudy` + `ProtectSystem=strict`,
    /// so a relative default resolves to `/opt/rudy/.rudyd/logs.db` and
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
    /// `./.rudyd/audit.jsonl`, so this is a no-op — the log DB stays at
    /// `./.rudyd/logs.db` next to it.
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
mod tests {
    use super::*;

    fn cfg_with(audit_log: &str, db_path: Option<&str>) -> Config {
        Config {
            http: HttpConfig {
                bind: "127.0.0.1:0".into(),
            },
            webtransport: WebTransportConfig {
                bind: "127.0.0.1:0".into(),
                enabled: false,
                cert_path: None,
                key_path: None,
            },
            paths: PathsConfig {
                actuator_spec: PathBuf::from("spec.yaml"),
                inventory: PathBuf::from("inv.yaml"),
                inventory_seed: None,
                audit_log: PathBuf::from(audit_log),
            },
            can: CanConfig {
                mock: true,
                buses: vec![],
            },
            telemetry: TelemetryConfig {
                poll_interval_ms: default_poll_ms(),
            },
            safety: SafetyConfig {
                require_verified: true,
                boot_max_step_rad: default_boot_max_step_rad(),
                auto_recovery_max_rad: default_auto_recovery_max_rad(),
                recovery_margin_rad: default_recovery_margin_rad(),
                step_size_rad: default_step_size_rad(),
                tick_interval_ms: default_tick_interval_ms(),
                tracking_error_max_rad: default_tracking_error_max_rad(),
                target_tolerance_rad: default_target_tolerance_rad(),
                homer_timeout_ms: default_homer_timeout_ms(),
                auto_recovery_enabled: true,
                max_feedback_age_ms: default_max_feedback_age_ms(),
                commission_readback_tolerance_rad: default_commission_readback_tolerance_rad(),
                auto_home_on_boot: true,
            },
            logs: LogsConfig {
                db_path: db_path
                    .map(PathBuf::from)
                    .unwrap_or_else(default_logs_db_path),
                ..Default::default()
            },
        }
    }

    #[test]
    fn normalize_relative_db_path_anchors_to_absolute_audit_log_parent() {
        // Pi-shaped config: absolute audit log + the relative default db_path
        // (i.e. operator never wrote a `[logs]` section). The fix must
        // re-home the SQLite DB next to the audit log so it lands on the
        // writable StateDirectory instead of the read-only release tree.
        let mut cfg = cfg_with("/var/lib/rudy/audit.jsonl", None);
        cfg.normalize_paths();
        assert_eq!(cfg.logs.db_path, PathBuf::from("/var/lib/rudy/logs.db"));
    }

    #[test]
    fn normalize_keeps_dev_relative_paths_unchanged() {
        // Dev workflow: cwd is the repo root and the audit log is
        // intentionally relative. We must not turn that into an absolute
        // path or the DB would land outside the repo.
        let mut cfg = cfg_with("./.rudyd/audit.jsonl", None);
        cfg.normalize_paths();
        assert_eq!(cfg.logs.db_path, PathBuf::from(".rudyd/logs.db"));
    }

    #[test]
    fn normalize_respects_explicit_absolute_db_path() {
        // Operator explicitly chose a different absolute db_path; we leave
        // it alone even if it doesn't share the audit log's parent.
        let mut cfg = cfg_with("/var/lib/rudy/audit.jsonl", Some("/srv/logs/rudy.db"));
        cfg.normalize_paths();
        assert_eq!(cfg.logs.db_path, PathBuf::from("/srv/logs/rudy.db"));
    }
}
