//! Shared preflight checks for any code path that issues a velocity
//! setpoint or a **target position** (MIT streaming).
//!
//! Lifted verbatim from the equivalent gauntlet at the top of
//! [`crate::api::jog::jog`] so REST [`crate::api::motion`], the WebTransport
//! [`crate::motion::registry`] entry, and the per-tick body of
//! [`crate::motion::controller`] all run identical checks. The "REST jog
//! and WT jog disagreed about whether telemetry was stale" failure mode
//! is impossible by construction once everyone calls [`PreflightChecks::run`].
//!
//! What gets checked, in order (cheap → expensive):
//!
//! 1. Inventory presence + `present` / `verified` flags.
//! 2. Boot-state gate (only `InBand` and `Homed` permit motion).
//! 3. Stale-telemetry refusal (`safety.max_feedback_age_ms`).
//! 4. Active motor faults: non-zero `fault_sta` or fatal `warn_sta` bits on the fresh row.
//! 5. Path-aware travel-band check on the projected position (velocity
//!    projection) **or** on `current → target_position_rad` when set, via
//!    [`crate::can::travel::enforce_position_with_path`].
//! 6. While not `Homed`, step delta vs. `safety.boot_max_step_rad` ceiling.
//!
//! Returns a [`PreflightFailure`] enum so the REST layer can map to its
//! existing 4xx codes and the controller can map to a
//! [`crate::motion::status::MotionStopReason`] without re-classifying
//! strings.

use chrono::Utc;
use std::sync::atomic::Ordering;
use tracing::warn;

use crate::boot_state::{self, BootState};
use crate::can::angle::UnwrappedAngle;
use crate::can::motion::shortest_signed_delta;
use crate::can::travel::{enforce_position_with_path, BandCheck};
use crate::inventory::Actuator;
use crate::state::SharedState;
use crate::types::{LimbQuarantineMotor, MotorFeedback};

pub(crate) fn has_fatal_fault_or_warning(
    fault_sta: u32,
    warn_sta: u32,
    fatal_fault_mask: u32,
    fatal_warn_mask: u32,
) -> bool {
    (fault_sta & fatal_fault_mask) != 0 || (warn_sta & fatal_warn_mask) != 0
}

/// Why a motion request was refused. Each variant maps to a distinct
/// REST status code and a distinct
/// [`crate::motion::status::MotionStopReason`].
#[derive(Debug, Clone)]
pub enum PreflightFailure {
    UnknownActuator,
    Absent,
    NotVerified,
    BootNotReady {
        detail: String,
    },
    BootOutOfBand {
        mech_pos_rad: f32,
        min_rad: f32,
        max_rad: f32,
    },
    StaleTelemetry {
        age_ms: i64,
        max_age_ms: i64,
        last_type2_age_ms: Option<i64>,
    },
    NoTelemetry,
    OutOfBand {
        attempted_rad: f32,
        min_rad: f32,
        max_rad: f32,
    },
    PathViolation {
        current_rad: f32,
        target_rad: f32,
        min_rad: f32,
        max_rad: f32,
    },
    StepTooLarge {
        delta_rad: f32,
        cap_rad: f32,
    },
    /// Another motor on the same limb is `OutOfBand`, `OffsetChanged`, or
    /// `HomeFailed`; refuse starting or continuing closed-loop motion.
    LimbQuarantined {
        limb: String,
        failed_motors: Vec<LimbQuarantineMotor>,
    },
    /// Runtime DB re-seeded from seed files; operator must `POST /api/settings/recovery/ack`.
    SettingsRecovery,
    /// Decoded `fault_sta` / `warn_sta` from latest telemetry (type-2 + type-0x15).
    ActiveFault {
        fault_sta: u32,
        warn_sta: u32,
    },
    Internal(String),
}

impl PreflightFailure {
    /// Short snake_case identifier matching the REST `error` field for
    /// each failure shape. The SPA matches on these strings already.
    pub fn code(&self) -> &'static str {
        match self {
            PreflightFailure::UnknownActuator => "unknown_motor",
            PreflightFailure::Absent => "motor_absent",
            PreflightFailure::NotVerified => "not_verified",
            PreflightFailure::BootNotReady { .. } => "not_ready",
            PreflightFailure::BootOutOfBand { .. } => "out_of_band",
            PreflightFailure::StaleTelemetry { .. } => "stale_telemetry",
            PreflightFailure::NoTelemetry => "stale_telemetry",
            PreflightFailure::OutOfBand { .. } => "travel_limit_violation",
            PreflightFailure::PathViolation { .. } => "path_violation",
            PreflightFailure::StepTooLarge { .. } => "step_too_large",
            PreflightFailure::LimbQuarantined { .. } => "limb_quarantined",
            PreflightFailure::SettingsRecovery => "settings_recovery",
            PreflightFailure::ActiveFault { .. } => "motor_fault",
            PreflightFailure::Internal(_) => "internal",
        }
    }

    /// Human-readable detail for the error envelope and audit log.
    pub fn detail(&self, role: &str) -> String {
        match self {
            PreflightFailure::UnknownActuator => format!("no motor with role={role}"),
            PreflightFailure::Absent => {
                format!("inventory entry for {role} has present=false")
            }
            PreflightFailure::NotVerified => format!(
                "inventory entry for {role} has verified=false; commission before motion"
            ),
            PreflightFailure::BootNotReady { detail } => detail.clone(),
            PreflightFailure::BootOutOfBand {
                mech_pos_rad,
                min_rad,
                max_rad,
            } => format!(
                "{role} is at {mech_pos_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]; manual recovery required"
            ),
            PreflightFailure::StaleTelemetry {
                age_ms,
                max_age_ms,
                last_type2_age_ms,
            } => {
                let type2 = last_type2_age_ms
                    .map(|t| format!("{t} ms"))
                    .unwrap_or_else(|| "never".into());
                format!(
                    "feedback for {role} is {age_ms} ms old (> {max_age_ms} ms); last type-2 frame {type2} ago; refusing motion"
                )
            }
            PreflightFailure::NoTelemetry => format!("no fresh feedback for {role}; refusing motion"),
            PreflightFailure::OutOfBand {
                attempted_rad,
                min_rad,
                max_rad,
            } => format!(
                "projected position {attempted_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]"
            ),
            PreflightFailure::PathViolation {
                current_rad,
                target_rad,
                min_rad,
                max_rad,
            } => format!(
                "current {current_rad:.3} -> target {target_rad:.3} sweeps outside [{min_rad:.3}, {max_rad:.3}]"
            ),
            PreflightFailure::StepTooLarge { delta_rad, cap_rad } => format!(
                "projected delta {delta_rad:.3} rad exceeds boot_max_step_rad {cap_rad:.3} rad; run /home first"
            ),
            PreflightFailure::LimbQuarantined { limb, failed_motors } => {
                let names = failed_motors
                    .iter()
                    .map(|f| f.role.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("limb {limb} quarantined ({names})")
            }
            PreflightFailure::SettingsRecovery => {
                "settings DB recovered from seed — acknowledge in Settings before motion".to_string()
            }
            PreflightFailure::ActiveFault {
                fault_sta,
                warn_sta,
            } => format!(
                "{role} reports fault_sta=0x{fault_sta:08x} warn_sta=0x{warn_sta:08x}; clear faults before motion"
            ),
            PreflightFailure::Internal(s) => s.clone(),
        }
    }
}

/// Output of a successful preflight: the freshly-resolved motor entry,
/// the (live, fresh) feedback row used for the band projection, and the
/// boot state at check time. Callers stash these so the first controller
/// tick doesn't have to re-read.
#[derive(Debug, Clone)]
pub struct PreflightOk {
    pub motor: Actuator,
    pub feedback: MotorFeedback,
    pub boot_state: BootState,
}

/// Inputs needed for a single preflight pass.
///
/// **Velocity mode:** `vel_rad_s` and `horizon_ms` project where the motor
/// would be after the upcoming frame (matches jog projection).
///
/// **Position mode:** when `target_position_rad` is `Some`, the band/path
/// check uses that target instead of the velocity projection; `vel_rad_s`
/// / `horizon_ms` are ignored for the geometric check (staleness/boot gates
/// still apply).
pub struct PreflightChecks<'a> {
    pub state: &'a SharedState,
    pub role: &'a str,
    /// Velocity the controller is about to command, in rad/s. Used for
    /// the projection only when [`Self::target_position_rad`] is `None`.
    pub vel_rad_s: f32,
    /// Projection horizon when using velocity projection.
    pub horizon_ms: u64,
    /// When `Some`, validate path/band/step against this logical target
    /// position (rad) instead of `feedback + vel * horizon`.
    pub target_position_rad: Option<f32>,
}

impl PreflightChecks<'_> {
    /// Run every check. On success the caller has everything it needs
    /// (motor, latest feedback, boot state) without re-reading the locks.
    pub fn run(&self) -> Result<PreflightOk, PreflightFailure> {
        if self.state.settings_recovery_pending.load(Ordering::SeqCst) {
            return Err(PreflightFailure::SettingsRecovery);
        }
        let motor = {
            let inv = self.state.inventory.read().expect("inventory poisoned");
            inv.actuator_by_role(self.role).cloned()
        };
        let motor = motor.ok_or(PreflightFailure::UnknownActuator)?;

        if !motor.common.present {
            return Err(PreflightFailure::Absent);
        }
        if self.state.read_effective().safety.require_verified && !motor.common.verified {
            return Err(PreflightFailure::NotVerified);
        }

        let limb_id = crate::limb_health::effective_limb_id(&motor);
        let failed =
            crate::limb_health::sibling_quarantine_failures(self.state, &limb_id, self.role);
        if !failed.is_empty() {
            return Err(PreflightFailure::LimbQuarantined {
                limb: limb_id,
                failed_motors: failed
                    .iter()
                    .map(|(role, bs)| LimbQuarantineMotor {
                        role: role.clone(),
                        state_kind: crate::limb_health::boot_state_kind_snake(bs).to_string(),
                    })
                    .collect(),
            });
        }

        let bs = boot_state::current(self.state, self.role);
        match bs {
            BootState::Unknown => {
                return Err(PreflightFailure::BootNotReady {
                    detail: format!("no telemetry yet for {}; cannot start motion", self.role),
                });
            }
            BootState::OutOfBand {
                mech_pos_rad,
                min_rad,
                max_rad,
            } => {
                return Err(PreflightFailure::BootOutOfBand {
                    mech_pos_rad,
                    min_rad,
                    max_rad,
                });
            }
            BootState::OffsetChanged {
                stored_rad,
                current_rad,
            } => {
                return Err(PreflightFailure::BootNotReady {
                    detail: format!(
                        "{} commissioned_zero_offset disagrees with firmware (stored={stored_rad:.4}, current={current_rad:.4}); restore or re-commission before motion",
                        self.role
                    ),
                });
            }
            BootState::AutoHoming { .. } => {
                return Err(PreflightFailure::BootNotReady {
                    detail: format!("{} is auto-homing; wait before starting motion", self.role),
                });
            }
            BootState::HomeFailed {
                reason,
                last_pos_rad,
            } => {
                return Err(PreflightFailure::BootNotReady {
                    detail: format!(
                        "{} home failed ({reason}) at {last_pos_rad:.3} rad; retry /home before motion",
                        self.role
                    ),
                });
            }
            BootState::InBand | BootState::Homed => {}
        }

        let max_age_ms = self.state.read_effective().safety.max_feedback_age_ms as i64;
        let now_ms = Utc::now().timestamp_millis();
        let feedback = match self
            .state
            .latest
            .read()
            .expect("latest poisoned")
            .get(self.role)
            .cloned()
        {
            Some(fb) if now_ms.saturating_sub(fb.t_ms) <= max_age_ms => fb,
            Some(fb) => {
                let last_type2 = self
                    .state
                    .last_type2_at
                    .read()
                    .expect("last_type2_at poisoned")
                    .get(self.role)
                    .copied();
                return Err(PreflightFailure::StaleTelemetry {
                    age_ms: now_ms.saturating_sub(fb.t_ms),
                    max_age_ms,
                    last_type2_age_ms: last_type2.map(|t| now_ms.saturating_sub(t)),
                });
            }
            None => return Err(PreflightFailure::NoTelemetry),
        };

        if self.state.real_can.is_some() {
            let last_type2 = self
                .state
                .last_type2_at
                .read()
                .expect("last_type2_at poisoned")
                .get(self.role)
                .copied();
            match last_type2 {
                Some(t) if now_ms.saturating_sub(t) <= max_age_ms => {}
                Some(t) => {
                    warn!(
                        role = %self.role,
                        feedback_age_ms = now_ms.saturating_sub(feedback.t_ms),
                        last_type2_age_ms = now_ms.saturating_sub(t),
                        max_age_ms,
                        "motion preflight refused: stale type-2 position telemetry"
                    );
                    return Err(PreflightFailure::StaleTelemetry {
                        age_ms: now_ms.saturating_sub(feedback.t_ms),
                        max_age_ms,
                        last_type2_age_ms: Some(now_ms.saturating_sub(t)),
                    });
                }
                None => {
                    warn!(
                        role = %self.role,
                        feedback_age_ms = now_ms.saturating_sub(feedback.t_ms),
                        max_age_ms,
                        "motion preflight refused: no type-2 position telemetry decoded"
                    );
                    return Err(PreflightFailure::StaleTelemetry {
                        age_ms: now_ms.saturating_sub(feedback.t_ms),
                        max_age_ms,
                        last_type2_age_ms: None,
                    });
                }
            }
        }

        if has_fatal_fault_or_warning(
            feedback.fault_sta,
            feedback.warn_sta,
            self.state.read_effective().safety.fatal_fault_mask,
            self.state.read_effective().safety.fatal_warn_mask,
        ) {
            return Err(PreflightFailure::ActiveFault {
                fault_sta: feedback.fault_sta,
                warn_sta: feedback.warn_sta,
            });
        }

        let projected = match self.target_position_rad {
            Some(t) => t,
            None => feedback.mech_pos_rad + self.vel_rad_s * (self.horizon_ms as f32 / 1000.0),
        };
        let check = enforce_position_with_path(
            self.state,
            self.role,
            UnwrappedAngle::new(feedback.mech_pos_rad),
            UnwrappedAngle::new(projected),
        )
        .map_err(|e| PreflightFailure::Internal(format!("{e:#}")))?;

        match check {
            BandCheck::OutOfBand {
                min_rad,
                max_rad,
                attempted_rad,
            } => {
                return Err(PreflightFailure::OutOfBand {
                    attempted_rad,
                    min_rad,
                    max_rad,
                });
            }
            BandCheck::PathViolation {
                min_rad,
                max_rad,
                current_rad,
                target_rad,
            } => {
                return Err(PreflightFailure::PathViolation {
                    current_rad,
                    target_rad,
                    min_rad,
                    max_rad,
                });
            }
            _ => {}
        }

        if !matches!(bs, BootState::Homed) {
            let delta = shortest_signed_delta(feedback.mech_pos_rad, projected).abs();
            let cap = self.state.read_effective().safety.boot_max_step_rad;
            if delta > cap {
                return Err(PreflightFailure::StepTooLarge {
                    delta_rad: delta,
                    cap_rad: cap,
                });
            }
        }

        Ok(PreflightOk {
            motor,
            feedback,
            boot_state: bs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::has_fatal_fault_or_warning;

    #[test]
    fn nonfatal_rs03_warn_bit5_does_not_block_motion_by_default() {
        assert!(!has_fatal_fault_or_warning(0, 0x20, !0x20, 0x1));
    }

    #[test]
    fn nonfatal_rs03_fault_bit5_does_not_block_motion_by_default() {
        assert!(!has_fatal_fault_or_warning(0x20, 0, !0x20, 0x1));
    }

    #[test]
    fn documented_motor_overtemp_warning_blocks_motion_by_default() {
        assert!(has_fatal_fault_or_warning(0, 0x1, !0x20, 0x1));
    }

    #[test]
    fn any_fault_status_blocks_even_when_warning_is_nonfatal() {
        assert!(has_fatal_fault_or_warning(0x80, 0x20, !0x20, 0x1));
    }

    #[test]
    fn operator_can_widen_fatal_warning_mask() {
        assert!(has_fatal_fault_or_warning(0, 0x20, !0x20, 0x21));
    }

    #[test]
    fn operator_can_widen_fatal_fault_mask() {
        assert!(has_fatal_fault_or_warning(0x20, 0, u32::MAX, 0x1));
    }
}
