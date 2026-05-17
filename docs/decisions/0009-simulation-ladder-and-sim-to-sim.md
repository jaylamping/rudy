# ADR 0009: Simulation ladder and sim-to-sim (2026-05)

## Status

Accepted

## Context

Rudy already intends to be sim-first, with Isaac Lab scaffolding and versioned domain-randomization config. Unitree's public RL flow makes the missing middle step explicit: train in Isaac Lab, run sim-to-sim in MuJoCo, then deploy sim-to-real. Rudy should adopt that ladder before fast learned motion or Cartesian behaviors reach hardware.

Sim-to-sim is not a replacement for hardware testing. It is a filter that catches controller assumptions that only work in one physics engine, one contact model, one actuator model, or one timestep setup.

## Decision

### D1. Use a staged simulation ladder

Rudy's motion validation ladder is:

```text
unit / parity tests
    -> single-engine simulation
    -> sim-to-sim replay
    -> hardware shadow mode
    -> low-speed hardware
    -> full hardware envelope
```

The required rung depends on risk:

- **Docs, UI-only, read-only telemetry:** no sim required.
- **Slow hand-authored joint primitives:** unit tests + mock `cortex`; sim-to-sim recommended before repeated demos.
- **Cartesian primitives:** sim-to-sim required before hardware.
- **Learned policies or fast whole-limb motion:** sim-to-sim and shadow mode required before hardware.
- **New actuator/current/limit behavior:** mock tests plus at least one low-speed hardware gate.

### D2. Define a simulator-neutral command/state contract

Both Isaac and MuJoCo adapters should consume and emit the same Rudy-level concepts:

- primitive invocation,
- joint command target,
- joint state,
- actuator estimate,
- contact / collision event,
- runtime state,
- scenario seed and timing metadata.

Adapters may translate these into simulator-native action spaces internally, but comparison and reports happen at the Rudy contract boundary.

### D3. Isaac Lab is the first-class training / task environment

Keep Isaac Lab as the main task and training environment because the repo already has `ros/src/simulation/` scaffolding and config for domain randomization, contact, and actuator dynamics.

Initial work:

- replace `SimEnvStub` with a headless replay entrypoint when Isaac is available,
- keep imports lazy so CI and lightweight developer machines can still run tests without Isaac installed,
- load domain-randomization and actuator configs from versioned YAML.

### D4. MuJoCo is the first sim-to-sim cross-check

Add a MuJoCo adapter that can replay the same primitive scenarios and policy outputs as Isaac. It does not need to be feature-complete before it is useful. The first target is joint-space motion with no rich perception:

1. `home`
2. `move_joint`
3. `hold`
4. `oscillate`
5. `sequence(home -> move_joint -> hold)`

Cartesian scenarios (`reach_to`, `point_at`, `track`) come later after end-effector frames and IK are formalized.

### D5. Compare behavior with explicit metrics

A sim-to-sim report records:

- scenario name, seed, simulator versions, model hashes,
- joint position RMS and max error,
- endpoint pose error where an end-effector frame exists,
- max velocity and acceleration,
- torque / current estimate envelope,
- soft-limit margin,
- contact and collision events,
- runtime stops, timeouts, and validation failures,
- pass / fail thresholds.

Reports should be machine-readable first (JSON), then rendered for humans where useful.

### D6. Add hardware shadow mode before real execution

For risky motion classes, `cortex` should support a "shadow" execution mode: accept the primitive, run validation and runtime-state transitions, emit status/audit, but do not write CAN frames. This catches integration mistakes between LLM / primitive / UI / bridge layers before actuators move.

## Migration plan

1. **Docs PR.** Land this ADR and link it from the architecture docs.
2. **Schema PR.** Add `SimCommand`, `SimState`, `SimScenario`, and `SimReport` types under the simulation package or a shared schema location.
3. **Scenario PR.** Add a tiny joint-space scenario catalog in YAML.
4. **Isaac replay PR.** Implement a headless Isaac adapter behind lazy imports.
5. **MuJoCo replay PR.** Implement the same scenario contract against MuJoCo.
6. **Compare PR.** Add `scripts/sim2sim_compare.py` and JSON report output.
7. **CI smoke PR.** Run one tiny scenario without requiring GPU-heavy Isaac in normal CI; keep full sim jobs manual or self-hosted.
8. **Shadow-mode PR.** Add `cortex` shadow mode for primitive execution.

## Consequences

### Positive

- Learned and Cartesian motion get a safety gate before metal.
- Simulator-specific assumptions become visible early.
- Rudy can compare physics behavior without changing the robot runtime API.
- The same primitive scenarios become regression tests for `cortex`, sim, and future LLM integration.

### Negative / trade-offs

- Two simulator adapters add maintenance cost.
- Perfect sim agreement is impossible; thresholds must focus on useful safety and behavior signals.
- GPU-heavy Isaac tests cannot be mandatory for every cloud-agent or CI run.

## See also

- [ADR-0008: Control plane and runtime FSM](0008-control-plane-and-runtime-fsm.md)
- [ros/src/simulation/README.md](../../ros/src/simulation/README.md)
- [docs/runbooks/isaac_lab.md](../runbooks/isaac_lab.md)
- [docs/robotics-best-practices-reference.md](../robotics-best-practices-reference.md)
