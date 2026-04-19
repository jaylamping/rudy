//! POST /api/motors/:role/commission — flash-persistent zero + readback.
//!
//! The supported flash-persistent zeroing path. Replaces the bench-flow
//! "click set_zero, then separately click save" that operators were
//! using before — both because that flow is two-step and easy to half-
//! complete, and because nothing in the daemon was confirming the
//! firmware actually accepted either step. This endpoint is the
//! atomic, audited, single-button equivalent.
//!
//! Sequence (every step must succeed; any failure aborts and leaves
//! `inventory.yaml` untouched):
//!
//! 1. Control-lock check — only the session that holds the implicit
//!    operator lock may commission a motor.
//! 2. `motor.present` check — refuse with 409 `motor_absent` if the
//!    inventory marks the motor as not on the bus.
//! 3. type-6 SetZero — re-anchor the firmware's `add_offset` (0x702B)
//!    at the joint's current physical position.
//! 4. type-22 SaveParams — flush every RAM-resident parameter to flash
//!    so the new zero survives a power cycle.
//! 5. Sleep 100 ms — give the firmware time to finish its internal
//!    flash bookkeeping. The RS03's flash write is asynchronous w.r.t.
//!    the type-22 ACK; if we read `add_offset` back too quickly we can
//!    see the pre-write value.
//! 6. Read `add_offset` over CAN — confirm the firmware actually
//!    persisted what we asked for.
//! 7. Atomic inventory rewrite — store the readback value in
//!    `commissioned_zero_offset` and bump `commissioned_at` to "now".
//!    On any earlier failure the file is NOT touched.
//! 8. Emit `SafetyEvent::Commissioned { role, offset_rad }` so the
//!    dashboard can flip the motor's status badge without polling.
//! 9. Audit-log the success with the readback value in `details`.
//!
//! On every failure path the response carries:
//!
//! ```json
//! { "error": "commission_failed",
//!   "detail": "step N (descriptor): ...",
//!   "readback_rad": <number or null> }
//! ```
//!
//! `readback_rad` is `null` when the failure happened before the
//! readback step, and carries the firmware's reported value when the
//! readback step itself failed sanity (e.g. "expected ~0.0, got 0.42").
//! The SPA uses this to render a more helpful "we asked the firmware
//! to zero, but it reports X" toast than a generic error.
//!
//! Mock-mode safety: when `state.real_can = None` (every non-Linux dev
//! host, plus Linux with `cfg.can.mock = true`), steps 3 / 4 / 6 are
//! treated as no-ops that succeed and the readback defaults to 0.0.
//! This lets contract tests for the endpoint AND for the upcoming boot
//! orchestrator (Phase C) run without a real bus.

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::Serialize;

use crate::audit::{AuditEntry, AuditResult};
use crate::inventory;
use crate::state::SharedState;
use crate::types::SafetyEvent;
use crate::util::session_from_headers;

/// Successful commission response.
#[derive(Debug, Serialize)]
pub struct CommissionResp {
    pub ok: bool,
    pub role: String,
    /// Value of `add_offset` (0x702B) read back from the firmware
    /// AFTER the SaveParams flush. Recorded in `inventory.yaml` as
    /// `commissioned_zero_offset` and used by the boot orchestrator on
    /// every subsequent boot for the Class-1 shenanigan check.
    pub offset_rad: f32,
    /// ISO 8601 wallclock at which the inventory was rewritten. Mirrors
    /// the value the daemon stored in `motor.commissioned_at`.
    pub commissioned_at: String,
}

/// Build a `commission_failed` response carrying the optional readback.
/// All commission failure paths route through this so the wire shape is
/// uniform: `{ error: "commission_failed", detail, readback_rad }`.
fn fail(
    status: StatusCode,
    detail: String,
    readback_rad: Option<f32>,
) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({
            "error": "commission_failed",
            "detail": detail,
            "readback_rad": readback_rad,
        })),
    )
}

/// Audit a commission outcome. Every entry — Ok or Denied — carries the
/// step at which the flow finished plus the readback value when one
/// was obtained, so post-hoc analysis can answer "why did this
/// commission fail?" or "what offset did the firmware actually flash?"
/// without parsing the response body.
fn audit(
    state: &SharedState,
    session: Option<String>,
    role: &str,
    result: AuditResult,
    step: &str,
    readback_rad: Option<f32>,
    detail: Option<&str>,
) {
    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "commission".into(),
        target: Some(role.into()),
        details: serde_json::json!({
            "step": step,
            "readback_rad": readback_rad,
            "detail": detail,
        }),
        result,
    });
}

pub async fn commission(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
) -> Result<Json<CommissionResp>, (StatusCode, Json<serde_json::Value>)> {
    let session = session_from_headers(&headers);

    // Step 1 — control-lock check. Convert the daemon's standard
    // 423 `lock_held` shape into the commission-specific envelope so
    // the SPA's commission UI can use one parser for every failure.
    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        let detail = format!("control lock is held by session {holder}");
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 1 (lock_held)",
            None,
            Some(&detail),
        );
        return Err(fail(
            StatusCode::from_u16(423).unwrap(),
            format!("step 1 (lock_held): {detail}"),
            None,
        ));
    }

    // Step 2 — motor presence + existence.
    let motor = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role(&role)
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
                Some(&detail),
            );
            fail(
                StatusCode::NOT_FOUND,
                format!("step 2 (unknown_motor): {detail}"),
                None,
            )
        })?;
    if !motor.common.present {
        let detail = format!("inventory entry for {role} has present=false");
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 2 (motor_absent)",
            None,
            Some(&detail),
        );
        return Err(fail(
            StatusCode::CONFLICT,
            format!("step 2 (motor_absent): {detail}"),
            None,
        ));
    }

    // Steps 3 / 4 / 6 — the CAN-talking part. Wrap in a single
    // `spawn_blocking` so we don't sit on a Tokio worker thread for
    // the firmware's flash-flush window. Real-CAN (Linux + non-mock):
    // set_zero → save → sleep 100ms → read_add_offset. Mock: no-op
    // and the readback defaults to 0.0 (the documented stub contract;
    // see `crates/rudydae/src/can/mod.rs::RealCanHandle::read_add_offset`).
    let readback_rad = if let Some(core) = state.real_can.clone() {
        let state_for_blocking = state.clone();
        let motor_for_blocking = motor.clone();
        let result = tokio::task::spawn_blocking(move || -> Result<f32, (String, &'static str)> {
            // Step 3.
            core.set_zero(&motor_for_blocking)
                .map_err(|e| (format!("{e:#}"), "step 3 (set_zero)"))?;
            // Step 4.
            core.save_to_flash(&motor_for_blocking)
                .map_err(|e| (format!("{e:#}"), "step 4 (save_to_flash)"))?;
            // Step 5 — flash-flush settle window.
            std::thread::sleep(std::time::Duration::from_millis(100));
            // Step 6.
            core.read_add_offset(&state_for_blocking, &motor_for_blocking)
                .map_err(|e| (format!("{e:#}"), "step 6 (read_add_offset)"))
        })
        .await
        .expect("commission CAN task panicked");

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
                    None,
                    Some(&detail),
                );
                return Err(fail(StatusCode::BAD_GATEWAY, detail, None));
            }
        }
    } else {
        // Mock-mode equivalent of the readback.
        0.0
    };

    // Sanity check on the readback value itself: any non-finite reply
    // means the firmware misbehaved. (The "expected ~0.0" tolerance
    // check happens on every BOOT, not at commission time — at
    // commission time the operator has just declared "this position
    // IS the new zero", so any finite value is by definition correct.
    // The boot-time tolerance lives in `safety.commission_readback_tolerance_rad`,
    // landing in Phase C.4.)
    if !readback_rad.is_finite() {
        let detail = format!("step 6 (readback): firmware reported non-finite value {readback_rad}");
        audit(
            &state,
            session.clone(),
            &role,
            AuditResult::Denied,
            "step 6 (readback)",
            Some(readback_rad),
            Some(&detail),
        );
        return Err(fail(
            StatusCode::BAD_GATEWAY,
            detail,
            Some(readback_rad),
        ));
    }

    // Step 7 — atomic inventory rewrite. If this fails (ENOSPC,
    // permission, validation), the on-disk file is unchanged because
    // `write_atomic` writes-then-renames a sibling tempfile.
    let path = state.cfg.paths.inventory.clone();
    let role_for_closure = role.clone();
    let now_iso = Utc::now().to_rfc3339();
    let now_iso_for_closure = now_iso.clone();
    let new_inv = tokio::task::spawn_blocking(move || {
        inventory::write_atomic(&path, |inv| {
            for d in &mut inv.devices {
                if let inventory::Device::Actuator(a) = d {
                    if a.common.role == role_for_closure {
                        a.common.commissioned_zero_offset = Some(readback_rad);
                        a.common.commissioned_at = Some(now_iso_for_closure);
                        return Ok(());
                    }
                }
            }
            anyhow::bail!("motor disappeared from inventory");
        })
    })
    .await
    .expect("commission inventory write task panicked");

    let new_inv = match new_inv {
        Ok(inv) => inv,
        Err(e) => {
            let detail = format!("step 7 (inventory_write): {e:#}");
            audit(
                &state,
                session.clone(),
                &role,
                AuditResult::Denied,
                "step 7 (inventory_write)",
                Some(readback_rad),
                Some(&detail),
            );
            return Err(fail(
                StatusCode::INTERNAL_SERVER_ERROR,
                detail,
                Some(readback_rad),
            ));
        }
    };

    *state.inventory.write().expect("inventory poisoned") = new_inv;

    // Step 8 — broadcast so the dashboard can refresh without polling.
    let now_ms = Utc::now().timestamp_millis();
    let _ = state.safety_event_tx.send(SafetyEvent::Commissioned {
        t_ms: now_ms,
        role: role.clone(),
        offset_rad: readback_rad,
    });

    // Step 9 — audit success.
    audit(
        &state,
        session,
        &role,
        AuditResult::Ok,
        "ok",
        Some(readback_rad),
        None,
    );

    Ok(Json(CommissionResp {
        ok: true,
        role,
        offset_rad: readback_rad,
        commissioned_at: now_iso,
    }))
}
