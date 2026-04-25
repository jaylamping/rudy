//! [`BusHandle`]: typed facade to a per-bus worker thread.

use std::io;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use driver::CanBus;

use super::command::{Cmd, ReplyBytes, WriteValue, REPLY_TIMEOUT};

/// Handle to a per-bus worker. Cheap to clone (the inner `Sender` is an
/// `Arc`-equivalent under the hood). Dropping the last handle closes the
/// channel, which causes the worker thread to exit cleanly on its next
/// `try_recv`.
///
/// The handle also retains an `Arc<Mutex<CanBus>>` so callers that need
/// **exclusive, blocking** access to the bus (the bench-routine handler
/// in `api/motors/bench.rs` runs `driver::rs03::tests::run_*` against the raw
/// `&CanBus` for seconds at a time) can lock the bus directly via
/// [`BusHandle::with_exclusive_bus`]. While that lock is held, the
/// worker thread will be blocked on its next `bus.recv()` call — i.e.
/// type-2 streaming pauses for the duration of the bench routine.
/// That's the same trade-off the per-iface mutex made before this
/// refactor; benches are never run during normal operator-driven
/// motion, so the safety surface is unchanged.
#[derive(Clone)]
pub struct BusHandle {
    pub(super) iface: String,
    pub(super) tx: Sender<Cmd>,
    pub(super) bus: Arc<Mutex<CanBus>>,
}

impl BusHandle {
    pub(super) fn new(iface: String, tx: Sender<Cmd>, bus: Arc<Mutex<CanBus>>) -> Self {
        Self { iface, tx, bus }
    }

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

    pub fn active_report(&self, host_id: u8, motor_id: u8, enable: bool) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::ActiveReport {
            motor_id,
            host_id,
            enable,
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

    /// Profile-position hold (`RUN_MODE=1`, `LOC_REF`, `cmd_enable` after `cmd_stop`).
    pub fn set_position_hold(
        &self,
        host_id: u8,
        motor_id: u8,
        role: &str,
        target_principal_rad: f32,
    ) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::SetPositionHold {
            motor_id,
            host_id,
            target_principal_rad,
            role: role.to_string(),
            reply: tx,
        })?;
        recv_blocking(rx, REPLY_TIMEOUT)?
    }

    /// MIT spring-damper hold. Sends a single `OperationCtrl` frame after
    /// `cmd_stop` → `RUN_MODE=0` → `cmd_enable`; the firmware then closes the
    /// loop on encoder + the standing kp/kd alone (no streamed setpoint).
    pub fn set_mit_hold(
        &self,
        host_id: u8,
        motor_id: u8,
        role: &str,
        target_principal_rad: f32,
        kp_nm_per_rad: f32,
        kd_nm_s_per_rad: f32,
    ) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::SetMitHold {
            motor_id,
            host_id,
            target_principal_rad,
            kp_nm_per_rad,
            kd_nm_s_per_rad,
            role: role.to_string(),
            reply: tx,
        })?;
        recv_blocking(rx, REPLY_TIMEOUT)?
    }

    /// Streaming MIT command (operation mode). See [`Cmd::SetMitCommand`].
    pub fn set_mit_command(
        &self,
        host_id: u8,
        motor_id: u8,
        role: &str,
        position_rad: f32,
        velocity_rad_s: f32,
        torque_ff_nm: f32,
        kp_nm_per_rad: f32,
        kd_nm_s_per_rad: f32,
    ) -> io::Result<()> {
        let (tx, rx) = mpsc::channel();
        self.submit(Cmd::SetMitCommand {
            motor_id,
            host_id,
            position_rad,
            velocity_rad_s,
            torque_ff_nm,
            kp_nm_per_rad,
            kd_nm_s_per_rad,
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

pub(super) fn recv_blocking<T>(rx: Receiver<T>, timeout: Duration) -> io::Result<T> {
    rx.recv_timeout(timeout)
        .map_err(|e| io::Error::new(io::ErrorKind::TimedOut, format!("{e}")))
}
