# Copyright 2026 Murphy contributors
# SPDX-License-Identifier: Apache-2.0

"""Parity checks between URDF and actuator spec (gold standard tests)."""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
XACRO = REPO_ROOT / "src" / "murphy_description" / "urdf" / "murphy.urdf.xacro"


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
    """Every revolute joint limit effort/velocity match RS03 caps from xacro properties."""
    # Extract first revolute limit line as reference
    m = re.search(
        r'<limit\s+lower="[^"]+"\s+upper="[^"]+"\s+effort="([^"]+)"\s+velocity="([^"]+)"',
        expanded_urdf,
    )
    assert m, "No revolute <limit> found in expanded URDF"
    effort, velocity = float(m.group(1)), float(m.group(2))
    assert effort == pytest.approx(60.0)
    assert velocity == pytest.approx(50.0)


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
