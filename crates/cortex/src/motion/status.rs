//! Wire types for motion controller status and stop reasons.

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
    /// Underlying CAN call failed.
    BusError(String),
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
            MotionStopReason::BusError(_) => "bus_error",
            MotionStopReason::Shutdown => "shutdown",
        }
    }

    /// Free-form detail (e.g. inner CAN error message). Used on the audit
    /// entry's `details` field; falls back to the label when there's no
    /// extra information.
    pub fn detail(&self) -> String {
        match self {
            MotionStopReason::BusError(e) => e.clone(),
            other => other.label().into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_state_serializes_snake_case() {
        let s = serde_json::to_string(&MotionState::Running).unwrap();
        assert_eq!(s, r#""running""#);
        let s = serde_json::to_string(&MotionState::Stopped).unwrap();
        assert_eq!(s, r#""stopped""#);
    }

    #[test]
    fn stop_reason_label_matches_audit_contract() {
        assert_eq!(MotionStopReason::Operator.label(), "operator");
        assert_eq!(
            MotionStopReason::HeartbeatLapsed.label(),
            "heartbeat_lapsed"
        );
        assert_eq!(MotionStopReason::Superseded.label(), "superseded");
        assert_eq!(
            MotionStopReason::BusError("nope".into()).label(),
            "bus_error"
        );
    }

    #[test]
    fn stop_reason_detail_carries_inner_error() {
        let r = MotionStopReason::BusError("ENOBUFS".into());
        assert_eq!(r.detail(), "ENOBUFS");
        let r = MotionStopReason::Operator;
        assert_eq!(r.detail(), "operator");
    }
}
