//! Wire types for the server-side motion controller.
//!
//! Two pieces fit together here:
//!
//! - [`MotionIntent`]: what the operator wants the motor to do. Built once
//!   per motion run (from a REST POST or an inbound WT `ClientFrame`) and
//!   handed to the controller. Intent variants carry only the parameters
//!   the per-pattern step function needs to compute the next velocity
//!   setpoint — there is no per-tick state in here.
//! - [`MotionStatus`]: what the controller broadcasts back to subscribers
//!   over the new `motion_status` WT stream. Same shape for every pattern;
//!   the variant on `state` distinguishes "running, here's the latest
//!   sample" from "stopped, here's why."
//!
//! See `crate::motion` for why this lives in its own module rather than
//! glued onto the existing `jog.rs` REST handler. Short version: every
//! closed-loop motion belongs server-side, and the SPA only ever expresses
//! intent + observes status.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Closed-loop motion pattern the operator asked for. Carried in the
/// `MotionRegistry` for the lifetime of one run; the per-pattern step
/// function in [`crate::motion::sweep`] / [`crate::motion::wave`] /
/// [`crate::motion::jog`] reads it on every tick.
///
/// The variants are intentionally minimal: if a pattern needs derived
/// state (turnaround direction, the band-edge midpoint, etc.) the
/// controller computes it once at start and stashes it inline. Keeping
/// `MotionIntent` parameter-only makes "operator updated the slider
/// mid-run" a single `swap` on a `watch::Sender` rather than a
/// destructive restart.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MotionIntent {
    /// Sweep the full configured travel band, reversing just shy of each
    /// edge. The controller reads `travel_limits` from inventory each tick
    /// (so a mid-run band edit is honored) and clamps the velocity to the
    /// firmware envelope.
    Sweep {
        /// Magnitude of the velocity setpoint, in rad/s. Always positive
        /// here; the controller alternates the sign at each band edge.
        speed_rad_s: f32,
        /// Inset from each band edge at which the controller flips
        /// direction. When the REST handler omits it, the daemon picks
        /// `SWEEP_BASE_INSET_RAD + speed_rad_s * OVERSHOOT_S` so the
        /// buffer scales with the motor's brake distance — a 0.5 rad/s
        /// sweep gets ~10° of headroom, a 0.05 rad/s sweep ~3.6°. See
        /// [`default_turnaround_rad`]. Larger values trade range for a
        /// softer turnaround.
        turnaround_rad: f32,
    },
    /// Symmetric oscillation around `center_rad` with `amplitude_rad`
    /// half-swing. The center is captured at start and clipped against
    /// the band so a mid-run band shrink doesn't push the wave outside.
    Wave {
        center_rad: f32,
        amplitude_rad: f32,
        speed_rad_s: f32,
        /// Same role as `Sweep::turnaround_rad`; defaults to
        /// `WAVE_BASE_INSET_RAD + speed_rad_s * OVERSHOOT_S` when
        /// omitted. See [`default_turnaround_rad`].
        turnaround_rad: f32,
    },
    /// Hold-to-jog: drive at `vel_rad_s` for as long as the operator's
    /// dead-man signal stays alive. The controller re-arms the deadline
    /// on every heartbeat (REST or WT); when it lapses the controller
    /// stops on its own with `MotionStopReason::HeartbeatLapsed`.
    ///
    /// The deadline lives in the controller's private state rather than
    /// the intent so a heartbeat can refresh it cheaply (`watch::send`
    /// would re-emit the entire intent). The intent is updated only
    /// when the *velocity* changes (slider drag while held).
    Jog { vel_rad_s: f32 },
}

impl MotionIntent {
    /// snake_case discriminator used in the audit log and `MotionStatus`
    /// envelope. Stable wire identity even if the variants gain fields.
    pub fn kind_str(&self) -> &'static str {
        match self {
            MotionIntent::Sweep { .. } => "sweep",
            MotionIntent::Wave { .. } => "wave",
            MotionIntent::Jog { .. } => "jog",
        }
    }
}

/// One snapshot from a running [`crate::motion::controller`]. Broadcast
/// on `state.motion_status_tx` after every successful tick (so the SPA
/// can render the live "running: sweep" badge without polling) and a
/// final time on every exit path with `state = Stopped`.
///
/// `t_ms` is wallclock at emission so subscribers can detect a stalled
/// controller by comparing to `Date.now()`. `vel_rad_s` is whatever the
/// controller most recently *commanded* (post-clamp), not what the motor
/// reports — pair with `MotorFeedback::mech_vel_rad_s` for the measured
/// value.
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

/// Base turnaround inset, in rad. Covers the algorithmic margin (the
/// step function flips direction when `pos >= edge - inset`, so we need
/// at least one tick's worth of headroom even at v=0). Sized to match
/// the original fixed defaults so a low-speed sweep behaves exactly as
/// before this scaling was introduced.
pub const SWEEP_BASE_INSET_RAD: f32 = 0.05;
pub const WAVE_BASE_INSET_RAD: f32 = 0.02;

/// Per-rad/s overshoot allowance, in seconds. Multiplied by the
/// commanded speed to estimate how far the motor will coast past the
/// turnaround threshold before the velocity loop reverses it. Tuned for
/// the RS03 at default gains; if you measure actual overshoot from
/// `MotionStatus { state = stopped, reason = "travel_limit_violation" }`
/// frames, scale this up so the motor stops *inside* the travel band
/// rather than asymptotically approaching the edge.
///
/// Physical intuition: at v rad/s with deceleration ~1/T_OVERSHOOT, the
/// stopping distance is roughly v * T_OVERSHOOT / 2.
///
/// Empirical re-tune (2 rad/s sweep against a 60° band): with the
/// previous 0.25 value the controller flipped at 28.5° and peaks landed
/// at ~38–40°, leaving ~20° of unused range. Measured overshoot past
/// the flip threshold was ~0.17–0.21 rad, implying an effective T of
/// ~0.085–0.105 s. Bumped to 0.15 to keep a comfortable safety margin
/// (~8° to the limit at 2 rad/s) without giving up the conservative
/// `v * T` framing — if you ever see `travel_limit_violation` come
/// back, raise this rather than dropping it further.
pub const OVERSHOOT_S: f32 = 0.15;

/// Resolve the default turnaround inset for a given pattern when the
/// REST handler / client frame omits it. Centralised so the SPA doesn't
/// have to know the magic numbers.
///
/// `speed_rad_s` is the magnitude of the commanded velocity; the
/// returned inset grows linearly with speed so a fast sweep gets a
/// proportionally larger brake-distance buffer. This is the fix for the
/// "controller exits with `travel_limit_violation` because the motor
/// coasts past the band edge after the direction flip" failure mode —
/// see `docs/decisions/0004-operator-console.md` and the discussion in
/// the original PR if you're tempted to revert to a fixed value.
pub fn default_turnaround_rad(kind: &MotionIntent, speed_rad_s: f32) -> f32 {
    let speed = speed_rad_s.abs();
    let base = match kind {
        MotionIntent::Sweep { .. } => SWEEP_BASE_INSET_RAD,
        MotionIntent::Wave { .. } => WAVE_BASE_INSET_RAD,
        MotionIntent::Jog { .. } => return 0.0,
    };
    base + speed * OVERSHOOT_S
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_kind_str_matches_serde_tag() {
        // The kind_str helper is the canonical wire identity used by the
        // audit log; serde's tag serialization must produce the same
        // string so log entries and WT frames line up.
        let cases = [
            (
                MotionIntent::Sweep {
                    speed_rad_s: 0.1,
                    turnaround_rad: 0.05,
                },
                "sweep",
            ),
            (
                MotionIntent::Wave {
                    center_rad: 0.0,
                    amplitude_rad: 0.5,
                    speed_rad_s: 0.1,
                    turnaround_rad: 0.02,
                },
                "wave",
            ),
            (MotionIntent::Jog { vel_rad_s: 0.1 }, "jog"),
        ];
        for (intent, expected) in cases {
            assert_eq!(intent.kind_str(), expected);
            let json = serde_json::to_value(&intent).unwrap();
            assert_eq!(json["kind"], expected);
        }
    }

    #[test]
    fn motion_state_serializes_snake_case() {
        // ts-rs exports the same casing; the SPA literally compares to
        // these strings.
        let s = serde_json::to_string(&MotionState::Running).unwrap();
        assert_eq!(s, r#""running""#);
        let s = serde_json::to_string(&MotionState::Stopped).unwrap();
        assert_eq!(s, r#""stopped""#);
    }

    #[test]
    fn stop_reason_label_matches_audit_contract() {
        // The SPA matches on these labels for stop-reason tooltips; once
        // a label ships it can't be silently renamed.
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

    #[test]
    fn default_turnaround_scales_with_speed() {
        let sweep = MotionIntent::Sweep {
            speed_rad_s: 0.0,
            turnaround_rad: 0.0,
        };
        // At v=0 the formula collapses to the algorithmic base inset —
        // identical to the previous fixed default, so a slow-speed sweep
        // doesn't lose any range.
        let zero = default_turnaround_rad(&sweep, 0.0);
        assert!((zero - SWEEP_BASE_INSET_RAD).abs() < 1e-6);
        // At 0.5 rad/s (the previous UI cap) the inset is base + a
        // small brake-distance buffer ≈ 0.125 rad ≈ 7°.
        let mid = default_turnaround_rad(&sweep, 0.5);
        assert!((mid - (SWEEP_BASE_INSET_RAD + 0.5 * OVERSHOOT_S)).abs() < 1e-6);
        // At 2.0 rad/s (the new UI cap) the inset is ~0.35 rad ≈ 20°,
        // which lands peak excursion ~8° inside a 60° band — enough
        // headroom for run-to-run variance in the brake distance.
        let fast = default_turnaround_rad(&sweep, 2.0);
        assert!((fast - (SWEEP_BASE_INSET_RAD + 2.0 * OVERSHOOT_S)).abs() < 1e-6);
    }

    #[test]
    fn default_turnaround_uses_per_pattern_base() {
        // Wave gets a tighter base inset than sweep (oscillation is
        // usually around a fixed center, the band edges are a backstop
        // not the operating envelope).
        let sweep = default_turnaround_rad(
            &MotionIntent::Sweep {
                speed_rad_s: 0.0,
                turnaround_rad: 0.0,
            },
            0.0,
        );
        let wave = default_turnaround_rad(
            &MotionIntent::Wave {
                center_rad: 0.0,
                amplitude_rad: 0.0,
                speed_rad_s: 0.0,
                turnaround_rad: 0.0,
            },
            0.0,
        );
        assert!(sweep > wave);
        assert!((sweep - SWEEP_BASE_INSET_RAD).abs() < 1e-6);
        assert!((wave - WAVE_BASE_INSET_RAD).abs() < 1e-6);
    }

    #[test]
    fn default_turnaround_is_always_zero_for_jog() {
        // Jog has no turnaround concept; the dead-man timeout is what
        // bounds the motion. Pinned so a future refactor that adds an
        // inset field to MotionIntent::Jog has to opt in deliberately.
        let v = default_turnaround_rad(&MotionIntent::Jog { vel_rad_s: 0.5 }, 0.5);
        assert_eq!(v, 0.0);
    }

    #[test]
    fn default_turnaround_treats_negative_speed_as_magnitude() {
        // The formula is direction-agnostic; a sweep with `speed_rad_s =
        // -0.5` (which clamp_speed wouldn't produce, but a future
        // caller might) must not produce a negative inset that would
        // expand the band rather than shrink it.
        let sweep = MotionIntent::Sweep {
            speed_rad_s: 0.0,
            turnaround_rad: 0.0,
        };
        let pos = default_turnaround_rad(&sweep, 0.5);
        let neg = default_turnaround_rad(&sweep, -0.5);
        assert!((pos - neg).abs() < 1e-6);
        assert!(neg > 0.0);
    }
}
