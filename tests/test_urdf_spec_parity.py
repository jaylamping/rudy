# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Parity checks between URDF and actuator spec (gold standard tests)."""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
XACRO = REPO_ROOT / "ros" / "src" / "description" / "urdf" / "robot.urdf.xacro"


@pytest.fixture(scope="module")
def expanded_urdf() -> str:
    try:
        out = subprocess.check_output(["xacro", str(XACRO)], text=True)
    except FileNotFoundError:
        pytest.skip("xacro not installed")
    except subprocess.CalledProcessError as e:
        pytest.fail(f"xacro failed: {e}")
    return out


def test_revolute_joint_limits_have_matching_effort_velocity(expanded_urdf: str) -> None:
    """Every revolute joint <limit> must carry effort=60 / velocity=50 (RS03 caps)."""
    # xacro can shuffle attribute order on expansion (especially after macro
    # substitution), so assert per-attribute on every revolute limit element
    # rather than pinning a literal attribute order in a regex.
    import xml.etree.ElementTree as ET

    root = ET.fromstring(expanded_urdf)
    revolute_count = 0
    for joint in root.findall("joint"):
        if joint.get("type") != "revolute":
            continue
        revolute_count += 1
        lim = joint.find("limit")
        assert lim is not None, f"joint {joint.get('name')!r} missing <limit>"
        assert float(lim.attrib["effort"]) == pytest.approx(60.0)
        assert float(lim.attrib["velocity"]) == pytest.approx(50.0)
    assert revolute_count > 0, "expected at least one revolute joint in URDF"


def test_soft_limits_inside_hard_limits(expanded_urdf: str) -> None:
    """Reuse validate_urdf soft-limit logic via XML parse."""
    import xml.etree.ElementTree as ET

    root = ET.fromstring(expanded_urdf)
    for joint in root.findall("joint"):
        if joint.get("type") != "revolute":
            continue
        lim = joint.find("limit")
        safe = joint.find("safety_controller")
        assert lim is not None and safe is not None
        lo = float(lim.attrib["lower"])
        hi = float(lim.attrib["upper"])
        slo = float(safe.attrib["soft_lower_limit"])
        shi = float(safe.attrib["soft_upper_limit"])
        assert lo < slo < hi
        assert lo < shi < hi


def test_validate_urdf_script_smoke() -> None:
    script = REPO_ROOT / "scripts" / "validate_urdf.py"
    rc = subprocess.call([sys.executable, str(script), "--xacro", str(XACRO)])
    assert rc == 0
