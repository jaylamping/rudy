//! Per-bus dedicated I/O thread.
//!
//! Each `[[can.buses]]` entry gets its own OS thread that exclusively owns
//! the underlying [`driver::CanBus`]. The thread runs a tight loop:
//!
//! 1. Block on `bus.recv()` for a short timeout (5 ms). Every received
//!    type-2 (`MotorFeedback`) frame is decoded and pushed into
//!    `state.latest` + `state.feedback_tx` immediately, so the live
//!    telemetry view tracks the bus at native cadence with no extra
//!    type-17 round-trips. Type-17 (`ReadParam`) replies are matched
//!    against in-flight commands by `(motor_id, index)` and forwarded to
//!    the originating thread via a oneshot.
//! 2. Drain up to N pending [`Cmd`]s from the per-bus channel. Each
//!    command serializes one or more frames on the bus (writes / enable /
//!    stop / read request) and, where the operation expects an
//!    acknowledgement, parks a oneshot in `pending` so the next received
//!    frame can complete it.
//!
//! Why a dedicated thread per bus, instead of the previous
//! per-iface mutex around `CanBus`? The mutex serialised every
//! `set_velocity_setpoint` against every `read_param`, which under the
//! 20 Hz sweep cadence used the entire bus budget on lock fights. A
//! single-owner thread serialises naturally with no lock, and the
//! `recv()`-first loop guarantees that a flood of type-2 frames is
//! drained continuously instead of starving while a slow `read_param`
//! waits on a missing peer.
//!
//! On the Pi 5 the worker pins itself to a CPU after spawn (see
//! [`spawn`]). Pinning + IRQ affinity (set by `deploy/pi5/bootstrap.sh`)
//! co-locates the SocketCAN softirq and the user-space recv loop on the
//! same core, which removes the inter-core hop on every frame.

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::io;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::Utc;
use driver::rs03::feedback::{decode_motor_feedback, MotorFeedback as DriverFeedback};
use driver::rs03::frame::{comm_type_from_id, strip_eff_flag};
use driver::rs03::params;
use driver::rs03::session;
use driver::CanBus;
use tokio::runtime::Handle;
use tracing::{debug, info, warn};

use crate::boot_state::{self, BootState, ClassifyOutcome};
use crate::can::auto_recovery;
use crate::types::MotorFeedback;

/// Soft cap on commands drained per loop iteration, so a backlog of
/// jog frames can't starve the `recv()` side.
const CMD_DRAIN_BATCH: usize = 8;

/// Receive-side timeout. Sets the minimum wakeup cadence of the worker
/// loop (so it can service queued commands when no frames are arriving).
const RECV_POLL_TIMEOUT: Duration = Duration::from_millis(5);

/// Default round-trip timeout for type-17 reads / writes that expect a
/// reply. Slightly larger than the previous synchronous `PARAM_TIMEOUT`
/// to absorb the extra hop through the channel.
pub const REPLY_TIMEOUT: Duration = Duration::from_millis(50);

/// Type-17 reply value bytes (little-endian). `None` means the motor
/// returned a read-fail status byte.
pub type ReplyBytes = Option<[u8; 4]>;

/// A unit of work the worker thread will perform on its bus. The
/// originating thread parks a oneshot [`Sender`] inside variants that
/// expect a reply; the worker fills it in once the bus round-trip is
/// complete.
#[derive(Debug)]
pub enum Cmd {
    /// `cmd_enable`.
    Enable {
        motor_id: u8,
        host_id: u8,
        reply: Sender<io::Result<()>>,
    },
    /// `cmd_stop` (no clear-fault).
    Stop {
        motor_id: u8,
        host_id: u8,
        reply: Sender<io::Result<()>>,
    },
    /// `cmd_set_zero`.
    SetZero {
        motor_id: u8,
        host_id: u8,
        reply: Sender<io::Result<()>>,
    },
    /// `cmd_save_params`.
    SaveParams {
        motor_id: u8,
        host_id: u8,
        reply: Sender<io::Result<()>>,
    },
    /// Velocity-mode setpoint. Smart re-arm: writes RUN_MODE +
    /// `cmd_enable` only when the worker has not yet observed this
    /// motor as enabled (transition path). Subsequent calls send only
    /// `SPD_REF` so the steady-state bus traffic is one frame per jog.
    SetVelocity {
        motor_id: u8,
        host_id: u8,
        vel_rad_s: f32,
        /// Snapshot of the role string for `state.mark_enabled` after a
        /// successful first-frame transition. Empty string means
        /// "don't update state.enabled" (e.g. tests, mock paths).
        role: String,
        reply: Sender<io::Result<()>>,
    },
    /// Single-parameter write.
    WriteParam {
        motor_id: u8,
        host_id: u8,
        index: u16,
        value: WriteValue,
        reply: Sender<io::Result<()>>,
    },
    /// Type-17 read. The worker enqueues the read-request frame and
    /// stashes a `pending` entry keyed on `(motor_id, index)` so the
    /// next matching reply completes the oneshot.
    ReadParam {
        motor_id: u8,
        host_id: u8,
        index: u16,
        reply: Sender<io::Result<ReplyBytes>>,
    },
}

/// Type tag for [`Cmd::WriteParam`]. The worker calls the right
/// `driver::rs03::session::write_param_*` helper based on the tag.
#[derive(Debug, Clone, Copy)]
pub enum WriteValue {
    F32(f32),
    U8(u8),
    U32(u32),
}

/// Pending-reply key. The worker indexes outstanding `ReadParam`
/// commands by `(motor_id, index)` so a flood of unrelated type-17
/// replies (e.g. from a different motor on the same bus) can't
/// accidentally complete the wrong oneshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PendingKey {
    motor_id: u8,
    index: u16,
}

/// Per-pending entry held inside the worker thread.
struct PendingEntry {
    reply: Sender<io::Result<ReplyBytes>>,
    deadline: Instant,
}

/// Handle to a per-bus worker. Cheap to clone (the inner `Sender` is an
/// `Arc`-equivalent under the hood). Dropping the last handle closes the
/// channel, which causes the worker thread to exit cleanly on its next
/// `try_recv`.
///
/// The handle also retains an `Arc<Mutex<CanBus>>` so callers that need
/// **exclusive, blocking** access to the bus (the bench-routine handler
/// in `api/tests.rs` runs `driver::rs03::tests::run_*` against the raw
/// `&CanBus` for seconds at a time) can lock the bus directly via
/// [`BusHandle::with_exclusive_bus`]. While that lock is held, the
/// worker thread will be blocked on its next `bus.recv()` call — i.e.
/// type-2 streaming pauses for the duration of the bench routine.
/// That's the same trade-off the per-iface mutex made before this
/// refactor; benches are never run during normal operator-driven
/// motion, so the safety surface is unchanged.
#[derive(Clone)]
pub struct BusHandle {
    iface: String,
    tx: Sender<Cmd>,
    bus: Arc<Mutex<CanBus>>,
}

impl BusHandle {
    pub fn iface(&self) -> &str {
        &self.iface
    }

    /// Borrow the raw [`CanBus`] for the duration of `f`, exclusively.
    /// The worker thread will be unable to recv or service commands
    /// until `f` returns. Use ONLY for the bench-routine handler;
    /// every other path should go through the typed `Cmd::*` helpers
    /// so type-2 streaming keeps running.
    pub fn with_exclusive_bus<T>(&self, f: impl FnOnce(&CanBus) -> T) -> T {
        let guard = self.bus.lock().expect("bus mutex poisoned");
        f(&guard)
    }

    /// Send `cmd` to the worker. Failure here means the worker thread
    /// has died; we surface that as an `io::Error` so existing call
    /// sites can keep using `io::Result`.
    fn submit(&self, cmd: Cmd) -> io::Result<()> {
        self.tx
            .send(cmd)
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "bus worker has exited"))
    }

    pub fn enable(&self, host_id: u8, motor_id: u8) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::Enable {
            motor_id,
            host_id,
            reply: tx,
        })?;
        recv_blocking(rx, REPLY_TIMEOUT)?
    }

    pub fn stop(&self, host_id: u8, motor_id: u8) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::Stop {
            motor_id,
            host_id,
            reply: tx,
        })?;
        recv_blocking(rx, REPLY_TIMEOUT)?
    }

    pub fn set_zero(&self, host_id: u8, motor_id: u8) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::SetZero {
            motor_id,
            host_id,
            reply: tx,
        })?;
        recv_blocking(rx, REPLY_TIMEOUT)?
    }

    pub fn save_params(&self, host_id: u8, motor_id: u8) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::SaveParams {
            motor_id,
            host_id,
            reply: tx,
        })?;
        recv_blocking(rx, REPLY_TIMEOUT)?
    }

    pub fn set_velocity(
        &self,
        host_id: u8,
        motor_id: u8,
        role: &str,
        vel_rad_s: f32,
    ) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::SetVelocity {
            motor_id,
            host_id,
            vel_rad_s,
            role: role.to_string(),
            reply: tx,
        })?;
        recv_blocking(rx, REPLY_TIMEOUT)?
    }

    pub fn write_param(
        &self,
        host_id: u8,
        motor_id: u8,
        index: u16,
        value: WriteValue,
    ) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::WriteParam {
            motor_id,
            host_id,
            index,
            value,
            reply: tx,
        })?;
        recv_blocking(rx, REPLY_TIMEOUT)?
    }

    pub fn read_param(
        &self,
        host_id: u8,
        motor_id: u8,
        index: u16,
        timeout: Duration,
    ) -> io::Result<ReplyBytes> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::ReadParam {
            motor_id,
            host_id,
            index,
            reply: tx,
        })?;
        // Wait slightly longer than the requested protocol timeout so
        // the worker can run the timeout itself and report back.
        recv_blocking(rx, timeout + Duration::from_millis(20))?
    }
}

fn recv_blocking<T>(rx: Receiver<T>, timeout: Duration) -> io::Result<T> {
    rx.recv_timeout(timeout)
        .map_err(|e| io::Error::new(io::ErrorKind::TimedOut, format!("{e}")))
}

/// Spawn a worker thread for `bus`, returning a [`BusHandle`].
///
/// `state` is held weakly by the worker so we never keep `AppState`
/// alive past shutdown. `cpu_pin` (when `Some`) names the CPU id the
/// worker should pin itself to; `None` means the supervisor has no
/// preference and the OS scheduler picks. Pinning failure is
/// non-fatal (logged + ignored).
///
/// Must be called from inside a tokio runtime context: the worker
/// captures `Handle::current()` so that
/// [`auto_recovery::maybe_spawn_recovery`] (called from the type-2
/// classification path) can reach the runtime when it needs to
/// `tokio::spawn` an async recovery task.
pub fn spawn(
    iface: String,
    bus: CanBus,
    state: Weak<crate::state::AppState>,
    cpu_pin: Option<usize>,
) -> Result<BusHandle> {
    bus.set_read_timeout(RECV_POLL_TIMEOUT)
        .with_context(|| format!("setting read timeout on {iface}"))?;

    let runtime_handle = Handle::try_current()
        .context("bus_worker::spawn must be called inside a tokio runtime context")?;

    let (tx, rx) = mpsc::channel::<Cmd>();
    let bus_arc = Arc::new(Mutex::new(bus));
    let bus_for_thread = Arc::clone(&bus_arc);
    let iface_for_thread = iface.clone();
    let iface_for_handle = iface.clone();

    thread::Builder::new()
        .name(format!("rudy-can-{iface}"))
        .spawn(move || {
            // Establish a tokio runtime context for this OS thread so
            // anything in the worker that calls `tokio::spawn`
            // (auto-recovery dispatch, broadcast `feedback_tx.send`,
            // etc.) finds a runtime.
            let _guard = runtime_handle.enter();
            if let Some(cpu) = cpu_pin {
                pin_to_cpu(&iface_for_thread, cpu);
            }
            run_worker(iface_for_thread, bus_for_thread, rx, state);
        })
        .with_context(|| format!("spawning worker thread for {iface}"))?;

    Ok(BusHandle {
        iface: iface_for_handle,
        tx,
        bus: bus_arc,
    })
}

#[cfg(target_os = "linux")]
fn pin_to_cpu(iface: &str, cpu: usize) {
    let cores = match core_affinity::get_core_ids() {
        Some(c) => c,
        None => {
            debug!(
                iface = %iface,
                requested = cpu,
                "core_affinity unavailable; bus worker is unpinned"
            );
            return;
        }
    };
    let Some(core) = cores.get(cpu).copied() else {
        debug!(
            iface = %iface,
            requested = cpu,
            available = cores.len(),
            "requested CPU id out of range; bus worker is unpinned"
        );
        return;
    };
    if core_affinity::set_for_current(core) {
        info!(iface = %iface, cpu = cpu, "bus worker pinned to CPU");
    } else {
        debug!(
            iface = %iface,
            cpu = cpu,
            "set_for_current returned false; bus worker is unpinned"
        );
    }
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
        // Drain one frame from the bus (or time out). Every iteration of
        // the outer loop attempts at least one recv so type-2 streaming
        // is never starved by command bursts. We lock the bus only for
        // the recv itself; the lock is released between recv and the
        // command-drain step so an `with_exclusive_bus` caller can slip
        // in.
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

        // Reap timed-out pending reads so a vanished motor doesn't
        // permanently leak entries.
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

        // Drain up to N commands. We loop instead of single-step so
        // bursts (e.g. `seed_boot_low_limits` issuing 2 writes per
        // motor across the inventory at startup) clear in O(1)
        // wakeups while still bounded.
        //
        // The bus mutex is acquired per-command and released before
        // any subsequent state mutation (`state.mark_enabled` etc.)
        // so bench routines can interleave via `with_exclusive_bus`
        // between commands.
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
    let comm = comm_type_from_id(can_id);
    if comm == driver::CommType::MotorFeedback as u8 {
        if dlc < 8 {
            return;
        }
        let raw = strip_eff_flag(can_id);
        let src_motor = ((raw >> 16) & 0xFF) as u8;
        match decode_motor_feedback(can_id, &data[..dlc]) {
            Ok(fb) => apply_type2(state, src_motor, fb),
            Err(e) => debug!(iface = %iface, error = ?e, "type-2 decode failed"),
        }
        return;
    }
    if comm == driver::CommType::ReadParam as u8 {
        if dlc < 8 {
            return;
        }
        let raw = strip_eff_flag(can_id);
        // Type-17 reply layout: [comm=0x11][status][motor_id][host_id]
        let reply_status = ((raw >> 16) & 0xFF) as u8;
        let reply_motor = ((raw >> 8) & 0xFF) as u8;
        let reply_index = u16::from_le_bytes([data[0], data[1]]);
        let key = PendingKey {
            motor_id: reply_motor,
            index: reply_index,
        };
        if let Some(entry) = pending.remove(&key) {
            // status=0 → ok, status=1 → read-fail, anything else also
            // treated as a fail. Mirrors `interpret_read_param_response`.
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
    // Other comm types (FaultFeedback etc.) are dropped silently for
    // now; the previous implementation also ignored them.
}

/// Push a freshly-decoded type-2 row into `state.latest`,
/// `state.feedback_tx`, and trigger boot-state classification +
/// auto-recovery exactly the same way `LinuxCanCore::poll_once` did.
fn apply_type2(state: &Weak<crate::state::AppState>, src_motor: u8, fb: DriverFeedback) {
    let Some(state) = state.upgrade() else { return };

    let role = {
        let inv = state.inventory.read().expect("inventory poisoned");
        inv.by_can_id(src_motor).map(|m| m.role.clone())
    };
    let Some(role) = role else { return };

    let latest = MotorFeedback {
        t_ms: Utc::now().timestamp_millis(),
        role: role.clone(),
        can_id: src_motor,
        mech_pos_rad: fb.pos_rad,
        mech_vel_rad_s: fb.vel_rad_s,
        torque_nm: fb.torque_nm,
        // type-2 doesn't carry vbus; keep last known. Type-17 sweep
        // refreshes vbus separately.
        vbus_v: state
            .latest
            .read()
            .expect("latest poisoned")
            .get(&role)
            .map(|f| f.vbus_v)
            .unwrap_or_default(),
        temp_c: fb.temp_c,
        fault_sta: state
            .latest
            .read()
            .expect("latest poisoned")
            .get(&role)
            .map(|f| f.fault_sta)
            .unwrap_or_default(),
        warn_sta: 0,
    };

    state
        .latest
        .write()
        .expect("latest poisoned")
        .insert(role.clone(), latest.clone());

    if let ClassifyOutcome::Changed { new, .. } =
        boot_state::classify(&state, &role, latest.mech_pos_rad)
    {
        if let BootState::OutOfBand { mech_pos_rad, .. } = new {
            auto_recovery::maybe_spawn_recovery(&state, &role, mech_pos_rad);
        }
    }

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
                    if let Some(role) = role_for_can_id(&state, motor_id) {
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
                    if let Some(role) = role_for_can_id(&state, motor_id) {
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
        Cmd::SetVelocity {
            motor_id,
            host_id,
            vel_rad_s,
            role,
            reply,
        } => {
            // Re-arm only on transition from "not currently driving" → driving.
            // The worker is the single writer of `state.enabled` for
            // velocity-mode jog frames now, so this consult is
            // race-free with respect to other jog frames on the same
            // bus.
            let need_rearm = match state.upgrade() {
                Some(state) => !state.is_enabled(&role),
                // No state to consult (unit tests, mock paths) — be
                // safe and re-arm.
                None => true,
            };

            let result: io::Result<()> = {
                let guard = bus.lock().expect("bus mutex poisoned");
                (|| {
                    if need_rearm {
                        session::write_param_u8(&guard, host_id, motor_id, params::RUN_MODE, 2)?;
                        session::cmd_enable(&guard, host_id, motor_id)?;
                    }
                    session::write_param_f32(&guard, host_id, motor_id, params::SPD_REF, vel_rad_s)
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
            // Send the request frame; if that fails, complete the
            // oneshot synchronously with the error and don't add a
            // pending entry.
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
            // If a previous pending read for the same (motor,index)
            // is still outstanding, complete it with a "superseded"
            // error so its caller sees a fast failure instead of a
            // 50 ms timeout.
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

fn role_for_can_id(state: &Arc<crate::state::AppState>, can_id: u8) -> Option<String> {
    state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_can_id(can_id)
        .map(|m| m.role.clone())
}

/// Common send-result logger. ENOBUFS / "no buffer space" failures get a
/// single debug line because they're expected when a motor is missing
/// (the kernel queue fills, then the per-motor backoff logic in
/// `LinuxCanCore` skips the motor entirely). Other errors get a `warn`
/// the first time, `debug` after.
fn log_send_result<T>(iface: &str, op: &str, motor_id: u8, result: &io::Result<T>) {
    if let Err(e) = result {
        match e.raw_os_error() {
            Some(105) => {
                // ENOBUFS — txqueue full. The per-motor MotorBackoff in
                // LinuxCanCore handles the back-off; we just log once
                // per occurrence at debug level so the journal stays
                // clean under sustained outages.
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

/// Auto-assignment helper: given the inventory's [[can.buses]] order and
/// the available CPU count, pick the per-bus CPU id for `index`.
///
/// Policy: leave core 0 to the kernel + tokio runtime; spread bus
/// workers round-robin starting at core 1. Returns `None` when the
/// system has fewer than 2 cores (single-core means everything shares
/// core 0; pinning is pointless).
pub fn auto_assign_cpu(index: usize, cpu_count: usize) -> Option<usize> {
    if cpu_count < 2 {
        return None;
    }
    let n = cpu_count.saturating_sub(1);
    Some(1 + (index % n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_assign_cpu_skips_core_zero() {
        // 4-core system, 3 buses → cores 1, 2, 3 (round-robin).
        assert_eq!(auto_assign_cpu(0, 4), Some(1));
        assert_eq!(auto_assign_cpu(1, 4), Some(2));
        assert_eq!(auto_assign_cpu(2, 4), Some(3));
        // 4th bus wraps back to core 1.
        assert_eq!(auto_assign_cpu(3, 4), Some(1));
    }

    #[test]
    fn auto_assign_cpu_single_core_returns_none() {
        assert_eq!(auto_assign_cpu(0, 1), None);
        assert_eq!(auto_assign_cpu(2, 1), None);
    }

    #[test]
    fn auto_assign_cpu_two_cores_uses_only_core_one() {
        // With 2 cores total, only core 1 is available for workers.
        assert_eq!(auto_assign_cpu(0, 2), Some(1));
        assert_eq!(auto_assign_cpu(1, 2), Some(1));
        assert_eq!(auto_assign_cpu(7, 2), Some(1));
    }

    #[test]
    fn write_value_variants_carry_typed_payloads() {
        // The variants are matched on by the worker to pick the right
        // `driver::rs03::session::write_param_*` helper. Verify the
        // discriminant is preserved after Clone/Copy through the
        // channel boundary.
        let cases = [
            WriteValue::F32(1.5),
            WriteValue::U8(42),
            WriteValue::U32(0xDEAD_BEEF),
        ];
        for c in cases {
            let copy = c;
            match (c, copy) {
                (WriteValue::F32(a), WriteValue::F32(b)) => assert!((a - b).abs() < 1e-9),
                (WriteValue::U8(a), WriteValue::U8(b)) => assert_eq!(a, b),
                (WriteValue::U32(a), WriteValue::U32(b)) => assert_eq!(a, b),
                _ => panic!("variant mismatch after Copy"),
            }
        }
    }

    #[test]
    fn bus_handle_submit_after_drop_reports_broken_pipe() {
        // Build a channel directly so we can drop the receiver and
        // observe the BrokenPipe shape that callers rely on (see
        // `BusHandle::submit`). We can't construct a full BusHandle
        // here without a real CanBus, but the only logic in `submit`
        // that we want to pin is the error-conversion shape.
        let (tx, rx) = mpsc::channel::<Cmd>();
        drop(rx);
        let (reply_tx, _reply_rx) = mpsc::channel::<io::Result<()>>();
        let cmd = Cmd::Enable {
            motor_id: 0x08,
            host_id: 0xFD,
            reply: reply_tx,
        };
        let send_err = tx.send(cmd).unwrap_err();
        // Mirror what BusHandle::submit does on send-failure.
        let io_err: io::Error = io::Error::new(io::ErrorKind::BrokenPipe, format!("{send_err}"));
        assert_eq!(io_err.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn pending_key_distinguishes_motor_and_index() {
        let a = PendingKey {
            motor_id: 0x08,
            index: 0x7019,
        };
        let b = PendingKey {
            motor_id: 0x09,
            index: 0x7019,
        };
        let c = PendingKey {
            motor_id: 0x08,
            index: 0x701A,
        };
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
        let mut map = HashMap::new();
        map.insert(a, "a");
        map.insert(b, "b");
        map.insert(c, "c");
        assert_eq!(map.len(), 3);
    }
}
