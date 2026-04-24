//! Inventory device rows: actuators, sensors, batteries.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::limb::JointKind;

use super::travel_limits::TravelLimits;

fn default_true() -> bool {
    true
}

fn default_direction_sign() -> i8 {
    1
}

fn deserialize_direction_sign<'de, D>(de: D) -> Result<i8, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{Error, Unexpected};
    let v = i8::deserialize(de)?;
    match v {
        1 | -1 => Ok(v),
        other => Err(D::Error::invalid_value(
            Unexpected::Signed(other as i64),
            &"+1 or -1 (RobStride mech_pos_rad polarity vs cortex's logical frame)",
        )),
    }
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
    /// Optional override for home-ramp nominal speed (rad/s). `None` uses
    /// global `cortex.toml` [`crate::config::SafetyConfig::effective_homing_speed_rad_s`].
    #[serde(default)]
    pub homing_speed_rad_s: Option<f32>,
    /// Per-actuator override for the post-home MIT spring-damper hold
    /// stiffness (Nm/rad). `None` falls back to
    /// [`crate::config::SafetyConfig::hold_kp_nm_per_rad`].
    ///
    /// Heavily-loaded joints (shoulder_pitch with arm payload,
    /// elbow_pitch at full extension) need stiffer springs than
    /// lightly-loaded wrist joints to keep gravity droop inside
    /// `hold_verification`'s 2× effective_tolerance window during the
    /// 500 ms post-MIT settle. Per-joint override here lets each joint
    /// run at the kp it actually needs without forcing every motor to
    /// the worst-case global default.
    #[serde(default)]
    pub hold_kp_nm_per_rad: Option<f32>,
    /// Per-actuator override for the post-home MIT damping
    /// (Nm·s/rad). `None` falls back to
    /// [`crate::config::SafetyConfig::hold_kd_nm_s_per_rad`]. Scale
    /// with `sqrt(kp_ratio)` to preserve the spring-damper damping
    /// ratio when bumping `hold_kp_nm_per_rad`.
    #[serde(default)]
    pub hold_kd_nm_s_per_rad: Option<f32>,
    /// Polarity of this motor's mechanical encoder relative to the
    /// firmware velocity command sign — i.e., does commanding a
    /// positive `vel_rad_s` make `mech_pos_rad` increase (+1) or
    /// decrease (-1)?
    ///
    /// All cortex internal state (home target, travel_limits,
    /// commissioned_zero_offset, jog vel, telemetry rows in
    /// `state.latest`) lives in the **logical frame** where positive
    /// vel always grows positive position. Sign translation happens
    /// only at the CAN boundary:
    ///   - `set_velocity_setpoint` multiplies the logical vel by sign
    ///     before sending RUN_MODE=2 spd_ref;
    ///   - `set_position_hold` / `set_mit_hold` multiply the logical
    ///     target by sign before writing LOC_REF / OperationCtrl;
    ///   - type-2 / type-17 telemetry decode multiplies the
    ///     firmware-reported `mech_pos_rad` and `mech_vel_rad_s` by
    ///     sign on ingest.
    ///
    /// Only `+1` (encoder agrees with command) and `-1` (encoder
    /// inverted relative to command — typically a downstream gearbox
    /// flipping rotation direction, or a mounting orientation that
    /// makes "logical positive" the operator-meaningful direction
    /// while the motor was wired the opposite way) are valid.
    ///
    /// Defaults to `+1`. Set to `-1` only after a bench test
    /// (operator jogs at +0.2 rad/s, mech_pos_rad in `state.latest`
    /// goes DOWN); a misconfigured `-1` will make the home-ramp
    /// command in the wrong direction and trip its tracking-error
    /// gate within ~150 ms of the first tick (the symmetric of the
    /// bug this knob fixes).
    #[serde(
        default = "default_direction_sign",
        deserialize_with = "deserialize_direction_sign"
    )]
    #[ts(type = "number")]
    pub direction_sign: i8,
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

impl ActuatorCommon {
    /// `+1.0` or `-1.0` view of [`Self::direction_sign`] for arithmetic at the
    /// CAN boundary (telemetry decode, vel/pos command write). Cortex's
    /// internal state is always in the logical frame; this conversion only
    /// matters at the firmware-edge translation sites.
    pub fn direction_sign_f32(&self) -> f32 {
        if self.direction_sign < 0 {
            -1.0
        } else {
            1.0
        }
    }
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

#[cfg(test)]
mod direction_sign_tests {
    use super::*;

    fn minimal_common(direction_sign: i8) -> ActuatorCommon {
        ActuatorCommon {
            role: "test".into(),
            can_bus: "can0".into(),
            can_id: 1,
            present: true,
            verified: false,
            commissioned_at: None,
            firmware_version: None,
            travel_limits: None,
            commissioned_zero_offset: None,
            active_report_persisted: false,
            predefined_home_rad: None,
            homing_speed_rad_s: None,
            hold_kp_nm_per_rad: None,
            hold_kd_nm_s_per_rad: None,
            limb: None,
            joint_kind: None,
            notes_yaml: None,
            desired_params: BTreeMap::new(),
            direction_sign,
        }
    }

    #[test]
    fn direction_sign_f32_maps_minus_one_to_minus_one() {
        let c = minimal_common(-1);
        assert_eq!(c.direction_sign_f32(), -1.0);
    }

    #[test]
    fn direction_sign_f32_maps_plus_one_to_plus_one() {
        let c = minimal_common(1);
        assert_eq!(c.direction_sign_f32(), 1.0);
    }

    #[test]
    fn missing_direction_sign_in_yaml_defaults_to_plus_one() {
        // Mirrors the on-disk inventory.yaml format used by every
        // pre-direction-sign entry. The serde default must be `+1`
        // so old inventories migrate without behavior change.
        let yaml = "role: m\ncan_bus: can0\ncan_id: 1\n";
        let c: ActuatorCommon = serde_yaml::from_str(yaml).expect("parse defaults");
        assert_eq!(c.direction_sign, 1);
        assert_eq!(c.direction_sign_f32(), 1.0);
    }

    #[test]
    fn explicit_minus_one_in_yaml_round_trips() {
        let yaml = "role: m\ncan_bus: can0\ncan_id: 1\ndirection_sign: -1\n";
        let c: ActuatorCommon = serde_yaml::from_str(yaml).expect("parse explicit -1");
        assert_eq!(c.direction_sign, -1);
        assert_eq!(c.direction_sign_f32(), -1.0);
    }

    #[test]
    fn invalid_direction_sign_zero_is_rejected_at_parse() {
        // Guard against the most plausible operator typo (zero
        // instead of one) that would otherwise silently no-op every
        // velocity command and every telemetry sample by multiplying
        // by zero. Better to fail-stop at config-load than to ship
        // a process that's deaf and dumb to its motors.
        let yaml = "role: m\ncan_bus: can0\ncan_id: 1\ndirection_sign: 0\n";
        let err = serde_yaml::from_str::<ActuatorCommon>(yaml)
            .expect_err("zero direction_sign must reject");
        let msg = err.to_string();
        assert!(
            msg.contains("+1") && msg.contains("-1"),
            "error must explain valid values; got: {msg}"
        );
    }

    #[test]
    fn invalid_direction_sign_two_is_rejected_at_parse() {
        let yaml = "role: m\ncan_bus: can0\ncan_id: 1\ndirection_sign: 2\n";
        assert!(
            serde_yaml::from_str::<ActuatorCommon>(yaml).is_err(),
            "out-of-range direction_sign must reject"
        );
    }
}
