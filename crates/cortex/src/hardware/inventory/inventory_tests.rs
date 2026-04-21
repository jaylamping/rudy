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
                    active_report_persisted: false,
                    predefined_home_rad: None,
                    homing_speed_rad_s: None,
                    limb: None,
                    joint_kind: None,
                    notes_yaml: None,
                    desired_params: std::collections::BTreeMap::new(),
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
                    active_report_persisted: false,
                    predefined_home_rad: None,
                    homing_speed_rad_s: None,
                    limb: None,
                    joint_kind: None,
                    notes_yaml: None,
                    desired_params: std::collections::BTreeMap::new(),
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

#[test]
fn desired_params_roundtrips_in_yaml() {
    use serde_json::json;
    use std::collections::BTreeMap;

    let dir = tempfile::tempdir().expect("tmpdir");
    let path = dir.path().join("inv.yaml");
    let mut desired = BTreeMap::new();
    desired.insert("limit_torque".into(), json!(10.0));
    let inv = Inventory {
        schema_version: Some(2),
        devices: vec![Device::Actuator(Actuator {
            common: ActuatorCommon {
                role: "bench.m1".into(),
                can_bus: "can0".into(),
                can_id: 3,
                present: true,
                verified: false,
                commissioned_at: None,
                firmware_version: None,
                travel_limits: None,
                commissioned_zero_offset: None,
                active_report_persisted: false,
                predefined_home_rad: None,
                homing_speed_rad_s: None,
                limb: None,
                joint_kind: None,
                notes_yaml: None,
                desired_params: desired,
            },
            family: ActuatorFamily::Robstride {
                model: RobstrideModel::Rs03,
            },
        })],
    };
    inv.validate().expect("validate");
    let yaml = serde_yaml::to_string(&inv).expect("serialize");
    std::fs::write(&path, &yaml).expect("write");
    let back = Inventory::load(&path).expect("load");
    let a = back.actuator_by_role("bench.m1").expect("motor");
    assert_eq!(
        a.common
            .desired_params
            .get("limit_torque")
            .and_then(|v| v.as_f64()),
        Some(10.0)
    );
}
