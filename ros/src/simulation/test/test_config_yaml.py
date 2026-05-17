# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

from pathlib import Path

import yaml


def test_domain_rand_yaml_loads():
    root = Path(__file__).resolve().parents[1]
    cfg = root / "configs" / "domain_rand.yaml"
    data = yaml.safe_load(cfg.read_text(encoding="utf-8"))
    assert data["schema_version"] == 1
    assert "mass_scale" in data


def test_joint_space_scenario_yaml_loads():
    root = Path(__file__).resolve().parents[1]
    cfg = root / "configs" / "scenarios" / "joint_space_smoke.yaml"
    data = yaml.safe_load(cfg.read_text(encoding="utf-8"))
    assert data["schema_version"] == 1
    assert data["name"] == "joint_space_smoke"
    assert [command["primitive"] for command in data["commands"]] == [
        "home",
        "move_joint",
        "hold",
        "oscillate",
    ]
