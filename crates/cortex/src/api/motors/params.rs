//! GET /api/motors/:role/params, PUT /api/motors/:role/params/:name,
//! POST adopt/sync.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;

use crate::audit::{AuditEntry, AuditResult};
use crate::inventory::{self, Device};
use crate::state::SharedState;
use crate::types::{ApiError, ParamSnapshot, ParamSyncRequest, ParamWrite};

pub async fn get_params(
    State(state): State<SharedState>,
    Path(role): Path<String>,
) -> Result<Json<ParamSnapshot>, (StatusCode, Json<ApiError>)> {
    state
        .params
        .read()
        .expect("params poisoned")
        .get(&role)
        .cloned()
        .map(Json)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ApiError {
                    error: "unknown_motor".into(),
                    detail: Some(format!("no motor with role={role}")),
                    ..Default::default()
                }),
            )
        })
}

fn err(status: StatusCode, error: &str, detail: Option<String>) -> (StatusCode, Json<ApiError>) {
    (
        status,
        Json(ApiError {
            error: error.into(),
            detail,
            ..Default::default()
        }),
    )
}

/// Upsert `desired_params` in `inventory.yaml` and swap `state.inventory`.
fn persist_desired_params(
    state: &SharedState,
    role: &str,
    upsert: std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<(), anyhow::Error> {
    let role_owned = role.to_string();
    let inv_path = state.cfg.paths.inventory.clone();
    let new_inv = inventory::write_atomic(&inv_path, |inv| {
        let actuator = inv
            .devices
            .iter_mut()
            .find_map(|device| match device {
                Device::Actuator(a) if a.common.role == role_owned => Some(a),
                _ => None,
            })
            .ok_or_else(|| anyhow::anyhow!("unknown motor role {role_owned}"))?;
        for (k, v) in upsert {
            actuator.common.desired_params.insert(k, v);
        }
        Ok(())
    })?;
    *state.inventory.write().expect("inventory poisoned") = new_inv;
    Ok(())
}

fn redecorate_role(state: &SharedState, role: &str) {
    let motor = {
        let inv = state.inventory.read().expect("inventory poisoned");
        inv.actuator_by_role(role).cloned()
    };
    let Some(motor) = motor else {
        return;
    };
    let spec = state.spec_for(motor.robstride_model());
    let mut params = state.params.write().expect("params poisoned");
    let Some(snap) = params.get_mut(role) else {
        return;
    };
    let n = crate::param_sync::decorate_snapshot(&motor, &spec, snap);
    state
        .drift_counts
        .write()
        .expect("drift_counts poisoned")
        .insert(role.to_string(), n);
}

pub async fn put_param(
    State(state): State<SharedState>,
    Path((role, name)): Path<(String, String)>,
    Json(body): Json<ParamWrite>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let motor = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role(&role)
        .cloned()
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?;

    if !motor.common.present {
        state.audit.write(AuditEntry {
            timestamp: Utc::now(),
            session_id: None,
            remote: None,
            action: "param_write".into(),
            target: Some(format!("{role}.{name}")),
            details: serde_json::json!({ "value": body.value, "reason": "motor_absent" }),
            result: AuditResult::Denied,
        });
        return Err(err(
            StatusCode::CONFLICT,
            "motor_absent",
            Some(format!(
                "inventory entry for {role} has present=false; nothing to talk to on the bus"
            )),
        ));
    }

    let spec = state.spec_for(motor.robstride_model());
    let desc = spec.firmware_limits.get(&name).cloned().ok_or_else(|| {
        err(
            StatusCode::BAD_REQUEST,
            "not_writable",
            Some(format!("parameter {name} is not a writable firmware limit")),
        )
    })?;

    // Range-check floats against hardware_range.
    if let (Some([lo, hi]), Some(v)) = (desc.hardware_range, body.value.as_f64()) {
        if (v as f32) < lo || (v as f32) > hi {
            let entry = AuditEntry {
                timestamp: Utc::now(),
                session_id: None,
                remote: None,
                action: "param_write".into(),
                target: Some(format!("{role}.{name}")),
                details: serde_json::json!({ "value": body.value, "range": [lo, hi] }),
                result: AuditResult::Denied,
            };
            state.audit.write(entry);
            return Err(err(
                StatusCode::BAD_REQUEST,
                "out_of_range",
                Some(format!("{v} not in [{lo}, {hi}]")),
            ));
        }
    }

    let _ = body.save_after;

    let normalized_value = if let Some(core) = state.real_can.clone() {
        tokio::task::spawn_blocking({
            let motor = motor.clone();
            let desc = desc.clone();
            let value = body.value.clone();
            move || core.write_param(&motor, &desc, &value, true)
        })
        .await
        .expect("put_param task panicked")
        .map_err(|e| {
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: None,
                remote: None,
                action: "param_write".into(),
                target: Some(format!("{role}.{name}")),
                details: serde_json::json!({ "value": body.value, "error": format!("{e:#}") }),
                result: AuditResult::Denied,
            });
            err(
                StatusCode::BAD_GATEWAY,
                "can_command_failed",
                Some(format!("param write failed for {role}.{name}: {e:#}")),
            )
        })?
    } else {
        body.value.clone()
    };

    {
        let mut map = std::collections::BTreeMap::new();
        map.insert(name.clone(), normalized_value.clone());
        if let Err(e) = persist_desired_params(&state, &role, map) {
            tracing::error!(error = ?e, "persist desired_params failed");
            return Err(err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "inventory_write_failed",
                Some(e.to_string()),
            ));
        }
    }

    {
        let mut params = state.params.write().expect("params poisoned");
        if let Some(snap) = params.get_mut(&role) {
            if let Some(pv) = snap.values.get_mut(&name) {
                pv.value = normalized_value.clone();
            }
        }
    }
    redecorate_role(&state, &role);

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "param_write_and_save".into(),
        target: Some(format!("{role}.{name}")),
        details: serde_json::json!({
            "can_id": motor.common.can_id,
            "index": format!("0x{:04X}", desc.index),
            "value": normalized_value,
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(serde_json::json!({
        "ok": true,
        "saved": true,
        "role": role,
        "name": name,
        "value": normalized_value,
    })))
}

/// POST /api/motors/:role/params/:name/adopt — set desired = current live (inventory only).
pub async fn adopt_param(
    State(state): State<SharedState>,
    Path((role, name)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let motor = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role(&role)
        .cloned()
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?;
    let spec = state.spec_for(motor.robstride_model());
    if !spec.firmware_limits.contains_key(&name) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "not_writable",
            Some(format!("{name} is not a writable firmware limit")),
        ));
    }

    let live = {
        let params = state.params.read().expect("params poisoned");
        let snap = params
            .get(&role)
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "no_param_snapshot", None))?;
        snap.values
            .get(&name)
            .map(|p| p.value.clone())
            .ok_or_else(|| err(StatusCode::NOT_FOUND, "unknown_param", None))?
    };

    {
        let mut map = std::collections::BTreeMap::new();
        map.insert(name.clone(), live.clone());
        persist_desired_params(&state, &role, map).map_err(|e| {
            err(
                StatusCode::INTERNAL_SERVER_ERROR,
                "inventory_write_failed",
                Some(e.to_string()),
            )
        })?;
    }
    redecorate_role(&state, &role);

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: None,
        remote: None,
        action: "param_desired_adopt".into(),
        target: Some(format!("{role}.{name}")),
        details: serde_json::json!({ "value": live }),
        result: AuditResult::Ok,
    });

    Ok(Json(serde_json::json!({
        "ok": true,
        "role": role,
        "name": name,
        "desired": live,
    })))
}

/// POST /api/motors/:role/params/sync — push desired values to the device (write + save).
pub async fn sync_params(
    State(state): State<SharedState>,
    Path(role): Path<String>,
    Json(body): Json<ParamSyncRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    let motor = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role(&role)
        .cloned()
        .ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?;

    if !motor.common.present {
        return Err(err(
            StatusCode::CONFLICT,
            "motor_absent",
            Some("motor is not present on the bus".into()),
        ));
    }

    let spec = state.spec_for(motor.robstride_model());
    let names_to_sync: Vec<String> = {
        let params = state.params.read().expect("params poisoned");
        let snap = params.get(&role).ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "no_param_snapshot",
                Some(role.clone()),
            )
        })?;

        match &body.names {
            Some(names) => names.clone(),
            None => snap
                .values
                .iter()
                .filter(|(_, pv)| pv.drift.is_some())
                .map(|(k, _)| k.clone())
                .collect(),
        }
    };

    if names_to_sync.is_empty() {
        return Ok(Json(serde_json::json!({
            "ok": true,
            "role": role,
            "synced": [],
        })));
    }

    let mut synced = Vec::new();
    for name in names_to_sync {
        let desired = motor
            .common
            .desired_params
            .get(&name)
            .cloned()
            .ok_or_else(|| {
                err(
                    StatusCode::BAD_REQUEST,
                    "no_desired",
                    Some(format!("no desired value for {name} in inventory")),
                )
            })?;
        let desc = spec.firmware_limits.get(&name).cloned().ok_or_else(|| {
            err(
                StatusCode::BAD_REQUEST,
                "not_writable",
                Some(format!("{name} is not writable")),
            )
        })?;

        if let Some(core) = state.real_can.clone() {
            let m = motor.clone();
            let d = desc.clone();
            let to_write = desired.clone();
            tokio::task::spawn_blocking(move || core.write_param(&m, &d, &to_write, true))
                .await
                .expect("sync task panicked")
                .map_err(|e| {
                    err(
                        StatusCode::BAD_GATEWAY,
                        "can_command_failed",
                        Some(format!("{e:#}")),
                    )
                })?;
        }

        {
            let mut params = state.params.write().expect("params poisoned");
            if let Some(snap) = params.get_mut(&role) {
                if let Some(pv) = snap.values.get_mut(&name) {
                    pv.value = desired;
                }
            }
        }
        redecorate_role(&state, &role);
        synced.push(name);
    }

    Ok(Json(serde_json::json!({
        "ok": true,
        "role": role,
        "synced": synced,
    })))
}
