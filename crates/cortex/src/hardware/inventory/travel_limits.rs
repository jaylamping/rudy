//! Per-actuator soft travel-limits band (radians).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Per-actuator soft travel-limits band (radians).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct TravelLimits {
    pub min_rad: f32,
    pub max_rad: f32,
    #[serde(default)]
    pub updated_at: Option<String>,
}
