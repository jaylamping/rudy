//! RX path: passive observation, type-2 telemetry, type-0x15 fault, type-17 replies.

use std::collections::HashMap;
use std::sync::Weak;

use chrono::Utc;
use driver::rs03::fault_feedback::decode_fault_dwords;
use driver::rs03::feedback::decode_motor_feedback;
use driver::rs03::frame::{comm_type_from_id, passive_observer_node_id, strip_eff_flag};
use driver::CommType;
use tracing::{debug, trace};

use crate::boot_state;
use crate::can::angle::UnwrappedAngle;
use crate::state::AppState;
use crate::types::MotorFeedback;

use super::command::{PendingEntry, PendingKey, ReplyBytes};
use super::health::BusHealth;

/// Route one SocketCAN RX frame into app state / pending read map.
pub fn route_frame(
    iface: &str,
    state: &Weak<AppState>,
    pending: &mut HashMap<PendingKey, PendingEntry>,
    health: &BusHealth,
    can_id: u32,
    data: &[u8; 8],
    dlc: usize,
) {
    health.record_rx_frame();

    if let Some(node) = passive_observer_node_id(can_id) {
        if let Some(st) = state.upgrade() {
            st.record_passive_seen(iface, node);
        }
    }

    let comm = comm_type_from_id(can_id);
    if comm == CommType::MotorFeedback as u8 {
        if dlc < 8 {
            return;
        }
        let raw = strip_eff_flag(can_id);
        let src_motor = ((raw >> 16) & 0xFF) as u8;
        match decode_motor_feedback(can_id, &data[..dlc]) {
            Ok(fb) => apply_type2(state, iface, src_motor, fb),
            Err(e) => {
                health.record_type2_decode_fail();
                debug!(iface = %iface, error = ?e, "type-2 decode failed");
            }
        }
        return;
    }

    if comm == CommType::FaultFeedback as u8 {
        if dlc < 8 {
            return;
        }
        let raw = strip_eff_flag(can_id);
        let src_motor = ((raw >> 8) & 0xFF) as u8;
        match decode_fault_dwords(&data[..dlc]) {
            Ok(dw) => {
                health.record_fault_frame();
                apply_fault_feedback(state, iface, src_motor, dw);
            }
            Err(e) => {
                debug!(iface = %iface, error = ?e, "fault-feedback decode failed");
            }
        }
        return;
    }

    if comm == CommType::ReadParam as u8 {
        if dlc < 8 {
            return;
        }
        let raw = strip_eff_flag(can_id);
        let reply_status = ((raw >> 16) & 0xFF) as u8;
        let reply_motor = ((raw >> 8) & 0xFF) as u8;
        let reply_index = u16::from_le_bytes([data[0], data[1]]);
        let key = PendingKey {
            motor_id: reply_motor,
            index: reply_index,
        };
        if let Some(entry) = pending.remove(&key) {
            let result: ReplyBytes = if reply_status == 0 {
                let mut v = [0u8; 4];
                v.copy_from_slice(&data[4..8]);
                Some(v)
            } else {
                None
            };
            let _ = entry.reply.send(Ok(result));
        }
    }
}

fn apply_type2(
    state: &Weak<AppState>,
    iface: &str,
    src_motor: u8,
    fb: driver::rs03::feedback::MotorFeedback,
) {
    let Some(state) = state.upgrade() else {
        return;
    };

    let role = {
        let inv = state.inventory.read().expect("inventory poisoned");
        let Some(dev) = inv.by_can_id(iface, src_motor) else {
            return;
        };
        dev.role().to_string()
    };

    let now_ms = Utc::now().timestamp_millis();

    let (prev_t_ms, prev_vbus, prev_torque, prev_fault, prev_warn) = {
        let guard = state.latest.read().expect("latest poisoned");
        match guard.get(&role) {
            Some(f) => (Some(f.t_ms), f.vbus_v, f.torque_nm, f.fault_sta, f.warn_sta),
            None => (None, 0.0, 0.0, 0, 0),
        }
    };

    // Type-2 arb ID status byte: vendor layout fault_bits(6) | mode(2) in bits 15..8
    // (see `tools/robstride/rs03_can.py` + ADR-0002). Merge low 6 bits into `fault_sta`;
    // keep high dword bits from last type-0x15 fault frame.
    let fault_low6 = ((fb.status_byte >> 2) & 0x3F) as u32;
    let fault_sta = (prev_fault & !0x3F) | fault_low6;

    let latest = MotorFeedback {
        t_ms: now_ms,
        role: role.clone(),
        can_id: src_motor,
        mech_pos_rad: fb.pos_rad,
        mech_vel_rad_s: fb.vel_rad_s,
        torque_nm: if fb.torque_nm != 0.0 {
            fb.torque_nm
        } else {
            prev_torque
        },
        vbus_v: prev_vbus,
        temp_c: fb.temp_c,
        fault_sta,
        warn_sta: prev_warn,
    };

    state
        .latest
        .write()
        .expect("latest poisoned")
        .insert(role.clone(), latest.clone());

    state
        .last_type2_at
        .write()
        .expect("last_type2_at poisoned")
        .insert(role.clone(), now_ms);

    let gap_ms = prev_t_ms
        .map(|prev| now_ms.saturating_sub(prev))
        .unwrap_or(-1);
    trace!(
        role = %role,
        can_id = src_motor,
        gap_ms = gap_ms,
        "type-2 frame applied"
    );

    let classify_outcome =
        boot_state::classify(&state, &role, UnwrappedAngle::new(latest.mech_pos_rad));
    crate::boot_orchestrator::spawn_if_orchestrator_qualifies(
        state.clone(),
        role.clone(),
        classify_outcome,
        false,
    );

    let _ = state.feedback_tx.send(latest);
}

fn apply_fault_feedback(
    state: &Weak<AppState>,
    iface: &str,
    src_motor: u8,
    dw: driver::rs03::fault_feedback::FaultDwords,
) {
    let Some(state) = state.upgrade() else {
        return;
    };

    let role = {
        let inv = state.inventory.read().expect("inventory poisoned");
        let Some(dev) = inv.by_can_id(iface, src_motor) else {
            return;
        };
        dev.role().to_string()
    };

    let now_ms = Utc::now().timestamp_millis();
    let mut row = state
        .latest
        .read()
        .expect("latest poisoned")
        .get(&role)
        .cloned()
        .unwrap_or_else(|| MotorFeedback {
            t_ms: now_ms,
            role: role.clone(),
            can_id: src_motor,
            mech_pos_rad: 0.0,
            mech_vel_rad_s: 0.0,
            torque_nm: 0.0,
            vbus_v: 0.0,
            temp_c: 0.0,
            fault_sta: 0,
            warn_sta: 0,
        });

    row.fault_sta = dw.fault;
    row.warn_sta = dw.warn;
    row.t_ms = now_ms;

    state
        .latest
        .write()
        .expect("latest poisoned")
        .insert(role.clone(), row.clone());

    let _ = state.feedback_tx.send(row);
}
