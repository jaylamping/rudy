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
}

fn default_true() -> bool {
    true
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
