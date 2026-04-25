//! POST /api/hardware/onboard/robstride — append a new RobStride actuator to inventory.
//!
//! Used by the Hardware page onboarding wizard. Validates canonical role derived
//! from `limb` + `joint_kind`, travel band vs spec rail, and predefined home inside
//! the band. Hot-swaps `state.inventory` and drops the `(can_bus, can_id)` from
//! `seen_can_ids` when present.

use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::api::error::err;
use crate::audit::{AuditEntry, AuditResult};
use crate::can::travel::validate_band;
use crate::inventory::{
    self, Actuator, ActuatorCommon, ActuatorFamily, Device, RobstrideModel, TravelLimits,
};
use crate::limb::JointKind;
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

#[derive(Debug, Deserialize)]
pub struct OnboardRobstrideBody {
    pub can_bus: String,
    pub can_id: u8,
    pub model: RobstrideModel,
    pub limb: String,
    pub joint_kind: JointKind,
    pub travel_min_rad: f32,
    pub travel_max_rad: f32,
    #[serde(default)]
    pub predefined_home_rad: Option<f32>,
}

#[derive(Debug, Serialize)]
pub struct OnboardRobstrideResp {
    pub ok: bool,
    pub role: String,
}

fn validate_limb_segment(limb: &str) -> Result<(), String> {
    if limb.is_empty() {
        return Err("limb is empty".into());
    }
    if limb.contains('.') {
        return Err("limb must not contain '.'".into());
    }
    for c in limb.chars() {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return Err(format!("limb contains illegal character: {c:?}"));
        }
    }
    Ok(())
}

pub async fn onboard_robstride(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Json(body): Json<OnboardRobstrideBody>,
) -> Result<Json<OnboardRobstrideResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }

    if let Err(e) = validate_limb_segment(&body.limb) {
        return Err(err(StatusCode::BAD_REQUEST, "invalid_limb", Some(e)));
    }

    let role = format!("{}.{}", body.limb, body.joint_kind.as_snake_case());
    if let Err(e) = inventory::validate_canonical_role(&role) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_role",
            Some(format!("{e:#}")),
        ));
    }

    if !state.specs.contains_key(&body.model) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "unknown_model",
            Some(format!(
                "no actuator spec loaded for {}; add robstride_{}.yaml",
                body.model.as_spec_label(),
                body.model.robstride_yaml_suffix()
            )),
        ));
    }

    if body.can_id == 0 {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_can_id",
            Some("can_id 0 is reserved".into()),
        ));
    }

    let (hw_lo, hw_hi) = state.spec_for(body.model).mit_position_rail_rad();
    if let Err(reason) = validate_band(body.travel_min_rad, body.travel_max_rad, hw_lo, hw_hi) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "travel_band_invalid",
            Some(reason.to_string()),
        ));
    }

    let home = body.predefined_home_rad.unwrap_or(0.0);
    if !home.is_finite() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "invalid_predefined_home",
            Some("predefined_home_rad must be finite".into()),
        ));
    }
    if home < body.travel_min_rad || home > body.travel_max_rad {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "predefined_home_outside_band",
            Some(format!(
                "predefined_home_rad {home} must lie within [{}, {}]",
                body.travel_min_rad, body.travel_max_rad
            )),
        ));
    }

    {
        let inv = state.inventory.read().expect("inventory poisoned");
        if inv.by_role(&role).is_some() {
            return Err(err(
                StatusCode::CONFLICT,
                "role_in_use",
                Some(format!("a device with role {role} already exists")),
            ));
        }
        if inv.by_can_id(&body.can_bus, body.can_id).is_some() {
            return Err(err(
                StatusCode::CONFLICT,
                "can_id_in_use",
                Some(format!(
                    "({}, 0x{:02x}) is already assigned in inventory",
                    body.can_bus, body.can_id
                )),
            ));
        }
    }

    let inv_path = state.cfg.paths.inventory.clone();
    let db_ctx = state.runtime_inventory_persist();
    let home_rad = home;
    let body_clone = OnboardRobstrideBody {
        can_bus: body.can_bus.clone(),
        can_id: body.can_id,
        model: body.model,
        limb: body.limb.clone(),
        joint_kind: body.joint_kind,
        travel_min_rad: body.travel_min_rad,
        travel_max_rad: body.travel_max_rad,
        predefined_home_rad: body.predefined_home_rad,
    };
    let role_for_task = role.clone();

    let new_inv = tokio::task::spawn_blocking(move || {
        inventory::write_atomic(&inv_path, db_ctx, |inv| {
            if inv.by_role(&role_for_task).is_some() {
                anyhow::bail!("role_in_use");
            }
            if inv
                .by_can_id(&body_clone.can_bus, body_clone.can_id)
                .is_some()
            {
                anyhow::bail!("can_id_in_use");
            }
            let actuator = Actuator {
                common: ActuatorCommon {
                    role: role_for_task.clone(),
                    can_bus: body_clone.can_bus.clone(),
                    can_id: body_clone.can_id,
                    present: true,
                    verified: false,
                    commissioned_at: None,
                    firmware_version: None,
                    travel_limits: Some(TravelLimits {
                        min_rad: body_clone.travel_min_rad,
                        max_rad: body_clone.travel_max_rad,
                        updated_at: None,
                    }),
                    commissioned_zero_offset: None,
                    active_report_persisted: false,
                    predefined_home_rad: Some(home_rad),
                    homing_speed_rad_s: None,
                    hold_kp_nm_per_rad: None,
                    hold_kd_nm_s_per_rad: None,
                    mit_command_kp_nm_per_rad: None,
                    mit_command_kd_nm_s_per_rad: None,
                    mit_max_angle_step_rad: None,
                    limb: Some(body_clone.limb.clone()),
                    joint_kind: Some(body_clone.joint_kind),
                    notes_yaml: None,
                    desired_params: std::collections::BTreeMap::new(),
                    // Onboarding wizard doesn't expose
                    // direction_sign yet; default to +1 and let the
                    // operator flip via inventory edit + restart if a
                    // bench jog reveals inverted polarity. See
                    // ActuatorCommon::direction_sign.
                    direction_sign: 1,
                },
                family: ActuatorFamily::Robstride {
                    model: body_clone.model,
                },
            };
            inv.devices.push(Device::Actuator(actuator));
            Ok(())
        })
    })
    .await
    .expect("onboard write task panicked")
    .map_err(|e| {
        let msg = format!("{e:#}");
        let code = if msg.contains("role_in_use") || msg.contains("can_id_in_use") {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        err(code, "inventory_write_failed", Some(msg))
    })?;

    *state.inventory.write().expect("inventory poisoned") = new_inv;

    {
        let mut seen = state.seen_can_ids.write().expect("seen_can_ids poisoned");
        seen.remove(&(body.can_bus.clone(), body.can_id));
    }

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session.clone(),
        remote: None,
        action: "hardware_onboard_robstride".into(),
        target: Some(role.clone()),
        details: serde_json::json!({
            "can_bus": body.can_bus,
            "can_id": body.can_id,
            "model": body.model.as_spec_label(),
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(OnboardRobstrideResp { ok: true, role }))
}
