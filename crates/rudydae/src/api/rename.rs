//! POST /api/motors/:role/rename and POST /api/motors/:role/assign.
//!
//! `rename` changes a motor's primary key. Validates new_role canonical
//! form, refuses duplicates. If the motor is currently enabled the daemon
//! transparently issues `cmd_stop` on the bus, performs the rename, then
//! re-issues `cmd_enable` under the new role — the response carries
//! `auto_stopped` / `auto_reenabled` flags so the SPA can surface what
//! happened. Atomically rewrites inventory.yaml, migrates the in-memory
//! `state.latest` / `state.params` / `state.boot_state` maps, audit-logs
//! the change, and emits a `MotorRenamed` safety event so subscribers
//! drop per-role caches.
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
    /// True when the motor was enabled at request time and the daemon
    /// auto-issued a stop on the bus before performing the rename. The
    /// SPA surfaces this so the operator sees that torque was briefly
    /// dropped instead of having to figure it out from a 409.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_stopped: bool,
    /// True when the daemon successfully re-enabled the motor on its
    /// new role after the rename. Only meaningful when `auto_stopped`
    /// is also true. False (with `auto_stopped: true`) means the
    /// rename succeeded but the motor is currently disabled — the
    /// operator must re-enable manually. Failure detail is in
    /// `auto_reenable_error`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub auto_reenabled: bool,
    /// Populated only when `auto_stopped` is true and the subsequent
    /// re-enable failed. Lets the SPA show "rename succeeded but
    /// re-enable failed: <reason>" in one banner.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_reenable_error: Option<String>,
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
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
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

    // Snapshot whether the motor was previously assigned, plus a clone of
    // the motor record we'll need below for any auto-stop / auto-reenable
    // CAN calls. A first-time `assign` (motor has no limb/joint_kind on
    // file) is a pure labeling operation: it does not move the motor,
    // doesn't change CAN IDs, and the in-memory boot_state migrates under
    // the new key below. There is no motion-safety reason to gate it on
    // `enabled`. So we only consider the auto-stop/auto-reenable cycle on
    // `rename` and on `assign`-of-already-assigned.
    let (was_unassigned, motor_snapshot) = {
        let inv = state.inventory.read().expect("inventory poisoned");
        let Some(motor) = inv.by_role(&role) else {
            return Err(err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            ));
        };
        if inv.by_role(&new_role).is_some() {
            return Err(err(
                StatusCode::CONFLICT,
                "role_in_use",
                Some(format!("another motor already has role={new_role}")),
            ));
        }
        (
            motor.limb.is_none() && motor.joint_kind.is_none(),
            motor.clone(),
        )
    };

    let is_first_time_assign = assignment.is_some() && was_unassigned;

    // Was the motor driving when this request landed? If yes, we transparently
    // drop torque on the bus, perform the rename, then restore the enabled
    // state under the new role. Operators were hitting a confusing 409
    // `motor_active` and had to context-switch to the Controls tab to click
    // Stop before retrying — the daemon does that round-trip itself now and
    // surfaces what happened in the response (`auto_stopped`,
    // `auto_reenabled`). First-time assign is exempt: it never engaged the
    // gate to begin with, so there's nothing to stop.
    let needs_auto_stop = !is_first_time_assign && state.is_enabled(&role);
    if needs_auto_stop {
        if let Some(core) = state.real_can.clone() {
            let motor_for_stop = motor_snapshot.clone();
            let stop_result =
                tokio::task::spawn_blocking(move || core.stop(&motor_for_stop))
                    .await
                    .expect("rename auto-stop task panicked");
            if let Err(e) = stop_result {
                state.audit.write(AuditEntry {
                    timestamp: Utc::now(),
                    session_id: session.clone(),
                    remote: None,
                    action: "rename_auto_stop".into(),
                    target: Some(role.clone()),
                    details: serde_json::json!({ "error": format!("{e:#}") }),
                    result: AuditResult::Denied,
                });
                return Err(err(
                    StatusCode::BAD_GATEWAY,
                    "can_command_failed",
                    Some(format!("auto-stop before rename failed for {role}: {e:#}")),
                ));
            }
        }
        // Clear the gate immediately so the migration below sees a clean
        // (no enabled bit on either old or new role) state. We re-set it
        // after the rename + successful re-enable.
        state.mark_stopped(&role);
        state.audit.write(AuditEntry {
            timestamp: Utc::now(),
            session_id: session.clone(),
            remote: None,
            action: "rename_auto_stop".into(),
            target: Some(role.clone()),
            details: serde_json::Value::Null,
            result: AuditResult::Ok,
        });
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
    {
        let mut en = state.enabled.write().expect("enabled poisoned");
        if en.remove(&role) {
            en.insert(new_role.clone());
        }
    }

    let _ = state.safety_event_tx.send(SafetyEvent::MotorRenamed {
        t_ms: Utc::now().timestamp_millis(),
        old_role: role.clone(),
        new_role: new_role.clone(),
    });

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session.clone(),
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

    // Restore torque under the new role if we auto-stopped. We deliberately
    // call `core.enable()` directly rather than re-routing through the
    // `enable` HTTP handler: the motor was already enabled an instant ago,
    // so by definition it had already passed the band / boot-state /
    // verified gates. Re-running them on a possibly-stale telemetry frame
    // would risk a spurious denial. `cmd_enable` is idempotent on the
    // firmware side, so the worst case is one redundant frame.
    let mut auto_reenabled = false;
    let mut auto_reenable_error: Option<String> = None;
    if needs_auto_stop {
        // Pull a fresh motor record under the new role: the rename rewrote
        // inventory and we want the post-rename copy in case anything else
        // races to mutate it later.
        let motor_for_enable = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .by_role(&new_role)
            .cloned();
        match motor_for_enable {
            Some(motor) => {
                if let Some(core) = state.real_can.clone() {
                    let motor_for_blocking = motor.clone();
                    match tokio::task::spawn_blocking(move || core.enable(&motor_for_blocking))
                        .await
                        .expect("rename auto-reenable task panicked")
                    {
                        Ok(()) => {
                            state.mark_enabled(&new_role);
                            auto_reenabled = true;
                            state.audit.write(AuditEntry {
                                timestamp: Utc::now(),
                                session_id: session.clone(),
                                remote: None,
                                action: "rename_auto_reenable".into(),
                                target: Some(new_role.clone()),
                                details: serde_json::Value::Null,
                                result: AuditResult::Ok,
                            });
                        }
                        Err(e) => {
                            auto_reenable_error = Some(format!("{e:#}"));
                            state.audit.write(AuditEntry {
                                timestamp: Utc::now(),
                                session_id: session.clone(),
                                remote: None,
                                action: "rename_auto_reenable".into(),
                                target: Some(new_role.clone()),
                                details: serde_json::json!({
                                    "error": auto_reenable_error.clone(),
                                }),
                                result: AuditResult::Denied,
                            });
                        }
                    }
                } else {
                    // Mock mode: there's no bus to talk to, but the
                    // bookkeeping still needs to follow the role so that
                    // subsequent gates see a consistent state.
                    state.mark_enabled(&new_role);
                    auto_reenabled = true;
                }
            }
            None => {
                auto_reenable_error =
                    Some(format!("motor {new_role} disappeared after rename"));
            }
        }
    }

    Ok(Json(RenameResp {
        ok: true,
        new_role,
        auto_stopped: needs_auto_stop,
        auto_reenabled,
        auto_reenable_error,
    }))
}
