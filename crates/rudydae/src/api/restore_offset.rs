//! POST /api/motors/:role/restore_offset — recover from `OffsetChanged`.
//!
//! When the boot orchestrator detects that firmware `add_offset` (0x702B)
//! disagrees with `commissioned_zero_offset` in inventory, it lands the motor
//! in [`BootState::OffsetChanged`]. This endpoint writes the **inventory**
//! value back to the firmware (RAM + flash via type-22), verifies readback,
//! then resets boot state to [`BootState::Unknown`] so telemetry can
//! re-classify and the orchestrator can run again.
//!
//! Mock-CAN (`state.real_can = None`): no frames are sent; readback is
//! assumed to match the stored commission value.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Serialize;

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_orchestrator;
use crate::boot_state::{self, BootState};
use crate::state::SharedState;
use crate::util::session_from_headers;

#[derive(Debug, Serialize)]
pub struct RestoreOffsetResp {
    pub ok: bool,
    pub role: String,
    pub restored_rad: f32,
    pub readback_rad: f32,
}

fn fail(
    status: StatusCode,
    detail: String,
    readback_rad: Option<f32>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": "restore_failed",
            "detail": detail,
            "readback_rad": readback_rad,
        })),
    )
}

fn audit(
    state: &SharedState,
    session: Option<String>,
    role: &str,
    result: AuditResult,
    step: &str,
    restored_rad: Option<f32>,
    readback_rad: Option<f32>,
    detail: Option<&str>,
) {
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "restore_offset".into(),
        target: Some(role.into()),
        details: serde_json::json!({
            "step": step,
            "restored_rad": restored_rad,
            "readback_rad": readback_rad,
            "detail": detail,
        }),
        result,
    });
}

pub async fn restore_offset(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
) -> Result<Json<RestoreOffsetResp>, (StatusCode, Json<serde_json::Value>)> {
    let session = session_from_headers(&headers);

    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        let detail = format!("control lock is held by session {holder}");
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 1 (lock_held)",
            None,
            None,
            Some(&detail),
        );
        return Err(fail(
            StatusCode::from_u16(423).unwrap(),
            format!("step 1 (lock_held): {detail}"),
            None,
        ));
    }

    let motor = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .by_role(&role)
        .cloned()
        .ok_or_else(|| {
            let detail = format!("no motor with role={role}");
            audit(
                &state,
                session.clone(),
                &role,
                AuditResult::Denied,
                "step 2 (unknown_motor)",
                None,
                None,
                Some(&detail),
            );
            fail(
                StatusCode::NOT_FOUND,
                format!("step 2 (unknown_motor): {detail}"),
                None,
            )
        })?;

    if !motor.present {
        let detail = format!("inventory entry for {role} has present=false");
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 2 (motor_absent)",
            None,
            None,
            Some(&detail),
        );
        return Err(fail(
            StatusCode::CONFLICT,
            format!("step 2 (motor_absent): {detail}"),
            None,
        ));
    }

    let Some(stored_commission) = motor.commissioned_zero_offset else {
        let detail = "motor has no commissioned_zero_offset; use POST /commission first".to_string();
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 3 (not_commissioned)",
            None,
            None,
            Some(&detail),
        );
        return Err(fail(StatusCode::CONFLICT, format!("step 3 (not_commissioned): {detail}"), None));
    };

    if !stored_commission.is_finite() {
        let detail = format!("commissioned_zero_offset is non-finite ({stored_commission})");
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 3 (bad_inventory)",
            None,
            None,
            Some(&detail),
        );
        return Err(fail(
            StatusCode::CONFLICT,
            format!("step 3 (bad_inventory): {detail}"),
            None,
        ));
    }

    let bs = boot_state::current(&state, &role);
    let BootState::OffsetChanged { stored_rad, .. } = bs else {
        let detail = format!("boot_state is {:?}, not OffsetChanged; nothing to restore", bs);
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 4 (wrong_boot_state)",
            None,
            None,
            Some(&detail),
        );
        return Err(fail(
            StatusCode::CONFLICT,
            format!("step 4 (wrong_boot_state): {detail}"),
            None,
        ));
    };

    let tol = state.cfg.safety.commission_readback_tolerance_rad;
    if (stored_rad - stored_commission).abs() > tol {
        let detail = format!(
            "OffsetChanged.stored_rad ({stored_rad}) disagrees with inventory commissioned_zero_offset ({stored_commission}) beyond tolerance ({tol})"
        );
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 4 (inventory_boot_mismatch)",
            None,
            None,
            Some(&detail),
        );
        return Err(fail(
            StatusCode::CONFLICT,
            format!("step 4 (inventory_boot_mismatch): {detail}"),
            None,
        ));
    }

    let readback_rad = if let Some(core) = state.real_can.clone() {
        let state_for_blocking = state.clone();
        let motor_for_blocking = motor.clone();
        let target = stored_commission;
        let result = tokio::task::spawn_blocking(move || -> Result<f32, (String, &'static str)> {
            core.write_add_offset_persisted(&state_for_blocking, &motor_for_blocking, target)
                .map_err(|e| (format!("{e:#}"), "step 5 (write_add_offset_persisted)"))?;
            std::thread::sleep(std::time::Duration::from_millis(100));
            core.read_add_offset(&state_for_blocking, &motor_for_blocking)
                .map_err(|e| (format!("{e:#}"), "step 6 (read_add_offset)"))
        })
        .await
        .expect("restore_offset CAN task panicked");

        match result {
            Ok(v) => v,
            Err((cause, step)) => {
                let detail = format!("{step}: {cause}");
                audit(
                    &state,
                    session.clone(),
                    &role,
                    AuditResult::Denied,
                    step,
                    Some(stored_commission),
                    None,
                    Some(&detail),
                );
                return Err(fail(StatusCode::BAD_GATEWAY, detail, None));
            }
        }
    } else {
        stored_commission
    };

    if !readback_rad.is_finite() {
        let detail = format!("step 6 (readback): firmware reported non-finite value {readback_rad}");
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 6 (readback)",
            Some(stored_commission),
            Some(readback_rad),
            Some(&detail),
        );
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            detail,
            Some(readback_rad),
        ));
    }

    if (readback_rad - stored_commission).abs() > tol {
        let detail = format!(
            "readback {readback_rad} disagrees with commissioned value {stored_commission} (tolerance {tol})"
        );
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 7 (readback_mismatch)",
            Some(stored_commission),
            Some(readback_rad),
            Some(&detail),
        );
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            detail,
            Some(readback_rad),
        ));
    }

    boot_state::reset_to_unknown(&state, &role);
    boot_orchestrator::clear_orchestrator_attempted(&state, &role);

    audit(
        &state,
        session,
        &role,
        AuditResult::Ok,
        "ok",
        Some(stored_commission),
        Some(readback_rad),
        None,
    );

    Ok(Json(RestoreOffsetResp {
        ok: true,
        role,
        restored_rad: stored_commission,
        readback_rad,
    }))
}
