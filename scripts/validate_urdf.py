#!/usr/bin/env python3
"""Validate Rudy xacro/URDF without a full ROS install (uses xacro + urdfdom-py + ElementTree)."""
from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET
from pathlib import Path


def soft_limits_valid(joint_el: ET.Element) -> tuple[bool, str]:
    lim = joint_el.find("limit")
    safe = joint_el.find("safety_controller")
    if lim is None or safe is None:
        return True, ""
    lo = float(lim.attrib["lower"])
    hi = float(lim.attrib["upper"])
    slo = float(safe.attrib["soft_lower_limit"])
    shi = float(safe.attrib["soft_upper_limit"])
    if not (lo < slo < hi):
        return False, f"{joint_el.attrib['name']}: soft_lower {slo} not in ({lo}, {hi})"
    if not (lo < shi < hi):
        return False, f"{joint_el.attrib['name']}: soft_upper {shi} not in ({lo}, {hi})"
    return True, ""


def required_frames_valid(root: ET.Element) -> tuple[bool, str]:
    links = {link.attrib.get("name") for link in root.findall("link")}
    joints = {joint.attrib.get("name"): joint for joint in root.findall("joint")}
    required_links = {"l_arm_tip", "r_arm_tip"}
    missing_links = sorted(required_links - links)
    if missing_links:
        return False, f"missing required tip link(s): {', '.join(missing_links)}"

    right_arm_joints = {
        "r_shoulder_pitch_joint",
        "r_shoulder_roll_joint",
        "r_upper_arm_yaw_joint",
        "r_elbow_pitch_joint",
        "r_lower_arm_yaw_joint",
    }
    missing_joints = sorted(right_arm_joints - set(joints))
    if missing_joints:
        return False, f"missing right-arm joint(s): {', '.join(missing_joints)}"

    tip_joint = joints.get("r_lower_arm_yaw_to_tip")
    if tip_joint is None or tip_joint.get("type") != "fixed":
        return False, "r_arm_tip must be attached by fixed joint r_lower_arm_yaw_to_tip"

    return True, ""


def expand_xacro_to_file(xacro_path: Path, urdf_path: Path) -> bool:
    xacro_exe = shutil.which("xacro")
    if xacro_exe is not None:
        with urdf_path.open("w", encoding="utf-8") as out:
            subprocess.run(
                [xacro_exe, str(xacro_path)],
                check=True,
                stdout=out,
                stderr=sys.stderr,
            )
        return True

    try:
        import xacro
    except ImportError:
        return False

    doc = xacro.process_file(str(xacro_path))
    urdf_path.write_text(doc.toprettyxml(indent="  "), encoding="utf-8")
    return True


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--xacro",
        type=Path,
        default=Path(__file__).resolve().parents[1]
        / "ros/src/description/urdf/robot.urdf.xacro",
    )
    args = parser.parse_args()

    if not args.xacro.is_file():
        print(f"Missing xacro file: {args.xacro}", file=sys.stderr)
        return 1

    with tempfile.NamedTemporaryFile(mode="w", suffix=".urdf", delete=False) as tmp:
        urdf_path = Path(tmp.name)

    try:
        try:
            expanded = expand_xacro_to_file(args.xacro, urdf_path)
        except (FileNotFoundError, subprocess.CalledProcessError):
            expanded = False
        if not expanded:
            print(
                "xacro not found. Install with: python3 -m venv .venv && .venv/bin/pip install xacro urdfdom-py",
                file=sys.stderr,
            )
            return 1

        try:
            from urdf_parser_py.urdf import URDF
        except ImportError:
            print("urdfdom-py required: pip install urdfdom-py", file=sys.stderr)
            return 1

        robot = URDF.from_xml_file(str(urdf_path))
        print(f"OK: parsed URDF — {len(robot.links)} links, {len(robot.joints)} joints")

        tree = ET.parse(urdf_path)
        root = tree.getroot()
        for joint in root.findall("joint"):
            if joint.get("type") != "revolute":
                continue
            ok, msg = soft_limits_valid(joint)
            if not ok:
                print(f"FAIL: {msg}", file=sys.stderr)
                return 1

        print("OK: safety_controller soft limits inside hard limits (all revolute joints)")
        ok, msg = required_frames_valid(root)
        if not ok:
            print(f"FAIL: {msg}", file=sys.stderr)
            return 1
        print("OK: arm-tip frames and right-arm 5-DOF chain present")
        return 0
    finally:
        urdf_path.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(main())
