//! POST /api/motors/:role/rename and POST /api/motors/:role/assign.
//!
//! `rename` changes a motor's primary key. Validates new_role canonical
//! form, refuses duplicates, refuses while motor is enabled. Atomically
//! rewrites inventory.yaml, migrates the in-memory `state.latest` /
//! `state.params` / `state.boot_state` maps, audit-logs the change, and
//! emits a `MotorRenamed` safety event so subscribers drop per-role caches.
//!
//! `assign` is a convenience wrapper for "this motor is currently
//! unassigned (no limb / joint_kind); set them and recompute the role to
//! canonical form." Internally calls the rename pipeline with the derived
//! role.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state::{self, BootState};
use crate::inventory::{self, validate_canonical_role, Motor};
use crate::limb::JointKind;
use crate::state::SharedState;
use crate::types::{ApiError, SafetyEvent};
use crate::util::session_from_headers;

#[derive(Debug, Deserialize)]
pub struct RenameBody {
    pub new_role: String,
}

#[derive(Debug, Deserialize)]
pub struct AssignBody {
    pub limb: String,
    pub joint_kind: JointKind,
}

#[derive(Debug, Serialize)]
pub struct RenameResp {
    pub ok: bool,
    pub new_role: String,
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

pub async fn rename(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<RenameBody>,
) -> Result<Json<RenameResp>, (StatusCode, Json<ApiError>)> {
    do_rename(state, headers, role, body.new_role, None).await
}

pub async fn assign(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<AssignBody>,
) -> Result<Json<RenameResp>, (StatusCode, Json<ApiError>)> {
    let new_role = format!("{}.{}", body.limb, body.joint_kind.as_snake_case());
    do_rename(
        state,
        headers,
        role,
        new_role,
        Some((body.limb, body.joint_kind)),
    )
    .await
}

async fn do_rename(
    state: SharedState,
    headers: axum::http::HeaderMap,
    role: String,
    new_role: String,
    assignment: Option<(String, JointKind)>,
) -> Result<Json<RenameResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);
    if !state.has_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some("another operator holds the control lock".into()),
        ));
    }

    if let Err(e) = validate_canonical_role(&new_role) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_role",
            Some(format!("{e:#}")),
        ));
    }

    if new_role == role {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "no_op",
            Some("new_role equals current role".into()),
        ));
    }

    {
        let inv = state.inventory.read().expect("inventory poisoned");
        if inv.by_role(&role).is_none() {
            return Err(err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            ));
        }
        if inv.by_role(&new_role).is_some() {
            return Err(err(
                StatusCode::CONFLICT,
                "role_in_use",
                Some(format!("another motor already has role={new_role}")),
            ));
        }
        if matches!(boot_state::current(&state, &role), BootState::Homed) {
            // The plan calls for refusing rename while the motor is
            // active. We approximate "active" with "Homed" — an enabled
            // motor must be Homed first, so this catches the dangerous
            // case and admits the safe one.
            return Err(err(
                StatusCode::CONFLICT,
                "motor_active",
                Some("disable the motor (POST /stop) before renaming".into()),
            ));
        }
    }

    let inv_path = state.cfg.paths.inventory.clone();
    let role_for_closure = role.clone();
    let new_role_for_closure = new_role.clone();
    let assignment_for_closure = assignment.clone();

    let new_inv = tokio::task::spawn_blocking(move || {
        inventory::write_atomic(&inv_path, |inv| {
            let m: &mut Motor = inv
                .motors
                .iter_mut()
                .find(|m| m.role == role_for_closure)
                .ok_or_else(|| anyhow::anyhow!("motor disappeared"))?;
            m.role = new_role_for_closure.clone();
            if let Some((limb, jk)) = &assignment_for_closure {
                m.limb = Some(limb.clone());
                m.joint_kind = Some(*jk);
            }
            Ok(())
        })
    })
    .await
    .expect("rename write task panicked")
    .map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "inventory_write_failed",
            Some(format!("{e:#}")),
        )
    })?;

    *state.inventory.write().expect("inventory poisoned") = new_inv;

    // Migrate live in-memory maps so the new role can be looked up
    // immediately without waiting for the next telemetry tick.
    {
        let mut latest = state.latest.write().expect("latest poisoned");
        if let Some(fb) = latest.remove(&role) {
            let mut fb = fb;
            fb.role = new_role.clone();
            latest.insert(new_role.clone(), fb);
        }
    }
    {
        let mut params = state.params.write().expect("params poisoned");
        if let Some(snap) = params.remove(&role) {
            let mut snap = snap;
            snap.role = new_role.clone();
            params.insert(new_role.clone(), snap);
        }
    }
    {
        let mut bs = state.boot_state.write().expect("boot_state poisoned");
        if let Some(s) = bs.remove(&role) {
            bs.insert(new_role.clone(), s);
        }
    }

    let _ = state.safety_event_tx.send(SafetyEvent::MotorRenamed {
        t_ms: Utc::now().timestamp_millis(),
        old_role: role.clone(),
        new_role: new_role.clone(),
    });

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "motor_renamed".into(),
        target: Some(role.clone()),
        details: serde_json::json!({
            "old_role": role,
            "new_role": new_role,
            "assignment": assignment.map(|(l, jk)| serde_json::json!({"limb": l, "joint_kind": jk})),
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(RenameResp {
        ok: true,
        new_role,
    }))
}
