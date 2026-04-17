//! High-level RS03 frames on a [`crate::socketcan_bus::CanBus`] (blocking).

use std::io;
use std::time::{Duration, Instant};

use super::comm_types::CommType;
use super::feedback::{decode_motor_feedback, MotorFeedback};
use super::frame::{self, strip_eff_flag};
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

pub fn cmd_enable(bus: &CanBus, host_id: u8, motor_id: u8) -> io::Result<()> {
    send_frame(bus, CommType::Enable, host_id, motor_id, &[])
}

pub fn cmd_set_zero(bus: &CanBus, host_id: u8, motor_id: u8) -> io::Result<()> {
    send_frame(bus, CommType::SetZero, host_id, motor_id, &[1])
}

pub fn cmd_save_params(bus: &CanBus, host_id: u8, motor_id: u8) -> io::Result<()> {
    send_frame(bus, CommType::SaveParams, host_id, motor_id, &[])
}

pub fn write_param_f32(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    value: f32,
) -> io::Result<()> {
    let mut p = [0u8; 8];
    p[0..2].copy_from_slice(&index.to_le_bytes());
    p[4..8].copy_from_slice(&value.to_le_bytes());
    send_frame(bus, CommType::WriteParam, host_id, motor_id, &p)
}

pub fn write_param_u8(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    value: u8,
) -> io::Result<()> {
    let mut p = [0u8; 8];
    p[0..2].copy_from_slice(&index.to_le_bytes());
    p[4] = value;
    send_frame(bus, CommType::WriteParam, host_id, motor_id, &p)
}

pub fn write_param_u32(
    bus: &CanBus,
    host_id: u8,
    motor_id: u8,
    index: u16,
    value: u32,
) -> io::Result<()> {
    let mut p = [0u8; 8];
    p[0..2].copy_from_slice(&index.to_le_bytes());
    p[4..8].copy_from_slice(&value.to_le_bytes());
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
                if e.kind() == io::ErrorKind::TimedOut
                    || e.kind() == io::ErrorKind::WouldBlock =>
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
    Ok(read_param_raw(bus, host_id, motor_id, index, timeout)?.map(|b| f32::from_le_bytes(b)))
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
    Ok(read_param_raw(bus, host_id, motor_id, index, timeout)?.map(|b| u32::from_le_bytes(b)))
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
                if e.kind() == io::ErrorKind::TimedOut
                    || e.kind() == io::ErrorKind::WouldBlock =>
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
    use super::*;
    use super::frame::arb_id;

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
}
