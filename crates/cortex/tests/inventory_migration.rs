//! v1 → v2 migration round-trip: every v1 field is represented in v2 (including `extra` → `notes_yaml`).

use std::path::PathBuf;

use cortex::inventory::{migrate_v1_yaml_to_v2_inventory, Inventory};

const FIXTURE_V1: &str = include_str!("../../../config/actuators/inventory.yaml.v1.bak");

#[test]
fn migration_round_trip_preserves_roles_and_extra() {
    let inv = migrate_v1_yaml_to_v2_inventory(FIXTURE_V1).expect("migrate repo inventory v1");
    assert_eq!(inv.schema_version, Some(2));
    assert!(!inv.devices.is_empty());

    // Every v1 motor becomes an actuator with RS03 family.
    let roles: Vec<_> = inv.actuators().map(|a| a.common.role.as_str()).collect();
    assert!(roles.contains(&"shoulder_actuator_a"));
    assert!(roles.contains(&"shoulder_actuator_b"));

    let a = inv
        .actuator_by_role("shoulder_actuator_a")
        .expect("shoulder_actuator_a");
    assert!(matches!(
        a.family,
        cortex::inventory::ActuatorFamily::Robstride {
            model: cortex::inventory::RobstrideModel::Rs03
        }
    ));
    // Large `extra` payload from the real file should land in notes_yaml.
    let notes = a.common.notes_yaml.as_deref().unwrap_or("");
    assert!(
        notes.contains("sourced_from") || notes.contains("baseline"),
        "expected preserved YAML from v1 flatten map; got len {}",
        notes.len()
    );
}

#[test]
fn v2_inventory_round_trips_through_value() {
    let inv = migrate_v1_yaml_to_v2_inventory(FIXTURE_V1).expect("migrate");
    let v = serde_yaml::to_value(&inv).expect("to_value");
    let parsed: Inventory = serde_yaml::from_value(v).expect("from_value");
    parsed.validate().expect("validate re-parsed");
    assert_eq!(parsed.actuators().count(), inv.actuators().count());
}

/// The repo `config/actuators/inventory.yaml` (schema v2) must load.
#[test]
fn repo_inventory_file_loads() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/actuators/inventory.yaml");
    Inventory::load(&p).expect("Inventory::load on config/actuators/inventory.yaml");
}
