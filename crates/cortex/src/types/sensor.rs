//! Physical sensor wire types.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
#[serde(rename_all = "snake_case")]
pub enum SensorHealth {
    Ok,
    Stale,
    Unavailable,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ImuSample {
    /// Unit quaternion from the BNO085 rotation-vector report: [x, y, z, w].
    pub quaternion_xyzw: [f32; 4],
    pub accel_m_s2: [f32; 3],
    pub gyro_rad_s: [f32; 3],
    /// Rotation-vector accuracy status: 0 unreliable, 1 low, 2 medium, 3 high.
    pub rotation_accuracy: u8,
    pub rotation_accuracy_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SensorSample {
    /// Wallclock at sample time, ms since unix epoch.
    pub t_ms: i64,
    pub sensor_id: String,
    pub frame_id: String,
    pub kind: String,
    pub health: SensorHealth,
    pub stale_after_ms: u64,
    pub message: Option<String>,
    pub imu: Option<ImuSample>,
}
