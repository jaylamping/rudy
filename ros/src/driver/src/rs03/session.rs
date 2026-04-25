//! High-level RS03 frames on a [`crate::socketcan_bus::CanBus`] (blocking).

use std::io;
use std::time::{Duration, Instant};

use super::comm_types::CommType;
use super::feedback::{decode_motor_feedback, MotorFeedback};
use super::frame::{self, passive_observer_node_id, strip_eff_flag};
use super::param_dword::{type18_payload_f32, type18_payload_u32, type18_payload_u8};
use super::params;

use crate::socketcan_bus::CanBus;

/// Default socket read timeout (matches Python `rs03_can.DEFAULT_SOCKET_TIMEOUT_S`).
pub const DEFAULT_SOCKET_TIMEOUT_S: f64 = 0.5;

pub fn send_frame(
    bus: &CanBus,
    comm: CommType,
    host_id: u8,
    motor_id: u8,
    payload: &[u8],
) -> io::Result<()> {
    if payload.len() > 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "CAN payload exceeds 8 bytes",
        ));
    }
    let mut data = [0u8; 8];
    data[..payload.len()].copy_from_slice(payload);
    let id = frame::arb_id(comm, host_id, motor_id);
    bus.send_ext(id, &data)
}

pub fn cmd_stop(bus: &CanBus, host_id: u8, motor_id: u8, clear_fault: bool) -> io::Result<()> {
    let mut d = [0u8; 8];
    d[0] = u8::from(clear_fault);
    send_frame(bus, CommType::Stop, host_id, motor_id, &d[..1])
}

/// Enter the RS03 high-speed magnetic encoder calibration mode.
///
/// Motor Studio V13 exposes this as "Encoder Calibation" /
/// `pushButtonCaliEncoder` and reports "Set the calibration mode for the
/// high-speed encoder--device". The vendor manual documents feedback mode
/// `1 = Cali mode` but omits communication type 5 between stop (4) and
/// set-zero (6); the proprietary tool uses that gap for this calibration
/// request. This is a commissioning-time command: the motor may move
/// autonomously while the firmware measures encoder offset.
pub fn cmd_calibrate_encoder(bus: &CanBus, host_id: u8, motor_id: u8) -> io::Result<()> {
    send_frame(bus, CommType::CalibrateEncoder, host_id, motor_id, &[])
}

pub fn cmd_enable(bus: &CanBus, host_id: u8, motor_id: u8) -> io::Result<()> {
    send_frame(bus, CommType::Enable, host_id, motor_id, &[])
}

pub fn cmd_set_zero(bus: &CanBus, host_id: u8, motor_id: u8) -> io::Result<()> {
    send_frame(bus, CommType::SetZero, host_id, motor_id, &[1])
}

pub fn cmd_save_params(bus: &CanBus, host_id: u8, motor_id: u8) -> io::Result<()> {
    send_frame(bus, CommType::SaveParams, host_id, motor_id, &[])
}

#[inline]
fn active_report_payload(enable: bool) -> [u8; 8] {
    // RS03 type-24 docs use byte 7 (`F_CMD`) for the switch:
    // 0x00 = disable, 0x01 = enable.
    let mut payload = [0u8; 8];
    payload[7] = u8::from(enable);
    payload
}

pub fn cmd_active_report(bus: &CanBus, host_id: u8, motor_id: u8, enable: bool) -> io::Result<()> {
    let payload = active_report_payload(enable);
    send_frame(bus, CommType::ActiveReport, host_id, motor_id, &payload)
}

pub fn write_param_f32(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    value: f32,
) -> io::Result<()> {
    let p = type18_payload_f32(index, value);
    send_frame(bus, CommType::WriteParam, host_id, motor_id, &p)
}

pub fn write_param_u8(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    value: u8,
) -> io::Result<()> {
    let p = type18_payload_u8(index, value);
    send_frame(bus, CommType::WriteParam, host_id, motor_id, &p)
}

pub fn write_param_u32(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    value: u32,
) -> io::Result<()> {
    let p = type18_payload_u32(index, value);
    send_frame(bus, CommType::WriteParam, host_id, motor_id, &p)
}

/// Classify a received extended frame as a type-17 reply.
///
/// - `None` — not the matching reply (keep listening).
/// - `Some(None)` — matching reply with read-fail status or bad status byte.
/// - `Some(Some(bytes))` — successful value dword (little-endian).
pub fn interpret_read_param_response(
    can_id: u32,
    data: &[u8; 8],
    dlc: usize,
    tx_raw: u32,
    host_id: u8,
    motor_id: u8,
    index: u16,
) -> Option<Option<[u8; 4]>> {
    let raw = strip_eff_flag(can_id);
    if raw == tx_raw {
        return None;
    }
    if frame::comm_type_from_id(can_id) != CommType::ReadParam as u8 {
        return None;
    }
    let reply_status = ((raw >> 16) & 0xFF) as u8;
    let reply_motor = ((raw >> 8) & 0xFF) as u8;
    let reply_host = (raw & 0xFF) as u8;
    if reply_motor != motor_id || reply_host != host_id {
        return None;
    }
    if dlc < 8 {
        return None;
    }
    let reply_index = u16::from_le_bytes([data[0], data[1]]);
    if reply_index != index {
        return None;
    }
    if reply_status == 1 {
        return Some(None);
    }
    if reply_status != 0 {
        return Some(None);
    }
    let mut v = [0u8; 4];
    v.copy_from_slice(&data[4..8]);
    Some(Some(v))
}

/// Raw type-17 read: returns value bytes or `None` on timeout / rejection / mismatch.
pub fn read_param_raw(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    timeout: Duration,
) -> io::Result<Option<[u8; 4]>> {
    let mut req = [0u8; 8];
    req[0..2].copy_from_slice(&index.to_le_bytes());
    send_frame(bus, CommType::ReadParam, host_id, motor_id, &req)?;

    let deadline = Instant::now() + timeout;
    let tx_raw = frame::arb_id(CommType::ReadParam, host_id, motor_id);

    while Instant::now() < deadline {
        let (can_id, data, dlc) = match bus.recv() {
            Ok(x) => x,
            Err(e)
                if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(e) => return Err(e),
        };

        if let Some(result) =
            interpret_read_param_response(can_id, &data, dlc, tx_raw, host_id, motor_id, index)
        {
            return Ok(result);
        }
    }
    Ok(None)
}

pub fn read_param_f32(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    timeout: Duration,
) -> io::Result<Option<f32>> {
    Ok(read_param_raw(bus, host_id, motor_id, index, timeout)?.map(f32::from_le_bytes))
}

pub fn read_param_u8(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    timeout: Duration,
) -> io::Result<Option<u8>> {
    Ok(read_param_raw(bus, host_id, motor_id, index, timeout)?.map(|b| b[0]))
}

pub fn read_param_u32(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    timeout: Duration,
) -> io::Result<Option<u32>> {
    Ok(read_param_raw(bus, host_id, motor_id, index, timeout)?.map(u32::from_le_bytes))
}

/// One responder seen during a [`broadcast_device_id_scan`] sweep.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BroadcastResponse {
    /// Node id (1..=127) of the responder.
    pub motor_id: u8,
    /// Comm type that carried the reply (informational; useful when the
    /// firmware answers with a `MotorFeedback` heartbeat instead of a
    /// `GetDeviceId` reply).
    pub comm_type: u8,
    /// Raw 8-byte payload from the reply frame.
    pub data: [u8; 8],
}

/// Broadcast-style discovery sweep.
///
/// Sends a single `GetDeviceId` (type-0) frame addressed to `0xFF` (the
/// RobStride broadcast slot) and then drains *every* extended frame the
/// bus delivers for `total_listen` time, returning one entry per unique
/// responder node id. Caller is responsible for whatever else they want
/// to do with those node ids (record in `seen_can_ids`, follow up with a
/// targeted type-17 read for firmware version, etc.).
///
/// `restore_read_timeout` is re-applied to the socket before returning,
/// regardless of whether we exited via the deadline or an error. The
/// per-bus worker (`crates/cortex/src/can/worker`) installs a 5 ms read
/// timeout at startup; callers running this helper while holding
/// `BusHandle::with_exclusive_bus` should pass that same value back so
/// the worker resumes its tight poll cadence as soon as the lock is
/// released.
///
/// Why not call `cmd_stop` / `read_param` per id? On a 6-DOF arm we'd
/// burn `127 * timeout` per bus before knowing whether anything was
/// alive. A single broadcast lets a quiet powered RS03 announce itself
/// in tens of milliseconds — matching the official RobStride tool's
/// behavior.
pub fn broadcast_device_id_scan(
    bus: &CanBus,
    host_id: u8,
    total_listen: Duration,
    restore_read_timeout: Duration,
) -> io::Result<Vec<BroadcastResponse>> {
    // Per ADR-0002 / RobStride manual: type-0 broadcast uses motor_id =
    // 0xFF as the wildcard recipient. The firmware replies with its real
    // node id in the high byte (bits 16..23).
    send_frame(bus, CommType::GetDeviceId, host_id, 0xFF, &[])?;

    // Use a per-recv read timeout small enough to not block past the
    // overall deadline if the bus is silent, but large enough to coalesce
    // bursty replies into one syscall.
    let _ = bus.set_read_timeout(Duration::from_millis(20));

    let deadline = Instant::now() + total_listen;
    let mut seen: std::collections::BTreeMap<u8, BroadcastResponse> =
        std::collections::BTreeMap::new();
    let mut last_err: Option<io::Error> = None;

    while Instant::now() < deadline {
        match bus.recv() {
            Ok((can_id, data, _dlc)) => {
                let Some(node) = passive_observer_node_id(can_id) else {
                    continue;
                };
                let comm = frame::comm_type_from_id(can_id);
                seen.entry(node).or_insert(BroadcastResponse {
                    motor_id: node,
                    comm_type: comm,
                    data,
                });
            }
            Err(e)
                if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock =>
            {
                continue;
            }
            Err(e) => {
                // Hold onto the last bus error but still try to restore
                // the worker's read timeout below.
                last_err = Some(e);
                break;
            }
        }
    }

    let _ = bus.set_read_timeout(restore_read_timeout);

    if let Some(e) = last_err {
        return Err(e);
    }
    Ok(seen.into_values().collect())
}

pub fn defang_motor(bus: &CanBus, host_id: u8, motor_id: u8) -> io::Result<()> {
    cmd_stop(bus, host_id, motor_id, false)?;
    write_param_u8(bus, host_id, motor_id, params::RUN_MODE, 0)?;
    write_param_f32(bus, host_id, motor_id, params::SPD_REF, 0.0)?;
    Ok(())
}

/// Non-blocking-ish drain of type-2 frames; returns latest matching feedback.
pub fn drain_motor_feedback(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    poll_timeout: Duration,
) -> io::Result<Option<MotorFeedback>> {
    let _ = bus.set_read_timeout(poll_timeout);
    let mut last: Option<MotorFeedback> = None;
    loop {
        let (can_id, data, dlc) = match bus.recv() {
            Ok(x) => x,
            Err(e)
                if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock =>
            {
                break;
            }
            Err(e) => return Err(e),
        };
        if frame::comm_type_from_id(can_id) != CommType::MotorFeedback as u8 {
            continue;
        }
        let raw = strip_eff_flag(can_id);
        let src = ((raw >> 16) & 0xFF) as u8;
        let dst = (raw & 0xFF) as u8;
        if src != motor_id || dst != host_id {
            continue;
        }
        if dlc < 8 {
            continue;
        }
        if let Ok(fb) = decode_motor_feedback(can_id, &data[..dlc]) {
            last = Some(fb);
        }
    }
    Ok(last)
}

#[cfg(test)]
mod tests {
    use super::frame::arb_id;
    use super::*;

    #[test]
    fn read_param_reply_status_fail_yields_some_none() {
        let host = 0xFDu8;
        let motor = 0x08u8;
        let index = 0x3016u16;
        let tx = arb_id(CommType::ReadParam, host, motor);
        // Reply layout: [0x11][status][motor][host]; status=1 => read failed (0x70xx only rule).
        let reply_id = 0x1101_08FDu32;
        let mut data = [0u8; 8];
        data[0..2].copy_from_slice(&index.to_le_bytes());
        let r = interpret_read_param_response(reply_id, &data, 8, tx, host, motor, index);
        assert_eq!(r, Some(None));
    }

    #[test]
    fn read_param_reply_ok_yields_value_bytes() {
        let host = 0xFDu8;
        let motor = 0x08u8;
        let index = 0x7019u16;
        let tx = arb_id(CommType::ReadParam, host, motor);
        let reply_id = 0x1100_08FDu32;
        let mut data = [0u8; 8];
        data[0..2].copy_from_slice(&index.to_le_bytes());
        data[4..8].copy_from_slice(&1.25f32.to_le_bytes());
        let r = interpret_read_param_response(reply_id, &data, 8, tx, host, motor, index);
        assert_eq!(r, Some(Some(1.25f32.to_le_bytes())));
    }

    #[test]
    fn active_report_frame_uses_type24_comm_bits() {
        let raw_id = arb_id(CommType::ActiveReport, 0xFD, 0x08);
        assert_eq!(raw_id, 0x1800_FD08);
    }

    #[test]
    fn active_report_payload_sets_f_cmd_in_byte7() {
        assert_eq!(active_report_payload(false), [0, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(active_report_payload(true), [0, 0, 0, 0, 0, 0, 0, 1]);
    }
}
