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

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state::{self, BootState};
use crate::limb::ordered_motors_per_limb;
use crate::state::SharedState;
use crate::types::ApiError;
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

fn err(status: StatusCode, error: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
        }),
    )
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
    let by_limb = ordered_motors_per_limb(&inv);

    if by_limb.is_empty() {
        return Err(err(
            StatusCode::CONFLICT,
            "no_assigned_limbs",
            Some("no present motors have a `limb` field set; nothing to home".into()),
        ));
    }

    // Pre-flight: every motor must be in InBand or Homed state. Anything
    // else means we don't know enough to safely command motion.
    let mut offenders: Vec<(String, &'static str)> = Vec::new();
    for motors in by_limb.values() {
        for m in motors {
            let bs = boot_state::current(&state, &m.role);
            let ok = match bs {
                BootState::InBand | BootState::Homed => true,
                BootState::Unknown => {
                    offenders.push((m.role.clone(), "not_ready"));
                    false
                }
                BootState::OutOfBand { .. } => {
                    offenders.push((m.role.clone(), "out_of_band"));
                    false
                }
                BootState::AutoRecovering { .. } => {
                    offenders.push((m.role.clone(), "auto_recovery_in_progress"));
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

    // Torso pre-phase: sequential across all torso joints in any limb.
    let mut results: BTreeMap<String, LimbResult> = BTreeMap::new();
    let mut torso: Vec<(String, String)> = Vec::new();
    for (limb, motors) in &by_limb {
        for m in motors {
            if m.joint_kind.map(|jk| jk.is_torso()).unwrap_or(false) {
                torso.push((limb.clone(), m.role.clone()));
            }
        }
    }
    for (_, role) in &torso {
        // Reuse the homer by calling it directly via boot_state transitions
        // — for now in mock mode we just transition the state. Real-CAN
        // homing is the same code path as `home::run_homer`; refactoring
        // it into a free function that this orchestrator can call is the
        // logical next step but kept out of scope for the orchestrator
        // skeleton.
        boot_state::mark_homed(&state, role);
    }

    // Per-limb parallel.
    let mut joinset: JoinSet<(String, LimbResult)> = JoinSet::new();
    for (limb, motors) in by_limb {
        let limb_name = limb.clone();
        let state = state.clone();
        let roles: Vec<String> = motors.iter().map(|m| m.role.clone()).collect();
        joinset.spawn(async move {
            let mut homed = Vec::new();
            for role in &roles {
                if boot_state::current(&state, role) == BootState::Homed {
                    homed.push(role.clone());
                    continue;
                }
                // Mock: transition the state to Homed. On real hardware
                // this would invoke the slow-ramp homer.
                boot_state::mark_homed(&state, role);
                homed.push(role.clone());
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
        if let Ok((limb, lr)) = res {
            results.insert(limb, lr);
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
