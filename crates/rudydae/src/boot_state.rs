//! Per-power-cycle classification of where each motor's reported position
//! sits relative to its configured travel band.
//!
//! Lives separately from the on-disk `verified` / `present` flags in
//! `inventory.yaml` because boot state is **per-power-cycle** — a motor that
//! the operator homed yesterday is still `Unknown` after a daemon restart
//! today. The classifier runs from the telemetry loop on every successful
//! `read_live_feedback`; the enable handler consults it to refuse motion
//! commands until the operator runs the slow-ramp homer.
//!
//! See `.cursor/plans/boot-time_travel-band_gate_*.plan.md` for the full
//! disaster-prevention rationale (multi-turn-encoder confusion across
//! power-off mechanical motion).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::can::motion::{shortest_signed_delta, wrap_to_pi};
use crate::inventory::TravelLimits;
use crate::state::SharedState;
use crate::types::SafetyEvent;

/// One motor's boot-time gate state. Initialized to `Unknown` for every
/// present motor at daemon start; transitions through the states described
/// in the boot-flow diagram. Only `Homed` permits `enable`.
///
/// Not `Copy`: `HomeFailed` carries the abort reason as a `String` so the
/// SPA can show what went wrong without a follow-up audit-log query.
/// All callers that previously relied on implicit copies use `.cloned()`
/// or pattern-bind by reference.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BootState {
    /// No telemetry yet, or last classification couldn't decide. The enable
    /// handler refuses commands in this state — a stale or missing read is
    /// not safe to act on.
    Unknown,
    /// Position read OK; motor sits outside its `travel_limits` band by
    /// more than the auto-recovery budget (or auto-recovery is disabled /
    /// failed). Operator must physically move the joint into band.
    OutOfBand {
        mech_pos_rad: f32,
        min_rad: f32,
        max_rad: f32,
    },
    /// Layer 6 routine is currently driving the motor toward the band edge.
    /// All other command paths are refused until this finishes (success or
    /// failure transitions to `InBand` or `OutOfBand` respectively).
    ///
    /// Slated for removal alongside `crate::can::auto_recovery` in Phase
    /// H.1 of the commissioned-zero plan; the boot orchestrator's
    /// `AutoHoming` variant supersedes it. Kept in place for now so
    /// Layer 6 continues to function during the migration window.
    AutoRecovering {
        from_rad: f32,
        target_rad: f32,
        progress_rad: f32,
    },
    /// Position confirmed inside band, but the operator hasn't done the
    /// Verify & Home ritual yet. Enable is still refused; per-step ceiling
    /// still applies to any command that does land.
    InBand,
    /// Operator clicked Verify & Home, the slow-ramp homer reached its
    /// target without faulting. Full per-motor torque/speed limits restored.
    /// This is the only state in which `enable` is allowed.
    Homed,
    /// Class-1 shenanigan detected by the boot orchestrator: the
    /// firmware's reported `add_offset` (0x702B) disagrees with the
    /// `commissioned_zero_offset` recorded in `inventory.yaml` by more
    /// than `safety.commission_readback_tolerance_rad`. Refuses motion
    /// until the operator either re-commissions (POST /commission, the
    /// new position becomes the recorded zero) or restores
    /// (POST /restore_offset, the daemon writes the stored value back
    /// to firmware and re-saves to flash).
    OffsetChanged {
        stored_rad: f32,
        current_rad: f32,
    },
    /// Boot orchestrator is currently driving the motor toward
    /// `predefined_home_rad` via the slow-ramp homer
    /// (`crate::can::slow_ramp::run`). Refuses operator-initiated motion
    /// until the homer finishes (Homed) or aborts (HomeFailed).
    /// `progress_rad` is the orchestrator's tick-by-tick estimate so the
    /// SPA can render a progress bar without polling.
    AutoHoming {
        from_rad: f32,
        target_rad: f32,
        progress_rad: f32,
    },
    /// Boot orchestrator's slow-ramp homer aborted (tracking error,
    /// fault, timeout, path violation). Operator must investigate and
    /// either retry via `POST /api/motors/:role/home` or re-position
    /// the joint manually. Carries the abort reason from
    /// `slow_ramp::run` and the last position the homer saw.
    HomeFailed {
        reason: String,
        last_pos_rad: f32,
    },
}

impl BootState {
    /// Convenience: does this state allow the enable handler to proceed?
    pub fn permits_enable(&self) -> bool {
        matches!(self, BootState::Homed)
    }

    /// Convenience: is the auto-recovery routine currently driving this
    /// motor? While true, jog / enable / params writes / bench tests are
    /// all refused. Slated for removal in Phase H.1 of the
    /// commissioned-zero plan when Layer 6 goes away.
    pub fn is_auto_recovering(&self) -> bool {
        matches!(self, BootState::AutoRecovering { .. })
    }
}

/// Outcome of running [`classify`] once. Returned to callers so the
/// telemetry loop can decide whether to spawn the auto-recovery routine
/// (Layer 6) or the new boot orchestrator (Phase C.5+).
///
/// Not `Copy` because `BootState` is no longer `Copy` (HomeFailed
/// carries a String reason).
#[derive(Debug, Clone)]
pub enum ClassifyOutcome {
    /// State did not change. No action needed.
    Unchanged,
    /// State changed; the new state is in `state.boot_state` already.
    Changed { new: BootState, prev: BootState },
}

/// Classify (or re-classify) a single motor based on the latest position
/// read. Updates `state.boot_state` in place; never demotes `Homed` (only
/// `set_zero` or daemon restart can clear that).
///
/// Returns the previous and new state when they differ so the caller can
/// log the transition or trigger Layer 6 auto-recovery.
pub fn classify(state: &SharedState, role: &str, mech_pos_rad: f32) -> ClassifyOutcome {
    let limits: Option<TravelLimits> = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_role(role)
        .and_then(|m| m.travel_limits.clone());

    // No band on file -> degenerate case: treat as InBand. The enable
    // handler still requires Homed via the BootState ritual; this just
    // unblocks the InBand transition so the operator can run the homer.
    let Some(limits) = limits else {
        return transition(state, role, BootState::InBand);
    };

    if !mech_pos_rad.is_finite() {
        return transition(state, role, BootState::Unknown);
    }

    let principal = wrap_to_pi(mech_pos_rad);
    let new = if principal >= limits.min_rad && principal <= limits.max_rad {
        BootState::InBand
    } else {
        BootState::OutOfBand {
            mech_pos_rad: principal,
            min_rad: limits.min_rad,
            max_rad: limits.max_rad,
        }
    };

    transition(state, role, new)
}

/// Reset `role`'s boot state back to `Unknown`. Called from `set_zero`
/// success: a re-zero invalidates the prior home attestation because all
/// position readings are about to be measured against a different origin.
/// Bypasses the "never demote Homed" rule that protects against telemetry
/// glitches — set_zero is an explicit operator action.
pub fn reset_to_unknown(state: &SharedState, role: &str) {
    force_set(state, role, BootState::Unknown);
}

/// Mark `role` as `Homed`. Called by the slow-ramp homer on successful
/// completion. Bypasses the "never demote Homed" rule (which only protects
/// against telemetry-driven downgrades).
pub fn mark_homed(state: &SharedState, role: &str) {
    force_set(state, role, BootState::Homed);
}

/// Unconditionally set `role`'s boot state. The classifier and other
/// transition-aware paths must NOT use this — they go through `transition`,
/// which enforces the "never demote Homed" rule. Used by explicit operator
/// actions (`set_zero`, successful `home`) that ARE the source of truth
/// for those transitions.
fn force_set(state: &SharedState, role: &str, new: BootState) {
    let mut map = state.boot_state.write().expect("boot_state poisoned");
    map.insert(role.to_string(), new);
}

/// Mark `role` as currently being driven by the auto-recovery routine.
/// Carries the from/target so the UI can render a progress bar.
pub fn mark_auto_recovering(state: &SharedState, role: &str, from_rad: f32, target_rad: f32) {
    let _ = transition(
        state,
        role,
        BootState::AutoRecovering {
            from_rad,
            target_rad,
            progress_rad: 0.0,
        },
    );
}

/// Update the in-flight `progress_rad` on an `AutoRecovering` state without
/// emitting a transition event. Used by the auto-recovery loop to drive the
/// UI progress bar tick-by-tick.
pub fn update_auto_recovery_progress(state: &SharedState, role: &str, progress_rad: f32) {
    let mut map = state.boot_state.write().expect("boot_state poisoned");
    if let Some(BootState::AutoRecovering {
        from_rad,
        target_rad,
        ..
    }) = map.get(role).cloned()
    {
        map.insert(
            role.to_string(),
            BootState::AutoRecovering {
                from_rad,
                target_rad,
                progress_rad,
            },
        );
    }
}

/// Mark `role` as Class-1-shenanigan-detected: the firmware's
/// reported `add_offset` disagrees with the stored
/// `commissioned_zero_offset` by more than the configured tolerance.
/// Bypasses the "never demote Homed" rule because this is an explicit
/// orchestrator-initiated transition.
pub fn force_set_offset_changed(
    state: &SharedState,
    role: &str,
    stored_rad: f32,
    current_rad: f32,
) {
    force_set(
        state,
        role,
        BootState::OffsetChanged {
            stored_rad,
            current_rad,
        },
    );
}

/// Mark `role` as currently being driven by the boot orchestrator's
/// auto-home flow. Carries the from/target so the UI can render a
/// progress bar without polling.
pub fn force_set_auto_homing(
    state: &SharedState,
    role: &str,
    from_rad: f32,
    target_rad: f32,
) {
    force_set(
        state,
        role,
        BootState::AutoHoming {
            from_rad,
            target_rad,
            progress_rad: 0.0,
        },
    );
}

/// Update the in-flight `progress_rad` on an `AutoHoming` state
/// without emitting a transition event. Mirrors
/// [`update_auto_recovery_progress`] but for the boot orchestrator's
/// auto-home flow.
pub fn update_auto_homing_progress(state: &SharedState, role: &str, progress_rad: f32) {
    let mut map = state.boot_state.write().expect("boot_state poisoned");
    if let Some(BootState::AutoHoming {
        from_rad,
        target_rad,
        ..
    }) = map.get(role).cloned()
    {
        map.insert(
            role.to_string(),
            BootState::AutoHoming {
                from_rad,
                target_rad,
                progress_rad,
            },
        );
    }
}

/// Mark `role` as having had its boot-orchestrator auto-home aborted.
/// Operator must investigate (the SPA renders the reason and a Retry
/// button against `POST /api/motors/:role/home`).
pub fn force_set_home_failed(state: &SharedState, role: &str, reason: String, last_pos_rad: f32) {
    force_set(
        state,
        role,
        BootState::HomeFailed {
            reason,
            last_pos_rad,
        },
    );
}

/// Look up the current boot state, returning `Unknown` if the motor isn't
/// tracked yet. Convenience for handlers that need to gate on it.
pub fn current(state: &SharedState, role: &str) -> BootState {
    state
        .boot_state
        .read()
        .expect("boot_state poisoned")
        .get(role)
        .cloned()
        .unwrap_or(BootState::Unknown)
}

/// Distance (in radians, positive) to the nearest band edge that brings
/// the motor back into the band. Returns 0.0 if already in band.
pub fn distance_to_band(mech_pos_rad: f32, limits: &TravelLimits) -> f32 {
    let principal = wrap_to_pi(mech_pos_rad);
    if principal >= limits.min_rad && principal <= limits.max_rad {
        return 0.0;
    }
    // Pick the band edge (min or max) with the shorter principal-angle
    // distance. The recovery target is `edge +/- margin` on the in-band
    // side; this function returns the distance to the EDGE, not the target.
    let to_min = shortest_signed_delta(principal, limits.min_rad).abs();
    let to_max = shortest_signed_delta(principal, limits.max_rad).abs();
    to_min.min(to_max)
}

/// Compute the auto-recovery target: the band edge nearest to `mech_pos`
/// plus a small inside-the-band margin. Returns `None` if already in band.
pub fn recovery_target(mech_pos_rad: f32, limits: &TravelLimits, margin_rad: f32) -> Option<f32> {
    let principal = wrap_to_pi(mech_pos_rad);
    if principal >= limits.min_rad && principal <= limits.max_rad {
        return None;
    }
    let to_min = shortest_signed_delta(principal, limits.min_rad).abs();
    let to_max = shortest_signed_delta(principal, limits.max_rad).abs();
    let target = if to_min <= to_max {
        // Nearest edge is min; aim margin INSIDE the band (toward max).
        limits.min_rad + margin_rad
    } else {
        limits.max_rad - margin_rad
    };
    // Defensive: if the band is narrower than 2*margin, clamp the target
    // to the band midpoint rather than crossing past the far edge.
    let mid = 0.5 * (limits.min_rad + limits.max_rad);
    Some(if limits.max_rad - limits.min_rad < 2.0 * margin_rad {
        mid
    } else {
        target
    })
}

fn transition(state: &SharedState, role: &str, new: BootState) -> ClassifyOutcome {
    let mut map = state.boot_state.write().expect("boot_state poisoned");
    let prev = map.get(role).cloned().unwrap_or(BootState::Unknown);

    // Never demote Homed via classify; only set_zero / mark_homed escape this.
    if matches!(prev, BootState::Homed) && !matches!(new, BootState::Homed) {
        return ClassifyOutcome::Unchanged;
    }

    if std::mem::discriminant(&prev) == std::mem::discriminant(&new) && prev == new {
        return ClassifyOutcome::Unchanged;
    }

    map.insert(role.to_string(), new.clone());
    drop(map);

    if let BootState::OutOfBand {
        mech_pos_rad,
        min_rad,
        max_rad,
    } = new
    {
        // Out-of-band is a safety-relevant transition; broadcast it so
        // dashboards can highlight without polling. Reusing the existing
        // TravelLimitViolation event keeps the wire shape stable.
        let _ = state
            .safety_event_tx
            .send(SafetyEvent::TravelLimitViolation {
                t_ms: chrono::Utc::now().timestamp_millis(),
                role: role.to_string(),
                attempted_rad: mech_pos_rad,
                min_rad,
                max_rad,
            });
        ClassifyOutcome::Changed {
            new: BootState::OutOfBand {
                mech_pos_rad,
                min_rad,
                max_rad,
            },
            prev,
        }
    } else {
        ClassifyOutcome::Changed { new, prev }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(min: f32, max: f32) -> TravelLimits {
        TravelLimits {
            min_rad: min,
            max_rad: max,
            updated_at: None,
        }
    }

    #[test]
    fn distance_to_band_zero_when_in_band() {
        let l = limits(-1.0, 1.0);
        assert_eq!(distance_to_band(0.0, &l), 0.0);
        assert_eq!(distance_to_band(-1.0, &l), 0.0);
        assert_eq!(distance_to_band(1.0, &l), 0.0);
    }

    #[test]
    fn distance_to_band_picks_nearer_edge() {
        let l = limits(-1.0, 1.0);
        assert!((distance_to_band(1.5, &l) - 0.5).abs() < 1e-5);
        assert!((distance_to_band(-1.5, &l) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn recovery_target_lands_inside_band() {
        let l = limits(-1.0, 1.0);
        let t = recovery_target(1.5, &l, 0.1).expect("out of band on max side");
        assert!((t - 0.9).abs() < 1e-5, "got {t}");
        let t = recovery_target(-1.5, &l, 0.1).expect("out of band on min side");
        assert!((t - (-0.9)).abs() < 1e-5, "got {t}");
    }

    #[test]
    fn recovery_target_none_when_in_band() {
        let l = limits(-1.0, 1.0);
        assert!(recovery_target(0.0, &l, 0.1).is_none());
    }

    #[test]
    fn boot_state_permits_enable_only_when_homed() {
        assert!(BootState::Homed.permits_enable());
        assert!(!BootState::InBand.permits_enable());
        assert!(!BootState::Unknown.permits_enable());
        assert!(!BootState::OutOfBand {
            mech_pos_rad: 0.0,
            min_rad: 0.0,
            max_rad: 0.0,
        }
        .permits_enable());
    }

    /// New boot-orchestrator variants must NOT permit enable. Class-1
    /// shenanigans (OffsetChanged) refuse motion until recovery; the
    /// orchestrator's own AutoHoming flow refuses operator commands
    /// while it's running; HomeFailed requires operator investigation
    /// before any motion is allowed.
    #[test]
    fn new_boot_state_variants_refuse_enable() {
        let oc = BootState::OffsetChanged {
            stored_rad: 0.0,
            current_rad: 0.5,
        };
        let ah = BootState::AutoHoming {
            from_rad: 0.0,
            target_rad: 0.0,
            progress_rad: 0.0,
        };
        let hf = BootState::HomeFailed {
            reason: "tracking_error".into(),
            last_pos_rad: 0.42,
        };
        assert!(!oc.permits_enable());
        assert!(!ah.permits_enable());
        assert!(!hf.permits_enable());
        assert!(!oc.is_auto_recovering());
        assert!(!ah.is_auto_recovering());
        assert!(!hf.is_auto_recovering());
    }
}
