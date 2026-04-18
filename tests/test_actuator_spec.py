# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Validate config/actuators/robstride_rs03.yaml structure and numeric sanity.

The spec file uses schema_version 2 (current as of 2026-04-17). Per ADR-0004
+ ADR-0002, the spec is now organised around `protocol`, `hardware`,
`firmware_limits`, and `observables` rather than the legacy v1
`limits` / `dynamics` / `control_modes` shape these tests originally pinned.
"""
from __future__ import annotations

from pathlib import Path

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
SPEC_PATH = REPO_ROOT / "config" / "actuators" / "robstride_rs03.yaml"
URDF_XACRO_PATH = REPO_ROOT / "ros" / "src" / "description" / "urdf" / "robot.urdf.xacro"


def _load() -> dict:
    assert SPEC_PATH.is_file(), f"Missing actuator spec: {SPEC_PATH}"
    with SPEC_PATH.open("r", encoding="utf-8") as f:
        return yaml.safe_load(f)


def test_schema_version_and_model() -> None:
    data = _load()
    assert data.get("schema_version") == 2
    assert data.get("actuator_model") == "RS03"


def test_protocol_fields() -> None:
    proto = _load()["protocol"]
    assert proto["bitrate_bps"] == 1_000_000
    assert proto["frame_format"] == "extended_29bit"
    assert proto["data_length"] == 8


def test_op_control_scaling_ranges_are_ordered() -> None:
    """MIT-style operation-control scaling ranges must have lo <= hi per channel."""
    scaling = _load()["op_control_scaling"]
    for channel in ("position", "velocity", "kp", "kd", "torque_ff"):
        ch = scaling[channel]
        lo, hi = ch["range"]
        assert lo <= hi, f"op_control_scaling.{channel}.range out of order: [{lo}, {hi}]"


def test_firmware_limits_have_descriptors() -> None:
    """Every firmware limit entry must have an index + type so the daemon
    catalog loader can serialize it without falling back to silent Nones."""
    limits = _load()["firmware_limits"]
    expected = {
        "limit_torque",
        "limit_spd",
        "limit_cur",
        "can_timeout",
        "run_mode",
        "zero_sta",
        "damper",
        "add_offset",
    }
    assert expected.issubset(limits.keys()), f"Missing firmware_limits: {expected - limits.keys()}"
    for name, entry in limits.items():
        assert "index" in entry, f"firmware_limits.{name} missing index"
        assert "type" in entry, f"firmware_limits.{name} missing type"


def test_firmware_limits_hardware_ranges_are_positive_and_ordered() -> None:
    """Float firmware-limit entries must have hardware_range[0] < hardware_range[1]."""
    limits = _load()["firmware_limits"]
    for name in ("limit_torque", "limit_spd", "limit_cur"):
        rng = limits[name].get("hardware_range")
        assert rng is not None, f"firmware_limits.{name} should have hardware_range"
        lo, hi = rng
        assert 0.0 <= lo < hi, f"firmware_limits.{name} range out of order: [{lo}, {hi}]"


def test_hardware_caps_match_urdf_caps() -> None:
    """Gold standard: URDF effort_cap / velocity_cap align with RS03 hardware spec.

    URDF picks the *peak* hardware envelope so per-joint limits can be
    arbitrary subsets without invalidating the cap. The xacro property names
    are pinned so any mass refactor hits this assertion immediately.
    """
    hw = _load()["hardware"]
    assert hw["torque_peak_nm"] == pytest.approx(60.0)
    assert URDF_XACRO_PATH.is_file(), f"Missing URDF xacro: {URDF_XACRO_PATH}"
    text = URDF_XACRO_PATH.read_text(encoding="utf-8")
    assert 'name="effort_cap" value="60.0"' in text
    assert 'name="velocity_cap" value="50.0"' in text


def test_commissioning_defaults_inside_hardware_ranges() -> None:
    """Per-joint commissioning starter values must be a subset of the
    motor's firmware envelope so writing them via PUT /api/.../params/:name
    never hits the daemon's range-check rejection path."""
    data = _load()
    defaults = data["commissioning_defaults"]
    limits = data["firmware_limits"]
    for default_key, limit_key in (
        ("limit_torque_nm", "limit_torque"),
        ("limit_spd_rad_s", "limit_spd"),
        ("limit_cur_a", "limit_cur"),
    ):
        v = defaults[default_key]
        lo, hi = limits[limit_key]["hardware_range"]
        assert lo <= v <= hi, (
            f"commissioning_defaults.{default_key}={v} outside "
            f"firmware_limits.{limit_key}.hardware_range=[{lo}, {hi}]"
        )


def test_thermal_thresholds_are_positive_and_ordered() -> None:
    th = _load()["thermal"]
    assert th["derating_start_c"] < th["max_winding_temp_c"]
