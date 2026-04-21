//! HTTP listener configuration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    /// Address cortex binds for the REST + SPA listener.
    ///
    /// Always plaintext HTTP. On the Pi this is `127.0.0.1:8443`, fronted by
    /// `tailscale serve` which terminates TLS using the auto-managed Tailscale
    /// Let's Encrypt cert. See `deploy/pi5/tailscale-cert.md` and ADR-0004
    /// addendum for the rationale (shrinks cortex, kills the manual cert
    /// renewal dance).
    pub bind: String,
}
