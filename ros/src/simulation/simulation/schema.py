# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Simulator-neutral Rudy command/state/report contract.

These types intentionally avoid Isaac Lab or MuJoCo imports. Simulator adapters
translate at their own boundary; comparisons happen here.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

import yaml


JsonMap = dict[str, Any]


def _require_mapping(value: Any, context: str) -> JsonMap:
    if not isinstance(value, dict):
        raise ValueError(f"{context} must be a mapping")
    return value


def _require_number(value: Any, context: str) -> float:
    if not isinstance(value, int | float):
        raise ValueError(f"{context} must be a number")
    return float(value)


@dataclass(frozen=True)
class JointCommandTarget:
    """Rudy-level joint target consumed by every simulator adapter."""

    position_rad: float
    velocity_rad_s: float | None = None
    effort_nm: float | None = None

    @classmethod
    def from_mapping(cls, data: JsonMap) -> "JointCommandTarget":
        position_rad = _require_number(data.get("position_rad"), "position_rad")
        velocity = data.get("velocity_rad_s")
        effort = data.get("effort_nm")
        return cls(
            position_rad=position_rad,
            velocity_rad_s=None if velocity is None else _require_number(velocity, "velocity_rad_s"),
            effort_nm=None if effort is None else _require_number(effort, "effort_nm"),
        )

    def to_mapping(self) -> JsonMap:
        data: JsonMap = {"position_rad": self.position_rad}
        if self.velocity_rad_s is not None:
            data["velocity_rad_s"] = self.velocity_rad_s
        if self.effort_nm is not None:
            data["effort_nm"] = self.effort_nm
        return data


@dataclass(frozen=True)
class SimCommand:
    """Primitive invocation at a scenario timestamp."""

    name: str
    primitive: str
    at_s: float
    duration_s: float
    joint_targets: dict[str, JointCommandTarget] = field(default_factory=dict)

    @classmethod
    def from_mapping(cls, data: JsonMap) -> "SimCommand":
        targets = _require_mapping(data.get("joint_targets", {}), "joint_targets")
        return cls(
            name=str(data.get("name", data.get("primitive", ""))),
            primitive=str(data["primitive"]),
            at_s=_require_number(data.get("at_s"), "at_s"),
            duration_s=_require_number(data.get("duration_s"), "duration_s"),
            joint_targets={
                str(joint): JointCommandTarget.from_mapping(_require_mapping(target, f"{joint} target"))
                for joint, target in targets.items()
            },
        )

    def to_mapping(self) -> JsonMap:
        return {
            "name": self.name,
            "primitive": self.primitive,
            "at_s": self.at_s,
            "duration_s": self.duration_s,
            "joint_targets": {
                joint: target.to_mapping() for joint, target in sorted(self.joint_targets.items())
            },
        }


@dataclass(frozen=True)
class SimScenario:
    """Versioned scenario catalog entry from YAML."""

    name: str
    seed: int
    dt_s: float
    duration_s: float
    commands: tuple[SimCommand, ...]
    model: JsonMap = field(default_factory=dict)
    thresholds: dict[str, float] = field(default_factory=dict)
    schema_version: int = 1

    @classmethod
    def from_mapping(cls, data: JsonMap) -> "SimScenario":
        commands = data.get("commands", [])
        if not isinstance(commands, list) or not commands:
            raise ValueError("commands must be a non-empty list")
        thresholds = _require_mapping(data.get("thresholds", {}), "thresholds")
        return cls(
            schema_version=int(data.get("schema_version", 1)),
            name=str(data["name"]),
            seed=int(data["seed"]),
            dt_s=_require_number(data.get("dt_s"), "dt_s"),
            duration_s=_require_number(data.get("duration_s"), "duration_s"),
            model=dict(_require_mapping(data.get("model", {}), "model")),
            thresholds={str(key): _require_number(value, f"thresholds.{key}") for key, value in thresholds.items()},
            commands=tuple(
                SimCommand.from_mapping(_require_mapping(command, "command")) for command in commands
            ),
        )

    def to_mapping(self) -> JsonMap:
        return {
            "schema_version": self.schema_version,
            "name": self.name,
            "seed": self.seed,
            "dt_s": self.dt_s,
            "duration_s": self.duration_s,
            "model": self.model,
            "thresholds": dict(sorted(self.thresholds.items())),
            "commands": [command.to_mapping() for command in self.commands],
        }


@dataclass(frozen=True)
class JointState:
    position_rad: float
    velocity_rad_s: float = 0.0
    effort_nm: float = 0.0
    soft_limit_margin_rad: float | None = None

    @classmethod
    def from_mapping(cls, data: JsonMap) -> "JointState":
        margin = data.get("soft_limit_margin_rad")
        return cls(
            position_rad=_require_number(data.get("position_rad"), "position_rad"),
            velocity_rad_s=_require_number(data.get("velocity_rad_s", 0.0), "velocity_rad_s"),
            effort_nm=_require_number(data.get("effort_nm", 0.0), "effort_nm"),
            soft_limit_margin_rad=None
            if margin is None
            else _require_number(margin, "soft_limit_margin_rad"),
        )

    def to_mapping(self) -> JsonMap:
        data: JsonMap = {
            "position_rad": self.position_rad,
            "velocity_rad_s": self.velocity_rad_s,
            "effort_nm": self.effort_nm,
        }
        if self.soft_limit_margin_rad is not None:
            data["soft_limit_margin_rad"] = self.soft_limit_margin_rad
        return data


@dataclass(frozen=True)
class SimState:
    """One timestamp of simulator output at the Rudy contract boundary."""

    time_s: float
    joints: dict[str, JointState]
    runtime_state: str = "running"
    contacts: tuple[JsonMap, ...] = ()
    validation_failures: tuple[str, ...] = ()

    @classmethod
    def from_mapping(cls, data: JsonMap) -> "SimState":
        joints = _require_mapping(data.get("joints"), "joints")
        contacts = data.get("contacts", [])
        failures = data.get("validation_failures", [])
        if not isinstance(contacts, list):
            raise ValueError("contacts must be a list")
        if not isinstance(failures, list):
            raise ValueError("validation_failures must be a list")
        return cls(
            time_s=_require_number(data.get("time_s"), "time_s"),
            runtime_state=str(data.get("runtime_state", "running")),
            joints={
                str(joint): JointState.from_mapping(_require_mapping(state, f"{joint} state"))
                for joint, state in joints.items()
            },
            contacts=tuple(_require_mapping(contact, "contact") for contact in contacts),
            validation_failures=tuple(str(failure) for failure in failures),
        )

    def to_mapping(self) -> JsonMap:
        return {
            "time_s": self.time_s,
            "runtime_state": self.runtime_state,
            "joints": {joint: state.to_mapping() for joint, state in sorted(self.joints.items())},
            "contacts": list(self.contacts),
            "validation_failures": list(self.validation_failures),
        }


@dataclass(frozen=True)
class SimMetrics:
    joint_position_rms_rad: float
    joint_position_max_abs_rad: float
    max_velocity_rad_s: float
    max_acceleration_rad_s2: float
    torque_abs_max_nm: float
    min_soft_limit_margin_rad: float | None
    contact_event_count: int
    runtime_stop_count: int
    validation_failure_count: int

    def to_mapping(self) -> JsonMap:
        return {
            "joint_position_rms_rad": self.joint_position_rms_rad,
            "joint_position_max_abs_rad": self.joint_position_max_abs_rad,
            "max_velocity_rad_s": self.max_velocity_rad_s,
            "max_acceleration_rad_s2": self.max_acceleration_rad_s2,
            "torque_abs_max_nm": self.torque_abs_max_nm,
            "min_soft_limit_margin_rad": self.min_soft_limit_margin_rad,
            "contact_event_count": self.contact_event_count,
            "runtime_stop_count": self.runtime_stop_count,
            "validation_failure_count": self.validation_failure_count,
        }


@dataclass(frozen=True)
class SimReport:
    scenario_name: str
    seed: int
    simulator_versions: dict[str, str]
    model_hashes: dict[str, str]
    metrics: SimMetrics
    thresholds: dict[str, float]
    passed: bool

    def to_mapping(self) -> JsonMap:
        return {
            "scenario_name": self.scenario_name,
            "seed": self.seed,
            "simulator_versions": dict(sorted(self.simulator_versions.items())),
            "model_hashes": dict(sorted(self.model_hashes.items())),
            "metrics": self.metrics.to_mapping(),
            "thresholds": dict(sorted(self.thresholds.items())),
            "passed": self.passed,
        }


def load_scenario(path: str | Path) -> SimScenario:
    data = yaml.safe_load(Path(path).read_text(encoding="utf-8"))
    return SimScenario.from_mapping(_require_mapping(data, "scenario"))
