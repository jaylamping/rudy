//! `/api/config` + `/api/system` contract tests.
//!
#![allow(unused_imports)] // shared prelude matches other `tests/api/*.rs` suites

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
async fn get_config_returns_server_config_shape() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

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
    assert_eq!(cfg.actuator_models, vec!["RS03".to_string()]);

    // Disabled-WT advert: enabled=false AND url=None ΓÇö the SPA's
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

    assert!(!cfg.paths.inventory.is_empty());
    assert!(
        cfg.paths.inventory.ends_with("inventory.yaml"),
        "expected test fixture path to end with inventory.yaml, got {:?}",
        cfg.paths.inventory
    );
}

#[tokio::test]
async fn get_config_advertises_resolvable_wt_url_when_enabled() {
    let (state, _dir) = common::make_state_with_wt_advert();
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .header("host", "rudy-pi.tail0b414.ts.net")
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
    assert_eq!(
        url, "https://rudy-pi.tail0b414.ts.net:4433/wt",
        "WT URL should reuse the inbound Host hostname (sans :port) and the WT bind port",
    );
}

/// `Host` headers from the browser often include a `:port` (e.g. dev servers
/// bound to `:5173`). We must strip it before reattaching the WT port,
/// otherwise we'd advertise `https://localhost:5173:4433/wt` which the
/// browser refuses to parse.
#[tokio::test]
async fn get_config_strips_port_from_host_header() {
    let (state, _dir) = common::make_state_with_wt_advert();
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .header("host", "localhost:5173")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let cfg: ServerConfig = body_json(resp).await;
    let url = cfg.webtransport.url.expect("WT URL when enabled");
    assert_eq!(url, "https://localhost:4433/wt");
}

/// If no `Host` header is present we cannot synthesise a URL the browser can
/// resolve, so we omit it. The frontend treats `enabled=true, url=None` the
/// same as disabled (no WT session opens) rather than crashing on a bad URL.
#[tokio::test]
async fn get_config_omits_wt_url_when_host_header_missing() {
    let (state, _dir) = common::make_state_with_wt_advert();
    let app = cortex::build_app(state);

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
    assert!(cfg.webtransport.url.is_none());
}

/// `GET /api/system` returns a `SystemSnapshot`. With `cfg.can.mock = true`
/// (the test fixture's default) the snapshot is mocked: `is_mock=true`, all
/// numeric fields populated. Pins the wire shape the dashboard's
/// `SystemHealthCard` consumes.
#[tokio::test]
async fn get_system_returns_system_snapshot_shape() {
    let (state, _dir) = common::make_state();
    let app = cortex::build_app(state);

    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/system")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let snap: SystemSnapshot = body_json(resp).await;
    assert!(snap.is_mock, "test fixture has cfg.can.mock=true");
    assert!(snap.cpu_pct >= 0.0 && snap.cpu_pct <= 100.0);
    assert!(snap.mem_total_mb > 0);
    assert!(snap.t_ms > 0);
    assert_eq!(snap.load.len(), 3);
}
