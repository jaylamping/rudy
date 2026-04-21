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

use anyhow::{anyhow, bail, Context, Result};
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
    Device, DisplayModel, FanModel, ForceSensorModel, GyroSensorModel, LedModel, LidarModel,
    MicrophoneModel, MotionSensorModel, Peripheral, PeripheralCommon, PeripheralFamily,
    RobstrideModel, Sensor, SensorCommon, SensorFamily, SpeakerModel,
};
pub use error::InventoryError;
pub use migration::migrate_v1_yaml_to_v2_inventory;
pub use role::{validate_canonical_role, validate_role_format};
pub use store::{ensure_seeded, write_atomic, write_replace};
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

    pub fn peripherals(&self) -> impl Iterator<Item = &Peripheral> {
        self.devices.iter().filter_map(|d| match d {
            Device::Peripheral(p) => Some(p),
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

/// Set each actuator's `role` to `limb` + `joint_kind` (canonical form) when both
/// are set and the string disagrees. Use after a partial YAML edit or to recover
/// from `role` / `limb+joint_kind` skew.
///
/// Fails with an error if the target role is already used by a different device,
/// or if more than one actuator would end up with the same new role.
pub fn repair_canonical_actuator_roles(inv: &mut Inventory) -> Result<Vec<(String, String)>> {
    use std::collections::BTreeSet;

    let mut work: Vec<(usize, String, String)> = Vec::new();
    for (idx, d) in inv.devices.iter().enumerate() {
        let Device::Actuator(a) = d else {
            continue;
        };
        if let (Some(limb), Some(jk)) = (&a.common.limb, a.common.joint_kind) {
            let canonical = format!("{limb}.{}", jk.as_snake_case());
            if a.common.role == canonical {
                continue;
            }
            let me = (a.common.can_bus.as_str(), a.common.can_id);
            for d2 in &inv.devices {
                if d2.role() == canonical {
                    let other = device_actuator_id(d2);
                    if other != Some(me) {
                        bail!(
                            "cannot repair {} -> {canonical}: role {canonical} is already used by another device ({} 0x{:02x})",
                            a.common.role,
                            d2.can_bus(),
                            d2.can_id()
                        );
                    }
                }
            }
            work.push((idx, a.common.role.clone(), canonical));
        }
    }

    let mut seen_new: BTreeSet<String> = BTreeSet::new();
    for (_, _old, new) in &work {
        if !seen_new.insert(new.clone()) {
            bail!(
                "cannot repair: multiple actuators would become role {new} — fix manually (swap or clear one row)"
            );
        }
    }

    for (idx, _old, new) in &work {
        if let Device::Actuator(a) = &mut inv.devices[*idx] {
            a.common.role = new.clone();
        }
    }

    let changes: Vec<(String, String)> = work.into_iter().map(|(_, old, new)| (old, new)).collect();
    Ok(changes)
}

fn device_actuator_id(d: &Device) -> Option<(&str, u8)> {
    match d {
        Device::Actuator(a) => Some((a.common.can_bus.as_str(), a.common.can_id)),
        _ => None,
    }
}

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
