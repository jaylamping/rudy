//! Home, jog, motion patterns, limb quarantine.
//!
#![allow(unused_imports)]

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use cortex::inventory::{Device, Inventory, TravelLimits};
use cortex::types::{
    ApiError, MotorFeedback, MotorSummary, ParamSnapshot, Reminder, SafetyEvent, ServerConfig,
    ServerFeatures, SystemSnapshot, WebTransportAdvert,
};

#[path = "../common/mod.rs"]
mod common;
use common::body_json;

use cortex::boot_state::BootState;

/// Layer 5: home transitions BootState to Homed, then enable returns 200.
#[tokio::test]
async fn home_succeeds_then_enable_succeeds() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);
    let app = cortex::build_app(state.clone());

    // Mock CAN does not clock `state.latest` during `finish_home_success`'s settle;
    // mirror type-2 freshness + mech_pos at home so hold verification passes.
    let clock = common::spawn_latest_timestamp_refresh(state.clone(), "shoulder_actuator_a", 0.0);

    // POST /home with an empty body (defaults to target=0).
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/home")
                .header("content-type", "application/json")
                .body(Body::from(b"{}".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    clock.abort();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "home should succeed in mock mode when motor is InBand"
    );

    let bs = cortex::boot_state::current(&state, "shoulder_actuator_a");
    assert!(matches!(bs, BootState::Homed));

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "enable after home");
}

/// Home refuses to start when motor is OutOfBand.
#[tokio::test]
async fn home_when_out_of_band_is_forbidden() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::set_boot_state(
        &state,
        "shoulder_actuator_a",
        BootState::OutOfBand {
            mech_pos_rad: 1.5,
            min_rad: -1.0,
            max_rad: 1.0,
        },
    );
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/home")
                .header("content-type", "application/json")
                .body(Body::from(b"{}".to_vec()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_band");
}

/// Home target outside the configured band returns 409 `out_of_band`.
#[tokio::test]
async fn home_target_outside_band_is_forbidden() {
    let (state, _dir) = common::make_state();
    {
        let mut inv = state.inventory.write().expect("inventory");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").unwrap();
        a.common.travel_limits = Some(cortex::inventory::TravelLimits {
            min_rad: -1.0,
            max_rad: 1.0,
            updated_at: None,
        });
    }
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/home")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"target_rad": 5.0})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_band");
}

/// Layer 2: jog projecting > boot_max_step_rad delta is refused while not
/// Homed even when the projected position is in-band.
#[tokio::test]
async fn jog_step_too_large_when_not_homed_is_forbidden() {
    let (state, _dir) = common::make_state();
    {
        let mut inv = state.inventory.write().expect("inventory");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").unwrap();
        a.common.travel_limits = Some(cortex::inventory::TravelLimits {
            min_rad: -1.0,
            max_rad: 1.0,
            updated_at: None,
        });
    }
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::InBand);
    let app = cortex::build_app(state);

    // Velocity 0.5 rad/s ├ù 1 s = 0.5 rad delta ΓÇö well above the 0.087 rad ceiling.
    let body = serde_json::to_vec(&json!({"vel_rad_s": 0.5, "ttl_ms": 1000})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/jog")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "step_too_large");
}

/// Sweep-safe CAN I/O (fail-closed): when `state.latest[role]` is missing
/// or older than `safety.max_feedback_age_ms`, jog must refuse with
/// `409 stale_telemetry` rather than silently bypassing the band check.
///
/// This is the primary regression test for the "Sweep travel limits"
/// safety hole where bus contention froze `state.latest` and every
/// subsequent jog approved every projected position forever.
#[tokio::test]
async fn jog_refuses_on_stale_feedback() {
    let (state, _dir) = common::make_state();
    {
        let mut inv = state.inventory.write().expect("inventory");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").unwrap();
        a.common.travel_limits = Some(TravelLimits {
            min_rad: -1.0,
            max_rad: 1.0,
            updated_at: None,
        });
    }
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);

    // Seed a deliberately stale row: 500 ms old > the 100 ms default.
    {
        let mut latest = state.latest.write().expect("latest");
        let now_ms = chrono::Utc::now().timestamp_millis();
        latest.insert(
            "shoulder_actuator_a".into(),
            MotorFeedback {
                t_ms: now_ms - 500,
                role: "shoulder_actuator_a".into(),
                can_id: 0x08,
                mech_pos_rad: 0.0,
                mech_vel_rad_s: 0.0,
                torque_nm: 0.0,
                vbus_v: 48.0,
                temp_c: 30.0,
                fault_sta: 0,
                warn_sta: 0,
            },
        );
    }
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({"vel_rad_s": 0.1, "ttl_ms": 200})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/jog")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "stale_telemetry");
    assert!(
        err.detail
            .as_deref()
            .map(|d| d.contains("ms old"))
            .unwrap_or(false),
        "stale_telemetry detail should include the age, got {:?}",
        err.detail,
    );
}

/// Companion to `jog_refuses_on_stale_feedback`: when `state.latest`
/// has no row at all for the role, the same 409 fires.
#[tokio::test]
async fn jog_refuses_with_no_feedback() {
    let (state, _dir) = common::make_state();
    {
        let mut inv = state.inventory.write().expect("inventory");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").unwrap();
        a.common.travel_limits = Some(TravelLimits {
            min_rad: -1.0,
            max_rad: 1.0,
            updated_at: None,
        });
    }
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    // Note: deliberately do NOT call seed_feedback.
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({"vel_rad_s": 0.1, "ttl_ms": 200})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/jog")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "stale_telemetry");
}
/// `GET /api/motors/:role/motion` returns 204 when no motion is running.
/// The SPA's `api.motion.current` distinguishes 204 from 200 + JSON;
/// any change here breaks the actuator detail page on mount.
#[tokio::test]
async fn get_motion_returns_204_when_idle() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/motion")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

/// `POST /api/motors/:role/motion/sweep` without travel limits accepts
/// the request (the start preflight runs against vel=0 so the
/// path-violation check doesn't trip there) but the spawned controller
/// exits on its first tick. Pin both halves so we don't accidentally
/// flip to a "reject up front" model that would make the SPA's
/// "configure travel limits first" hint go unused.
#[tokio::test]
async fn motion_sweep_without_travel_limits_self_terminates() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);
    let mut status_rx = state.motion_status_tx.subscribe();
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({"speed_rad_s": 0.1})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/sweep")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    let run_id = payload["run_id"].as_str().unwrap().to_string();

    // The controller exits within one tick with TravelLimitViolation.
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(2);
    let mut saw_violation = false;
    while !saw_violation {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            panic!("sweep without limits never produced a stopped frame");
        }
        let frame = match tokio::time::timeout(remaining, status_rx.recv()).await {
            Ok(Ok(f)) => f,
            Ok(Err(e)) => panic!("status channel closed: {e}"),
            Err(_) => panic!("timed out waiting for stopped frame"),
        };
        if frame.run_id != run_id {
            continue;
        }
        if matches!(frame.state, cortex::motion::MotionState::Stopped) {
            assert_eq!(frame.reason.as_deref(), Some("travel_limit_violation"));
            saw_violation = true;
        }
    }
}

/// `POST /api/motors/:role/motion/sweep` happy path: starts a sweep and
/// returns `{ run_id, clamped_speed_rad_s }`. Issues an immediate
/// `motion/stop` to keep the test fixture quiet.
#[tokio::test]
async fn motion_sweep_starts_and_returns_run_id() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);
    {
        let mut inv = state.inventory.write().expect("inventory");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").unwrap();
        a.common.travel_limits = Some(TravelLimits {
            min_rad: -0.5,
            max_rad: 0.5,
            updated_at: None,
        });
    }
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({"speed_rad_s": 0.1})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/sweep")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    let run_id = payload["run_id"].as_str().expect("run_id string");
    assert!(!run_id.is_empty());
    let clamped = payload["clamped_speed_rad_s"]
        .as_f64()
        .expect("clamped_speed_rad_s number");
    assert!((clamped - 0.1).abs() < 1e-6);

    // Cleanup: stop returns `{ stopped: bool }`.
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    assert_eq!(payload["stopped"], json!(true));
}

/// `POST /api/motors/:role/motion/stop` is idempotent and returns
/// `{ stopped: false }` when nothing was running.
#[tokio::test]
async fn motion_stop_when_idle_returns_false() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    assert_eq!(payload["stopped"], json!(false));
}

/// `POST /api/motors/:role/motion/sweep` clamps a speed beyond
/// `MAX_PATTERN_VEL_RAD_S` (2.0) silently. The SPA mirrors this constant,
/// so a regression here surfaces as the slider top end no longer
/// matching the actual cap. Note this is intentionally HIGHER than the
/// jog cap (0.5) ΓÇö sweep self-reverses inside the travel band, so it's
/// safe at speeds the dead-man jog wouldn't be.
#[tokio::test]
async fn motion_sweep_clamps_excessive_speed() {
    let (state, _dir) = common::make_state();
    common::force_homed(&state);
    common::seed_feedback(&state);
    {
        let mut inv = state.inventory.write().expect("inventory");
        let a = common::actuator_mut(&mut inv, "shoulder_actuator_a").unwrap();
        a.common.travel_limits = Some(TravelLimits {
            min_rad: -0.5,
            max_rad: 0.5,
            updated_at: None,
        });
    }
    let app = cortex::build_app(state);

    // Request 10 rad/s ΓÇö well above the 2.0 sweep cap and the 3.0
    // firmware envelope. The handler must clamp to MAX_PATTERN_VEL_RAD_S.
    let body = serde_json::to_vec(&json!({"speed_rad_s": 10.0})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/sweep")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let payload: serde_json::Value = body_json(resp).await;
    let clamped = payload["clamped_speed_rad_s"].as_f64().unwrap();
    assert!(
        clamped <= 2.0 + 1e-6,
        "expected clamped speed <= 2.0, got {clamped}"
    );
    // And clamping must actually have taken effect (i.e. the test isn't
    // just passing because the request was below the cap).
    assert!(
        clamped >= 2.0 - 1e-6,
        "expected clamp to saturate at 2.0, got {clamped}"
    );

    let _ = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/motion/stop")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
}

/// When any **sibling** on the same `limb` is `HomeFailed` / `OffsetChanged` /
/// `OutOfBand`, motion on a healthy peer returns `409 limb_quarantined`.
#[tokio::test]
async fn limb_quarantine_blocks_sibling_jog() {
    let (state, _dir) = common::make_state();
    {
        let mut inv = state.inventory.write().expect("inventory");
        for d in &mut inv.devices {
            if let Device::Actuator(a) = d {
                a.common.limb = Some("test_limb".into());
                if a.common.role == "shoulder_actuator_b" {
                    a.common.verified = true;
                }
            }
        }
    }
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    common::set_boot_state(
        &state,
        "shoulder_actuator_b",
        BootState::HomeFailed {
            reason: "fixture".into(),
            last_pos_rad: 0.0,
        },
    );
    let app = cortex::build_app(state);
    let body = serde_json::to_vec(&json!({"vel_rad_s": 0.01, "ttl_ms": 200})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_a/jog")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "limb_quarantined");
    assert_eq!(err.limb.as_deref(), Some("test_limb"));
    let failed = err.failed_motors.expect("failed_motors");
    assert!(failed.iter().any(|m| m.role == "shoulder_actuator_b"));
}

/// Limb quarantine must not block intentional recovery paths that skip the
/// motion gate ΓÇö e.g. RAM-only `set_zero` on the failed sibling.
#[tokio::test]
async fn limb_quarantine_allows_recovery_set_zero() {
    let (state, _dir) = common::make_state();
    {
        let mut inv = state.inventory.write().expect("inventory");
        for d in &mut inv.devices {
            if let Device::Actuator(a) = d {
                a.common.limb = Some("test_limb".into());
                if a.common.role == "shoulder_actuator_b" {
                    a.common.verified = true;
                }
            }
        }
    }
    common::seed_feedback(&state);
    common::set_boot_state(&state, "shoulder_actuator_a", BootState::Homed);
    common::set_boot_state(
        &state,
        "shoulder_actuator_b",
        BootState::HomeFailed {
            reason: "fixture".into(),
            last_pos_rad: 0.0,
        },
    );
    let app = cortex::build_app(state);
    let body = serde_json::to_vec(&json!({"confirm_advanced": true})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_b/set_zero")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}
