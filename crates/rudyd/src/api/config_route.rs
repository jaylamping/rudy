//! GET /api/config.

use axum::{extract::State, Json};

use crate::state::SharedState;
use crate::types::{ServerConfig, ServerFeatures, WebTransportAdvert};

pub async fn get_config(State(state): State<SharedState>) -> Json<ServerConfig> {
    let wt_url = if state.cfg.webtransport.enabled {
        // Best-effort: assume https://<host>:<port>/wt. The operator's
        // browser resolves the host via the same hostname they typed for the
        // SPA; the port comes from config.
        let port = state
            .cfg
            .webtransport
            .bind
            .rsplit_once(':')
            .map(|(_, p)| p.to_string())
            .unwrap_or_else(|| "4433".to_string());
        Some(format!("https://HOSTPLACEHOLDER:{port}/wt"))
    } else {
        None
    };

    Json(ServerConfig {
        version: env!("CARGO_PKG_VERSION").to_string(),
        actuator_model: state.spec.actuator_model.clone(),
        webtransport: WebTransportAdvert {
            enabled: state.cfg.webtransport.enabled,
            url: wt_url,
        },
        features: ServerFeatures {
            mock_can: state.cfg.can.mock,
            require_verified: state.cfg.safety.require_verified,
        },
    })
}
