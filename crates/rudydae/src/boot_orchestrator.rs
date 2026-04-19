//! Boot orchestrator: per-motor "first valid telemetry" hook that
//! verifies the firmware's stored zero matches the commissioning record
//! and (when matched + in-band) auto-homes the motor to its predefined
//! neutral pose without operator intervention.
//!
//! Spawned from the telemetry hook (`spawn_if_orchestrator_qualifies` in
//! `bus_worker`, `linux::merge_aux_into_latest`, and the mock CAN loop)
//! on these BootState
//! transitions, all of which signal "this motor's position just became
//! trustworthy":
//!
//! 1. `Unknown → InBand` — first valid telemetry after daemon start.
//! 2. `OutOfBand → InBand` — operator physically moved the joint into
//!    band, retriggering the orchestrator after a previous skip.
//!
//! Idempotent within one daemon lifetime per role: `state.boot_orchestrator_attempted`
//! tracks which roles have already been processed so a stuttering
//! telemetry stream doesn't double-spawn the slow-ramp homer. Roles
//! are removed from the set when leaving the orchestrator's "in-band"
//! flight envelope (OutOfBand transition) so a future re-entry can
//! retrigger.
//!
//! Decision tree on every fire:
//!
//! - `cfg.safety.auto_home_on_boot == false` → log info, return.
//! - motor `commissioned_zero_offset == None` → log info, return
//!   (uncommissioned motors keep their pre-orchestrator behavior:
//!    the operator must manually click Verify & Home every boot).
//! - `read_add_offset` fails (CAN error or firmware read-fail) → retry
//!   once after 200 ms; if still failing, log warn and return without
//!   force_setting any state. The next telemetry tick will retrigger
//!   the orchestrator if the role qualifies again.
//! - readback mismatches stored value by more than
//!   `cfg.safety.commission_readback_tolerance_rad` → force_set
//!   `OffsetChanged { stored, current }`, audit-log, emit
//!   `SafetyEvent::OffsetChanged`. Return; the operator must
//!   re-commission or restore.
//! - latest mech_pos_rad missing or stale (older than
//!   `cfg.safety.max_feedback_age_ms`) → log info, return without
//!   force_setting state. The next valid type-2 frame will retrigger.
//! - wrap_to_pi(mech_pos_rad) outside `travel_limits` → no-op (the
//!   classifier already set OutOfBand). Clear the attempted flag so a
//!   future InBand transition retriggers.
//! - all checks pass → force_set `AutoHoming`, call `slow_ramp::run`
//!   toward `predefined_home_rad.unwrap_or(0.0)`. On success
//!   mark_homed + emit `SafetyEvent::AutoHomed`; on failure force_set
//!   `HomeFailed { reason }` + emit `SafetyEvent::HomeFailed`.
//!
//! The orchestrator does NOT touch motors with no `commissioned_zero_offset`,
//! does NOT touch the `present: false` motors, and does NOT bypass any
//! of the existing safety gates inside `slow_ramp::run` (path-aware
//! band check on every tick, tracking-error abort, `homer_timeout_ms`
//! ceiling). It is a thin orchestrator above existing primitives.

use std::time::Duration;

use chrono::Utc;
use tokio::time::sleep;
use tracing::{info, warn};

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state::{self, BootState, ClassifyOutcome};
use crate::can::motion::wrap_to_pi;
use crate::can::slow_ramp;
use crate::state::SharedState;
use crate::types::SafetyEvent;

/// Max age of cached telemetry the orchestrator will trust before
/// declining to act and waiting for a fresh tick. Reuses the existing
/// `safety.max_feedback_age_ms` so config drift is impossible.
fn max_feedback_age_ms(state: &SharedState) -> u64 {
    state.cfg.safety.max_feedback_age_ms
}

/// Inter-retry sleep on transient `read_add_offset` failures. Single
/// retry is enough — by design the underlying `BusHandle` already
/// internally backs off; this is a "one quick second chance" so the
/// orchestrator absorbs an ENOBUFS bounce without giving up.
const READBACK_RETRY_DELAY: Duration = Duration::from_millis(200);

/// After telemetry updates `state.latest` and runs [`boot_state::classify`],
/// call this to possibly spawn [`maybe_run`] without blocking the CAN thread
/// or poll loop.
///
/// Fires when classification transitions `Unknown → InBand` or
/// `OutOfBand → InBand`, or when the Linux aux-merge path seeds the first
/// `latest` row (`Seeded`) while the motor is already `InBand` (edge case).
pub fn spawn_if_orchestrator_qualifies(
    state: SharedState,
    role: String,
    classify_outcome: ClassifyOutcome,
    aux_seeded_first_row: bool,
) {
    let in_band_transition = match &classify_outcome {
        ClassifyOutcome::Changed { prev, new } => {
            matches!(new, BootState::InBand)
                && matches!(prev, BootState::Unknown | BootState::OutOfBand { .. })
        }
        ClassifyOutcome::Unchanged => false,
    };
    let seeded_in_band = aux_seeded_first_row
        && matches!(boot_state::current(&state, &role), BootState::InBand);
    if !in_band_transition && !seeded_in_band {
        return;
    }
    tokio::spawn(async move {
        maybe_run(state, role).await;
    });
}

/// Public entrypoint, spawned by the telemetry hook on a qualifying
/// BootState transition. Cheap to call; idempotency is enforced
/// internally via `state.boot_orchestrator_attempted`.
///
/// Spawning convention: callers MUST `tokio::spawn` this — the
/// orchestrator runs the slow-ramp homer (potentially seconds long)
/// and we never want a telemetry tick blocked on it.
pub async fn maybe_run(state: SharedState, role: String) {
    // Step 1: master switch.
    if !state.cfg.safety.auto_home_on_boot {
        info!(
            role = %role,
            "boot_orchestrator: skipping (auto_home_on_boot=false)",
        );
        return;
    }

    // Step 1b: idempotency — only fire once per role per daemon lifetime
    // unless explicitly cleared (which the OutOfBand path below does).
    {
        let mut attempted = state
            .boot_orchestrator_attempted
            .lock()
            .expect("boot_orchestrator_attempted poisoned");
        if attempted.contains(&role) {
            return;
        }
        attempted.insert(role.clone());
    }

    // Step 2: load motor from inventory.
    let motor = match state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_role(&role)
        .cloned()
    {
        Some(m) => m,
        None => {
            warn!(role = %role, "boot_orchestrator: motor disappeared from inventory");
            clear_attempted(&state, &role);
            return;
        }
    };

    // Step 3: skip uncommissioned motors. Their pre-orchestrator
    // behavior (manual Verify & Home on every boot) is unchanged.
    let Some(stored_offset) = motor.commissioned_zero_offset else {
        info!(
            role = %role,
            "boot_orchestrator: skipping motor uncommissioned, run POST /commission first",
        );
        // Don't clear attempted — re-running won't change the answer
        // until the operator commissions, at which point the SafetyEvent::Commissioned
        // path could later (Phase F or beyond) explicitly clear it.
        return;
    };

    // Step 4: read add_offset from firmware, with one retry. If still
    // failing, leave state untouched and let a future telemetry tick
    // retrigger; clear attempted so retrigger is possible.
    let current_offset = match read_add_offset_with_retry(&state, &motor).await {
        Some(v) => v,
        None => {
            warn!(
                role = %role,
                "boot_orchestrator: add_offset readback failed twice; leaving state untouched",
            );
            clear_attempted(&state, &role);
            return;
        }
    };

    // Step 5: tolerance check. Mismatch → OffsetChanged terminal state
    // (operator action required).
    let tolerance = state.cfg.safety.commission_readback_tolerance_rad;
    if (current_offset - stored_offset).abs() > tolerance {
        warn!(
            role = %role,
            stored = stored_offset,
            current = current_offset,
            tolerance,
            "boot_orchestrator: add_offset readback disagrees with stored; refusing to home",
        );
        boot_state::force_set_offset_changed(&state, &role, stored_offset, current_offset);
        audit_offset_changed(&state, &role, stored_offset, current_offset);
        let _ = state.safety_event_tx.send(SafetyEvent::OffsetChanged {
            t_ms: Utc::now().timestamp_millis(),
            role: role.clone(),
            stored_rad: stored_offset,
            current_rad: current_offset,
        });
        // Leave attempted set: the operator's recovery action
        // (commission or restore_offset) is the only thing that should
        // re-enable auto-home for this motor.
        return;
    }

    // Step 6: latest mech_pos and freshness check.
    let now_ms = Utc::now().timestamp_millis();
    let max_age_ms = max_feedback_age_ms(&state) as i64;
    let mech_pos_rad = match state.latest.read().expect("latest poisoned").get(&role) {
        Some(fb) => {
            if now_ms - fb.t_ms > max_age_ms {
                info!(
                    role = %role,
                    age_ms = now_ms - fb.t_ms,
                    "boot_orchestrator: telemetry stale; will retry on next tick",
                );
                clear_attempted(&state, &role);
                return;
            }
            fb.mech_pos_rad
        }
        None => {
            info!(
                role = %role,
                "boot_orchestrator: no telemetry yet; will retry on next tick",
            );
            clear_attempted(&state, &role);
            return;
        }
    };

    // Step 7: principal-angle band check. If outside band, the classifier
    // has already set OutOfBand; clear attempted so a future
    // OutOfBand → InBand transition retriggers this orchestrator.
    let limits = motor.travel_limits.clone();
    if let Some(limits) = &limits {
        let principal = wrap_to_pi(mech_pos_rad);
        if principal < limits.min_rad || principal > limits.max_rad {
            info!(
                role = %role,
                mech_pos_rad,
                principal,
                min = limits.min_rad,
                max = limits.max_rad,
                "boot_orchestrator: motor outside band; awaiting operator nudge into range",
            );
            clear_attempted(&state, &role);
            return;
        }
    }

    // Step 8: transition to AutoHoming and run the slow ramp.
    let target_rad = motor.predefined_home_rad.unwrap_or(0.0);
    boot_state::force_set_auto_homing(&state, &role, mech_pos_rad, target_rad);
    audit_auto_homing_started(&state, &role, mech_pos_rad, target_rad);
    info!(
        role = %role,
        from_rad = mech_pos_rad,
        target_rad,
        "boot_orchestrator: starting auto-home",
    );

    // Spawn a background progress-tracker that polls latest and feeds
    // `update_auto_homing_progress`. This is best-effort — the actual
    // tracking-error and timeout safety lives inside slow_ramp::run.
    let progress_state = state.clone();
    let progress_role = role.clone();
    let progress_handle = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let bs = boot_state::current(&progress_state, &progress_role);
            let BootState::AutoHoming { from_rad, .. } = bs else {
                // We left AutoHoming; the run completed (Homed) or
                // failed (HomeFailed) or got reset. Stop polling.
                break;
            };
            let cur = progress_state
                .latest
                .read()
                .expect("latest poisoned")
                .get(&progress_role)
                .map(|fb| fb.mech_pos_rad)
                .unwrap_or(from_rad);
            let progress = (cur - from_rad).abs();
            boot_state::update_auto_homing_progress(&progress_state, &progress_role, progress);
        }
    });

    // Step 9: drive the slow-ramp homer to the predefined target.
    let outcome = slow_ramp::run(state.clone(), motor.clone(), mech_pos_rad, target_rad).await;
    progress_handle.abort();

    match outcome {
        Ok((final_pos_rad, ticks)) => {
            boot_state::mark_homed(&state, &role);
            audit_auto_homed(&state, &role, mech_pos_rad, target_rad, ticks, final_pos_rad);
            let _ = state.safety_event_tx.send(SafetyEvent::AutoHomed {
                t_ms: Utc::now().timestamp_millis(),
                role: role.clone(),
                from_rad: mech_pos_rad,
                target_rad,
                ticks,
            });
            info!(
                role = %role,
                final_pos_rad,
                ticks,
                "boot_orchestrator: auto-home succeeded",
            );
        }
        Err((reason, last_pos_rad)) => {
            boot_state::force_set_home_failed(&state, &role, reason.clone(), last_pos_rad);
            audit_home_failed(&state, &role, &reason, last_pos_rad);
            let _ = state.safety_event_tx.send(SafetyEvent::HomeFailed {
                t_ms: Utc::now().timestamp_millis(),
                role: role.clone(),
                reason: reason.clone(),
                last_pos_rad,
            });
            warn!(
                role = %role,
                reason = %reason,
                last_pos_rad,
                "boot_orchestrator: auto-home failed",
            );
            // Leave attempted set: the operator's POST /home is the
            // documented recovery path and shouldn't be racing the
            // orchestrator on the next telemetry tick.
        }
    }
}

/// Read `add_offset` once. On failure (CAN error or firmware
/// read-fail), wait `READBACK_RETRY_DELAY` and try one more time.
/// `None` means both attempts failed.
async fn read_add_offset_with_retry(
    state: &SharedState,
    motor: &crate::inventory::Motor,
) -> Option<f32> {
    let core = state.real_can.clone()?;
    let state_for_blocking = state.clone();
    let motor_for_blocking = motor.clone();
    let first = tokio::task::spawn_blocking(move || {
        core.read_add_offset(&state_for_blocking, &motor_for_blocking)
    })
    .await
    .ok()
    .and_then(|r| r.ok());

    if let Some(v) = first {
        return Some(v);
    }

    sleep(READBACK_RETRY_DELAY).await;

    let core = state.real_can.clone()?;
    let state_for_blocking = state.clone();
    let motor_for_blocking = motor.clone();
    tokio::task::spawn_blocking(move || {
        core.read_add_offset(&state_for_blocking, &motor_for_blocking)
    })
    .await
    .ok()
    .and_then(|r| r.ok())
}

/// Drop `role` from the orchestrator-attempted set so the next
/// qualifying telemetry transition can re-fire the orchestrator.
fn clear_attempted(state: &SharedState, role: &str) {
    state
        .boot_orchestrator_attempted
        .lock()
        .expect("boot_orchestrator_attempted poisoned")
        .remove(role);
}

/// Drop `role` from the orchestrator idempotency set so a later qualifying
/// telemetry transition can spawn [`maybe_run`] again. Used by
/// `POST /restore_offset` after clearing `OffsetChanged`.
pub fn clear_orchestrator_attempted(state: &SharedState, role: &str) {
    clear_attempted(state, role);
}

/// Audit-log entry for an offset-disagreement detection. Mirrors the
/// envelope established by `set_zero_advanced` and `commission` so a
/// post-hoc audit-log review can grep for the orchestrator's actions
/// uniformly.
fn audit_offset_changed(state: &SharedState, role: &str, stored_rad: f32, current_rad: f32) {
    let message = format!(
        "boot_orchestrator: detected offset change for {role}: stored={stored_rad} current={current_rad}"
    );
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "boot_orchestrator_offset_changed".into(),
        target: Some(role.into()),
        details: serde_json::json!({
            "message": message,
            "stored_rad": stored_rad,
            "current_rad": current_rad,
            "delta_rad": current_rad - stored_rad,
        }),
        result: AuditResult::Denied,
    });
}

fn audit_auto_homed(
    state: &SharedState,
    role: &str,
    from_rad: f32,
    target_rad: f32,
    ticks: u32,
    final_pos_rad: f32,
) {
    let message = format!(
        "boot_orchestrator: auto-homed {role}: from={from_rad} to={target_rad} ticks={ticks}"
    );
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "boot_orchestrator_auto_homed".into(),
        target: Some(role.into()),
        details: serde_json::json!({
            "message": message,
            "from_rad": from_rad,
            "target_rad": target_rad,
            "final_pos_rad": final_pos_rad,
            "ticks": ticks,
        }),
        result: AuditResult::Ok,
    });
}

fn audit_auto_homing_started(state: &SharedState, role: &str, from_rad: f32, target_rad: f32) {
    let message = format!(
        "boot_orchestrator: auto-homing started for {role}: from={from_rad} to={target_rad}"
    );
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "boot_orchestrator_auto_homing_started".into(),
        target: Some(role.into()),
        details: serde_json::json!({
            "message": message,
            "from_rad": from_rad,
            "target_rad": target_rad,
        }),
        result: AuditResult::Ok,
    });
}

fn audit_home_failed(state: &SharedState, role: &str, reason: &str, last_pos_rad: f32) {
    let message = format!("boot_orchestrator: auto-home failed for {role}: reason={reason}");
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "boot_orchestrator_home_failed".into(),
        target: Some(role.into()),
        details: serde_json::json!({
            "message": message,
            "reason": reason,
            "last_pos_rad": last_pos_rad,
        }),
        result: AuditResult::Denied,
    });
}
