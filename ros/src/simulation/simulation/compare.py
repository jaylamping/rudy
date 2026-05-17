# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Build ADR-0009 sim-to-sim JSON reports from Rudy-level traces."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

from simulation.schema import JsonMap, SimMetrics, SimReport, SimScenario, SimState, load_scenario


STOP_STATES = {"fault", "stopped", "timeout", "validation_failed", "e_stop"}


def load_state_trace(path: str | Path) -> tuple[SimState, ...]:
    """Load a trace JSON file.

    Accepted root forms are either a list of states or {"states": [...]}.
    """

    raw = json.loads(Path(path).read_text(encoding="utf-8"))
    states = raw.get("states") if isinstance(raw, dict) else raw
    if not isinstance(states, list) or not states:
        raise ValueError(f"{path} must contain a non-empty state list")
    return tuple(SimState.from_mapping(state) for state in states)


def _matching_pairs(
    reference: tuple[SimState, ...],
    candidate: tuple[SimState, ...],
) -> list[tuple[SimState, SimState, str]]:
    pairs: list[tuple[SimState, SimState, str]] = []
    for left, right in zip(reference, candidate, strict=False):
        for joint in sorted(set(left.joints) & set(right.joints)):
            pairs.append((left, right, joint))
    if not pairs:
        raise ValueError("traces have no shared joint samples")
    return pairs


def _max_acceleration(states: tuple[SimState, ...]) -> float:
    max_acceleration = 0.0
    for previous, current in zip(states, states[1:], strict=False):
        dt_s = current.time_s - previous.time_s
        if dt_s <= 0.0:
            continue
        for joint in set(previous.joints) & set(current.joints):
            acceleration = abs(
                current.joints[joint].velocity_rad_s - previous.joints[joint].velocity_rad_s
            ) / dt_s
            max_acceleration = max(max_acceleration, acceleration)
    return max_acceleration


def compute_metrics(
    reference: tuple[SimState, ...],
    candidate: tuple[SimState, ...],
) -> SimMetrics:
    pairs = _matching_pairs(reference, candidate)
    squared_error_sum = 0.0
    max_abs_error = 0.0
    max_velocity = 0.0
    max_effort = 0.0
    margins: list[float] = []
    contact_count = 0
    stop_count = 0
    failure_count = 0

    for states in (reference, candidate):
        for state in states:
            contact_count += len(state.contacts)
            failure_count += len(state.validation_failures)
            if state.runtime_state in STOP_STATES:
                stop_count += 1
            for joint_state in state.joints.values():
                max_velocity = max(max_velocity, abs(joint_state.velocity_rad_s))
                max_effort = max(max_effort, abs(joint_state.effort_nm))
                if joint_state.soft_limit_margin_rad is not None:
                    margins.append(joint_state.soft_limit_margin_rad)

    for left, right, joint in pairs:
        error = left.joints[joint].position_rad - right.joints[joint].position_rad
        squared_error_sum += error * error
        max_abs_error = max(max_abs_error, abs(error))

    return SimMetrics(
        joint_position_rms_rad=math.sqrt(squared_error_sum / len(pairs)),
        joint_position_max_abs_rad=max_abs_error,
        max_velocity_rad_s=max_velocity,
        max_acceleration_rad_s2=max(_max_acceleration(reference), _max_acceleration(candidate)),
        torque_abs_max_nm=max_effort,
        min_soft_limit_margin_rad=min(margins) if margins else None,
        contact_event_count=contact_count,
        runtime_stop_count=stop_count,
        validation_failure_count=failure_count,
    )


def passes_thresholds(metrics: SimMetrics, thresholds: dict[str, float]) -> bool:
    values = metrics.to_mapping()
    for name, threshold in thresholds.items():
        value = values.get(name)
        if value is None:
            continue
        if name == "min_soft_limit_margin_rad":
            if value < threshold:
                return False
            continue
        if value > threshold:
            return False
    return True


def build_report(
    scenario: SimScenario,
    reference: tuple[SimState, ...],
    candidate: tuple[SimState, ...],
    simulator_versions: dict[str, str],
    model_hashes: dict[str, str],
) -> SimReport:
    metrics = compute_metrics(reference, candidate)
    return SimReport(
        scenario_name=scenario.name,
        seed=scenario.seed,
        simulator_versions=simulator_versions,
        model_hashes=model_hashes,
        metrics=metrics,
        thresholds=scenario.thresholds,
        passed=passes_thresholds(metrics, scenario.thresholds),
    )


def _parse_key_value(values: list[str]) -> dict[str, str]:
    parsed: dict[str, str] = {}
    for value in values:
        if "=" not in value:
            raise ValueError(f"expected KEY=VALUE, got {value!r}")
        key, parsed_value = value.split("=", 1)
        parsed[key] = parsed_value
    return parsed


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scenario", required=True, help="Scenario YAML path")
    parser.add_argument("--isaac-trace", required=True, help="Reference Isaac trace JSON path")
    parser.add_argument("--mujoco-trace", required=True, help="Candidate MuJoCo trace JSON path")
    parser.add_argument("--out", required=True, help="Output report JSON path")
    parser.add_argument(
        "--sim-version",
        action="append",
        default=[],
        metavar="NAME=VERSION",
        help="Simulator version metadata; repeatable",
    )
    parser.add_argument(
        "--model-hash",
        action="append",
        default=[],
        metavar="NAME=HASH",
        help="Model hash metadata; repeatable",
    )
    return parser


def main(argv: list[str] | None = None) -> None:
    args = _parser().parse_args(argv)
    report = build_report(
        scenario=load_scenario(args.scenario),
        reference=load_state_trace(args.isaac_trace),
        candidate=load_state_trace(args.mujoco_trace),
        simulator_versions=_parse_key_value(args.sim_version),
        model_hashes=_parse_key_value(args.model_hash),
    )
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(json.dumps(report.to_mapping(), indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"wrote {out_path}: passed={report.passed}")


if __name__ == "__main__":
    main()
