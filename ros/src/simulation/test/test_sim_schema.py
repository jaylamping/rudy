# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

from pathlib import Path

from simulation.schema import SimScenario, load_scenario


def test_load_joint_space_scenario_contract():
    root = Path(__file__).resolve().parents[1]
    scenario = load_scenario(root / "configs" / "scenarios" / "joint_space_smoke.yaml")

    assert scenario == SimScenario.from_mapping(scenario.to_mapping())
    assert scenario.name == "joint_space_smoke"
    assert scenario.seed == 9
    assert scenario.dt_s == 0.02
    assert scenario.commands[0].primitive == "home"
    assert "l_elbow_pitch_joint" in scenario.commands[1].joint_targets
    assert scenario.thresholds["runtime_stop_count"] == 0.0
