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
    pub inventory: PathBuf,
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
}

fn default_true() -> bool {
    true
}

fn default_boot_max_step_rad() -> f32 {
    0.087
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

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let contents =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config = toml::from_str(&contents)
            .with_context(|| format!("parsing TOML in {}", path.display()))?;
        Ok(cfg)
    }
}
