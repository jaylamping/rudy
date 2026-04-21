//! Inventory, devices, hardware, motor list/feedback.
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

#[tokio::test]
async fn get_devices_returns_v2_inventory_devices() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/devices")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = body_json(resp).await;
    assert!(v.is_array());
    assert_eq!(v.as_array().unwrap().len(), 2);
    assert_eq!(v[0]["kind"], json!("actuator"));
}

#[tokio::test]
async fn get_hardware_unassigned_returns_empty_without_passive_seen() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/hardware/unassigned")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = body_json(resp).await;
    assert_eq!(v, json!([]));
}

#[tokio::test]
async fn delete_device_removes_actuator_and_clears_runtime_state() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    common::seed_params(&state);
    common::set_boot_state(
        &state,
        "shoulder_actuator_b",
        cortex::boot_state::BootState::Homed,
    );
    state.record_passive_seen("can1", 0x09);
    state
        .boot_orchestrator_attempted
        .lock()
        .expect("boot_orchestrator_attempted poisoned")
        .insert("shoulder_actuator_b".into());

    let mut safety_rx = state.safety_event_tx.subscribe();
    let app = cortex::build_app(state.clone());
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/devices/shoulder_actuator_b")
                .header("x-rudy-session", "session-A")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = body_json(resp).await;
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["role"], json!("shoulder_actuator_b"));

    // Scoped block: std locks must not be held across `await` (clippy::await_holding_lock).
    {
        let inv = state.inventory.read().expect("inventory poisoned");
        assert!(inv.actuator_by_role("shoulder_actuator_b").is_none());

        assert!(!state
            .latest
            .read()
            .expect("latest poisoned")
            .contains_key("shoulder_actuator_b"));
        assert!(!state
            .params
            .read()
            .expect("params poisoned")
            .contains_key("shoulder_actuator_b"));
        assert!(!state
            .boot_state
            .read()
            .expect("boot_state poisoned")
            .contains_key("shoulder_actuator_b"));
        assert!(!state
            .seen_can_ids
            .read()
            .expect("seen_can_ids poisoned")
            .contains_key(&(String::from("can1"), 0x09)));
        assert!(!state
            .boot_orchestrator_attempted
            .lock()
            .expect("boot_orchestrator_attempted poisoned")
            .contains("shoulder_actuator_b"));
    }

    let disk_inv = Inventory::load(&state.cfg.paths.inventory).expect("inventory from disk");
    assert!(disk_inv.actuator_by_role("shoulder_actuator_b").is_none());
    assert!(disk_inv.actuator_by_role("shoulder_actuator_a").is_some());

    let mut got_removed = false;
    for _ in 0..4 {
        let ev = tokio::time::timeout(std::time::Duration::from_millis(200), safety_rx.recv())
            .await
            .expect("safety_event timeout")
            .expect("safety_event receive");
        if let SafetyEvent::MotorRemoved { role, .. } = ev {
            assert_eq!(role, "shoulder_actuator_b");
            got_removed = true;
            break;
        }
    }
    assert!(got_removed, "expected SafetyEvent::MotorRemoved");
}

#[tokio::test]
async fn delete_device_refuses_enabled_motor() {
    let (state, _dir) = common::make_state();
    state.mark_enabled("shoulder_actuator_b");
    let app = cortex::build_app(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::DELETE)
                .uri("/api/devices/shoulder_actuator_b")
                .header("x-rudy-session", "session-A")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "motor_active");

    let inv = state.inventory.read().expect("inventory poisoned");
    assert!(inv.actuator_by_role("shoulder_actuator_b").is_some());
}

#[tokio::test]
async fn get_hardware_unassigned_lists_passive_seen_not_in_inventory() {
    let (state, _dir) = common::make_state();
    state.record_passive_seen("can1", 0x10);
    // Inventoried ID ΓÇö must not appear as unassigned.
    state.record_passive_seen("can1", 0x08);

    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/hardware/unassigned")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = body_json(resp).await;
    assert!(v.is_array());
    let arr = v.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["bus"], json!("can1"));
    assert_eq!(arr[0]["can_id"], json!(16));
    assert_eq!(arr[0]["source"], json!("passive"));
}

#[tokio::test]
async fn post_hardware_onboard_robstride_appends_actuator() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state.clone());

    let body = json!({
        "can_bus": "can1",
        "can_id": 30,
        "model": "rs03",
        "limb": "test_bench",
        "joint_kind": "shoulder_pitch",
        "travel_min_rad": -0.5_f32,
        "travel_max_rad": 0.5_f32,
        "predefined_home_rad": 0.0_f32,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/hardware/onboard/robstride")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = body_json(resp).await;
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["role"], json!("test_bench.shoulder_pitch"));

    let inv = state.inventory.read().expect("inventory poisoned");
    let m = inv
        .actuator_by_role("test_bench.shoulder_pitch")
        .expect("new actuator");
    assert_eq!(m.common.can_bus, "can1");
    assert_eq!(m.common.can_id, 30);
    assert_eq!(m.common.predefined_home_rad, Some(0.0));
}

#[tokio::test]
async fn post_hardware_onboard_rejects_duplicate_can_id() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let body = json!({
        "can_bus": "can1",
        "can_id": 8,
        "model": "rs03",
        "limb": "other",
        "joint_kind": "elbow_pitch",
        "travel_min_rad": -0.5_f32,
        "travel_max_rad": 0.5_f32,
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/hardware/onboard/robstride")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn post_hardware_scan_noops_on_mock_can_with_message() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/hardware/scan")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = body_json(resp).await;
    assert_eq!(v["ok"], json!(true));
    assert_eq!(v["discovered"], json!([]));
    let msg = v["message"].as_str().unwrap();
    assert!(
        msg.contains("mock") || msg.contains("non-Linux") || msg.contains("did not touch"),
        "unexpected scan message: {msg}"
    );
}

/// When WT is enabled, `/api/config` advertises a URL the browser opens
/// directly. The host is taken from the inbound `Host` header (which is what
/// the browser already resolved to reach the SPA ΓÇö on the Pi this is the
/// tailnet hostname forwarded through `tailscale serve`); the port comes
/// from `[webtransport].bind`.
#[tokio::test]
async fn list_motors_matches_motor_summary() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    let app = cortex::build_app(state.clone());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let motors: Vec<MotorSummary> = body_json(resp).await;
    assert_eq!(
        motors.len(),
        state
            .inventory
            .read()
            .expect("inventory poisoned")
            .actuators()
            .count()
    );

    let by_role: std::collections::BTreeMap<&str, &MotorSummary> =
        motors.iter().map(|m| (m.role.as_str(), m)).collect();
    let a = by_role
        .get("shoulder_actuator_a")
        .expect("shoulder_actuator_a present");
    assert_eq!(a.can_id, 0x08);
    assert_eq!(a.can_bus, "can1");
    assert!(a.verified);
    assert!(a.predefined_home_rad.is_none());
    assert!(a.latest.is_some(), "we just seeded feedback");

    let b = by_role
        .get("shoulder_actuator_b")
        .expect("shoulder_actuator_b present");
    assert!(!b.verified);
}

#[tokio::test]
async fn get_motor_404_returns_api_error_envelope() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/nonexistent")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "unknown_motor");
    assert!(err.detail.is_some(), "404s should explain themselves");
}

#[tokio::test]
async fn get_feedback_returns_motor_feedback_shape() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/feedback")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let fb: MotorFeedback = body_json(resp).await;
    assert_eq!(fb.role, "shoulder_actuator_a");
    assert_eq!(fb.can_id, 0x08);
    // i64 millisecond timestamp ΓÇö TS sees it as bigint per ts-rs, so changes
    // here ripple into useWebTransport / TelemetryGrid.
    assert!(fb.t_ms > 0);
}

#[tokio::test]
async fn get_feedback_404_when_no_telemetry_yet() {
    let (state, _dir) = common::make_state();
    // Note: deliberately NOT seeding feedback.
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/feedback")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "no_feedback");
}

/// `GET /api/motors/:role/inventory` returns the typed scalars + free-form
/// `extra` map; the SPA renders it directly in the Inventory tab.
#[tokio::test]
async fn get_inventory_returns_motor_record() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/inventory")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let v: serde_json::Value = body_json(resp).await;
    assert_eq!(
        v.get("role").and_then(|v| v.as_str()),
        Some("shoulder_actuator_a")
    );
    assert_eq!(v.get("can_id").and_then(|v| v.as_u64()), Some(0x08));
}

/// `PUT /api/motors/:role/verified` flips the flag and the next GET
/// reflects it (cache hot-swap is exercised end-to-end here).
#[tokio::test]
async fn put_verified_flips_flag() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({"verified": false, "note": "test"})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/verified")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let m: MotorSummary = body_json(resp).await;
    assert!(!m.verified);
}
