//! CAN bus configuration.

use serde::{Deserialize, Serialize};

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

pub fn default_bitrate() -> u32 {
    1_000_000
}
