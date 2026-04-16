#!/usr/bin/env python3
"""Validate Murphy xacro/URDF without a full ROS install (uses xacro + urdfdom-py + ElementTree)."""
from __future__ import annotations

import argparse
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


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--xacro",
        type=Path,
        default=Path(__file__).resolve().parents[1]
        / "src/murphy_description/urdf/murphy.urdf.xacro",
    )
    args = parser.parse_args()

    if not args.xacro.is_file():
        print(f"Missing xacro file: {args.xacro}", file=sys.stderr)
        return 1

    with tempfile.NamedTemporaryFile(mode="w", suffix=".urdf", delete=False) as tmp:
        urdf_path = Path(tmp.name)

    try:
        try:
            subprocess.run(
                ["xacro", str(args.xacro)],
                check=True,
                stdout=urdf_path.open("w"),
                stderr=sys.stderr,
            )
        except FileNotFoundError:
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
        return 0
    finally:
        urdf_path.unlink(missing_ok=True)


if __name__ == "__main__":
    raise SystemExit(main())
