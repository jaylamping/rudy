//! Per-bus worker command types and pending-reply bookkeeping.

use std::io;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// Soft cap on commands drained per loop iteration, so a backlog of
/// jog frames can't starve the `recv()` side.
pub(super) const CMD_DRAIN_BATCH: usize = 8;

/// Receive-side timeout. Sets the minimum wakeup cadence of the worker
/// loop (so it can service queued commands when no frames are arriving).
///
/// `pub(crate)` so the scan path (`can::handle::scan`) can re-arm this
/// value on the socket after temporarily widening it for the broadcast
/// drain in `driver::rs03::session::broadcast_device_id_scan`.
pub(crate) const RECV_POLL_TIMEOUT: Duration = Duration::from_millis(5);

/// Default round-trip timeout for worker commands that expect a reply
/// on the per-bus `mpsc` channel (`BusHandle::enable`, `set_velocity`,
/// etc.). Must exceed the worst-case handler duration: `Cmd::SetVelocity`
/// re-arm deliberately sleeps ~60 ms (two 30 ms firmware settle windows)
/// plus several CAN round-trips before `reply.send`, so 50 ms was too
/// tight and surfaced as `can_command_failed: timed out waiting on
/// channel` with `ticks=1` / `total_can_sends=0` in `home_ramp`.
pub const REPLY_TIMEOUT: Duration = Duration::from_millis(200);

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
    /// `cmd_active_report` (type-24 on RS03).
    ActiveReport {
        motor_id: u8,
        host_id: u8,
        enable: bool,
        reply: Sender<io::Result<()>>,
    },
    /// Velocity-mode setpoint. Smart re-arm: `cmd_stop` → `RUN_MODE` →
    /// `SPD_REF` → `cmd_enable` when re-arming (first frame or after a
    /// PP/MIT hold). Subsequent calls send only `SPD_REF` so steady-state
    /// bus traffic is one frame per tick.
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
    /// Profile-position hold: `cmd_stop` → `RUN_MODE=1` → `LOC_REF` → `cmd_enable`.
    /// `target_principal_rad` is defensive Principal-angle (−π, π] (see `home_ramp`).
    SetPositionHold {
        motor_id: u8,
        host_id: u8,
        target_principal_rad: f32,
        role: String,
        reply: Sender<io::Result<()>>,
    },
    /// MIT spring-damper hold (operation mode, `run_mode = 0`). Sequence:
    /// `cmd_stop` → `RUN_MODE = 0` → `cmd_enable` → single MIT control frame
    /// `(target_principal_rad, vel = 0, torque_ff = 0, kp, kd)`.
    ///
    /// After this single frame the firmware closes the loop on encoder + the
    /// standing kp/kd values **without** streaming a velocity setpoint, so
    /// there is no audible servo whine and no continuous current draw the
    /// way `Cmd::SetPositionHold` (PP, `run_mode = 1`) would produce. This is
    /// the post-home hold cortex actually uses; PP hold stays available for
    /// future stiff-positioning use cases.
    SetMitHold {
        motor_id: u8,
        host_id: u8,
        target_principal_rad: f32,
        kp_nm_per_rad: f32,
        kd_nm_s_per_rad: f32,
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
pub(crate) struct PendingKey {
    pub(crate) motor_id: u8,
    pub(crate) index: u16,
}

/// Per-pending entry held inside the worker thread.
pub(super) struct PendingEntry {
    pub(super) reply: Sender<io::Result<ReplyBytes>>,
    pub(super) deadline: Instant,
}
