//! POST /api/motors/:role/jog — hold-to-jog dead-man.
//!
//! The SPA fires this at ~20 Hz while the operator holds a jog button.
//! Each call:
//!
//!   1. Validates against firmware spec (`limit_spd.hardware_range`) and
//!      the motor's soft travel band (`travel_limits.{min,max}_rad`).
//!   2. Issues a velocity-mode setpoint via `cmd_enable + run_mode=2 +
//!      spd_ref`.
//!   3. Refreshes the per-motor TTL watchdog so a single shared
//!      background task issues `cmd_stop` if no follow-up jog frame
//!      arrives within `ttl_ms`.
//!
//! The watchdog matters because a hung browser tab or a dropped network
//! frame would otherwise leave the motor running forever — the firmware
//! `canTimeout` is the backstop, but rudydae's TTL is tighter and fires
//! a clean type-4 stop at the protocol layer.

use std::sync::OnceLock;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::audit::{AuditEntry, AuditResult};
use crate::boot_state::{self, BootState};
use crate::can::motion::shortest_signed_delta;
use crate::can::travel::{enforce_position_with_path, BandCheck};
use crate::state::SharedState;
use crate::types::ApiError;
use crate::util::session_from_headers;

/// Soft cap matching `bench_tool::MAX_TARGET_VEL_RAD_S`. The daemon refuses
/// jog setpoints above this even if the firmware envelope is wider, so the
/// browser UI can't accidentally request something the bench routines
/// would also reject.
const MAX_JOG_VEL_RAD_S: f32 = 0.5;
const MAX_TTL_MS: u64 = 1_000;

#[derive(Debug, Deserialize)]
pub struct JogBody {
    pub vel_rad_s: f32,
    /// How long the daemon should treat the previous jog frame as still
    /// "live". On the next tick after this expires the watchdog issues
    /// `cmd_stop`.
    pub ttl_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct JogResp {
    pub ok: bool,
    pub clamped_vel_rad_s: f32,
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

pub async fn jog(
    State(state): State<SharedState>,
    headers: axum::http::HeaderMap,
    Path(role): Path<String>,
    Json(body): Json<JogBody>,
) -> Result<Json<JogResp>, (StatusCode, Json<ApiError>)> {
    let session = session_from_headers(&headers);

    if let Err(holder) = state.ensure_control(session.as_deref().unwrap_or("")) {
        return Err(err(
            StatusCode::from_u16(423).unwrap(),
            "lock_held",
            Some(format!("control lock is held by session {holder}")),
        ));
    }

    let motor = {
        let inv = state.inventory.read().expect("inventory poisoned");
        inv.by_role(&role).cloned().ok_or_else(|| {
            err(
                StatusCode::NOT_FOUND,
                "unknown_motor",
                Some(format!("no motor with role={role}")),
            )
        })?
    };

    if !motor.present {
        return Err(err(
            StatusCode::CONFLICT,
            "motor_absent",
            Some(format!("inventory entry for {role} has present=false")),
        ));
    }
    if state.cfg.safety.require_verified && !motor.verified {
        return Err(err(
            StatusCode::FORBIDDEN,
            "not_verified",
            Some(format!(
                "inventory entry for {role} has verified=false; commission before jogging"
            )),
        ));
    }

    if !body.vel_rad_s.is_finite() {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "bad_request",
            Some("vel_rad_s must be finite".into()),
        ));
    }
    let clamped = body.vel_rad_s.clamp(-MAX_JOG_VEL_RAD_S, MAX_JOG_VEL_RAD_S);
    let ttl_ms = body.ttl_ms.clamp(50, MAX_TTL_MS);

    // Boot-state gate: refuse jog while Layer 6 is driving the motor, and
    // refuse while Unknown/OutOfBand. InBand is permitted (operator can jog
    // around for inspection), Homed is permitted.
    let bs = boot_state::current(&state, &role);
    if bs.is_auto_recovering() {
        return Err(err(
            StatusCode::CONFLICT,
            "auto_recovery_in_progress",
            Some(format!(
                "auto-recovery is driving {role}; wait for completion"
            )),
        ));
    }
    if matches!(bs, BootState::Unknown) {
        return Err(err(
            StatusCode::CONFLICT,
            "not_ready",
            Some(format!("no telemetry yet for {role}; cannot jog")),
        ));
    }
    if let BootState::OutOfBand {
        mech_pos_rad,
        min_rad,
        max_rad,
    } = bs
    {
        return Err(err(
            StatusCode::CONFLICT,
            "out_of_band",
            Some(format!(
                "{role} is at {mech_pos_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]; manual recovery required"
            )),
        ));
    }

    // Use the latest cached position to bound the next setpoint. The check
    // is conservative: if we have no recent feedback we let the firmware
    // envelope handle it.
    if let Some(fb) = state.latest.read().expect("latest poisoned").get(&role) {
        // Project where the motor would be after `ttl_ms` at `clamped` and
        // refuse if that lands outside the band. Path-aware: also rejects
        // any jog that would sweep across the band boundary.
        let projected = fb.mech_pos_rad + clamped * (ttl_ms as f32 / 1000.0);
        let check =
            enforce_position_with_path(&state, &role, fb.mech_pos_rad, projected).map_err(|e| {
                err(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    Some(format!("{e:#}")),
                )
            })?;
        match check {
            BandCheck::OutOfBand {
                min_rad,
                max_rad,
                attempted_rad,
            } => {
                return Err(err(
                    StatusCode::CONFLICT,
                    "travel_limit_violation",
                    Some(format!(
                    "projected position {attempted_rad:.3} rad outside [{min_rad:.3}, {max_rad:.3}]"
                )),
                ));
            }
            BandCheck::PathViolation {
                min_rad,
                max_rad,
                current_rad,
                target_rad,
            } => {
                return Err(err(
                    StatusCode::CONFLICT,
                    "path_violation",
                    Some(format!(
                    "current {current_rad:.3} -> target {target_rad:.3} sweeps outside [{min_rad:.3}, {max_rad:.3}]"
                )),
                ));
            }
            _ => {}
        }

        // Per-step ceiling (Defense Layer 2): while not Homed, the
        // projected motion can't exceed `boot_max_step_rad` regardless of
        // the band check above. This is the safety net that catches
        // anything trying to skip past the homer.
        if !matches!(bs, BootState::Homed) {
            let delta = shortest_signed_delta(fb.mech_pos_rad, projected).abs();
            if delta > state.cfg.safety.boot_max_step_rad {
                return Err(err(
                    StatusCode::CONFLICT,
                    "step_too_large",
                    Some(format!(
                    "projected delta {delta:.3} rad exceeds boot_max_step_rad {:.3} rad; run /home first",
                    state.cfg.safety.boot_max_step_rad
                )),
                ));
            }
        }
    }

    if let Some(core) = state.real_can.clone() {
        let motor_for_blocking = motor.clone();
        let v = clamped;
        tokio::task::spawn_blocking(move || core.set_velocity_setpoint(&motor_for_blocking, v))
            .await
            .expect("jog task panicked")
            .map_err(|e| {
                err(
                    StatusCode::BAD_GATEWAY,
                    "can_command_failed",
                    Some(format!("jog setpoint failed for {role}: {e:#}")),
                )
            })?;
    }

    watchdog_arm(state.clone(), &motor.role, ttl_ms);

    state.audit.write(AuditEntry {
        timestamp: Utc::now(),
        session_id: session,
        remote: None,
        action: "jog".into(),
        target: Some(role),
        details: serde_json::json!({
            "vel_rad_s": clamped,
            "ttl_ms": ttl_ms,
        }),
        result: AuditResult::Ok,
    });

    Ok(Json(JogResp {
        ok: true,
        clamped_vel_rad_s: clamped,
    }))
}

/// Refresh the per-motor watchdog deadline. Spawns the shared watchdog
/// loop on first use.
fn watchdog_arm(state: SharedState, role: &str, ttl_ms: u64) {
    let map = watchdog_state();
    {
        let mut guard = map.lock().expect("watchdog poisoned");
        guard.insert(
            role.to_string(),
            Instant::now() + Duration::from_millis(ttl_ms),
        );
    }
    static SPAWN: OnceLock<()> = OnceLock::new();
    SPAWN.get_or_init(|| {
        tokio::spawn(watchdog_loop(state));
    });
}

type DeadlineMap = std::sync::Mutex<std::collections::HashMap<String, Instant>>;

fn watchdog_state() -> &'static DeadlineMap {
    static MAP: OnceLock<DeadlineMap> = OnceLock::new();
    MAP.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

async fn watchdog_loop(state: SharedState) {
    use tokio::time::interval;
    let mut tick = interval(Duration::from_millis(25));
    loop {
        tick.tick().await;
        let now = Instant::now();
        let expired: Vec<String> = {
            let mut guard = watchdog_state().lock().expect("watchdog poisoned");
            let expired: Vec<String> = guard
                .iter()
                .filter_map(|(k, deadline)| {
                    if *deadline <= now {
                        Some(k.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for k in &expired {
                guard.remove(k);
            }
            expired
        };

        if expired.is_empty() {
            continue;
        }

        let inv_snap = state
            .inventory
            .read()
            .expect("inventory poisoned")
            .motors
            .clone();
        for role in expired {
            let Some(motor) = inv_snap.iter().find(|m| m.role == role).cloned() else {
                continue;
            };
            if let Some(core) = state.real_can.clone() {
                let _ = tokio::task::spawn_blocking(move || core.stop(&motor)).await;
            }
            // Watchdog timeout means rudydae just commanded a stop; clear
            // the rename / assign gate so the operator can edit role
            // metadata immediately afterward without first clicking STOP.
            state.mark_stopped(&role);
            state.audit.write(AuditEntry {
                timestamp: Utc::now(),
                session_id: None,
                remote: None,
                action: "jog_watchdog_stop".into(),
                target: Some(role),
                details: serde_json::Value::Null,
                result: AuditResult::Ok,
            });
        }
    }
}
