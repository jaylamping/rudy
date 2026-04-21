use super::*;
use std::io::Write;

fn repo_rs03_path() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/actuators/robstride_rs03.yaml")
}

#[test]
fn load_repo_rs03_parses_extended_sections() {
    let spec = ActuatorSpec::load(repo_rs03_path()).expect("load repo RS03 spec");
    assert_eq!(spec.actuator_model, "RS03");
    assert_eq!(spec.protocol.bitrate_bps, 1_000_000);
    assert_eq!(spec.protocol.comm_types.get("op_control"), Some(&1));
    assert_eq!(spec.protocol.id_layout.comm_type_bits, [24, 28]);
    assert_eq!(spec.hardware.gear_ratio, 9.0);
    assert_eq!(spec.hardware.encoder_resolution_bits, 14);
    assert!((spec.op_control_scaling.position.range[0] + 12.566_371).abs() < 1e-5);
    assert!((spec.op_control_scaling.position.range[1] - 12.566_371).abs() < 1e-5);
    assert_eq!(spec.thermal.max_winding_temp_c, 120.0);
    assert_eq!(spec.thermal.derating_start_c, 100.0);
    assert!(!spec.notes.is_empty());
}

#[test]
fn robstride_filename_mismatch_errors() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("robstride_rs03.yaml");
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(
        file,
        r"schema_version: 2
actuator_model: RS04
protocol: {{}}
hardware: {{}}
op_control_scaling:
  position: {{ units: rad, range: [0, 1] }}
  velocity: {{ units: rad_per_s, range: [0, 1] }}
  kp: {{ units: dimensionless, range: [0, 1] }}
  kd: {{ units: dimensionless, range: [0, 1] }}
  torque_ff: {{ units: nm, range: [0, 1] }}
firmware_limits: {{}}
observables: {{}}
thermal: {{}}
"
    )
    .unwrap();
    let err = ActuatorSpec::load(&path).unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("actuator_model") && msg.contains("RS04"),
        "unexpected error: {msg}"
    );
}

#[test]
fn non_robstride_filename_skips_model_check() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spec.yaml");
    std::fs::write(
        &path,
        "schema_version: 2\nactuator_model: ANYTHING\nfirmware_limits: {}\nobservables: {}\n",
    )
    .unwrap();
    let spec = ActuatorSpec::load(&path).expect("minimal spec");
    assert_eq!(spec.actuator_model, "ANYTHING");
}

#[test]
fn robstride_spec_wrapper_loads() {
    let s = RobstrideSpec::load(repo_rs03_path()).expect("wrapper load");
    assert_eq!(s.actuator_model, "RS03");
}

#[test]
fn travel_rail_from_spec_rs03_matches_op_control_range() {
    let spec = ActuatorSpec::load(repo_rs03_path()).expect("load");
    let (lo, hi) = spec.mit_position_rail_rad();
    assert!((lo - spec.op_control_scaling.position.range[0]).abs() < 1e-5);
    assert!((hi - spec.op_control_scaling.position.range[1]).abs() < 1e-5);
}

#[test]
fn travel_rail_from_spec_per_model_narrow_range() {
    let mut spec = ActuatorSpec::load(repo_rs03_path()).expect("load");
    spec.op_control_scaling.position.range = [-1.25, 2.5];
    assert_eq!(spec.mit_position_rail_rad(), (-1.25, 2.5));
}
