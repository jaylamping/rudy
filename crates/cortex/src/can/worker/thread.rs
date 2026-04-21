//! Worker thread entrypoints and recv / command dispatch.

use std::collections::HashMap;
use std::io;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result};
use chrono::Utc;
use driver::rs03::feedback::{decode_motor_feedback, MotorFeedback as DriverFeedback};
use driver::rs03::frame::{comm_type_from_id, passive_observer_node_id, strip_eff_flag};
use driver::rs03::session;
use driver::{CanBus, Rs03, RsActuator};
use tokio::runtime::Handle;
use tracing::{debug, info, warn};

use crate::boot_state;
use crate::types::MotorFeedback;

use super::command::{
    Cmd, PendingEntry, PendingKey, ReplyBytes, WriteValue, CMD_DRAIN_BATCH, RECV_POLL_TIMEOUT,
    REPLY_TIMEOUT,
};
use super::handle::BusHandle;
use super::pin::pin_to_cpu;

/// Spawn a worker thread for `bus`, returning a [`BusHandle`].
///
/// `state` is held weakly by the worker so we never keep `AppState`
/// alive past shutdown. `cpu_pin` (when `Some`) names the CPU id the
/// worker should pin itself to; `None` means the supervisor has no
/// preference and the OS scheduler picks. Pinning failure is
/// non-fatal (logged + ignored).
///
/// Must be called from inside a tokio runtime context: the worker
/// captures `Handle::current()` so async work spawned from the type-2
/// path (e.g. boot orchestrator) can reach the runtime.
pub fn spawn(
    iface: String,
    bus: CanBus,
    state: Weak<crate::state::AppState>,
    cpu_pin: Option<usize>,
) -> Result<BusHandle> {
    bus.set_read_timeout(RECV_POLL_TIMEOUT)
        .with_context(|| format!("setting read timeout on {iface}"))?;

    let runtime_handle = Handle::try_current()
        .context("worker::spawn must be called inside a tokio runtime context")?;

    let (tx, rx) = mpsc::channel::<Cmd>();
    let bus_arc = Arc::new(Mutex::new(bus));
    let bus_for_thread = Arc::clone(&bus_arc);
    let iface_for_thread = iface.clone();
    let iface_for_handle = iface.clone();

    thread::Builder::new()
        .name(format!("rudy-can-{iface}"))
        .spawn(move || {
            // Establish a tokio runtime context for this OS thread so
            // anything in the worker that calls `tokio::spawn` (boot
            // orchestrator, broadcast `feedback_tx.send`, etc.) finds a
            // runtime.
            let _guard = runtime_handle.enter();
            if let Some(cpu) = cpu_pin {
                pin_to_cpu(&iface_for_thread, cpu);
            }
            run_worker(iface_for_thread, bus_for_thread, rx, state);
        })
        .with_context(|| format!("spawning worker thread for {iface}"))?;

    Ok(BusHandle::new(iface_for_handle, tx, bus_arc))
}

/// Inner per-thread main loop.
fn run_worker(
    iface: String,
    bus: Arc<Mutex<CanBus>>,
    cmd_rx: Receiver<Cmd>,
    state: Weak<crate::state::AppState>,
) {
    info!(iface = %iface, "bus worker started");
    let mut pending: HashMap<PendingKey, PendingEntry> = HashMap::new();
    let mut backlog_drained: u64 = 0;

    'outer: loop {
        let recv_result = {
            let guard = bus.lock().expect("bus mutex poisoned");
            guard.recv()
        };
        match recv_result {
            Ok((can_id, data, dlc)) => {
                handle_frame(&iface, &state, &mut pending, can_id, &data, dlc);
            }
            Err(e)
                if e.kind() == io::ErrorKind::TimedOut || e.kind() == io::ErrorKind::WouldBlock => {
            }
            Err(e) => {
                warn!(iface = %iface, error = ?e, "bus recv failed");
            }
        }

        let now = Instant::now();
        pending.retain(|key, entry| {
            if entry.deadline <= now {
                let _ = entry.reply.send(Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!(
                        "no type-17 reply for motor=0x{:02x} index=0x{:04x}",
                        key.motor_id, key.index
                    ),
                )));
                return false;
            }
            true
        });

        for _ in 0..CMD_DRAIN_BATCH {
            match cmd_rx.try_recv() {
                Ok(cmd) => {
                    backlog_drained = backlog_drained.saturating_add(1);
                    handle_cmd(&iface, &bus, &state, &mut pending, cmd);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    info!(
                        iface = %iface,
                        backlog_drained,
                        "bus worker exiting (channel disconnected)"
                    );
                    break 'outer;
                }
            }
        }
    }
}

/// Route a single received frame.
fn handle_frame(
    iface: &str,
    state: &Weak<crate::state::AppState>,
    pending: &mut HashMap<PendingKey, PendingEntry>,
    can_id: u32,
    data: &[u8; 8],
    dlc: usize,
) {
    if let Some(node) = passive_observer_node_id(can_id) {
        if let Some(st) = state.upgrade() {
            st.record_passive_seen(iface, node);
        }
    }

    let comm = comm_type_from_id(can_id);
    if comm == driver::CommType::MotorFeedback as u8 {
        if dlc < 8 {
            return;
        }
        let raw = strip_eff_flag(can_id);
        let src_motor = ((raw >> 16) & 0xFF) as u8;
        match decode_motor_feedback(can_id, &data[..dlc]) {
            Ok(fb) => apply_type2(state, iface, src_motor, fb),
            Err(e) => debug!(iface = %iface, error = ?e, "type-2 decode failed"),
        }
        return;
    }
    if comm == driver::CommType::ReadParam as u8 {
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

/// Push a freshly-decoded type-2 row into `state.latest`,
/// `state.feedback_tx`, and trigger boot-state classification + boot
/// orchestrator the same way `LinuxCanCore::poll_once` does on aux merge.
fn apply_type2(
    state: &Weak<crate::state::AppState>,
    iface: &str,
    src_motor: u8,
    fb: DriverFeedback,
) {
    let Some(state) = state.upgrade() else { return };

    let role = {
        let inv = state.inventory.read().expect("inventory poisoned");
        inv.by_can_id(iface, src_motor)
            .map(|d| d.role().to_string())
    };
    let Some(role) = role else { return };

    let now_ms = Utc::now().timestamp_millis();

    let (prev_t_ms, prev_vbus, prev_fault) = {
        let guard = state.latest.read().expect("latest poisoned");
        match guard.get(&role) {
            Some(f) => (Some(f.t_ms), f.vbus_v, f.fault_sta),
            None => (None, 0.0, 0),
        }
    };

    let latest = MotorFeedback {
        t_ms: now_ms,
        role: role.clone(),
        can_id: src_motor,
        mech_pos_rad: fb.pos_rad,
        mech_vel_rad_s: fb.vel_rad_s,
        torque_nm: fb.torque_nm,
        vbus_v: prev_vbus,
        temp_c: fb.temp_c,
        fault_sta: prev_fault,
        warn_sta: 0,
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
    tracing::trace!(
        role = %role,
        can_id = src_motor,
        gap_ms = gap_ms,
        "type-2 frame applied"
    );

    let classify_outcome = boot_state::classify(&state, &role, latest.mech_pos_rad);
    crate::boot_orchestrator::spawn_if_orchestrator_qualifies(
        state.clone(),
        role.clone(),
        classify_outcome,
        false,
    );

    let _ = state.feedback_tx.send(latest);
}

/// Process one [`Cmd`]. The bus mutex is acquired only across the
/// actual `session::*` IO call(s) so subsequent state-mutation
/// (`state.mark_enabled` etc.) doesn't hold the lock against
/// `with_exclusive_bus` callers.
fn handle_cmd(
    iface: &str,
    bus: &Arc<Mutex<CanBus>>,
    state: &Weak<crate::state::AppState>,
    pending: &mut HashMap<PendingKey, PendingEntry>,
    cmd: Cmd,
) {
    match cmd {
        Cmd::Enable {
            motor_id,
            host_id,
            reply,
        } => {
            let result = {
                let guard = bus.lock().expect("bus mutex poisoned");
                session::cmd_enable(&guard, host_id, motor_id)
            };
            log_send_result(iface, "enable", motor_id, &result);
            if result.is_ok() {
                if let Some(state) = state.upgrade() {
                    if let Some(role) = role_for_can_id(&state, iface, motor_id) {
                        state.mark_enabled(&role);
                    }
                }
            }
            let _ = reply.send(result);
        }
        Cmd::Stop {
            motor_id,
            host_id,
            reply,
        } => {
            let result = {
                let guard = bus.lock().expect("bus mutex poisoned");
                session::cmd_stop(&guard, host_id, motor_id, false)
            };
            log_send_result(iface, "stop", motor_id, &result);
            if result.is_ok() {
                if let Some(state) = state.upgrade() {
                    if let Some(role) = role_for_can_id(&state, iface, motor_id) {
                        state.mark_stopped(&role);
                    }
                }
            }
            let _ = reply.send(result);
        }
        Cmd::SetZero {
            motor_id,
            host_id,
            reply,
        } => {
            let result = {
                let guard = bus.lock().expect("bus mutex poisoned");
                session::cmd_set_zero(&guard, host_id, motor_id)
            };
            log_send_result(iface, "set_zero", motor_id, &result);
            let _ = reply.send(result);
        }
        Cmd::SaveParams {
            motor_id,
            host_id,
            reply,
        } => {
            let result = {
                let guard = bus.lock().expect("bus mutex poisoned");
                session::cmd_save_params(&guard, host_id, motor_id)
            };
            log_send_result(iface, "save_params", motor_id, &result);
            let _ = reply.send(result);
        }
        Cmd::ActiveReport {
            motor_id,
            host_id,
            enable,
            reply,
        } => {
            let result = {
                let guard = bus.lock().expect("bus mutex poisoned");
                session::cmd_active_report(&guard, host_id, motor_id, enable)
            };
            log_send_result(iface, "active_report", motor_id, &result);
            let _ = reply.send(result);
        }
        Cmd::SetVelocity {
            motor_id,
            host_id,
            vel_rad_s,
            role,
            reply,
        } => {
            let need_rearm = match state.upgrade() {
                Some(state) => !state.is_enabled(&role),
                None => true,
            };

            let result: io::Result<()> = {
                let guard = bus.lock().expect("bus mutex poisoned");
                (|| {
                    let dev = Rs03::new(host_id, motor_id);
                    if need_rearm {
                        session::write_param_u8(
                            &guard,
                            dev.host_id(),
                            dev.motor_id(),
                            dev.param_index_run_mode(),
                            dev.run_mode_velocity(),
                        )?;
                        session::cmd_enable(&guard, dev.host_id(), dev.motor_id())?;
                    }
                    session::write_param_f32(
                        &guard,
                        dev.host_id(),
                        dev.motor_id(),
                        dev.param_index_spd_ref(),
                        vel_rad_s,
                    )
                })()
            };
            log_send_result(iface, "set_velocity", motor_id, &result);
            if result.is_ok() && need_rearm && !role.is_empty() {
                if let Some(state) = state.upgrade() {
                    state.mark_enabled(&role);
                }
            }
            let _ = reply.send(result);
        }
        Cmd::WriteParam {
            motor_id,
            host_id,
            index,
            value,
            reply,
        } => {
            let result = {
                let guard = bus.lock().expect("bus mutex poisoned");
                match value {
                    WriteValue::F32(v) => {
                        session::write_param_f32(&guard, host_id, motor_id, index, v)
                    }
                    WriteValue::U8(v) => {
                        session::write_param_u8(&guard, host_id, motor_id, index, v)
                    }
                    WriteValue::U32(v) => {
                        session::write_param_u32(&guard, host_id, motor_id, index, v)
                    }
                }
            };
            log_send_result(iface, "write_param", motor_id, &result);
            let _ = reply.send(result);
        }
        Cmd::ReadParam {
            motor_id,
            host_id,
            index,
            reply,
        } => {
            let mut req = [0u8; 8];
            req[0..2].copy_from_slice(&index.to_le_bytes());
            let send_result = {
                let guard = bus.lock().expect("bus mutex poisoned");
                session::send_frame(&guard, driver::CommType::ReadParam, host_id, motor_id, &req)
            };
            if let Err(e) = send_result {
                debug!(
                    iface = %iface,
                    op = "read_param",
                    motor_id = motor_id,
                    error = ?e,
                    "send request failed"
                );
                let _ = reply.send(Err(e));
                return;
            }
            let key = PendingKey { motor_id, index };
            if let Some(prev) = pending.insert(
                key,
                PendingEntry {
                    reply,
                    deadline: Instant::now() + REPLY_TIMEOUT,
                },
            ) {
                let _ = prev.reply.send(Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "superseded by another read for the same (motor,index)",
                )));
            }
        }
    }
}

fn role_for_can_id(state: &Arc<crate::state::AppState>, iface: &str, can_id: u8) -> Option<String> {
    state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_can_id(iface, can_id)
        .map(|d| d.role().to_string())
}

fn log_send_result<T>(iface: &str, op: &str, motor_id: u8, result: &io::Result<T>) {
    if let Err(e) = result {
        match e.raw_os_error() {
            Some(105) => {
                debug!(
                    iface = %iface,
                    op = op,
                    motor_id = motor_id,
                    "send returned ENOBUFS; caller will back off"
                );
            }
            _ => {
                debug!(iface = %iface, op = op, motor_id = motor_id, error = ?e, "command failed");
            }
        }
    }
}
