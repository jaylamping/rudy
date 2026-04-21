//! WebTransport (HTTP/3) listener configuration.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

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
