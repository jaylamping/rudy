# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Validate config/actuators/robstride_rs03.yaml structure and numeric sanity."""
from __future__ import annotations

from pathlib import Path

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
SPEC_PATH = REPO_ROOT / "config" / "actuators" / "robstride_rs03.yaml"


def _load() -> dict:
    assert SPEC_PATH.is_file(), f"Missing actuator spec: {SPEC_PATH}"
    with SPEC_PATH.open("r", encoding="utf-8") as f:
        return yaml.safe_load(f)


def test_schema_version_and_model() -> None:
    data = _load()
    assert data.get("schema_version") == 1
    assert data.get("actuator_model") == "RS03"


def test_protocol_fields() -> None:
    proto = _load()["protocol"]
    assert proto["bitrate_bps"] == 1_000_000
    assert proto["frame_format"] == "extended_29bit"
    assert proto["data_length"] == 8


def test_limits_positive_and_ordered() -> None:
    lim = _load()["limits"]
    assert lim["effort_max_nm"] > 0
    assert lim["velocity_max_rad_s"] > 0
    assert lim["position_min_rad"] < lim["position_max_rad"]


def test_control_modes_ranges() -> None:
    modes = _load()["control_modes"]
    mit = modes["mit"]
    assert mit["kp_range"][0] <= mit["kp_range"][1]
    assert mit["kd_range"][0] <= mit["kd_range"][1]


def test_dynamics_match_urdf_defaults() -> None:
    """Gold standard: URDF robot.urdf.xacro uses same damping/friction as spec."""
    dyn = _load()["dynamics"]
    assert dyn["joint_damping"] == pytest.approx(0.3)
    assert dyn["joint_friction"] == pytest.approx(0.08)
    assert dyn["safety_controller"]["k_position"] == pytest.approx(15.0)
    assert dyn["safety_controller"]["k_velocity"] == pytest.approx(10.0)


def test_effort_velocity_match_urdf_caps() -> None:
    """Gold standard: URDF effort_cap / velocity_cap align with RS03 spec."""
    lim = _load()["limits"]
    xacro = REPO_ROOT / "src" / "description" / "urdf" / "robot.urdf.xacro"
    text = xacro.read_text(encoding="utf-8")
    assert lim["effort_max_nm"] == pytest.approx(60.0)
    assert lim["velocity_max_rad_s"] == pytest.approx(50.0)
    assert 'name="effort_cap" value="60.0"' in text
    assert 'name="velocity_cap" value="50.0"' in text
