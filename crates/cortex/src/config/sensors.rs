//! Physical sensor configuration.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SensorsConfig {
    #[serde(default)]
    pub imu: ImuSensorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImuSensorConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default = "default_imu_id")]
    pub id: String,
    #[serde(default = "default_imu_frame_id")]
    pub frame_id: String,
    #[serde(default = "default_i2c_bus")]
    pub bus: u8,
    #[serde(default = "default_i2c_address")]
    pub address: u16,
    #[serde(default = "default_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_stale_after_ms")]
    pub stale_after_ms: u64,
}

impl Default for ImuSensorConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            required: false,
            id: default_imu_id(),
            frame_id: default_imu_frame_id(),
            bus: default_i2c_bus(),
            address: default_i2c_address(),
            poll_interval_ms: default_poll_interval_ms(),
            stale_after_ms: default_stale_after_ms(),
        }
    }
}

fn default_imu_id() -> String {
    "base_imu".to_string()
}

fn default_imu_frame_id() -> String {
    "imu_link".to_string()
}

fn default_i2c_bus() -> u8 {
    1
}

fn default_i2c_address() -> u16 {
    0x4b
}

fn default_poll_interval_ms() -> u64 {
    20
}

fn default_stale_after_ms() -> u64 {
    500
}
