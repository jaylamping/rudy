//! Active hardware discovery on real SocketCAN buses.
//!
//! The scan runs in three phases per bus:
//!
//! 1. **Broadcast sweep.** One type-0 `GetDeviceId` to `motor=0xFF`, then
//!    drain replies for `broadcast_listen`. Catches every powered RobStride
//!    on the bus in one round-trip — even ones that aren't currently
//!    streaming type-2 frames (the failure mode that made the old per-id
//!    sweep miss freshly-flashed boards). Recorded responders go straight
//!    into `seen_can_ids` via `record_active_scan_seen` so they show up
//!    under `GET /api/hardware/unassigned` immediately.
//!
//! 2. **Targeted per-id probe.** For every `(bus, can_id)` in
//!    `id_min..=id_max` that the broadcast didn't already surface, run the
//!    [`DeviceProbeRegistry`]'s per-id probes (`firmware_version` then
//!    `mcu_id`, retried). This catches devices that ignore the type-0
//!    wildcard for whatever reason (firmware bug, custom build, etc.).
//!
//! 3. **Diagnostics roll-up.** [`ScanDiagnostics`] is filled with per-bus
//!    counters (broadcast hits, type-17 sends, timeouts, elapsed) and the
//!    [`HardwareScanReport::message`] field gets a one-line human summary
//!    so the SPA's "Discover" button always surfaces *why* nothing was
//!    found, not just an empty list.

use std::io;
use std::time::{Duration, Instant};

use anyhow::Result;
use driver::rs03::session;

use crate::can::discovery::{
    BusBroadcaster, BusParamReader, DeviceProbeRegistry, DiscoveredDevice, HardwareScanReport,
    RawBroadcastReply, ScanAttempt, ScanDiagnostics,
};
use crate::state::SharedState;

use super::LinuxCanCore;

/// Worker's steady-state read timeout. Re-armed on the socket after the
/// broadcast drain widens it (kept in lockstep with `worker::command::RECV_POLL_TIMEOUT`).
const WORKER_READ_TIMEOUT: Duration = Duration::from_millis(5);

/// How long to wait for broadcast replies after a single type-0 send. Long
/// enough to catch every powered RS03 on a 1 Mbit bus (each device replies
/// once and the bus latency is sub-millisecond), short enough that a click
/// of "Discover" feels instant.
const BROADCAST_LISTEN: Duration = Duration::from_millis(150);

impl BusParamReader for LinuxCanCore {
    fn read_type17_register(
        &self,
        iface: &str,
        motor_id: u8,
        index: u16,
        timeout: Duration,
    ) -> io::Result<Option<[u8; 4]>> {
        let handle = self
            .handle_for(iface)
            .map_err(|e| io::Error::other(format!("{e:#}")))?;
        handle.read_param(self.host_id, motor_id, index, timeout)
    }
}

impl BusBroadcaster for LinuxCanCore {
    fn broadcast_get_device_id(
        &self,
        iface: &str,
        total_listen: Duration,
    ) -> io::Result<Vec<RawBroadcastReply>> {
        let handle = self
            .handle_for(iface)
            .map_err(|e| io::Error::other(format!("{e:#}")))?;
        let host_id = self.host_id;
        // Hold the bus exclusively for the broadcast + drain. The worker
        // is paused for the duration (same trade-off as the bench
        // routine), then automatically resumes once the closure returns
        // and we restore the steady-state read timeout inside the
        // session helper.
        let responses = handle.with_exclusive_bus(|bus| {
            session::broadcast_device_id_scan(bus, host_id, total_listen, WORKER_READ_TIMEOUT)
        })?;
        Ok(responses
            .into_iter()
            .map(|r| RawBroadcastReply {
                motor_id: r.motor_id,
                comm_type: r.comm_type,
                data: r.data,
            })
            .collect())
    }
}

/// Active scan: broadcast → diff against `seen_can_ids` → targeted sweep.
pub(crate) fn run_hardware_scan(
    core: &LinuxCanCore,
    state: &SharedState,
    bus_filter: Option<&str>,
    mut id_min: u8,
    mut id_max: u8,
    timeout: Duration,
) -> Result<HardwareScanReport> {
    let started = Instant::now();
    if id_min > id_max {
        std::mem::swap(&mut id_min, &mut id_max);
    }
    id_min = id_min.max(1);
    id_max = id_max.min(0x7F);

    let buses: Vec<String> = if let Some(b) = bus_filter {
        if !core.cfg.can.buses.iter().any(|c| c.iface == b) {
            anyhow::bail!("bus {b:?} is not configured in [[can.buses]]");
        }
        vec![b.to_string()]
    } else {
        core.cfg.can.buses.iter().map(|c| c.iface.clone()).collect()
    };

    if buses.is_empty() {
        return Ok(HardwareScanReport {
            discovered: vec![],
            attempts: vec![],
            message: Some("no [[can.buses]] configured — nothing to scan".into()),
            diagnostics: ScanDiagnostics::default(),
        });
    }

    let registry = DeviceProbeRegistry::with_default_probes();
    let mut discovered: Vec<DiscoveredDevice> = Vec::new();
    let mut attempts: Vec<ScanAttempt> = Vec::new();
    let mut diag = ScanDiagnostics::default();

    for bus in &buses {
        diag.buses_scanned += 1;

        // Phase 1: broadcast sweep.
        let responders = registry.run_broadcasts(core, bus, BROADCAST_LISTEN);
        diag.broadcast_responses += responders.len() as u32;
        let mut already_found: std::collections::BTreeSet<u8> = std::collections::BTreeSet::new();
        for r in responders {
            already_found.insert(r.motor_id);
            let payload = serde_json::json!({
                "source": "broadcast",
                "comm_type": format!("0x{:02x}", r.comm_type),
                "raw_le_bytes": r.data.iter().map(|b| format!("0x{b:02x}")).collect::<Vec<_>>(),
            });
            state.record_active_scan_seen(
                bus,
                r.motor_id,
                r.family_hint.clone(),
                Some(payload.clone()),
            );
            discovered.push(DiscoveredDevice {
                bus: bus.clone(),
                can_id: r.motor_id,
                family_hint: r.family_hint,
                identification_payload: Some(payload),
            });
            attempts.push(ScanAttempt {
                bus: bus.clone(),
                can_id: r.motor_id,
                probe: "robstride_broadcast".to_string(),
                found: true,
                attempt: 1,
            });
        }

        // Phase 2: targeted per-id sweep over the gap. Skip ids the
        // broadcast already surfaced — they're confirmed alive and
        // hammering them again with a type-17 read just delays the
        // response to the operator.
        for can_id in id_min..=id_max {
            if already_found.contains(&can_id) {
                continue;
            }
            diag.targeted_probes_sent += 1;
            let (dev, mut row) = registry.probe_one_id(core, bus, can_id, timeout);
            attempts.append(&mut row);
            match dev {
                Some(d) => {
                    diag.targeted_probes_succeeded += 1;
                    state.record_active_scan_seen(
                        &d.bus,
                        d.can_id,
                        d.family_hint.clone(),
                        d.identification_payload.clone(),
                    );
                    discovered.push(d);
                }
                None => {
                    diag.targeted_probes_timed_out += 1;
                }
            }
        }
    }

    diag.elapsed_ms = started.elapsed().as_millis() as u64;

    let message = build_message(&buses, &discovered, &diag);

    Ok(HardwareScanReport {
        discovered,
        attempts,
        message: Some(message),
        diagnostics: diag,
    })
}

/// One-line human-readable summary appended to every scan response so the
/// operator never has to guess why a scan came back empty.
fn build_message(
    buses: &[String],
    discovered: &[DiscoveredDevice],
    diag: &ScanDiagnostics,
) -> String {
    let bus_list = buses.join(", ");
    let elapsed_s = diag.elapsed_ms as f64 / 1_000.0;
    if discovered.is_empty() {
        format!(
            "scanned {bus_list} in {elapsed_s:.2}s — broadcast got 0 replies, \
             {sent} type-17 probes timed out. Verify cable, termination, \
             bitrate, and that devices are powered.",
            sent = diag.targeted_probes_sent,
        )
    } else {
        format!(
            "scanned {bus_list} in {elapsed_s:.2}s — {n} device{s} found \
             ({bcast} via broadcast, {targeted} via targeted probe).",
            n = discovered.len(),
            s = if discovered.len() == 1 { "" } else { "s" },
            bcast = diag.broadcast_responses,
            targeted = diag.targeted_probes_succeeded,
        )
    }
}
