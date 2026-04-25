//! Wire types for motion controller status and stop reasons.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One snapshot from a running [`crate::motion::controller`]. Broadcast
/// on `state.motion_status_tx` after every successful tick (so the SPA
/// can render the live "running: sweep" badge without polling) and a
/// final time on every exit path with `state = Stopped`.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct MotionStatus {
    pub run_id: String,
    pub role: String,
    /// snake_case pattern name (`"sweep"`, `"wave"`, `"jog"`).
    pub kind: String,
    pub t_ms: i64,
    pub state: MotionState,
    /// Last commanded velocity (rad/s). Always present even when stopped
    /// so the UI can render "stopped at +0.25 rad/s" without inventing a
    /// sentinel.
    pub vel_rad_s: f32,
    /// Live position the controller saw at emission time, in rad. Saves
    /// the SPA a join against `motor_feedback` for the running-badge view.
    pub mech_pos_rad: f32,
    /// Reason this status was emitted. Always `None` while running;
    /// populated on the terminal `Stopped` frame.
    #[serde(default)]
    pub reason: Option<String>,
}

/// Discriminator for [`MotionStatus::state`]. `Running` is high-rate;
/// `Stopped` is one-shot at exit and carries the reason inline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(rename_all = "snake_case")]
pub enum MotionState {
    Running,
    Stopped,
}

/// Classified CAN / motion IO failure (replaces stringly bus errors).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MotionBusError {
    Timeout { detail: String },
    BrokenPipe,
    Encode { detail: String },
    Protocol { detail: String },
    Backpressure { detail: String },
    Spawn { detail: String },
    Other(String),
}

impl fmt::Display for MotionBusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MotionBusError::Timeout { detail } => write!(f, "timeout: {detail}"),
            MotionBusError::BrokenPipe => write!(f, "broken_pipe"),
            MotionBusError::Encode { detail } => write!(f, "encode: {detail}"),
            MotionBusError::Protocol { detail } => write!(f, "protocol: {detail}"),
            MotionBusError::Backpressure { detail } => write!(f, "backpressure: {detail}"),
            MotionBusError::Spawn { detail } => write!(f, "spawn: {detail}"),
            MotionBusError::Other(s) => write!(f, "{s}"),
        }
    }
}

/// Map worker / `spawn_blocking` error strings into structured variants.
#[must_use]
pub fn classify_motion_bus_string(msg: impl AsRef<str>) -> MotionBusError {
    let msg = msg.as_ref();
    let ml = msg.to_ascii_lowercase();
    if ml.contains("timed out") || ml.contains("timeout") {
        return MotionBusError::Timeout {
            detail: msg.to_string(),
        };
    }
    if ml.contains("broken pipe") || ml.contains("brokenpipe") {
        return MotionBusError::BrokenPipe;
    }
    if ml.contains("enobufs") || ml.contains("nobufs") {
        return MotionBusError::Backpressure {
            detail: msg.to_string(),
        };
    }
    if ml.contains("decode") || ml.contains("protocol") || ml.contains("invalid") {
        return MotionBusError::Protocol {
            detail: msg.to_string(),
        };
    }
    if ml.contains("spawn_blocking") {
        return MotionBusError::Spawn {
            detail: msg.to_string(),
        };
    }
    MotionBusError::Other(msg.to_string())
}

/// Why a controller exited. Stringified into [`MotionStatus::reason`] and
/// the audit log so post-mortem grep is straightforward.
#[derive(Debug, Clone)]
pub enum MotionStopReason {
    /// Operator clicked Stop (REST `/motion/stop` or `ClientFrame::MotionStop`).
    Operator,
    /// Tab unmounted / WT bidi stream closed.
    ClientGone,
    /// Dead-man heartbeat didn't refresh in time (jog only).
    HeartbeatLapsed,
    /// Travel-band check failed at a tick.
    TravelLimitViolation,
    /// `safety.max_feedback_age_ms` exceeded mid-run.
    StaleTelemetry,
    /// `boot_state` transitioned away from `Homed`/`InBand` (motor faulted
    /// or boot state changed mid-run).
    BootStateLost,
    /// A different motion request superseded this one for the same role.
    Superseded,
    /// Underlying CAN / worker path failed.
    Bus(MotionBusError),
    /// Motor reported non-zero fault / warning registers (type-2 / type-0x15).
    MotorFault { fault_sta: u32, warn_sta: u32 },
    /// Daemon shutdown.
    Shutdown,
}

impl MotionStopReason {
    pub fn label(&self) -> &'static str {
        match self {
            MotionStopReason::Operator => "operator",
            MotionStopReason::ClientGone => "client_gone",
            MotionStopReason::HeartbeatLapsed => "heartbeat_lapsed",
            MotionStopReason::TravelLimitViolation => "travel_limit_violation",
            MotionStopReason::StaleTelemetry => "stale_telemetry",
            MotionStopReason::BootStateLost => "boot_state_lost",
            MotionStopReason::Superseded => "superseded",
            MotionStopReason::Bus(_) => "bus_error",
            MotionStopReason::MotorFault { .. } => "motor_fault",
            MotionStopReason::Shutdown => "shutdown",
        }
    }

    /// Free-form detail (e.g. inner CAN error message). Used on the audit
    /// entry's `details` field; falls back to the label when there's no
    /// extra information.
    pub fn detail(&self) -> String {
        match self {
            MotionStopReason::Bus(e) => e.to_string(),
            MotionStopReason::MotorFault {
                fault_sta,
                warn_sta,
            } => format!("fault_sta=0x{fault_sta:08x} warn_sta=0x{warn_sta:08x}"),
            other => other.label().into(),
        }
    }
}
