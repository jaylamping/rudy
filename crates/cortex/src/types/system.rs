//! Host metrics snapshot types (`GET /api/system`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// GET /api/system - host metrics for the operator-console dashboard.
///
/// Linux real values come from `/proc` + `/sys` + (on the Pi) `vcgencmd`;
/// when `cfg.can.mock == true` or running on non-Linux, fields are
/// slowly-varying mock numbers and `is_mock = true`. See `system.rs`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SystemSnapshot {
    /// Wallclock at sample time, ms since unix epoch.
    pub t_ms: i64,
    pub cpu_pct: f32,
    /// 1, 5, 15-minute load average from `/proc/loadavg`.
    pub load: [f32; 3],
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub temps_c: SystemTemps,
    pub throttled: SystemThrottled,
    /// Host/Pi uptime from `/proc/uptime`.
    pub uptime_s: u64,
    /// Cortex daemon uptime since this process started.
    pub cortex_uptime_s: u64,
    pub hostname: String,
    pub kernel: String,
    /// True when values are synthetic (no Linux host or `cfg.can.mock = true`).
    pub is_mock: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SystemTemps {
    pub cpu: Option<f32>,
    pub gpu: Option<f32>,
}

/// Pi-specific power/thermal throttling state. `now` and `ever` are derived
/// from `vcgencmd get_throttled` bits (0/2 -> now, 16/18 -> ever).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SystemThrottled {
    pub now: bool,
    pub ever: bool,
    pub raw_hex: Option<String>,
}
