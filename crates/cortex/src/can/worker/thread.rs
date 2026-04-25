//! Worker thread entrypoints and recv / command dispatch.

use std::collections::HashMap;
use std::io;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use driver::rs03::params::LOC_REF;
use driver::rs03::session;
use driver::rs03::{MitCommand, RobstrideCodec};
use driver::{CanBus, Rs03, RsActuator};
use tokio::runtime::Handle;
use tracing::{debug, info, warn};

use super::command::{
    Cmd, PendingEntry, PendingKey, WriteValue, CMD_DRAIN_BATCH, RECV_POLL_TIMEOUT, REPLY_TIMEOUT,
};
use super::feedback;
use super::handle::BusHandle;
use super::health::BusHealth;
use super::pin::pin_to_cpu;

/// RS03 `run_mode`: profile position (PP). See `driver::rs03::params::RUN_MODE`.
const RUN_MODE_PP: u8 = 1;

/// RS03 `run_mode`: operation (MIT). See `driver::rs03::params::RUN_MODE`.
/// Used by [`Cmd::SetMitHold`] for the post-home spring-damper hold; the
/// firmware closes the loop on encoder + the standing kp/kd values without
/// any streamed setpoint after the single `OperationCtrl` frame.
const RUN_MODE_OP: u8 = 0;

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
    let health = Arc::new(BusHealth::default());
    let health_for_thread = Arc::clone(&health);

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
            run_worker(
                iface_for_thread,
                bus_for_thread,
                rx,
                state,
                health_for_thread,
            );
        })
        .with_context(|| format!("spawning worker thread for {iface}"))?;

    Ok(BusHandle::new(iface_for_handle, tx, bus_arc, health))
}

/// Inner per-thread main loop.
fn run_worker(
    iface: String,
    bus: Arc<Mutex<CanBus>>,
    cmd_rx: Receiver<Cmd>,
    state: Weak<crate::state::AppState>,
    health: Arc<BusHealth>,
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
                feedback::route_frame(&iface, &state, &mut pending, &health, can_id, &data, dlc);
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
                    health.record_cmd_drained(1);
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
                Some(state) => !state.is_enabled(&role) || state.is_position_hold(&role),
                None => true,
            };

            // Do **not** hold `bus` across the firmware settle sleeps: (1)
            // `REPLY_TIMEOUT` must cover wall-clock handler time (see
            // `command::REPLY_TIMEOUT`); (2) releasing the mutex during
            // sleeps lets `with_exclusive_bus` contend instead of blocking
            // for the full settle window.
            let result: io::Result<()> = (|| {
                let dev = Rs03::new(host_id, motor_id);
                if need_rearm {
                    // Re-arm: same order as `bench_enable_disable.py` (stop, mode, spd_ref, enable).
                    //
                    // RS03 firmware quirk: a `RUN_MODE` write only
                    // commits if the motor's enable bit is genuinely
                    // off when the write arrives. After certain
                    // hold modes — notably MIT (`run_mode = 0`,
                    // post-`finish_home_success`) — a single
                    // `cmd_stop` propagates through the firmware
                    // state machine in ~20-30 ms; firing the
                    // `RUN_MODE` write in the same millisecond
                    // sometimes lands while the prior enable is
                    // still latched, the write is silently
                    // rejected, and the subsequent `cmd_enable`
                    // resumes the previous mode (PP or MIT) which
                    // ignores `SPD_REF` entirely. Symptom is a
                    // velocity command stream where every frame
                    // sends successfully but the motor doesn't
                    // move — exact failure mode hit by the boot
                    // orchestrator after a cortex restart while a
                    // motor was held.
                    //
                    // Belt-and-braces: cmd_stop, settle, RUN_MODE,
                    // cmd_stop again (to swallow a re-enable race
                    // from the prior latched state), settle,
                    // SPD_REF, cmd_enable. The two extra ~30 ms
                    // sleeps add ~60 ms to the very first jog after
                    // a hold (or first home_ramp tick after boot)
                    // and are no-cost on every subsequent
                    // SetVelocity (fast path skips this branch
                    // entirely once `state.enabled` is set).
                    {
                        let guard = bus.lock().expect("bus mutex poisoned");
                        session::cmd_stop(&guard, dev.host_id(), dev.motor_id(), false)?;
                    }
                    thread::sleep(Duration::from_millis(30));
                    {
                        let guard = bus.lock().expect("bus mutex poisoned");
                        session::write_param_u8(
                            &guard,
                            dev.host_id(),
                            dev.motor_id(),
                            dev.param_index_run_mode(),
                            dev.run_mode_velocity(),
                        )?;
                    }
                    {
                        let guard = bus.lock().expect("bus mutex poisoned");
                        session::cmd_stop(&guard, dev.host_id(), dev.motor_id(), false)?;
                    }
                    thread::sleep(Duration::from_millis(30));
                    {
                        let guard = bus.lock().expect("bus mutex poisoned");
                        session::write_param_f32(
                            &guard,
                            dev.host_id(),
                            dev.motor_id(),
                            dev.param_index_spd_ref(),
                            vel_rad_s,
                        )?;
                        session::cmd_enable(&guard, dev.host_id(), dev.motor_id())?;
                    }
                } else {
                    let guard = bus.lock().expect("bus mutex poisoned");
                    session::write_param_f32(
                        &guard,
                        dev.host_id(),
                        dev.motor_id(),
                        dev.param_index_spd_ref(),
                        vel_rad_s,
                    )?;
                }
                Ok(())
            })();
            log_send_result(iface, "set_velocity", motor_id, &result);
            if result.is_ok() && !role.is_empty() {
                if let Some(state) = state.upgrade() {
                    state.clear_position_hold(&role);
                    state.clear_mit_streaming(&role);
                }
            }
            if result.is_ok() && need_rearm && !role.is_empty() {
                if let Some(state) = state.upgrade() {
                    state.mark_enabled(&role);
                }
            }
            let _ = reply.send(result);
        }
        Cmd::SetPositionHold {
            motor_id,
            host_id,
            target_principal_rad,
            role: _,
            reply,
        } => {
            let result: io::Result<()> = {
                let guard = bus.lock().expect("bus mutex poisoned");
                (|| {
                    let dev = Rs03::new(host_id, motor_id);
                    session::cmd_stop(&guard, host_id, motor_id, false)?;
                    session::write_param_u8(
                        &guard,
                        dev.host_id(),
                        dev.motor_id(),
                        dev.param_index_run_mode(),
                        RUN_MODE_PP,
                    )?;
                    session::write_param_f32(
                        &guard,
                        dev.host_id(),
                        dev.motor_id(),
                        LOC_REF,
                        target_principal_rad,
                    )?;
                    session::cmd_enable(&guard, dev.host_id(), dev.motor_id())?;
                    Ok(())
                })()
            };
            log_send_result(iface, "set_position_hold", motor_id, &result);
            if result.is_ok() {
                if let Some(st) = state.upgrade() {
                    if let Some(role) = role_for_can_id(&st, iface, motor_id) {
                        st.clear_mit_streaming(&role);
                    }
                }
            }
            let _ = reply.send(result);
        }
        Cmd::SetMitHold {
            motor_id,
            host_id,
            target_principal_rad,
            kp_nm_per_rad,
            kd_nm_s_per_rad,
            role: _,
            reply,
        } => {
            // MIT spring-damper hold: cmd_stop -> RUN_MODE=0 (operation/MIT) ->
            // cmd_enable -> a single OperationCtrl frame carrying
            // (target, vel=0, torque_ff=0, kp, kd). The firmware then closes the
            // loop on encoder + the standing kp/kd values, so there is **no**
            // streamed velocity setpoint and (unlike PP / RUN_MODE=1) no
            // continuous current draw or audible servo whine while held.
            //
            // The stop -> write RUN_MODE -> enable sequence is **mandatory**
            // per the RS03 manual (Ch.2 item 2 and §4.3 operation-mode
            // transition): RUN_MODE can only be rewritten from the disabled
            // state, so we cannot skip `cmd_stop` or reorder these calls to
            // shrink the coast window. During this ~20-50 ms stretch the
            // drive is unpowered — if the motor still has residual velocity
            // when the sequence begins it coasts uncontrolled and static
            // friction then holds it wherever it lands, producing the
            // 0.8-1.2 deg run-to-run auto-home offset that the 2026-04
            // investigation traced through to this handoff.
            //
            // The fix has to live upstream in the homer's dwell predicate:
            // the homer now refuses to exit until `|mech_vel_rad_s|` is
            // below `SafetyConfig::target_dwell_max_vel_rad_s` (see
            // `home_ramp.rs` and `home_ramp_dwell_tests`). That turns the
            // unavoidable disabled window here into a non-event because
            // residual velocity at entry is already gated to near zero.
            // Do not try to "fix" this by collapsing the sequence — the
            // frames will be NACKed and the motor will end up either
            // wedged in RUN_MODE=2 with a zero velocity command (no
            // holding torque) or refusing to accept the next MIT frame.
            let result: io::Result<()> = {
                let guard = bus.lock().expect("bus mutex poisoned");
                (|| {
                    let dev = Rs03::new(host_id, motor_id);
                    session::cmd_stop(&guard, host_id, motor_id, false)?;
                    session::write_param_u8(
                        &guard,
                        dev.host_id(),
                        dev.motor_id(),
                        dev.param_index_run_mode(),
                        RUN_MODE_OP,
                    )?;
                    session::cmd_enable(&guard, dev.host_id(), dev.motor_id())?;
                    let codec = RobstrideCodec;
                    let cmd = MitCommand {
                        position_rad: target_principal_rad,
                        velocity_rad_s: 0.0,
                        kp: kp_nm_per_rad,
                        kd: kd_nm_s_per_rad,
                        torque_ff_nm: 0.0,
                    };
                    let (id, data) = codec.encode_mit(host_id, motor_id, cmd).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("encode MIT hold frame: {e:?}"),
                        )
                    })?;
                    guard.send_ext(id, &data)?;
                    Ok(())
                })()
            };
            log_send_result(iface, "set_mit_hold", motor_id, &result);
            if result.is_ok() {
                if let Some(st) = state.upgrade() {
                    if let Some(role) = role_for_can_id(&st, iface, motor_id) {
                        st.clear_mit_streaming(&role);
                    }
                }
            }
            let _ = reply.send(result);
        }
        Cmd::SetMitCommand {
            motor_id,
            host_id,
            position_rad,
            velocity_rad_s,
            torque_ff_nm,
            kp_nm_per_rad,
            kd_nm_s_per_rad,
            role,
            reply,
        } => {
            let need_entry = match state.upgrade() {
                Some(st) => {
                    if role.is_empty() {
                        true
                    } else {
                        !st.is_mit_streaming(&role)
                            || !st.is_enabled(&role)
                            || st.is_position_hold(&role)
                    }
                }
                None => true,
            };
            let result: io::Result<()> = {
                let guard = bus.lock().expect("bus mutex poisoned");
                (|| {
                    let dev = Rs03::new(host_id, motor_id);
                    if need_entry {
                        session::cmd_stop(&guard, dev.host_id(), dev.motor_id(), false)?;
                        session::write_param_u8(
                            &guard,
                            dev.host_id(),
                            dev.motor_id(),
                            dev.param_index_run_mode(),
                            RUN_MODE_OP,
                        )?;
                        session::cmd_enable(&guard, dev.host_id(), dev.motor_id())?;
                    }
                    let codec = RobstrideCodec;
                    let cmd = MitCommand {
                        position_rad,
                        velocity_rad_s,
                        kp: kp_nm_per_rad,
                        kd: kd_nm_s_per_rad,
                        torque_ff_nm,
                    };
                    let (id, data) = codec.encode_mit(host_id, motor_id, cmd).map_err(|e| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!("encode MIT command frame: {e:?}"),
                        )
                    })?;
                    guard.send_ext(id, &data)?;
                    Ok(())
                })()
            };
            log_send_result(iface, "set_mit_command", motor_id, &result);
            if result.is_ok() && !role.is_empty() {
                if let Some(st) = state.upgrade() {
                    st.clear_position_hold(&role);
                    st.mark_enabled(&role);
                    st.mark_mit_streaming(&role);
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
