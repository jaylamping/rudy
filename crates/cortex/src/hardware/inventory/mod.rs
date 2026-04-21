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

use crate::limb::JointKind;

mod devices;
mod error;
mod migration;
mod role;
mod store;
mod travel_limits;

pub use devices::{
    Actuator, ActuatorCommon, ActuatorFamily, Battery, BatteryCommon, BatteryFamily, CameraModel,
    Device, ForceSensorModel, GyroSensorModel, LidarModel, MotionSensorModel, Motor,
    RobstrideModel, Sensor, SensorCommon, SensorFamily,
};
pub use error::InventoryError;
pub use migration::migrate_v1_yaml_to_v2_inventory;
pub use role::{validate_canonical_role, validate_role_format};
pub use store::{ensure_seeded, write_atomic};
pub use travel_limits::TravelLimits;

pub(crate) const INVENTORY_SCHEMA_V2: u32 = 2;

fn default_schema_v2() -> Option<u32> {
    Some(INVENTORY_SCHEMA_V2)
}

// --- Top-level inventory -----------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    /// Must be `2` for [`Inventory::load`]. Serialized default for new files.
    #[serde(default = "default_schema_v2")]
    pub schema_version: Option<u32>,
    pub devices: Vec<Device>,
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
            validate_role_format(role)
                .with_context(|| format!("device {role} has invalid role format"))?;
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
        actuators.sort_by_key(|a| a.common.joint_kind.map(|jk| jk.home_order()).unwrap_or(255));
    }
    by_limb
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
        let a = inv
            .actuator_by_role("shoulder_actuator_a")
            .expect("actuator");
        assert!(a.common.notes_yaml.is_some());
        assert!(a
            .common
            .notes_yaml
            .as_ref()
            .expect("notes")
            .contains("sourced_from"));
    }
}
