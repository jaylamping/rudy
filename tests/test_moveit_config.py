# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Static checks for Rudy MoveIt scaffold."""
from __future__ import annotations

import xml.etree.ElementTree as ET
from pathlib import Path

import pytest
import yaml

REPO_ROOT = Path(__file__).resolve().parents[1]
MOVEIT_CONFIG = REPO_ROOT / "ros" / "src" / "moveit_config"

RIGHT_ARM_JOINTS = [
    "r_shoulder_pitch_joint",
    "r_shoulder_roll_joint",
    "r_upper_arm_yaw_joint",
    "r_elbow_pitch_joint",
    "r_lower_arm_yaw_joint",
]


def test_srdf_right_arm_chain_targets_arm_tip() -> None:
    srdf = MOVEIT_CONFIG / "config" / "rudy.srdf.xacro"
    root = ET.parse(srdf).getroot()

    group = root.find("./group[@name='right_arm']")
    assert group is not None
    chain = group.find("chain")
    assert chain is not None
    assert chain.attrib["base_link"] == "torso_upper_link"
    assert chain.attrib["tip_link"] == "r_arm_tip"


def test_position_only_ik_is_enabled_for_five_dof_arm() -> None:
    with (MOVEIT_CONFIG / "config" / "kinematics.yaml").open() as f:
        cfg = yaml.safe_load(f)

    right_arm = cfg["right_arm"]
    assert right_arm["kinematics_solver"] == "kdl_kinematics_plugin/KDLKinematicsPlugin"
    assert right_arm["position_only_ik"] is True


def test_moveit_joint_limits_cover_right_arm_joints() -> None:
    with (MOVEIT_CONFIG / "config" / "joint_limits.yaml").open() as f:
        cfg = yaml.safe_load(f)

    limits = cfg["joint_limits"]
    assert set(RIGHT_ARM_JOINTS) <= set(limits)
    for joint in RIGHT_ARM_JOINTS:
        assert limits[joint]["has_velocity_limits"] is True
        assert limits[joint]["max_velocity"] > 0.0


def test_fake_controller_matches_right_arm_joints() -> None:
    with (MOVEIT_CONFIG / "config" / "moveit_controllers.yaml").open() as f:
        cfg = yaml.safe_load(f)

    controller = cfg["moveit_fake_controller_manager"]["right_arm_controller"]
    assert controller["joints"] == RIGHT_ARM_JOINTS


def test_ros2_controller_matches_right_arm_joints() -> None:
    with (MOVEIT_CONFIG / "config" / "ros2_controllers.yaml").open() as f:
        cfg = yaml.safe_load(f)

    controller = cfg["right_arm_controller"]["ros__parameters"]
    assert controller["joints"] == RIGHT_ARM_JOINTS
    assert controller["command_interfaces"] == ["position"]
    assert controller["state_interfaces"] == ["position", "velocity"]


def test_moveit_package_declares_runtime_dependencies() -> None:
    package_xml = ET.parse(MOVEIT_CONFIG / "package.xml").getroot()
    deps = {el.text for el in package_xml.findall("exec_depend")}
    assert {
        "description",
        "moveit_ros_move_group",
        "moveit_kinematics",
        "moveit_planners_ompl",
        "xacro",
    } <= deps
