//! Inventory device rows: actuators, sensors, batteries.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::limb::JointKind;

use super::travel_limits::TravelLimits;

fn default_true() -> bool {
    true
}

// --- Polymorphic device ------------------------------------------------------

/// Inventory row: actuator, sensor, battery, or peripheral. JSON/YAML uses `kind` as the tag.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum Device {
    Actuator(Actuator),
    Sensor(Sensor),
    Battery(Battery),
    Peripheral(Peripheral),
}

/// RobStride actuator with shared [`ActuatorCommon`] plus a [`ActuatorFamily`] discriminator.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct Actuator {
    /// Flattened into the same YAML mapping as `family` (sibling keys under `kind: actuator`).
    #[serde(flatten)]
    pub common: ActuatorCommon,
    pub family: ActuatorFamily,
}

/// Fields shared by all actuators regardless of vendor family.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ActuatorCommon {
    pub role: String,
    pub can_bus: String,
    #[serde(with = "super::role::serde_u8_flex")]
    #[ts(as = "u8")]
    pub can_id: u8,
    #[serde(default = "default_true")]
    pub present: bool,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub commissioned_at: Option<String>,
    #[serde(default)]
    pub firmware_version: Option<String>,
    #[serde(default)]
    pub travel_limits: Option<TravelLimits>,
    #[serde(default)]
    pub commissioned_zero_offset: Option<f32>,
    #[serde(default)]
    pub active_report_persisted: bool,
    #[serde(default)]
    pub predefined_home_rad: Option<f32>,
    #[serde(default)]
    pub limb: Option<String>,
    #[serde(default)]
    pub joint_kind: Option<JointKind>,
    /// YAML fragment (string) preserving v1 `extra` map entries so nothing is silently dropped.
    #[serde(default)]
    pub notes_yaml: Option<String>,
    /// Operator intent for writable firmware parameters (RAM/flash mirrors of type-18 writes).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    #[ts(type = "Record<string, unknown>")]
    pub desired_params: BTreeMap<String, serde_json::Value>,
}

/// Protocol family inside actuators. Extensible (new vendor → new variant).
///
/// Internally tagged (`kind`) so serde YAML/JSON round-trips cleanly with `Device`'s own `kind`
/// field (ts-rs-compatible; avoids mixed untagged/tagged enum deserialization issues).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum ActuatorFamily {
    Robstride { model: RobstrideModel },
}

/// Concrete RobStride SKU; drives `config/actuators/robstride_rs0X.yaml` lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum RobstrideModel {
    Rs01,
    Rs02,
    Rs03,
    Rs04,
}

impl RobstrideModel {
    /// Value of YAML `actuator_model` for this SKU (e.g. `RS03`).
    pub fn as_spec_label(self) -> &'static str {
        match self {
            Self::Rs01 => "RS01",
            Self::Rs02 => "RS02",
            Self::Rs03 => "RS03",
            Self::Rs04 => "RS04",
        }
    }

    /// Filename fragment after `robstride_` (e.g. `rs03` → `robstride_rs03.yaml`).
    pub fn robstride_yaml_suffix(self) -> &'static str {
        match self {
            Self::Rs01 => "rs01",
            Self::Rs02 => "rs02",
            Self::Rs03 => "rs03",
            Self::Rs04 => "rs04",
        }
    }

    /// Parse `actuator_model` from a `robstride_*.yaml` hardware spec.
    pub fn from_spec_actuator_model(label: &str) -> anyhow::Result<Self> {
        let u = label.trim().to_ascii_uppercase();
        match u.as_str() {
            "RS01" => Ok(Self::Rs01),
            "RS02" => Ok(Self::Rs02),
            "RS03" => Ok(Self::Rs03),
            "RS04" => Ok(Self::Rs04),
            _ => anyhow::bail!("unknown RobStride actuator_model in spec: {label:?}"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct Sensor {
    #[serde(flatten)]
    pub common: SensorCommon,
    pub family: SensorFamily,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct SensorCommon {
    pub role: String,
    pub can_bus: String,
    #[serde(with = "super::role::serde_u8_flex")]
    #[ts(as = "u8")]
    pub can_id: u8,
    #[serde(default = "default_true")]
    pub present: bool,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub commissioned_at: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Which limb the sensor is mounted on (`head`, `torso`, `right_arm`, etc.).
    /// Optional so sensors that haven't been placed yet, or sensors that don't
    /// belong to a specific limb (e.g. a chest-mounted IMU could just be
    /// `torso`), still parse cleanly.
    #[serde(default)]
    pub limb: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum SensorFamily {
    Motion { model: MotionSensorModel },
    Force { model: ForceSensorModel },
    Gyro { model: GyroSensorModel },
    Camera { model: CameraModel },
    Lidar { model: LidarModel },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum MotionSensorModel {
    Bno085,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum ForceSensorModel {
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum GyroSensorModel {
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum CameraModel {
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum LidarModel {
    Placeholder,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct Battery {
    #[serde(flatten)]
    pub common: BatteryCommon,
    pub family: BatteryFamily,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct BatteryCommon {
    pub role: String,
    pub can_bus: String,
    #[serde(with = "super::role::serde_u8_flex")]
    #[ts(as = "u8")]
    pub can_id: u8,
    #[serde(default = "default_true")]
    pub present: bool,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum BatteryFamily {
    Placeholder,
}

// --- Peripherals -------------------------------------------------------------
//
// Catch-all for I/O hardware that isn't an actuator, perception sensor, or
// battery: speakers, microphones, status LEDs, displays, cooling fans, etc.
// These typically don't sit on the CAN bus (USB, I2C, I2S, GPIO, …) but we
// keep the same `(can_bus, can_id)` addressing for now so the inventory
// schema stays uniform across kinds. Rename the fields when a non-CAN
// transport actually lands.

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct Peripheral {
    #[serde(flatten)]
    pub common: PeripheralCommon,
    pub family: PeripheralFamily,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct PeripheralCommon {
    pub role: String,
    pub can_bus: String,
    #[serde(with = "super::role::serde_u8_flex")]
    #[ts(as = "u8")]
    pub can_id: u8,
    #[serde(default = "default_true")]
    pub present: bool,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub commissioned_at: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Which limb the peripheral is mounted on (`head`, `torso`, `right_arm`, …).
    /// Optional — peripherals like fans or status LEDs may not belong to any
    /// specific limb.
    #[serde(default)]
    pub limb: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum PeripheralFamily {
    Microphone { model: MicrophoneModel },
    Speaker { model: SpeakerModel },
    Led { model: LedModel },
    Display { model: DisplayModel },
    Fan { model: FanModel },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum MicrophoneModel {
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum SpeakerModel {
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum LedModel {
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum DisplayModel {
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum FanModel {
    Placeholder,
}

impl Actuator {
    /// RobStride model for this actuator (inventory is RobStride-only today).
    pub fn robstride_model(&self) -> RobstrideModel {
        match self.family {
            ActuatorFamily::Robstride { model } => model,
        }
    }

    /// `{limb}.{joint_kind}` when both are set.
    pub fn canonical_role(&self) -> Option<String> {
        Some(format!(
            "{}.{}",
            self.common.limb.as_ref()?,
            self.common.joint_kind?.as_snake_case()
        ))
    }
}

impl Device {
    pub fn role(&self) -> &str {
        match self {
            Device::Actuator(a) => &a.common.role,
            Device::Sensor(s) => &s.common.role,
            Device::Battery(b) => &b.common.role,
            Device::Peripheral(p) => &p.common.role,
        }
    }

    pub fn can_bus(&self) -> &str {
        match self {
            Device::Actuator(a) => &a.common.can_bus,
            Device::Sensor(s) => &s.common.can_bus,
            Device::Battery(b) => &b.common.can_bus,
            Device::Peripheral(p) => &p.common.can_bus,
        }
    }

    pub fn can_id(&self) -> u8 {
        match self {
            Device::Actuator(a) => a.common.can_id,
            Device::Sensor(s) => s.common.can_id,
            Device::Battery(b) => b.common.can_id,
            Device::Peripheral(p) => p.common.can_id,
        }
    }
}
