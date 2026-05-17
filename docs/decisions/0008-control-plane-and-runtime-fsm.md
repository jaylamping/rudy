# ADR 0008: Control plane and runtime FSM (2026-05)

## Status

Accepted

## Context

Rudy has three useful but easy-to-confuse motion layers:

- `cortex` owns the live robot runtime, safety gates, audit log, operator lock, and SocketCAN.
- ROS 2 / MoveIt provide robot-model, IK, collision, visualization, and sim/planning tools.
- The future LLM layer should translate human intent into Rudy motion primitives.

The important boundary is authority. ROS / MoveIt may compute candidate joint targets or trajectories, but they must not become the robot's brain or the hardware authority. `cortex` remains the place where mode, safety, validation, and execution converge.

Unitree's public stack uses a similar separation of concerns at a larger scale: SDK / DDS messages define robot command-state boundaries, ROS 2 is a compatibility and tooling surface, and deployment controllers run explicit runtime states before going to hardware. Rudy should borrow the boundary pattern, not copy Unitree's exact repo layout.

## Decision

### D1. `cortex` is the robot runtime authority

The control plane is:

```text
operator / voice / LLM / scripts
    -> Rudy primitive graph
    -> optional kinematics / collision oracle
    -> cortex validation + runtime FSM
    -> CAN worker
    -> RobStride actuators
```

`cortex` owns:

- runtime mode and state transitions,
- primitive validation,
- travel-limit and freshness checks,
- operator lock and audit,
- stop / estop behavior,
- final command emission to CAN.

ROS / MoveIt can produce candidate `JointTrajectory` or IK output, but `cortex` must validate and execute it through the same gates as browser or script commands.

### D2. The LLM emits Rudy primitives, not ROS commands

The LLM integration is a consumer of the primitive API from ADR-0006. It receives the primitive catalog and returns a primitive invocation such as:

```json
{
  "primitive": "sequence",
  "params": {
    "steps": [
      { "primitive": "move_joint", "params": { "joint": "r_shoulder_pitch", "target_rad": 0.4 } },
      { "primitive": "oscillate", "params": { "joint": "r_elbow_pitch", "amplitude_rad": 0.2, "cycles": 3 } }
    ]
  }
}
```

It does not publish ROS topics directly, open CAN, or bypass `cortex`.

### D3. ROS / MoveIt are geometry and integration tools

Use ROS / MoveIt for:

- URDF / TF / robot-state visualization,
- IK and collision checks,
- planning-scene tooling,
- fake-controller demos,
- future `JointTrajectory` bridge tests,
- simulator and rosbag integration.

Do not require ROS on the Pi until a concrete bridge needs it. On hardware, ROS is a client or oracle around `cortex`, not a competing control owner.

### D4. Add an explicit runtime FSM in `cortex`

The daemon should expose one runtime mode machine instead of letting API endpoint names imply state. Initial states:

| State | Meaning |
| --- | --- |
| `Passive` | daemon alive, no motor motion allowed except discovery / read-only telemetry |
| `Commissioning` | operator is assigning IDs, zero offsets, travel limits, verified flags |
| `Ready` | verified hardware, fresh telemetry, commands may be accepted |
| `Homing` | controlled home / auto-home in progress |
| `Holding` | motors enabled in position hold, no active primitive |
| `ManualJog` | dead-man jog active |
| `PrimitiveRunning` | non-interactive primitive or primitive graph active |
| `Faulted` | recoverable fault; motion blocked until recovery action |
| `EStop` | global stop asserted; all motion blocked until explicit reset |

Transitions are driven by events: telemetry freshness, boot-state classification, operator commands, lock ownership, CAN faults, current trips, estop, and primitive lifecycle.

### D5. Runtime state is an API and telemetry concept

Expose the current runtime state through:

- REST health / status,
- WebTransport telemetry,
- audit events on state transitions,
- UI status banner.

The operator should be able to answer: "What state is Rudy in, why, and what action can move it forward?"

## Migration plan

1. **Docs PR.** Land this ADR and update architecture diagrams.
2. **State model PR.** Add `RuntimeState`, transition reasons, and read-only API / telemetry exposure. No behavior changes.
3. **Gate PR.** Route motion endpoints through the FSM gate; preserve current successful paths.
4. **Primitive PRs.** Implement ADR-0006 so `ManualJog` and `PrimitiveRunning` share the same runtime state surface.
5. **ROS bridge PR.** When ready, bridge MoveIt output into `cortex` as a candidate trajectory that must pass the same validation gates.
6. **LLM ADR.** Define speech / text pipeline, prompt contract, hallucination handling, and primitive approval policy.

## Consequences

### Positive

- The robot has one authority for "can this move now?"
- LLM integration stays small: intent in, primitive graph out.
- ROS remains valuable without pulling hardware authority away from `cortex`.
- UI, logs, and runbooks gain a common state vocabulary.

### Negative / trade-offs

- More explicit state modeling in `cortex` before more demos can be added.
- Existing endpoint-local checks must be audited and moved behind shared gates.
- ROS bridge work becomes stricter: it cannot execute a plan merely because MoveIt found one.

## See also

- [ADR-0004: Operator console](0004-operator-console.md)
- [ADR-0006: Motion primitives as the unit of composition](0006-motion-primitives-as-unit-of-composition.md)
- [ADR-0009: Simulation ladder and sim-to-sim](0009-simulation-ladder-and-sim-to-sim.md)
- [docs/architecture.md](../architecture.md)
