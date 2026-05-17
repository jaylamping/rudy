# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

import json
from pathlib import Path

from simulation.compare import build_report, main
from simulation.schema import SimState, load_scenario


def _state(time_s: float, position_rad: float, velocity_rad_s: float) -> dict:
    return {
        "time_s": time_s,
        "runtime_state": "running",
        "joints": {
            "l_elbow_pitch_joint": {
                "position_rad": position_rad,
                "velocity_rad_s": velocity_rad_s,
                "effort_nm": 1.0,
                "soft_limit_margin_rad": 0.8,
            }
        },
        "contacts": [],
        "validation_failures": [],
    }


def test_build_report_passes_for_matching_joint_trace():
    root = Path(__file__).resolve().parents[1]
    scenario = load_scenario(root / "configs" / "scenarios" / "joint_space_smoke.yaml")
    reference = tuple(SimState.from_mapping(state) for state in [_state(0.0, 0.0, 0.0), _state(1.0, 0.4, 0.4)])
    candidate = tuple(SimState.from_mapping(state) for state in [_state(0.0, 0.0, 0.0), _state(1.0, 0.41, 0.4)])

    report = build_report(
        scenario=scenario,
        reference=reference,
        candidate=candidate,
        simulator_versions={"isaac": "test", "mujoco": "test"},
        model_hashes={"urdf": "test"},
    )

    assert report.passed
    assert report.metrics.joint_position_max_abs_rad < 0.05
    assert report.metrics.runtime_stop_count == 0


def test_cli_writes_json_report(tmp_path):
    root = Path(__file__).resolve().parents[1]
    scenario = root / "configs" / "scenarios" / "joint_space_smoke.yaml"
    isaac_trace = tmp_path / "isaac.json"
    mujoco_trace = tmp_path / "mujoco.json"
    report_path = tmp_path / "report.json"
    trace = [_state(0.0, 0.0, 0.0), _state(1.0, 0.4, 0.4)]
    isaac_trace.write_text(json.dumps({"states": trace}), encoding="utf-8")
    mujoco_trace.write_text(json.dumps({"states": trace}), encoding="utf-8")

    main(
        [
            "--scenario",
            str(scenario),
            "--isaac-trace",
            str(isaac_trace),
            "--mujoco-trace",
            str(mujoco_trace),
            "--out",
            str(report_path),
            "--sim-version",
            "isaac=test",
            "--sim-version",
            "mujoco=test",
            "--model-hash",
            "urdf=test",
        ]
    )

    report = json.loads(report_path.read_text(encoding="utf-8"))
    assert report["passed"] is True
    assert report["scenario_name"] == "joint_space_smoke"
    assert report["metrics"]["joint_position_rms_rad"] == 0.0
