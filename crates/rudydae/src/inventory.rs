//! Typed loader for `config/actuators/inventory.yaml` (schema v2).
//!
//! # Taxonomy (three levels)
//!
//! 1. **`Device::kind`** — actuator vs sensor vs battery. Drives boot/travel/commission semantics.
//! 2. **`ActuatorFamily`** — wire protocol family (e.g. RobStride). Dispatches codecs.
//! 3. **Per-family model** (e.g. [`RobstrideModel`]) — gear ratio, MIT ranges, param layout.
//!
//! v1 used a flat `motors:` list; v2 uses `devices:` with tagged [`Device`] entries. Loading v1
//! files fails with [`InventoryError::SchemaVersionMismatch`]; use `migrate_inventory` once.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ts_rs::TS;

use crate::limb::JointKind;

/// Backwards-compatible name for [`Actuator`] used in CAN/driver call sites during migration.
pub type Motor = Actuator;

// --- Schema version & errors -------------------------------------------------

const INVENTORY_SCHEMA_V2: u32 = 2;

fn default_schema_v2() -> Option<u32> {
    Some(INVENTORY_SCHEMA_V2)
}

/// Structured failure modes for [`Inventory::load`].
#[derive(Debug, Error)]
pub enum InventoryError {
    /// On-disk file is not schema v2 — run the migration tool once.
    #[error(
        "inventory schema version mismatch: found {found}, required {required} — {migration_hint}"
    )]
    SchemaVersionMismatch {
        found: u32,
        required: u32,
        migration_hint: String,
    },
}

// --- Top-level inventory -----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    /// Must be `2` for [`Inventory::load`]. Serialized default for new files.
    #[serde(default = "default_schema_v2")]
    pub schema_version: Option<u32>,
    pub devices: Vec<Device>,
}

// --- Polymorphic device ------------------------------------------------------

/// Inventory row: actuator, sensor, or battery. JSON/YAML uses `kind` as the tag.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[ts(export, export_to = "./")]
pub enum Device {
    Actuator(Actuator),
    Sensor(Sensor),
    Battery(Battery),
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
    #[serde(with = "crate::util::serde_u8_flex")]
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
    pub predefined_home_rad: Option<f32>,
    #[serde(default)]
    pub limb: Option<String>,
    #[serde(default)]
    pub joint_kind: Option<JointKind>,
    /// YAML fragment (string) preserving v1 `extra` map entries so nothing is silently dropped.
    #[serde(default)]
    pub notes_yaml: Option<String>,
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
    pub fn from_spec_actuator_model(label: &str) -> Result<Self> {
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
    #[serde(with = "crate::util::serde_u8_flex")]
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
    #[serde(with = "crate::util::serde_u8_flex")]
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

/// Per-actuator soft travel-limits band (radians).
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "./")]
pub struct TravelLimits {
    pub min_rad: f32,
    pub max_rad: f32,
    #[serde(default)]
    pub updated_at: Option<String>,
}

fn default_true() -> bool {
    true
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
        }
    }

    pub fn can_bus(&self) -> &str {
        match self {
            Device::Actuator(a) => &a.common.can_bus,
            Device::Sensor(s) => &s.common.can_bus,
            Device::Battery(b) => &b.common.can_bus,
        }
    }

    pub fn can_id(&self) -> u8 {
        match self {
            Device::Actuator(a) => a.common.can_id,
            Device::Sensor(s) => s.common.can_id,
            Device::Battery(b) => b.common.can_id,
        }
    }
}

// --- Role validation (same rules as v1) --------------------------------------

pub fn validate_role_format(role: &str) -> Result<()> {
    if role.is_empty() {
        return Err(anyhow!("role is empty"));
    }
    let bytes = role.as_bytes();
    if !bytes[0].is_ascii_lowercase() {
        return Err(anyhow!("role {role} must start with a lowercase letter"));
    }
    let dots = role.matches('.').count();
    if dots > 1 {
        return Err(anyhow!("role {role} contains more than one dot"));
    }
    for &b in bytes {
        let ok = b.is_ascii_lowercase() || b == b'_' || b == b'.' || b.is_ascii_digit();
        if !ok {
            return Err(anyhow!(
                "role {role} contains illegal character `{}`",
                b as char
            ));
        }
    }
    Ok(())
}

pub fn validate_canonical_role(role: &str) -> Result<()> {
    validate_role_format(role)?;
    if !role.contains('.') {
        return Err(anyhow!(
            "role {role} is not canonical (expected `{{limb}}.{{joint_kind}}`)"
        ));
    }
    let parts: Vec<&str> = role.split('.').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return Err(anyhow!("role {role} must have exactly one non-empty dot"));
    }
    Ok(())
}

impl Inventory {
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let raw: serde_yaml::Value = serde_yaml::from_str(&text)
            .with_context(|| format!("parsing YAML in {}", path.display()))?;
        let schema = raw
            .get("schema_version")
            .and_then(|v| v.as_u64())
            .map(|u| u as u32)
            .unwrap_or(1);
        if schema != INVENTORY_SCHEMA_V2 {
            return Err(InventoryError::SchemaVersionMismatch {
                found: schema,
                required: INVENTORY_SCHEMA_V2,
                migration_hint: "run `cargo run --bin migrate_inventory` (see docs/operator-guide/inventory-v2-migration.md)".into(),
            }
            .into());
        }
        let inv: Inventory = serde_yaml::from_value(raw)
            .with_context(|| format!("parsing YAML in {}", path.display()))?;
        inv.validate()
            .with_context(|| format!("validating {}", path.display()))?;
        Ok(inv)
    }

    pub fn validate(&self) -> Result<()> {
        let mut roles: BTreeSet<&str> = BTreeSet::new();
        let mut seen_ids: BTreeSet<(String, u8)> = BTreeSet::new();

        for d in &self.devices {
            let role = d.role();
            validate_role_format(role).with_context(|| format!("device {role} has invalid role format"))?;
            if !roles.insert(role) {
                return Err(anyhow!("duplicate role: {role}"));
            }
            let bus = d.can_bus().to_string();
            let id = d.can_id();
            if !seen_ids.insert((bus.clone(), id)) {
                return Err(anyhow!(
                    "duplicate (can_bus, can_id): ({bus}, 0x{id:02x}) — two devices share the same bus address"
                ));
            }

            if let Device::Actuator(a) = d {
                if a.common.joint_kind.is_some() && a.common.limb.is_none() {
                    return Err(anyhow!(
                        "actuator {} has joint_kind set without limb",
                        a.common.role
                    ));
                }
                if let (Some(_), Some(_)) = (&a.common.limb, a.common.joint_kind) {
                    if let Some(canonical) = a.canonical_role() {
                        if a.common.role != canonical {
                            return Err(anyhow!(
                                "actuator {} has limb+joint_kind but role does not match canonical form `{}`",
                                a.common.role,
                                canonical
                            ));
                        }
                    }
                }
            }
        }

        let mut seen_joints: BTreeSet<(String, JointKind)> = BTreeSet::new();
        for d in &self.devices {
            if let Device::Actuator(a) = d {
                if let (Some(limb), Some(jk)) = (&a.common.limb, a.common.joint_kind) {
                    let key = (limb.clone(), jk);
                    if !seen_joints.insert(key) {
                        return Err(anyhow!(
                            "duplicate joint_kind {:?} within limb {} (actuator {})",
                            jk,
                            limb,
                            a.common.role
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Any device with this role (actuator, sensor, or battery).
    pub fn by_role(&self, role: &str) -> Option<&Device> {
        self.devices.iter().find(|d| d.role() == role)
    }

    /// First actuator with this role, if the entry is an actuator.
    pub fn actuator_by_role(&self, role: &str) -> Option<&Actuator> {
        self.by_role(role).and_then(|d| match d {
            Device::Actuator(a) => Some(a),
            _ => None,
        })
    }

    pub fn by_can_id(&self, bus: &str, can_id: u8) -> Option<&Device> {
        self.devices
            .iter()
            .find(|d| d.can_bus() == bus && d.can_id() == can_id)
    }

    pub fn actuators(&self) -> impl Iterator<Item = &Actuator> {
        self.devices.iter().filter_map(|d| match d {
            Device::Actuator(a) => Some(a),
            _ => None,
        })
    }

    pub fn sensors(&self) -> impl Iterator<Item = &Sensor> {
        self.devices.iter().filter_map(|d| match d {
            Device::Sensor(s) => Some(s),
            _ => None,
        })
    }

    pub fn batteries(&self) -> impl Iterator<Item = &Battery> {
        self.devices.iter().filter_map(|d| match d {
            Device::Battery(b) => Some(b),
            _ => None,
        })
    }
}

/// Group present actuators by `limb`, sorted proximal-to-distal. Skips actuators without `limb`.
pub fn ordered_actuators_per_limb(inv: &Inventory) -> BTreeMap<String, Vec<&Actuator>> {
    let mut by_limb: BTreeMap<String, Vec<&Actuator>> = BTreeMap::new();
    for a in inv.actuators() {
        if !a.common.present {
            continue;
        }
        let Some(limb) = a.common.limb.as_ref() else {
            continue;
        };
        by_limb.entry(limb.clone()).or_default().push(a);
    }
    for actuators in by_limb.values_mut() {
        actuators.sort_by_key(|a| {
            a.common
                .joint_kind
                .map(|jk| jk.home_order())
                .unwrap_or(255)
        });
    }
    by_limb
}

// --- v1 migration -------------------------------------------------------------

/// v1 `motors:` row (flat + `extra` map). Used only by [`migrate_v1_yaml_to_v2_inventory`].
#[derive(Debug, Deserialize)]
pub(crate) struct LegacyMotorV1 {
    pub role: String,
    pub can_bus: String,
    #[serde(with = "crate::util::serde_u8_flex")]
    pub can_id: u8,
    #[serde(default)]
    pub firmware_version: Option<String>,
    #[serde(default)]
    pub verified: bool,
    #[serde(default)]
    pub commissioned_at: Option<String>,
    #[serde(default = "default_true")]
    pub present: bool,
    #[serde(default)]
    pub travel_limits: Option<TravelLimits>,
    #[serde(default)]
    pub commissioned_zero_offset: Option<f32>,
    #[serde(default)]
    pub predefined_home_rad: Option<f32>,
    #[serde(default)]
    pub limb: Option<String>,
    #[serde(default)]
    pub joint_kind: Option<JointKind>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml::Value>,
}

#[derive(Debug, Deserialize)]
struct LegacyInventoryV1 {
    motors: Vec<LegacyMotorV1>,
}

/// Convert v1 YAML text to a v2 [`Inventory`] (in memory). Used by `migrate_inventory` and tests.
pub fn migrate_v1_yaml_to_v2_inventory(yaml: &str) -> Result<Inventory> {
    let v1: LegacyInventoryV1 =
        serde_yaml::from_str(yaml).context("parse legacy v1 inventory YAML")?;
    let mut devices = Vec::with_capacity(v1.motors.len());
    for m in v1.motors {
        let notes_yaml = if m.extra.is_empty() {
            None
        } else {
            Some(serde_yaml::to_string(&m.extra).context("serialize v1 extra to YAML string")?)
        };
        let common = ActuatorCommon {
            role: m.role,
            can_bus: m.can_bus,
            can_id: m.can_id,
            present: m.present,
            verified: m.verified,
            commissioned_at: m.commissioned_at,
            firmware_version: m.firmware_version,
            travel_limits: m.travel_limits,
            commissioned_zero_offset: m.commissioned_zero_offset,
            predefined_home_rad: m.predefined_home_rad,
            limb: m.limb,
            joint_kind: m.joint_kind,
            notes_yaml,
        };
        devices.push(Device::Actuator(Actuator {
            common,
            family: ActuatorFamily::Robstride {
                model: RobstrideModel::Rs03,
            },
        }));
    }
    let inv = Inventory {
        schema_version: Some(INVENTORY_SCHEMA_V2),
        devices,
    };
    inv.validate().context("validate migrated inventory")?;
    Ok(inv)
}

pub fn write_atomic(
    path: &Path,
    mutate: impl FnOnce(&mut Inventory) -> Result<()>,
) -> Result<Inventory> {
    let mut inv = Inventory::load(path)
        .with_context(|| format!("re-reading {} for write_atomic", path.display()))?;
    mutate(&mut inv)?;

    inv.validate()
        .context("post-mutation inventory validation failed")?;

    let yaml = serde_yaml::to_string(&inv).context("serialising inventory back to YAML")?;

    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_stem = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("inventory.yaml");

    let mut tmp = tempfile::Builder::new()
        .prefix(&format!(".{file_stem}."))
        .suffix(".tmp")
        .tempfile_in(parent)
        .with_context(|| format!("creating tempfile next to {}", path.display()))?;

    {
        use std::io::Write;
        tmp.write_all(yaml.as_bytes())
            .context("writing inventory YAML to tempfile")?;
        tmp.as_file()
            .sync_all()
            .context("fsync inventory tempfile")?;
    }

    tmp.persist(path)
        .with_context(|| format!("rename tempfile -> {}", path.display()))?;
    Ok(inv)
}

/// Seed copy helper (unchanged contract from v1).
pub fn ensure_seeded(inventory: &Path, seed: Option<&Path>) -> Result<()> {
    if inventory.exists() {
        return Ok(());
    }
    let Some(seed) = seed else {
        return Ok(());
    };
    if !seed.exists() {
        return Ok(());
    }
    if let Some(parent) = inventory.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating parent dir {}", parent.display()))?;
        }
    }
    std::fs::copy(seed, inventory).with_context(|| {
        format!(
            "seeding inventory: copy {} -> {}",
            seed.display(),
            inventory.display()
        )
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_duplicate_can_id_same_bus() {
        let inv = Inventory {
            schema_version: Some(2),
            devices: vec![
                Device::Actuator(Actuator {
                    common: ActuatorCommon {
                        role: "a.m1".into(),
                        can_bus: "can0".into(),
                        can_id: 8,
                        present: true,
                        verified: false,
                        commissioned_at: None,
                        firmware_version: None,
                        travel_limits: None,
                        commissioned_zero_offset: None,
                        predefined_home_rad: None,
                        limb: None,
                        joint_kind: None,
                        notes_yaml: None,
                    },
                    family: ActuatorFamily::Robstride {
                        model: RobstrideModel::Rs03,
                    },
                }),
                Device::Actuator(Actuator {
                    common: ActuatorCommon {
                        role: "a.m2".into(),
                        can_bus: "can0".into(),
                        can_id: 8,
                        present: true,
                        verified: false,
                        commissioned_at: None,
                        firmware_version: None,
                        travel_limits: None,
                        commissioned_zero_offset: None,
                        predefined_home_rad: None,
                        limb: None,
                        joint_kind: None,
                        notes_yaml: None,
                    },
                    family: ActuatorFamily::Robstride {
                        model: RobstrideModel::Rs03,
                    },
                }),
            ],
        };
        assert!(inv.validate().is_err());
    }

    #[test]
    fn load_rejects_v1_schema() {
        let dir = tempfile::tempdir().expect("tmpdir");
        let p = dir.path().join("inv.yaml");
        std::fs::write(
            &p,
            r"
schema_version: 1
motors:
  - role: x
    can_bus: can0
    can_id: 1
",
        )
        .expect("write");
        let err = Inventory::load(&p).expect_err("v1 must be refused");
        assert!(err.to_string().contains("schema version mismatch"));
    }

    #[test]
    fn migration_preserves_extra_as_notes_yaml() {
        let v1 = r#"
schema_version: 1
motors:
  - role: shoulder_actuator_a
    can_bus: can1
    can_id: 8
    verified: false
    sourced_from: bench
"#;
        let inv = migrate_v1_yaml_to_v2_inventory(v1).expect("migrate");
        let a = inv.actuator_by_role("shoulder_actuator_a").expect("actuator");
        assert!(a.common.notes_yaml.is_some());
        assert!(a
            .common
            .notes_yaml
            .as_ref()
            .expect("notes")
            .contains("sourced_from"));
    }
}
