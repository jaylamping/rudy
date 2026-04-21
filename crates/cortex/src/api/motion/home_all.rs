//! POST /api/home_all — orchestrate homing across every assigned limb.
//!
//! Groups present motors by `limb`, runs torso/spine joints first
//! (sequentially, since a wobbling torso while arms ramp could collide),
//! then spawns one tokio task per limb that sequentially homes its motors
//! in proximal-to-distal order. Across-limb is parallel; within-limb is
//! sequential. A single failing motor inside a limb stops THAT limb's
//! sequence (homing the elbow while the shoulder is bound up could cause
//! collisions) but does NOT abort other limbs.
//!
//! Pre-flight: validates every motor in every limb passes the standard
//! pre-home checks before sending any motion frame. If any motor fails
//! pre-flight, the entire request is refused with a 409 listing all
//! offenders. Half-homed-then-crash is worse than no-home.

use std::collections::BTreeMap;

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::Serialize;
use tokio::task::JoinSet;
use tracing::warn;

use crate::api::error::err;
use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state::{self, BootState};
use crate::can::angle::UnwrappedAngle;
use crate::can::home_ramp;
use crate::can::travel::{enforce_position_with_path, BandCheck};
use crate::inventory::Actuator;
use crate::limb::ordered_motors_per_limb_owned;
use crate::limb_health;
use crate::state::SharedState;
use crate::types::{ApiError, SafetyEvent};
use crate::util::session_from_headers;

#[derive(Debug, Serialize)]
pub struct HomeAllResp {
    pub ok: bool,
    pub results: BTreeMap<String, LimbResult>,
}

#[derive(Debug, Serialize)]
pub struct LimbResult {
    pub status: &'static str,
    pub homed: Vec<String>,
    pub failed_at: Option<String>,
    pub failure_reason: Option<String>,
}

fn last_measured(state: &SharedState, role: &str, fallback: f32) -> f32 {
    state
        .latest
        .read()
        .expect("latest poisoned")
        .get(role)
        .map(|f| f.mech_pos_rad)
        .filter(|p| p.is_finite())
        .unwrap_or(fallback)
}

/// Slow-ramp to `predefined_home_rad` on the actuator row (or 0.0). Caller must skip
/// motors already [`BootState::Homed`]. On success does **not** transition
/// boot state — the orchestrator emits [`SafetyEvent::Homed`] and calls
/// [`boot_state::mark_homed`].
async fn drive_predefined_home(
    state: SharedState,
    motor: Actuator,
) -> Result<(f32, u32), (String, f32)> {
    let role = motor.common.role.clone();
    let bs = boot_state::current(&state, &role);
    match bs {
        BootState::InBand => {}
        BootState::Homed => {
            return Err(("already_homed".into(), last_measured(&state, &role, 0.0)));
        }
        BootState::Unknown => return Err(("not_ready".into(), f32::NAN)),
        BootState::OutOfBand {
            mech_pos_rad,
            min_rad,
            max_rad,
        } => {
            return Err((
                format!(
                    "out_of_band at {mech_pos_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]"
                ),
                mech_pos_rad,
            ));
        }
        BootState::OffsetChanged { .. } => return Err(("offset_changed".into(), f32::NAN)),
        BootState::AutoHoming { .. } => return Err(("auto_homing_in_progress".into(), f32::NAN)),
        BootState::HomeFailed { .. } => return Err(("home_failed".into(), f32::NAN)),
    }

    let target_rad = motor.common.predefined_home_rad.unwrap_or(0.0);
    if !target_rad.is_finite() {
        return Err(("bad_target".into(), last_measured(&state, &role, 0.0)));
    }

    let current_pos = state
        .latest
        .read()
        .expect("latest poisoned")
        .get(&role)
        .map(|f| f.mech_pos_rad)
        .ok_or_else(|| ("no_telemetry".into(), f32::NAN))?;

    let check = match enforce_position_with_path(
        &state,
        &role,
        UnwrappedAngle::new(current_pos),
        UnwrappedAngle::new(target_rad),
    ) {
        Ok(c) => c,
        Err(e) => return Err((format!("internal: {e:#}"), current_pos)),
    };
    match check {
        BandCheck::OutOfBand {
            min_rad,
            max_rad,
            attempted_rad,
        } => {
            return Err((
                format!("target {attempted_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]"),
                current_pos,
            ));
        }
        BandCheck::PathViolation { .. } => {
            return Err(("path_violation".into(), current_pos));
        }
        BandCheck::NoLimit | BandCheck::InBand { .. } => {}
    }

    home_ramp::run(state, motor, current_pos, target_rad).await
}

fn apply_home_success(state: &SharedState, role: &str, final_pos: f32, ticks: u32) {
    boot_state::mark_homed(state, role);
    let _ = state.safety_event_tx.send(SafetyEvent::Homed {
        t_ms: Utc::now().timestamp_millis(),
        role: role.to_string(),
        final_pos_rad: final_pos,
        samples_count: ticks,
    });
}

fn apply_home_failure(state: &SharedState, role: &str, reason: String, last_pos_rad: f32) {
    let lp = if last_pos_rad.is_finite() {
        last_pos_rad
    } else {
        0.0
    };
    boot_state::force_set_home_failed(state, role, reason.clone(), lp);
    let _ = state.safety_event_tx.send(SafetyEvent::HomeFailed {
        t_ms: Utc::now().timestamp_millis(),
        role: role.to_string(),
        reason,
        last_pos_rad: lp,
    });
}

pub async fn home_all(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
) -> Result<Json<HomeAllResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }

    let inv = state.inventory.read().expect("inventory poisoned").clone();
    let by_limb = ordered_motors_per_limb_owned(&inv);

    if by_limb.is_empty() {
        return Err(err(
            StatusCode::CONFLICT,
            "no_assigned_limbs",
            Some("no present motors have a `limb` field set; nothing to home".into()),
        ));
    }

    for limb_name in by_limb.keys() {
        if let limb_health::LimbStatus::Quarantined { failed_motors } =
            limb_health::limb_status(&state, limb_name)
        {
            return Err(limb_health::limb_quarantine_http(limb_name, failed_motors));
        }
    }

    // Pre-flight: every motor must be in InBand or Homed state. Anything
    // else means we don't know enough to safely command motion.
    let mut offenders: Vec<(String, &'static str)> = Vec::new();
    for motors in by_limb.values() {
        for m in motors {
            let bs = boot_state::current(&state, &m.common.role);
            let ok = match bs {
                BootState::InBand | BootState::Homed => true,
                BootState::Unknown => {
                    offenders.push((m.common.role.clone(), "not_ready"));
                    false
                }
                BootState::OutOfBand { .. } => {
                    offenders.push((m.common.role.clone(), "out_of_band"));
                    false
                }
                BootState::OffsetChanged { .. } => {
                    offenders.push((m.common.role.clone(), "offset_changed"));
                    false
                }
                BootState::AutoHoming { .. } => {
                    offenders.push((m.common.role.clone(), "auto_homing_in_progress"));
                    false
                }
                BootState::HomeFailed { .. } => {
                    offenders.push((m.common.role.clone(), "home_failed"));
                    false
                }
            };
            let _ = ok;
        }
    }
    if !offenders.is_empty() {
        let detail = offenders
            .iter()
            .map(|(r, why)| format!("{r}={why}"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(err(StatusCode::CONFLICT, "preflight_failed", Some(detail)));
    }

    // Torso pre-phase: sequential across all torso joints (global sort by
    // home_order, then limb, then role).
    let mut torso: Vec<(String, String)> = Vec::new();
    for (limb, motors) in &by_limb {
        for m in motors {
            if m.common.joint_kind.map(|jk| jk.is_torso()).unwrap_or(false) {
                torso.push((limb.clone(), m.common.role.clone()));
            }
        }
    }
    torso.sort_by(|(l_a, r_a), (l_b, r_b)| {
        let ord_a = inv
            .actuator_by_role(r_a)
            .and_then(|m| m.common.joint_kind)
            .map(|jk| jk.home_order())
            .unwrap_or(255);
        let ord_b = inv
            .actuator_by_role(r_b)
            .and_then(|m| m.common.joint_kind)
            .map(|jk| jk.home_order())
            .unwrap_or(255);
        (ord_a, l_a.as_str(), r_a.as_str()).cmp(&(ord_b, l_b.as_str(), r_b.as_str()))
    });

    for (_limb, role) in &torso {
        if matches!(boot_state::current(&state, role), BootState::Homed) {
            continue;
        }
        let motor = inv.actuator_by_role(role).cloned().ok_or_else(|| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal",
                Some(format!("inventory missing role {role} during home_all")),
            )
        })?;
        match drive_predefined_home(state.clone(), motor).await {
            Ok((final_pos, ticks)) => apply_home_success(&state, role, final_pos, ticks),
            Err((reason, last_pos)) => {
                apply_home_failure(&state, role, reason.clone(), last_pos);
                let lp = if last_pos.is_finite() { last_pos } else { 0.0 };
                return Err(err(
                    StatusCode::CONFLICT,
                    "home_all_aborted",
                    Some(format!("torso phase {role}: {reason} at {lp:.3} rad")),
                ));
            }
        }
    }

    // Per-limb parallel (non-torso motors only; torso was handled above).
    let mut results: BTreeMap<String, LimbResult> = BTreeMap::new();
    let mut joinset: JoinSet<(String, LimbResult)> = JoinSet::new();
    for (limb, motors) in by_limb {
        let limb_name = limb.clone();
        let state = state.clone();
        joinset.spawn(async move {
            let all = motors;
            let drive_queue: Vec<Actuator> = all
                .iter()
                .filter(|m| !m.common.joint_kind.map(|jk| jk.is_torso()).unwrap_or(false))
                .cloned()
                .collect();

            let mut homed: Vec<String> = Vec::new();
            for m in &all {
                if m.common.joint_kind.map(|jk| jk.is_torso()).unwrap_or(false)
                    && matches!(
                        boot_state::current(&state, &m.common.role),
                        BootState::Homed
                    )
                {
                    homed.push(m.common.role.clone());
                }
            }

            for motor in drive_queue {
                let role = motor.common.role.clone();
                if matches!(boot_state::current(&state, &role), BootState::Homed) {
                    if !homed.contains(&role) {
                        homed.push(role.clone());
                    }
                    continue;
                }
                match drive_predefined_home(state.clone(), motor).await {
                    Ok((final_pos, ticks)) => {
                        apply_home_success(&state, &role, final_pos, ticks);
                        if !homed.contains(&role) {
                            homed.push(role.clone());
                        }
                    }
                    Err((reason, last_pos)) => {
                        apply_home_failure(&state, &role, reason.clone(), last_pos);
                        return (
                            limb_name,
                            LimbResult {
                                status: "failed",
                                homed,
                                failed_at: Some(role),
                                failure_reason: Some(reason),
                            },
                        );
                    }
                }
            }

            (
                limb_name,
                LimbResult {
                    status: "ok",
                    homed,
                    failed_at: None,
                    failure_reason: None,
                },
            )
        });
    }
    while let Some(res) = joinset.join_next().await {
        match res {
            Ok((limb, lr)) => {
                results.insert(limb, lr);
            }
            Err(e) => warn!(error = %e, "home_all limb task panicked or was cancelled"),
        }
    }

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "home_all".into(),
        target: None,
        details: serde_json::to_value(&results).unwrap_or(serde_json::Value::Null),
        result: AuditResult::Ok,
    });

    Ok(Json(HomeAllResp { ok: true, results }))
}
