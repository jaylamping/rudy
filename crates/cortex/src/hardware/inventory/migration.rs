//! v1 → v2 inventory schema migration.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use serde::Deserialize;

use crate::limb::JointKind;

use super::devices::{Actuator, ActuatorCommon, ActuatorFamily, Device, RobstrideModel};
use super::travel_limits::TravelLimits;
use super::Inventory;
use super::INVENTORY_SCHEMA_V2;

/// v1 `motors:` row (flat + `extra` map). Used only by [`migrate_v1_yaml_to_v2_inventory`].
#[derive(Debug, Deserialize)]
pub(crate) struct LegacyMotorV1 {
    pub role: String,
    pub can_bus: String,
    #[serde(with = "super::role::serde_u8_flex")]
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
    pub active_report_persisted: bool,
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

fn default_true() -> bool {
    true
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
            active_report_persisted: m.active_report_persisted,
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
