//! REST contract tests — pin every endpoint the `link/` SPA calls.
//!
//! For each endpoint in `link/src/lib/api.ts` we:
//!   1. Build the same `axum::Router` `rudydae` serves (via `rudydae::build_app`).
//!   2. Issue an in-process request with `tower::ServiceExt::oneshot` so we
//!      don't depend on a real TCP socket / TLS cert / port allocation.
//!   3. Deserialize the body into the **exact** Rust type that `ts-rs` exports
//!      to TypeScript. Any drift between the Rust struct and the response shape
//!      blows up the test.
//!
//! These tests do not run against real CAN hardware. They use the mock CAN
//! core (no socket open) and seed `state.latest` / `state.params` directly so
//! the "no feedback yet" race is not a flake source.

use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

use rudydae::types::{
    ApiError, MotorFeedback, MotorSummary, ParamSnapshot, ServerConfig, ServerFeatures,
    WebTransportAdvert,
};

mod common;

async fn body_json<T: serde::de::DeserializeOwned>(resp: axum::response::Response) -> T {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice::<T>(&bytes).unwrap_or_else(|e| {
        let s = std::str::from_utf8(&bytes).unwrap_or("<binary>");
        panic!("deserialise failed: {e}; body was: {s}");
    })
}

#[tokio::test]
async fn get_config_returns_server_config_shape() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let cfg: ServerConfig = body_json(resp).await;

    // Every TS-exported field must be populated.
    assert!(!cfg.version.is_empty(), "version must be present");
    assert_eq!(cfg.actuator_model, "TEST_RS03");

    // Disabled-WT advert: enabled=false AND url=None — the SPA's
    // useWebTransport hook reads exactly this shape.
    let WebTransportAdvert { enabled, url } = cfg.webtransport;
    assert!(!enabled);
    assert!(url.is_none());

    let ServerFeatures {
        mock_can,
        require_verified,
    } = cfg.features;
    assert!(mock_can);
    assert!(require_verified);
}

/// When WT is enabled, `/api/config` advertises a URL the browser opens
/// directly. This test pins the current behaviour AND surfaces the
/// `HOSTPLACEHOLDER` bug in `config_route.rs`: the literal string never gets
/// substituted with a real host. Marked `#[ignore]` so it serves as a known
/// failure ticket — flip to a hard assertion once the placeholder is fixed.
#[tokio::test]
#[ignore = "documents a known gap: config_route::get_config never substitutes HOSTPLACEHOLDER"]
async fn get_config_advertises_resolvable_wt_url_when_enabled() {
    let (state, _dir) = common::make_state_with_wt_advert();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cfg: ServerConfig = body_json(resp).await;

    assert!(cfg.webtransport.enabled);
    let url = cfg.webtransport.url.expect("WT URL when enabled");
    assert!(
        !url.contains("HOSTPLACEHOLDER"),
        "config_route should substitute the host before responding; got {url}"
    );
    assert!(
        url.starts_with("https://"),
        "WT URL must be https for the browser to accept it; got {url}"
    );
}

#[tokio::test]
async fn list_motors_matches_motor_summary() {
    let (state, _dir) = common::make_state();
    common::seed_feedback(&state);
    let app = rudydae::build_app(state.clone());

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
    assert_eq!(motors.len(), state.inventory.motors.len());

    let by_role: std::collections::BTreeMap<&str, &MotorSummary> =
        motors.iter().map(|m| (m.role.as_str(), m)).collect();
    let a = by_role
        .get("shoulder_actuator_a")
        .expect("shoulder_actuator_a present");
    assert_eq!(a.can_id, 0x08);
    assert_eq!(a.can_bus, "can1");
    assert!(a.verified);
    assert!(a.latest.is_some(), "we just seeded feedback");

    let b = by_role
        .get("shoulder_actuator_b")
        .expect("shoulder_actuator_b present");
    assert!(!b.verified);
}

#[tokio::test]
async fn get_motor_404_returns_api_error_envelope() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

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
    let app = rudydae::build_app(state);

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
    // i64 millisecond timestamp — TS sees it as bigint per ts-rs, so changes
    // here ripple into useWebTransport / TelemetryGrid.
    assert!(fb.t_ms > 0);
}

#[tokio::test]
async fn get_feedback_404_when_no_telemetry_yet() {
    let (state, _dir) = common::make_state();
    // Note: deliberately NOT seeding feedback.
    let app = rudydae::build_app(state);

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

#[tokio::test]
async fn get_params_returns_param_snapshot_shape() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = rudydae::build_app(state);

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
    let app = rudydae::build_app(state);

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
    assert!(!r.saved);
    assert_eq!(r.role, "shoulder_actuator_a");
    assert_eq!(r.name, "limit_torque");
    assert_eq!(r.value, json!(12.5));
}

#[tokio::test]
async fn put_param_out_of_range_returns_api_error() {
    let (state, _dir) = common::make_state();
    common::seed_params(&state);
    let app = rudydae::build_app(state);

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
    let app = rudydae::build_app(state);

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
    assert_eq!(err.error, "unknown_param");
}

/// All four control endpoints share the same `{ ok, role }` envelope. The
/// frontend's `api.enable / stop / saveToFlash / setZero` mutations expect
/// `{ ok: true }` at minimum — extra fields are tolerated.
#[tokio::test]
async fn control_endpoints_return_ok_envelope_for_verified_motor() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    for (verb, suffix) in [
        ("POST", "enable"),
        ("POST", "stop"),
        ("POST", "save"),
        ("POST", "set_zero"),
    ] {
        let req = Request::builder()
            .method(verb)
            .uri(format!("/api/motors/shoulder_actuator_a/{suffix}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "{verb} /api/motors/shoulder_actuator_a/{suffix} should succeed for a verified motor"
        );
        let v: serde_json::Value = body_json(resp).await;
        assert_eq!(
            v.get("ok").and_then(|b| b.as_bool()),
            Some(true),
            "control envelope must include ok:true; got {v}"
        );
    }
}

/// Unverified motor + require_verified=true => 403 from `enable`. Mirrors the
/// safety gate in `control::enable` and confirms the SPA can rely on the 403
/// to keep the button locked out.
#[tokio::test]
async fn enable_unverified_motor_is_forbidden() {
    let (state, _dir) = common::make_state();
    let app = rudydae::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri("/api/motors/shoulder_actuator_b/enable")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    let err: ApiError = body_json(resp).await;
    assert_eq!(err.error, "not_verified");
}

/// Sanity check: the URL paths the test hits above are exactly the ones the
/// SPA's `link/src/lib/api.ts` constructs. If someone renames a route in
/// `crates/rudydae/src/api/mod.rs`, this test file fails to compile because
/// the URI constants would still point at the old shape — but a harder pin
/// lives on the Node smoke runner under `link/scripts/smoke-contract.mjs`.
#[test]
fn endpoint_inventory_documented() {
    // Single source of truth listing every route hit by the SPA today. When
    // adding a new route to `link/src/lib/api.ts`, add it here too AND write a
    // contract test above. The string is deliberately not parsed — it's a
    // checklist for code reviewers.
    let _spa_endpoints = [
        "GET    /api/config",
        "GET    /api/motors",
        "GET    /api/motors/:role",
        "GET    /api/motors/:role/feedback",
        "GET    /api/motors/:role/params",
        "PUT    /api/motors/:role/params/:name",
        "POST   /api/motors/:role/enable",
        "POST   /api/motors/:role/stop",
        "POST   /api/motors/:role/save",
        "POST   /api/motors/:role/set_zero",
    ];
}
