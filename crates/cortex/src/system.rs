//! Host-system metrics for the operator console dashboard.
//!
//! On Linux, real values are read from `/proc` and `/sys`:
//!   - CPU pct: derived from successive `/proc/stat` reads cached on the
//!     `SystemPoller`.
//!   - Memory: `/proc/meminfo` (MemTotal, MemAvailable).
//!   - Temperatures: `/sys/class/thermal/thermal_zone*/`. The first zone
//!     of `type=cpu-thermal` (Pi 5) wins for CPU; GPU is `vcgencmd
//!     measure_temp gpu` if available, else `None`.
//!   - Throttled: `vcgencmd get_throttled` parsed into now / ever bits;
//!     gracefully `None` on non-Pi.
//!   - Uptime + load: `/proc/uptime` + `/proc/loadavg`.
//!
//! On non-Linux (Windows dev) and whenever `cfg.can.mock == true`, the
//! poller emits slowly-varying mock numbers seeded by wallclock so the
//! dashboard animates without hardware. Same pattern motor telemetry uses
//! (see `crates/cortex/src/can/mock.rs`).

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tracing::{debug, info};

use crate::state::SharedState;
use crate::types::{SystemSnapshot, SystemTemps, SystemThrottled};

/// Cadence for the periodic system-snapshot broadcaster. Two seconds matches
/// the SPA's previous REST poll cadence and keeps the dashboard-side render
/// load negligible (one render every 2 s × handful of widgets). Cheap to
/// adjust — the wire shape doesn't care.
const BROADCAST_PERIOD: Duration = Duration::from_secs(2);

/// Spawn the periodic system-snapshot broadcaster.
///
/// Reads `state.system` (the existing poller used by `GET /api/system`) on a
/// fixed cadence and publishes each snapshot to `state.system_tx` so the WT
/// listener can fan it out as `WtFrame::SystemSnapshot` datagrams. We hold the
/// poller mutex only while sampling — never across the broadcast — so the
/// REST endpoint can still serve concurrent reads without contention.
pub fn spawn(state: SharedState) {
    info!(
        period_ms = BROADCAST_PERIOD.as_millis() as u64,
        "system-snapshot broadcaster spawned"
    );
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(BROADCAST_PERIOD);
        // Don't burst-fire if a tick gets missed under load — a stalled host
        // doesn't benefit from a thundering herd of catch-up snapshots.
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let snap = {
                let mut poller = state.system.lock().expect("system poller poisoned");
                poller.snapshot(state.cfg.can.mock)
            };
            if let Err(e) = state.system_tx.send(snap) {
                // SendError just means there are no subscribers — fine.
                debug!("system_tx send (no subscribers): {e}");
            }
        }
    });
}

/// Maintains state needed to compute deltas (CPU pct).
#[derive(Debug, Default)]
pub struct SystemPoller {
    // Read on Linux to compute CPU pct deltas; harmless dead state elsewhere.
    #[allow(dead_code)]
    last_cpu: Option<CpuTotals>,
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
struct CpuTotals {
    idle: u64,
    total: u64,
}

impl SystemPoller {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&mut self, mock: bool) -> SystemSnapshot {
        if mock || !cfg!(target_os = "linux") {
            return mock_snapshot();
        }
        #[cfg(target_os = "linux")]
        {
            linux::read_snapshot(self).unwrap_or_else(|_| mock_snapshot())
        }
        #[cfg(not(target_os = "linux"))]
        {
            mock_snapshot()
        }
    }
}

fn mock_snapshot() -> SystemSnapshot {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    // Slowly-varying mock values so the dashboard isn't a frozen card.
    let osc = (secs * 0.1).sin();
    let cpu = 18.0 + 14.0 * osc.abs();
    let cpu_temp = 47.0 + 6.0 * osc;
    let gpu_temp = 44.0 + 5.0 * (secs * 0.07).cos();
    SystemSnapshot {
        t_ms: now_ms(),
        cpu_pct: cpu as f32,
        load: [0.4 + 0.2 * osc.abs() as f32, 0.5, 0.6],
        mem_used_mb: 1850 + (200.0 * osc.abs()) as u64,
        mem_total_mb: 8192,
        temps_c: SystemTemps {
            cpu: Some(cpu_temp as f32),
            gpu: Some(gpu_temp as f32),
        },
        throttled: SystemThrottled {
            now: false,
            ever: false,
            raw_hex: None,
        },
        uptime_s: 3600 * 12 + (secs as u64 % 600),
        hostname: hostname_or("rudy-dev"),
        kernel: "mock".to_string(),
        is_mock: true,
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn hostname_or(default: &str) -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_else(|| default.to_string())
}

#[cfg(target_os = "linux")]
mod linux {
    use std::process::Command;

    use super::{hostname_or, now_ms, CpuTotals, SystemPoller};
    use crate::types::{SystemSnapshot, SystemTemps, SystemThrottled};

    pub fn read_snapshot(poller: &mut SystemPoller) -> std::io::Result<SystemSnapshot> {
        let cpu_now = read_cpu_totals()?;
        let cpu_pct = match poller.last_cpu {
            Some(prev) if cpu_now.total > prev.total => {
                let dt = cpu_now.total - prev.total;
                let di = cpu_now.idle.saturating_sub(prev.idle);
                let busy = (dt - di) as f32 / dt as f32;
                (busy * 100.0).clamp(0.0, 100.0)
            }
            _ => 0.0,
        };
        poller.last_cpu = Some(cpu_now);

        let (mem_total_mb, mem_used_mb) = read_meminfo()?;
        let temps = read_temps();
        let throttled = read_throttled();
        let uptime_s = read_uptime();
        let load = read_loadavg();
        let kernel = read_kernel();

        Ok(SystemSnapshot {
            t_ms: now_ms(),
            cpu_pct,
            load,
            mem_used_mb,
            mem_total_mb,
            temps_c: temps,
            throttled,
            uptime_s,
            hostname: hostname_or("rudy"),
            kernel,
            is_mock: false,
        })
    }

    fn read_cpu_totals() -> std::io::Result<CpuTotals> {
        let s = std::fs::read_to_string("/proc/stat")?;
        let line = s
            .lines()
            .next()
            .ok_or_else(|| std::io::Error::other("empty /proc/stat"))?;
        let cols: Vec<u64> = line
            .split_whitespace()
            .skip(1)
            .filter_map(|c| c.parse().ok())
            .collect();
        if cols.len() < 5 {
            return Err(std::io::Error::other("short /proc/stat cpu line"));
        }
        let idle = cols[3] + cols.get(4).copied().unwrap_or(0); // idle + iowait
        let total: u64 = cols.iter().sum();
        Ok(CpuTotals { idle, total })
    }

    fn read_meminfo() -> std::io::Result<(u64, u64)> {
        let s = std::fs::read_to_string("/proc/meminfo")?;
        let mut total_kb: Option<u64> = None;
        let mut avail_kb: Option<u64> = None;
        for l in s.lines() {
            if let Some(rest) = l.strip_prefix("MemTotal:") {
                total_kb = parse_meminfo_kb(rest);
            } else if let Some(rest) = l.strip_prefix("MemAvailable:") {
                avail_kb = parse_meminfo_kb(rest);
            }
        }
        let total_kb = total_kb.ok_or_else(|| std::io::Error::other("no MemTotal"))?;
        let avail_kb = avail_kb.unwrap_or(0);
        let total_mb = total_kb / 1024;
        let used_mb = total_kb.saturating_sub(avail_kb) / 1024;
        Ok((total_mb, used_mb))
    }

    fn parse_meminfo_kb(rest: &str) -> Option<u64> {
        rest.split_whitespace().next()?.parse().ok()
    }

    fn read_temps() -> SystemTemps {
        let cpu = read_thermal_zone("cpu-thermal").or_else(|| read_thermal_zone("cpu_thermal"));
        let gpu = read_vcgencmd_temp("gpu").or_else(|| read_thermal_zone("gpu-thermal"));
        SystemTemps { cpu, gpu }
    }

    fn read_thermal_zone(want_type: &str) -> Option<f32> {
        let entries = std::fs::read_dir("/sys/class/thermal").ok()?;
        for e in entries.flatten() {
            let p = e.path();
            if !p.file_name()?.to_string_lossy().starts_with("thermal_zone") {
                continue;
            }
            let t = std::fs::read_to_string(p.join("type"))
                .ok()
                .map(|s| s.trim().to_string());
            if t.as_deref() == Some(want_type) {
                if let Ok(s) = std::fs::read_to_string(p.join("temp")) {
                    if let Ok(milli) = s.trim().parse::<i32>() {
                        return Some(milli as f32 / 1000.0);
                    }
                }
            }
        }
        None
    }

    fn read_vcgencmd_temp(target: &str) -> Option<f32> {
        let out = Command::new("vcgencmd")
            .args(["measure_temp", target])
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout);
        let s = s.trim().strip_prefix("temp=")?;
        let n = s.trim_end_matches("'C").trim_end_matches('C');
        n.parse().ok()
    }

    fn read_throttled() -> SystemThrottled {
        let Some(out) = Command::new("vcgencmd")
            .arg("get_throttled")
            .output()
            .ok()
            .filter(|o| o.status.success())
        else {
            return SystemThrottled {
                now: false,
                ever: false,
                raw_hex: None,
            };
        };
        let s = String::from_utf8_lossy(&out.stdout);
        // Format: "throttled=0x50000"
        let hex = s
            .trim()
            .strip_prefix("throttled=0x")
            .unwrap_or("")
            .to_string();
        let bits = u64::from_str_radix(&hex, 16).unwrap_or(0);
        // Bits: 0=under-volt now, 2=throttled now, 16=under-volt ever, 18=throttled ever.
        let now = bits & ((1 << 0) | (1 << 2)) != 0;
        let ever = bits & ((1 << 16) | (1 << 18)) != 0;
        SystemThrottled {
            now,
            ever,
            raw_hex: Some(format!("0x{:x}", bits)),
        }
    }

    fn read_uptime() -> u64 {
        std::fs::read_to_string("/proc/uptime")
            .ok()
            .and_then(|s| s.split_whitespace().next().map(str::to_string))
            .and_then(|s| s.parse::<f64>().ok())
            .map(|f| f as u64)
            .unwrap_or(0)
    }

    fn read_loadavg() -> [f32; 3] {
        let s = std::fs::read_to_string("/proc/loadavg").unwrap_or_default();
        let cols: Vec<f32> = s
            .split_whitespace()
            .take(3)
            .filter_map(|c| c.parse().ok())
            .collect();
        [
            *cols.first().unwrap_or(&0.0),
            *cols.get(1).unwrap_or(&0.0),
            *cols.get(2).unwrap_or(&0.0),
        ]
    }

    fn read_kernel() -> String {
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string())
    }
}
