//! Per-bus worker command types and pending-reply bookkeeping.

use std::io;
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

/// Soft cap on commands drained per loop iteration, so a backlog of
/// jog frames can't starve the `recv()` side.
pub(super) const CMD_DRAIN_BATCH: usize = 8;

/// Receive-side timeout. Sets the minimum wakeup cadence of the worker
/// loop (so it can service queued commands when no frames are arriving).
pub(super) const RECV_POLL_TIMEOUT: Duration = Duration::from_millis(5);

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
    /// `cmd_active_report` (type-24 on RS03).
    ActiveReport {
        motor_id: u8,
        host_id: u8,
        enable: bool,
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
pub(crate) struct PendingKey {
    pub(crate) motor_id: u8,
    pub(crate) index: u16,
}

/// Per-pending entry held inside the worker thread.
pub(super) struct PendingEntry {
    pub(super) reply: Sender<io::Result<ReplyBytes>>,
    pub(super) deadline: Instant,
}
