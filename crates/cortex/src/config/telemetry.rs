//! Host / process telemetry polling configuration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    #[serde(default = "default_poll_ms")]
    pub poll_interval_ms: u64,
}

pub(crate) fn default_poll_ms() -> u64 {
    100
}
