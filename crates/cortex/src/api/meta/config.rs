//! GET /api/config.

use axum::{
    extract::State,
    http::{header, HeaderMap},
    Json,
};

use crate::inventory::RobstrideModel;
use crate::state::SharedState;
use crate::types::{ServerConfig, ServerFeatures, ServerPaths, WebTransportAdvert};

pub async fn get_config(
    State(state): State<SharedState>,
    headers: HeaderMap,
) -> Json<ServerConfig> {
    let wt_url = if state.cfg.webtransport.enabled {
        // The browser already resolved this exact `Host` to reach the SPA, so
        // reusing its hostname (stripping any `:port` suffix) is the most
        // reliable way to hand it back a URL it can also resolve. On the Pi
        // `tailscale serve` forwards the original `Host` header (e.g.
        // `rudy-pi.tail0b414.ts.net`) into cortex, so we get the tailnet
        // name for free; in `npm run dev` we get `localhost:5173`, etc.
        //
        // The WT port comes from config because `tailscale serve` cannot
        // proxy HTTP/3 — WebTransport binds the tailnet IP directly on
        // `:4433`, separate from the `:443` SPA listener.
        let host = headers
            .get(header::HOST)
            .and_then(|v| v.to_str().ok())
            .map(|h| h.split(':').next().unwrap_or(h).to_string());
        let port = state
            .cfg
            .webtransport
            .bind
            .rsplit_once(':')
            .map(|(_, p)| p.to_string())
            .unwrap_or_else(|| "4433".to_string());
        host.map(|h| format!("https://{h}:{port}/wt"))
    } else {
        None
    };

    let mut actuator_models: Vec<String> = state
        .specs
        .keys()
        .copied()
        .map(RobstrideModel::as_spec_label)
        .map(str::to_string)
        .collect();
    actuator_models.sort();

    let inventory = state.cfg.paths.inventory.to_string_lossy().into_owned();

    Json(ServerConfig {
        version: env!("CARGO_PKG_VERSION").to_string(),
        actuator_models,
        webtransport: WebTransportAdvert {
            enabled: state.cfg.webtransport.enabled,
            url: wt_url,
        },
        features: ServerFeatures {
            mock_can: state.cfg.can.mock,
            require_verified: state.cfg.safety.require_verified,
        },
        paths: ServerPaths { inventory },
    })
}
