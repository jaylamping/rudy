//! Config bootstrap types (`GET /api/config`).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// GET /api/config — what the UI needs to bootstrap.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ServerConfig {
    pub version: String,
    /// RobStride models with a loaded `robstride_*.yaml` (e.g. `RS03`), sorted for stable JSON.
    pub actuator_models: Vec<String>,
    pub webtransport: WebTransportAdvert,
    pub features: ServerFeatures,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct WebTransportAdvert {
    pub enabled: bool,
    /// Fully-qualified URL the browser should open. Example:
    /// `https://rudy.your-tailnet.ts.net:4433/wt`.
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ServerFeatures {
    pub mock_can: bool,
    pub require_verified: bool,
}
