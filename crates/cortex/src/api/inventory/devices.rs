//! `/api/devices` endpoints:
//! - GET `/api/devices` — full polymorphic inventory (`devices:` from `inventory.yaml`).
//! - DELETE `/api/devices/:role` — remove one actuator row from inventory.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Serialize;

use crate::api::error::err;
use crate::audit::{AuditEntry, AuditResult};
use crate::inventory::{self, Device};
use crate::state::SharedState;
use crate::types::{ApiError, SafetyEvent};
use crate::util::session_from_headers;

pub async fn list_devices(State(state): State<SharedState>) -> Json<Vec<Device>> {
    let inv = state.inventory.read().expect("inventory poisoned");
    Json(inv.devices.clone())
}

#[derive(Debug, Serialize)]
pub struct RemoveDeviceResp {
    pub ok: bool,
    pub role: String,
}

pub async fn remove_device(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
) -> Result<Json<RemoveDeviceResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).expect("status 423 is valid"),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }

    let (can_bus, can_id, family) = {
        let inv = state.inventory.read().expect("inventory poisoned");
        let Some(device) = inv.by_role(&role) else {
            return Err(err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            ));
        };
        match device {
            Device::Actuator(a) => (
                a.common.can_bus.clone(),
                a.common.can_id,
                match a.family {
                    crate::inventory::ActuatorFamily::Robstride { model } => {
                        format!("robstride:{model:?}")
                    }
                },
            ),
            Device::Sensor(_) | Device::Battery(_) => {
                return Err(err(
                    StatusCode::BAD_REQUEST,
                    "unsupported_kind",
                    Some(format!("role={role} is not an actuator")),
                ))
            }
        }
    };

    if state.is_enabled(&role) {
        return Err(err(
            StatusCode::CONFLICT,
            "motor_active",
            Some(format!("motor {role} is enabled; stop it before removing")),
        ));
    }
    if state.motion.current(&role).is_some() {
        return Err(err(
            StatusCode::CONFLICT,
            "motion_active",
            Some(format!("motor {role} has active motion; stop motion first")),
        ));
    }

    let inv_path = state.cfg.paths.inventory.clone();
    let role_for_closure = role.clone();
    let new_inv = tokio::task::spawn_blocking(move || {
        inventory::write_atomic(&inv_path, |inv| {
            let before = inv.devices.len();
            inv.devices
                .retain(|d| !matches!(d, Device::Actuator(a) if a.common.role == role_for_closure));
            if inv.devices.len() == before {
                anyhow::bail!("actuator disappeared");
            }
            Ok(())
        })
    })
    .await
    .expect("remove-device write task panicked")
    .map_err(|e| {
        err(
            StatusCode::INTERNAL_SERVER_ERROR,
            "inventory_write_failed",
            Some(format!("{e:#}")),
        )
    })?;

    *state.inventory.write().expect("inventory poisoned") = new_inv;

    {
        state.latest.write().expect("latest poisoned").remove(&role);
    }
    {
        state.params.write().expect("params poisoned").remove(&role);
    }
    {
        state
            .boot_state
            .write()
            .expect("boot_state poisoned")
            .remove(&role);
    }
    {
        state
            .enabled
            .write()
            .expect("enabled poisoned")
            .remove(&role);
    }
    {
        state
            .boot_orchestrator_attempted
            .lock()
            .expect("boot_orchestrator_attempted poisoned")
            .remove(&role);
    }

    let had_active_motion = state.motion.stop(&role).await;
    {
        let mut seen = state.seen_can_ids.write().expect("seen_can_ids poisoned");
        seen.remove(&(can_bus.clone(), can_id));
    }

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "motor_removed".into(),
        target: Some(role.clone()),
        details: serde_json::json!({
            "role": role,
            "can_bus": can_bus,
            "can_id": can_id,
            "family": family,
            "had_active_motion_cleanup": had_active_motion,
        }),
        result: AuditResult::Ok,
    });

    let _ = state.safety_event_tx.send(SafetyEvent::MotorRemoved {
        t_ms: Utc::now().timestamp_millis(),
        role: role.clone(),
    });

    Ok(Json(RemoveDeviceResp { ok: true, role }))
}
