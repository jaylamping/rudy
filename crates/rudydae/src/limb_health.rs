//! Per-limb quarantine: refuse new motion when another motor on the same
//! limb is in a hard failure posture (`OutOfBand`, `OffsetChanged`, `HomeFailed`).
//!
//! [`require_limb_healthy`] and motion preflight use **sibling-only** failure
//! detection so recovery on the bad actor (e.g. retry `home` after
//! `HomeFailed`) is not blocked by the actor's own state. [`limb_status`]
//! inspects every motor on the limb (no exclusion) — used when the whole
//! limb must be clean before a batch such as `home_all`.
//!
//! Unlimbed motors use [`effective_limb_id`] `== motor.role` so only that
//! motor participates in the check (today's single-motor behavior).

use axum::http::StatusCode;
use axum::Json;

use crate::boot_state::{self, BootState};
use crate::inventory::Motor;
use crate::state::SharedState;
use crate::types::{ApiError, LimbQuarantineMotor};

#[derive(Debug, Clone)]
pub enum LimbStatus {
    Healthy,
    Quarantined {
        failed_motors: Vec<(String, BootState)>,
    },
}

/// Limb grouping key: configured `limb` or the motor's own `role` when unset.
pub fn effective_limb_id(motor: &Motor) -> String {
    motor
        .common
        .limb
        .clone()
        .unwrap_or_else(|| motor.common.role.clone())
}

fn quarantining_boot_state(bs: &BootState) -> bool {
    matches!(
        bs,
        BootState::HomeFailed { .. }
            | BootState::OffsetChanged { .. }
            | BootState::OutOfBand { .. }
    )
}

pub fn boot_state_kind_snake(bs: &BootState) -> &'static str {
    match bs {
        BootState::Unknown => "unknown",
        BootState::OutOfBand { .. } => "out_of_band",
        BootState::InBand => "in_band",
        BootState::Homed => "homed",
        BootState::OffsetChanged { .. } => "offset_changed",
        BootState::AutoHoming { .. } => "auto_homing",
        BootState::HomeFailed { .. } => "home_failed",
    }
}

/// Every **present** inventoried motor whose [`effective_limb_id`] matches
/// `limb_id` is examined; any `OutOfBand` / `OffsetChanged` / `HomeFailed`
/// quarantines the whole limb.
pub fn limb_status(state: &SharedState, limb_id: &str) -> LimbStatus {
    limb_status_inner(state, limb_id, None)
}

/// Like [`limb_status`], but ignores `acting_role` when collecting failures.
/// Used for motion on `acting_role`: the actor's own bad boot state is gated
/// elsewhere; this only catches **sibling** motors on the same limb so recovery
/// endpoints (e.g. retry `home` on a `HomeFailed` motor) are not blocked by the
/// actor itself appearing in the failure list.
pub fn sibling_quarantine_failures(
    state: &SharedState,
    limb_id: &str,
    acting_role: &str,
) -> Vec<(String, BootState)> {
    match limb_status_inner(state, limb_id, Some(acting_role)) {
        LimbStatus::Healthy => Vec::new(),
        LimbStatus::Quarantined { failed_motors } => failed_motors,
    }
}

fn limb_status_inner(state: &SharedState, limb_id: &str, exclude_role: Option<&str>) -> LimbStatus {
    let inv = state.inventory.read().expect("inventory poisoned");
    let mut failed = Vec::new();
    for m in inv.actuators() {
        if !m.common.present {
            continue;
        }
        if effective_limb_id(m) != limb_id {
            continue;
        }
        if exclude_role.is_some_and(|r| r == m.common.role.as_str()) {
            continue;
        }
        let bs = boot_state::current(state, &m.common.role);
        if quarantining_boot_state(&bs) {
            failed.push((m.common.role.clone(), bs));
        }
    }
    if failed.is_empty() {
        LimbStatus::Healthy
    } else {
        LimbStatus::Quarantined {
            failed_motors: failed,
        }
    }
}

pub(crate) fn limb_quarantine_api_error(
    limb_id: &str,
    failed: Vec<(String, BootState)>,
) -> ApiError {
    let failed_motors: Vec<LimbQuarantineMotor> = failed
        .iter()
        .map(|(r, bs)| LimbQuarantineMotor {
            role: r.clone(),
            state_kind: boot_state_kind_snake(bs).to_string(),
        })
        .collect();
    let names = failed_motors
        .iter()
        .map(|f| f.role.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    ApiError {
        error: "limb_quarantined".into(),
        detail: Some(format!(
            "limb {limb_id} is quarantined until recovery: {names}"
        )),
        limb: Some(limb_id.to_string()),
        failed_motors: Some(failed_motors),
    }
}

/// Returns `Err` when another motor on the same limb as `role` is quarantining
/// (see [`sibling_quarantine_failures`]).
pub fn require_limb_healthy(state: &SharedState, role: &str) -> Result<(), ApiError> {
    let motor = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role(role)
        .cloned()
        .ok_or_else(|| ApiError {
            error: "unknown_motor".into(),
            detail: Some(format!("no motor with role={role}")),
            ..Default::default()
        })?;
    let limb_id = effective_limb_id(&motor);
    let failed = sibling_quarantine_failures(state, &limb_id, role);
    if failed.is_empty() {
        Ok(())
    } else {
        Err(limb_quarantine_api_error(&limb_id, failed))
    }
}

/// HTTP mapping helper for API handlers.
pub fn require_limb_healthy_http(
    state: &SharedState,
    role: &str,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    match require_limb_healthy(state, role) {
        Ok(()) => Ok(()),
        Err(e) if e.error == "unknown_motor" => Err((StatusCode::NOT_FOUND, Json(e))),
        Err(e) => Err((StatusCode::CONFLICT, Json(e))),
    }
}

pub fn limb_quarantine_http(
    limb_id: &str,
    failed: Vec<(String, BootState)>,
) -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::CONFLICT,
        Json(limb_quarantine_api_error(limb_id, failed)),
    )
}
