//! Active device discovery: probe registry, broadcast sweeps, and per-id probes.
//!
//! Passive CAN IDs are tracked in [`crate::state::AppState::seen_can_ids`] by the
//! per-bus worker. This module drives the **active** discovery side used by
//! `POST /api/hardware/scan`:
//!
//! 1. **Broadcast sweep** ([`BusBroadcastProbe`]) — sends one type-0
//!    `GetDeviceId` to the RobStride wildcard slot (`motor=0xFF`) and drains the
//!    bus, recording every responder. This is the cheap, parallel "who's out
//!    there?" query and finds powered devices that aren't streaming type-2.
//! 2. **Per-id probe** ([`DeviceProbe`]) — for IDs the broadcast didn't surface
//!    (or for IDs we want extra metadata about), send a targeted type-17 read
//!    of `firmware_version` (and a fallback to `MCU_ID` on a second pass). Each
//!    probe is retried so a dropped frame doesn't render a real device invisible.
//!
//! All probes return [`DiscoveredDevice`] entries which the caller folds into
//! `seen_can_ids` and the scan response.

use std::io;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// One device that responded to a probe during a scan run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredDevice {
    pub bus: String,
    pub can_id: u8,
    pub family_hint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identification_payload: Option<serde_json::Value>,
}

/// One probe attempt at a single `(bus, can_id)`. `attempt` is the 1-indexed
/// retry count for that probe (1 = first try).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanAttempt {
    pub bus: String,
    pub can_id: u8,
    pub probe: String,
    pub found: bool,
    #[serde(default = "default_attempt_index")]
    pub attempt: u8,
}

fn default_attempt_index() -> u8 {
    1
}

/// Aggregate counters from one scan run, surfaced to the operator so an empty
/// `discovered` list is self-explanatory ("we sent 127 reads, all timed out"
/// vs "we never opened the bus").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanDiagnostics {
    pub buses_scanned: u32,
    pub broadcast_responses: u32,
    pub targeted_probes_sent: u32,
    pub targeted_probes_succeeded: u32,
    pub targeted_probes_timed_out: u32,
    pub elapsed_ms: u64,
}

/// Result bundle for `can::hardware_active_scan`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HardwareScanReport {
    pub discovered: Vec<DiscoveredDevice>,
    pub attempts: Vec<ScanAttempt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default)]
    pub diagnostics: ScanDiagnostics,
}

impl HardwareScanReport {
    pub fn empty(message: impl Into<String>) -> Self {
        Self {
            discovered: vec![],
            attempts: vec![],
            message: Some(message.into()),
            diagnostics: ScanDiagnostics::default(),
        }
    }
}

/// Minimal read primitive for per-id probes (implemented by
/// [`crate::can::handle::LinuxCanCore`]).
pub trait BusParamReader: Send + Sync {
    fn read_type17_register(
        &self,
        iface: &str,
        motor_id: u8,
        index: u16,
        timeout: Duration,
    ) -> io::Result<Option<[u8; 4]>>;
}

/// One responder seen by a broadcast sweep. Mirrors
/// `driver::rs03::session::BroadcastResponse` with a little extra context
/// for the registry layer (which bus it came from).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroadcastResponder {
    pub bus: String,
    pub motor_id: u8,
    pub family_hint: String,
    pub comm_type: u8,
    pub data: [u8; 8],
}

/// A bus-level "ping everyone, listen for anyone" probe. Implementations
/// own the actual frame TX (e.g. [`RobstrideBroadcastProbe`] hits
/// `driver::rs03::session::broadcast_device_id_scan`).
pub trait BusBroadcastProbe: Send + Sync {
    fn family_name(&self) -> &'static str;

    /// Run the broadcast on `iface` and return every responder seen
    /// during `total_listen`. Implementations should not retry — the
    /// orchestrator decides whether a second sweep is worthwhile.
    fn sweep(
        &self,
        broadcaster: &dyn BusBroadcaster,
        iface: &str,
        total_listen: Duration,
    ) -> io::Result<Vec<BroadcastResponder>>;
}

/// Bus access surface used by [`BusBroadcastProbe`] implementations.
/// Decoupled from the per-id [`BusParamReader`] so a future probe (e.g.
/// a CANopen ENMT NMT scan) can have its own narrower trait without
/// forcing every reader to grow new methods.
pub trait BusBroadcaster: Send + Sync {
    /// Send a type-0 `GetDeviceId` broadcast and drain replies for
    /// `total_listen`. Implementations are expected to delegate to
    /// `driver::rs03::session::broadcast_device_id_scan` (linux) or
    /// return an empty vec (mock / non-linux).
    fn broadcast_get_device_id(
        &self,
        iface: &str,
        total_listen: Duration,
    ) -> io::Result<Vec<RawBroadcastReply>>;
}

/// Raw broadcast reply, family-agnostic (the broadcaster trait can't know
/// which family the reply is for; the [`BusBroadcastProbe`] tags it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawBroadcastReply {
    pub motor_id: u8,
    pub comm_type: u8,
    pub data: [u8; 8],
}

/// A family-specific per-id probe. Probes run in registration order; the
/// first `Some(_)` wins for that `(bus, can_id)`.
pub trait DeviceProbe: Send + Sync {
    fn family_name(&self) -> &'static str;

    fn probe(
        &self,
        reader: &dyn BusParamReader,
        bus: &str,
        can_id: u8,
        timeout: Duration,
    ) -> Option<DiscoveredDevice>;
}

/// RobStride broadcast probe — wraps `session::broadcast_device_id_scan`.
/// Tags every responder as `family_hint = "robstride"` since the type-0
/// broadcast is RobStride-specific.
#[derive(Debug, Clone, Copy, Default)]
pub struct RobstrideBroadcastProbe;

impl BusBroadcastProbe for RobstrideBroadcastProbe {
    fn family_name(&self) -> &'static str {
        "robstride"
    }

    fn sweep(
        &self,
        broadcaster: &dyn BusBroadcaster,
        iface: &str,
        total_listen: Duration,
    ) -> io::Result<Vec<BroadcastResponder>> {
        let raw = broadcaster.broadcast_get_device_id(iface, total_listen)?;
        Ok(raw
            .into_iter()
            .map(|r| BroadcastResponder {
                bus: iface.to_string(),
                motor_id: r.motor_id,
                family_hint: self.family_name().to_string(),
                comm_type: r.comm_type,
                data: r.data,
            })
            .collect())
    }
}

/// RobStride per-id probe: type-17 read of `firmware_version` (0x1003
/// per RS03 spec), with a `mcu_id` (0x7005) fallback. Any `Ok(_)` reply
/// (including a firmware read-fail status reply) counts as presence —
/// only an honest timeout means "nobody home".
#[derive(Debug, Clone, Copy)]
pub struct RobstrideProbe {
    pub firmware_version_index: u16,
    pub mcu_id_index: u16,
    /// Number of times to re-send the type-17 request before declaring
    /// the id absent. `1` = no retry; `2` = one retry; etc. Defaults to
    /// `2` so a single dropped frame on a busy bus doesn't shadow a real
    /// device.
    pub max_attempts: u8,
}

impl Default for RobstrideProbe {
    fn default() -> Self {
        Self {
            firmware_version_index: 0x1003,
            mcu_id_index: 0x7005,
            max_attempts: 2,
        }
    }
}

impl DeviceProbe for RobstrideProbe {
    fn family_name(&self) -> &'static str {
        "robstride"
    }

    fn probe(
        &self,
        reader: &dyn BusParamReader,
        bus: &str,
        can_id: u8,
        timeout: Duration,
    ) -> Option<DiscoveredDevice> {
        // Try `firmware_version` first; on timeout, fall back to
        // `mcu_id`. Each register gets `max_attempts` tries.
        for &(label, index) in &[
            ("firmware_version", self.firmware_version_index),
            ("mcu_id", self.mcu_id_index),
        ] {
            for _ in 0..self.max_attempts.max(1) {
                match reader.read_type17_register(bus, can_id, index, timeout) {
                    Ok(reply) => {
                        return Some(DiscoveredDevice {
                            bus: bus.to_string(),
                            can_id,
                            family_hint: self.family_name().to_string(),
                            identification_payload: Some(payload_for(label, index, reply)),
                        });
                    }
                    Err(_) => continue,
                }
            }
        }
        None
    }
}

fn payload_for(label: &str, index: u16, reply: Option<[u8; 4]>) -> serde_json::Value {
    let index_str = format!("0x{index:04x}");
    match reply {
        Some(bytes) => {
            let snippet = String::from_utf8_lossy(&bytes)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            // Show whichever label this register represents so callers can
            // tell at a glance whether they're looking at a firmware string
            // or an MCU UID dword.
            let snippet_key = format!("{label}_snippet");
            serde_json::json!({
                "param_index": index_str,
                "param_name": label,
                snippet_key: snippet,
                "raw_le_bytes": bytes.iter().map(|b| format!("0x{b:02x}")).collect::<Vec<_>>(),
            })
        }
        None => serde_json::json!({
            "param_index": index_str,
            "param_name": label,
            "read_status": "fail",
        }),
    }
}

/// Ordered list of probes used during a scan.
pub struct DeviceProbeRegistry {
    broadcasts: Vec<Box<dyn BusBroadcastProbe>>,
    per_id: Vec<Box<dyn DeviceProbe>>,
}

impl DeviceProbeRegistry {
    pub fn with_default_probes() -> Self {
        Self {
            broadcasts: vec![Box::new(RobstrideBroadcastProbe)],
            per_id: vec![Box::new(RobstrideProbe::default())],
        }
    }

    pub fn broadcasts(&self) -> &[Box<dyn BusBroadcastProbe>] {
        &self.broadcasts
    }

    /// Run every registered broadcast probe on `iface` and return their
    /// merged, deduplicated responder set. The first probe that claims a
    /// `(bus, motor_id)` wins; later probes' duplicates are dropped so we
    /// don't fight over the same device's family hint.
    pub fn run_broadcasts(
        &self,
        broadcaster: &dyn BusBroadcaster,
        iface: &str,
        total_listen: Duration,
    ) -> Vec<BroadcastResponder> {
        let mut by_id: std::collections::BTreeMap<u8, BroadcastResponder> =
            std::collections::BTreeMap::new();
        for probe in &self.broadcasts {
            match probe.sweep(broadcaster, iface, total_listen) {
                Ok(responders) => {
                    for r in responders {
                        by_id.entry(r.motor_id).or_insert(r);
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        iface = %iface,
                        family = %probe.family_name(),
                        error = ?e,
                        "broadcast probe failed",
                    );
                }
            }
        }
        by_id.into_values().collect()
    }

    /// Returns `(discovered, attempts)` for this `(bus, can_id)`. Stops at
    /// the first probe that returns `Some`. Attempts are reported per
    /// probe (one row per probe regardless of internal retry behavior).
    pub fn probe_one_id(
        &self,
        reader: &dyn BusParamReader,
        bus: &str,
        can_id: u8,
        timeout: Duration,
    ) -> (Option<DiscoveredDevice>, Vec<ScanAttempt>) {
        let mut attempts = Vec::new();
        for p in &self.per_id {
            let probe_name = p.family_name().to_string();
            if let Some(dev) = p.probe(reader, bus, can_id, timeout) {
                attempts.push(ScanAttempt {
                    bus: bus.to_string(),
                    can_id,
                    probe: probe_name,
                    found: true,
                    attempt: 1,
                });
                return (Some(dev), attempts);
            }
            attempts.push(ScanAttempt {
                bus: bus.to_string(),
                can_id,
                probe: probe_name,
                found: false,
                attempt: 1,
            });
        }
        (None, attempts)
    }
}

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod tests;
