//! Typed loader for `config/actuators/robstride_*.yaml` (RobStride family).
//!
//! [`ActuatorSpec`] holds everything deserialized from a RobStride actuator YAML.
//! [`RobstrideSpec`] is a nominal wrapper so call sites can require RobStride-shaped
//! specs while other actuator families get their own types later.
//!
//! For `robstride_<model>.yaml`, [`ActuatorSpec::load`] checks that `actuator_model`
//! matches the filename (e.g. `robstride_rs03.yaml` ⇒ `RS03`).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::inventory::RobstrideModel;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActuatorSpec {
    #[serde(default)]
    pub schema_version: Option<u32>,
    pub actuator_model: String,
    #[serde(default)]
    pub manual_ref: Option<String>,
    /// CAN / arbitration layout and comm type IDs (RobStride-specific).
    #[serde(default)]
    pub protocol: ProtocolSpec,
    /// Rated torque, gear ratio, encoder bits, etc.
    #[serde(default)]
    pub hardware: HardwareSpec,
    /// MIT op-control (type 1) uint16 scaling ranges.
    #[serde(default)]
    pub op_control_scaling: OpControlScaling,
    #[serde(default)]
    pub firmware_limits: BTreeMap<String, ParamDescriptor>,
    #[serde(default)]
    pub observables: BTreeMap<String, ParamDescriptor>,
    #[serde(default)]
    pub commissioning_defaults: BTreeMap<String, serde_yaml::Value>,
    #[serde(default)]
    pub thermal: ThermalSpec,
    #[serde(default)]
    pub notes: Vec<String>,
}

/// Physical and ID-layout sections under `protocol:` in the YAML.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtocolSpec {
    #[serde(default)]
    pub physical_layer: String,
    #[serde(default)]
    pub bitrate_bps: u32,
    #[serde(default)]
    pub frame_format: String,
    #[serde(default)]
    pub data_length: u8,
    #[serde(default)]
    pub id_layout: ProtocolIdLayout,
    #[serde(default)]
    pub comm_types: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProtocolIdLayout {
    /// Inclusive bit range `[low, high]` within the 29-bit arbitration ID.
    #[serde(default)]
    pub comm_type_bits: [u8; 2],
    #[serde(default)]
    pub data_area_2_bits: [u8; 2],
    #[serde(default)]
    pub dest_addr_bits: [u8; 2],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HardwareSpec {
    #[serde(default)]
    pub voltage_rated_vdc: f32,
    #[serde(default)]
    pub voltage_range_vdc: [f32; 2],
    #[serde(default)]
    pub torque_rated_nm: f32,
    #[serde(default)]
    pub torque_peak_nm: f32,
    #[serde(default)]
    pub phase_current_rated_apk: f32,
    #[serde(default)]
    pub phase_current_peak_apk: f32,
    #[serde(default)]
    pub no_load_speed_rpm: f32,
    #[serde(default)]
    pub encoder_resolution_bits: u8,
    #[serde(default)]
    pub gear_ratio: f32,
    #[serde(default)]
    pub torque_constant_nm_per_arms: f32,
    #[serde(default)]
    pub winding_limit_temp_c: f32,
    #[serde(default)]
    pub board_overtemp_protection_c: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpControlScaling {
    #[serde(default)]
    pub position: OpControlAxisScaling,
    #[serde(default)]
    pub velocity: OpControlAxisScaling,
    #[serde(default)]
    pub kp: OpControlAxisScaling,
    #[serde(default)]
    pub kd: OpControlAxisScaling,
    #[serde(default)]
    pub torque_ff: OpControlAxisScaling,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OpControlAxisScaling {
    #[serde(default)]
    pub units: String,
    #[serde(default)]
    pub range: [f32; 2],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThermalSpec {
    #[serde(default)]
    pub max_winding_temp_c: f32,
    #[serde(default)]
    pub derating_start_c: f32,
}

/// RobStride-family spec loaded from a `robstride_*.yaml` path (validated).
#[derive(Debug, Clone)]
pub struct RobstrideSpec(pub ActuatorSpec);

impl std::ops::Deref for RobstrideSpec {
    type Target = ActuatorSpec;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for RobstrideSpec {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl RobstrideSpec {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        Ok(Self(ActuatorSpec::load(path)?))
    }

    pub fn into_inner(self) -> ActuatorSpec {
        self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct ParamDescriptor {
    /// Hex index like `0x700B`, stored as u16 after parsing.
    #[serde(with = "serde_hex_u16")]
    #[ts(as = "u16")]
    pub index: u16,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub units: Option<String>,
    /// Present on firmware_limits entries only.
    #[serde(default)]
    pub hardware_range: Option<[f32; 2]>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub values: Option<BTreeMap<String, u32>>,
}

mod serde_hex_u16 {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn deserialize<'de, D>(d: D) -> Result<u16, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;
        let s: serde_yaml::Value = Deserialize::deserialize(d)?;
        match s {
            serde_yaml::Value::Number(n) => n
                .as_u64()
                .and_then(|v| u16::try_from(v).ok())
                .ok_or_else(|| Error::custom("index out of u16 range")),
            serde_yaml::Value::String(s) => {
                let s = s.trim();
                let stripped = s
                    .strip_prefix("0x")
                    .or_else(|| s.strip_prefix("0X"))
                    .unwrap_or(s);
                u16::from_str_radix(stripped, 16)
                    .map_err(|e| Error::custom(format!("parse hex {s}: {e}")))
            }
            _ => Err(Error::custom("expected number or hex string")),
        }
    }

    pub fn serialize<S>(v: &u16, s: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        v.serialize(s)
    }
}

/// Load every `robstride_*.yaml` under `actuators_dir`, keyed by [`RobstrideModel`].
///
/// If the directory has no matching files, falls back to `legacy_fallback` (e.g.
/// `paths.actuator_spec`) so tests can use a single ad-hoc YAML path.
pub fn load_robstride_specs(
    actuators_dir: &Path,
    legacy_fallback: Option<&Path>,
) -> Result<HashMap<RobstrideModel, Arc<ActuatorSpec>>> {
    let mut out: HashMap<RobstrideModel, Arc<ActuatorSpec>> = HashMap::new();

    if actuators_dir.is_dir() {
        for entry in std::fs::read_dir(actuators_dir).with_context(|| {
            format!(
                "reading actuator spec directory {}",
                actuators_dir.display()
            )
        })? {
            let entry = entry?;
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with("robstride_") {
                continue;
            }
            let lower = name.to_ascii_lowercase();
            if !lower.ends_with(".yaml") && !lower.ends_with(".yml") {
                continue;
            }
            let spec = Arc::new(ActuatorSpec::load(&path)?);
            let model = RobstrideModel::from_spec_actuator_model(&spec.actuator_model)?;
            if out.contains_key(&model) {
                anyhow::bail!(
                    "duplicate RobStride spec for model {} (second file {})",
                    model.as_spec_label(),
                    path.display()
                );
            }
            out.insert(model, spec);
        }
    }

    if out.is_empty() {
        if let Some(path) = legacy_fallback {
            let spec = Arc::new(ActuatorSpec::load(path)?);
            let model = RobstrideModel::from_spec_actuator_model(&spec.actuator_model)?;
            out.insert(model, spec);
        }
    }

    if out.is_empty() {
        anyhow::bail!(
            "no robstride_*.yaml specs found in {} and no legacy actuator_spec fallback",
            actuators_dir.display()
        );
    }

    Ok(out)
}

impl ActuatorSpec {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let spec: ActuatorSpec = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing YAML in {}", path.display()))?;
        spec.validate_actuator_model_against_filename(path)
            .with_context(|| format!("invalid actuator spec {}", path.display()))?;
        Ok(spec)
    }

    /// For `robstride_<suffix>.yaml`, `actuator_model` must match `<suffix>` (ASCII case-insensitive).
    fn validate_actuator_model_against_filename(&self, path: &Path) -> Result<()> {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            return Ok(());
        };
        let Some(model_from_name) = stem.strip_prefix("robstride_") else {
            return Ok(());
        };
        let expected = model_from_name.to_ascii_uppercase();
        let found = self.actuator_model.to_ascii_uppercase();
        if found != expected {
            anyhow::bail!(
                "actuator_model {:?} does not match filename {:?} (expected {:?} from {:?})",
                self.actuator_model,
                path.file_name().unwrap_or_default(),
                expected,
                stem
            );
        }
        Ok(())
    }

    /// Full parameter catalog suitable for the UI. The trailing
    /// `bool` is `true` for entries from `firmware_limits` (writable
    /// via `PUT /api/motors/:role/params/:name`) and `false` for
    /// entries from `observables`. Threaded into `ParamValue.writable`
    /// at every construction site so the SPA can split its writable /
    /// read-only tables on the spec section instead of guessing from
    /// `hardware_range` (which is only set on the three numeric-range
    /// firmware limits — `limit_torque`, `limit_spd`, `limit_cur` —
    /// and would silently misclassify `can_timeout`, `zero_sta`,
    /// `damper`, etc.).
    pub fn catalog(&self) -> Vec<(String, ParamDescriptor, bool)> {
        let mut out = Vec::with_capacity(self.firmware_limits.len() + self.observables.len());
        for (name, d) in &self.firmware_limits {
            out.push((name.clone(), d.clone(), true));
        }
        for (name, d) in &self.observables {
            out.push((name.clone(), d.clone(), false));
        }
        out
    }

    /// MIT op-control position outer rail from `op_control_scaling.position.range` (radians).
    ///
    /// Degenerate or non-finite YAML (common in minimal test fixtures) falls back to ±4π,
    /// matching the historical single–RS03 envelope.
    pub fn mit_position_rail_rad(&self) -> (f32, f32) {
        const FALLBACK_HALF_WIDTH: f32 = 4.0 * std::f32::consts::PI;
        let [lo, hi] = self.op_control_scaling.position.range;
        if lo.is_finite() && hi.is_finite() && lo < hi {
            (lo, hi)
        } else {
            (-FALLBACK_HALF_WIDTH, FALLBACK_HALF_WIDTH)
        }
    }
}

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;
