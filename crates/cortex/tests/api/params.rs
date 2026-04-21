//! Params, travel limits, predefined home, homing speed.
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
async fn get_params_returns_param_snapshot_shape() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/params")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let snap: ParamSnapshot = body_json(resp).await;
    assert_eq!(snap.role, "shoulder_actuator_a");

    let lt = snap
        .values
        .get("limit_torque")
        .expect("limit_torque must be in the snapshot from spec");
    assert_eq!(lt.index, 0x700B);
    assert_eq!(lt.ty, "float");
    assert_eq!(lt.hardware_range, Some([0.0, 60.0]));
    assert_eq!(lt.units.as_deref(), Some("nm"));
}

#[tokio::test]
async fn put_param_in_range_returns_write_envelope() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({ "value": 12.5, "save_after": false })).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/params/limit_torque")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Frontend literally destructures these fields in api.writeParam().
    #[derive(serde::Deserialize)]
    struct WriteResp {
        ok: bool,
        saved: bool,
        role: String,
        name: String,
        value: serde_json::Value,
    }
    let r: WriteResp = body_json(resp).await;
    assert!(r.ok);
    assert!(r.saved);
    assert_eq!(r.role, "shoulder_actuator_a");
    assert_eq!(r.name, "limit_torque");
    assert_eq!(r.value, json!(12.5));
}

#[tokio::test]
async fn put_param_out_of_range_returns_api_error() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = cortex::build_app(state);

    // hardware_range is [0.0, 60.0]; 9999 is well outside.
    let body = serde_json::to_vec(&json!({ "value": 9999.0, "save_after": false })).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/params/limit_torque")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_range");
}

#[tokio::test]
async fn put_param_unknown_param_404s() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({ "value": 1.0 })).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/params/no_such_param")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "not_writable");
}

/// `GET /api/motors/:role/travel_limits` returns 404 + `no_travel_limits`
/// when no band has been configured yet. This is the SPA's signal to fall
/// back to "use spec defaults" instead of erroring loudly.
#[tokio::test]
async fn get_travel_limits_404s_when_unset() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "no_travel_limits");
}

/// PUT /api/motors/:role/travel_limits with a valid band roundtrips to the
/// matching GET. The lock-gate is satisfied implicitly: the request omits
/// `X-Rudy-Session` and `ensure_control("")` permits anonymous mutators
/// when the lock is free.
#[tokio::test]
async fn put_travel_limits_roundtrips_through_get() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({"min_rad": -1.0, "max_rad": 1.0})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let limits: TravelLimits = body_json(resp).await;
    assert!((limits.min_rad - -1.0).abs() < 1e-6);
    assert!((limits.max_rad - 1.0).abs() < 1e-6);
    assert!(limits.updated_at.is_some());

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let limits: TravelLimits = body_json(resp).await;
    assert!((limits.max_rad - 1.0).abs() < 1e-6);
}

/// Bands wider than the hardware envelope are rejected before any disk
/// write happens.
#[tokio::test]
async fn put_travel_limits_rejects_out_of_range() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({"min_rad": -1000.0, "max_rad": 1000.0})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_range");
}

/// Inverted bands are rejected too (and audited as denied).
#[tokio::test]
async fn put_travel_limits_rejects_inverted_band() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({"min_rad": 0.5, "max_rad": -0.5})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_range");
}

/// PUT predefined_home without travel_limits returns 409 `no_travel_limits`.
#[tokio::test]
async fn put_predefined_home_requires_travel_limits() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let body = serde_json::to_vec(&json!({"predefined_home_rad": 0.0})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/predefined_home")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "no_travel_limits");
}

/// PUT predefined_home within the saved band persists to inventory.yaml.
#[tokio::test]
async fn put_predefined_home_persists_when_within_band() {
    let (state, dir) = common::make_state();
    let app = cortex::build_app(state.clone());

    let tlim = serde_json::to_vec(&json!({"min_rad": -1.0, "max_rad": 1.0})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .header("content-type", "application/json")
                .body(Body::from(tlim))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = serde_json::to_vec(&json!({"predefined_home_rad": 0.25})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/predefined_home")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let j: serde_json::Value = body_json(resp).await;
    assert_eq!(j["ok"], json!(true));
    assert_eq!(j["predefined_home_rad"], json!(0.25));

    let inv = cortex::inventory::Inventory::load(dir.path().join("inventory.yaml")).unwrap();
    let m = inv.actuator_by_role("shoulder_actuator_a").unwrap();
    assert_eq!(m.common.predefined_home_rad, Some(0.25_f32));

    let in_mem = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role("shoulder_actuator_a")
        .cloned()
        .unwrap();
    assert_eq!(in_mem.common.predefined_home_rad, Some(0.25_f32));
}

/// Values outside the soft travel band are rejected.
#[tokio::test]
async fn put_predefined_home_rejects_outside_band() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state.clone());

    let tlim = serde_json::to_vec(&json!({"min_rad": -1.0, "max_rad": 1.0})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/travel_limits")
                .header("content-type", "application/json")
                .body(Body::from(tlim))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = serde_json::to_vec(&json!({"predefined_home_rad": 1.5})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/predefined_home")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "outside_travel_band");
}

/// PUT homing_speed persists `homing_speed_rad_s` to inventory.yaml.
#[tokio::test]
async fn put_homing_speed_persists() {
    let (state, dir) = common::make_state();
    let app = cortex::build_app(state.clone());

    let v = 0.3_f32;
    let body = serde_json::to_vec(&json!({"homing_speed_rad_s": v})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/homing_speed")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let j: serde_json::Value = body_json(resp).await;
    assert_eq!(j["ok"], json!(true));
    assert!((j["homing_speed_rad_s"].as_f64().unwrap() - f64::from(v)).abs() < 1e-5);

    let inv = cortex::inventory::Inventory::load(dir.path().join("inventory.yaml")).unwrap();
    let m = inv.actuator_by_role("shoulder_actuator_a").unwrap();
    assert_eq!(m.common.homing_speed_rad_s, Some(v));

    let in_mem = state
        .inventory
        .read()
        .expect("inventory poisoned")
        .actuator_by_role("shoulder_actuator_a")
        .cloned()
        .unwrap();
    assert_eq!(in_mem.common.homing_speed_rad_s, Some(v));
}

/// Values outside [~1 deg/s, 100 deg/s ~= 1.745 rad/s] are rejected.
#[tokio::test]
async fn put_homing_speed_rejects_out_of_range() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let too_low = serde_json::to_vec(&json!({"homing_speed_rad_s": 0.001_f32})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/homing_speed")
                .header("content-type", "application/json")
                .body(Body::from(too_low))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_range");

    let too_high = serde_json::to_vec(&json!({"homing_speed_rad_s": 2.0_f32})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/homing_speed")
                .header("content-type", "application/json")
                .body(Body::from(too_high))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "out_of_range");
}

/// `null` clears a per-actuator homing speed override.
#[tokio::test]
async fn put_homing_speed_null_clears_override() {
    let (state, dir) = common::make_state();
    let app = cortex::build_app(state.clone());

    let body = serde_json::to_vec(&json!({"homing_speed_rad_s": 0.3_f32})).unwrap();
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/homing_speed")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let clear = serde_json::to_vec(&json!({"homing_speed_rad_s": null})).unwrap();
    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::PUT)
                .uri("/api/motors/shoulder_actuator_a/homing_speed")
                .header("content-type", "application/json")
                .body(Body::from(clear))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let inv = cortex::inventory::Inventory::load(dir.path().join("inventory.yaml")).unwrap();
    let m = inv.actuator_by_role("shoulder_actuator_a").unwrap();
    assert!(m.common.homing_speed_rad_s.is_none());
}
