# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Parity checks between URDF and actuator spec (gold standard tests)."""
from __future__ import annotations

import subprocess
import sys
import shutil
from pathlib import Path

import pytest

REPO_ROOT = Path(__file__).resolve().parents[1]
XACRO = REPO_ROOT / "ros" / "src" / "description" / "urdf" / "robot.urdf.xacro"


def xacro_command() -> list[str]:
    exe = shutil.which("xacro")
    if exe is not None:
        return [exe]
    return []


def expand_xacro(path: Path) -> str:
    cmd = xacro_command()
    if cmd:
        return subprocess.check_output([*cmd, str(path)], text=True)

    try:
        import xacro
    except ImportError:
        pytest.skip("xacro not installed")

    doc = xacro.process_file(str(path))
    return doc.toprettyxml(indent="  ")


@pytest.fixture(scope="module")
def expanded_urdf() -> str:
    try:
        out = expand_xacro(XACRO)
    except (FileNotFoundError, ModuleNotFoundError):
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


def test_right_arm_tip_and_five_dof_chain_exist(expanded_urdf: str) -> None:
    """Right arm exposes the 5-DOF chain and nub tip frame used by MoveIt."""
    import xml.etree.ElementTree as ET

    root = ET.fromstring(expanded_urdf)
    joints = {joint.get("name"): joint for joint in root.findall("joint")}
    links = {link.get("name") for link in root.findall("link")}

    right_arm = [
        "r_shoulder_pitch_joint",
        "r_shoulder_roll_joint",
        "r_upper_arm_yaw_joint",
        "r_elbow_pitch_joint",
        "r_lower_arm_yaw_joint",
    ]
    for name in right_arm:
        assert name in joints, f"missing right-arm joint {name}"
        assert joints[name].get("type") == "revolute"

    assert "r_arm_tip" in links
    assert "r_lower_arm_yaw_to_tip" in joints
    assert joints["r_lower_arm_yaw_to_tip"].get("type") == "fixed"


def test_ros2_control_exports_right_arm_position_interfaces(expanded_urdf: str) -> None:
    """The URDF advertises position command/state interfaces for MoveIt/control."""
    import xml.etree.ElementTree as ET

    root = ET.fromstring(expanded_urdf)
    ros2_control = root.find("ros2_control")
    assert ros2_control is not None, "missing <ros2_control> block"

    joints = {joint.get("name"): joint for joint in ros2_control.findall("joint")}
    for name in [
        "r_shoulder_pitch_joint",
        "r_shoulder_roll_joint",
        "r_upper_arm_yaw_joint",
        "r_elbow_pitch_joint",
        "r_lower_arm_yaw_joint",
    ]:
        joint = joints.get(name)
        assert joint is not None, f"missing ros2_control joint {name}"
        commands = {el.get("name") for el in joint.findall("command_interface")}
        states = {el.get("name") for el in joint.findall("state_interface")}
        assert "position" in commands
        assert {"position", "velocity"} <= states


def test_validate_urdf_script_smoke() -> None:
    script = REPO_ROOT / "scripts" / "validate_urdf.py"
    rc = subprocess.call([sys.executable, str(script), "--xacro", str(XACRO)])
    assert rc == 0
