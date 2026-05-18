# Isaac + Jetson learning path

Purpose: make the resource pile useful. Read in this order when preparing Rudy's sim-to-real and Jetson inference path.

## Mental model

Rudy has three different machines/contexts:

```text
Desktop GPU
  - Isaac Sim / Isaac Lab
  - ROS 2 / MoveIt visualization and planning
  - training, replay, sim reports

Jetson Orin Nano
  - local inference
  - camera / audio / perception experiments
  - intent adapter that calls cortex

Pi 5
  - cortex
  - operator console
  - SocketCAN
  - motion authority
```

Do not try to put Isaac Sim/Lab on the Jetson for Rudy V1. Use the Jetson for edge inference; use a bigger NVIDIA desktop/workstation for Isaac.

## Track A: Jetson Orin Nano basics

Read first:

1. NVIDIA Jetson Orin Nano getting started.
2. Jetson Linux Developer Guide quick start.
3. Jetson Linux power/performance docs.
4. Jetson AI Lab GenAI overview.
5. Tailscale Linux install.

Hands-on tasks:

1. Flash and boot.
2. Confirm network and SSH.
3. Install Tailscale.
4. Confirm `curl https://rudy-pi/api/config`.
5. Run `tegrastats` while idle and under a small workload.
6. Run one tiny local model or sample container.
7. Write a Python script that decomposes `"wave right arm slowly"` into allowed low-level actions only.

Important tools:

- `tegrastats` for CPU/GPU/memory/temp monitoring.
- NVIDIA container runtime for model demos.
- Jetson AI Lab examples for local LLM/VLM experiments.

## Track B: Isaac Sim basics

Read first:

1. Isaac Sim overview.
2. Isaac Sim installation/workstation requirements.
3. URDF importer docs.
4. ROS 2 bridge docs.
5. Physics and articulation tutorials.

Hands-on tasks:

1. Open Isaac Sim and run a built-in robot sample.
2. Import a tiny URDF.
3. Import Rudy's URDF after it validates.
4. Confirm joints, axes, masses, inertia, and collision shapes look sane.
5. Save a USD asset for repeatable loading.
6. Publish/inspect joint states through ROS 2 only for visualization/testing.

What to watch for:

- Wrong units.
- Merged fixed joints hiding expected links.
- Collision mesh too detailed or misaligned.
- Inertia missing or absurd.
- Joint axes mirrored on right arm.
- Physics step too coarse for actuator assumptions.

## Track C: Isaac Lab basics

Read first:

1. Isaac Lab overview.
2. "Add a new robot" / asset tutorials.
3. Direct workflow vs manager-based workflow.
4. Environment config examples.
5. RL examples only after joint-space replay works.

Hands-on tasks:

1. Run a built-in Isaac Lab task.
2. Add a simple custom robot asset.
3. Build a headless joint-space replay for Rudy's `joint_space_smoke` scenario.
4. Emit `SimState`-shaped traces matching `ros/src/simulation/simulation/schema.py`.
5. Compare against MuJoCo or another adapter later.

Rudy-specific rule:

- Match observation/action spaces to sensors and commands Rudy can actually have on hardware. No privileged teacher-only state in deployment policy.

## Track D: Simulation best practices

Learn these before trusting sim:

1. One source of truth for kinematics: Rudy URDF/xacro.
2. One source of truth for actuator envelopes: `config/actuators/robstride_rs03.yaml`.
3. Deterministic scenario catalog: YAML scenario, fixed seed, fixed thresholds.
4. Simulator-neutral reports: joint error, velocity, acceleration, torque/current estimate, contact count, soft-limit margin, runtime stops.
5. Sim-to-sim before risky learned or Cartesian behavior.
6. Hardware shadow mode before real execution for risky motion classes.

For first slow arm gesture:

- Full RL is unnecessary.
- Single-joint replay is enough to validate names, signs, limits, and logging.
- Real hardware must still start tiny.

## Track E: What not to study yet

Skip until after first gesture:

- Whole-body control papers.
- Humanoid locomotion RL.
- Complex reward design.
- Multi-camera synthetic data.
- Full hand manipulation.
- Cartesian reaching.
- Isaac Replicator datasets.

Those matter later, but they distract from the first proof: one safe joint oscillation.

## First resource queue

Open these in order:

1. Jetson Orin Nano getting started.
2. Jetson Linux Quick Start.
3. Jetson AI Lab GenAI overview.
4. Isaac Sim overview.
5. Isaac Sim URDF importer.
6. Isaac Sim ROS 2 bridge.
7. Isaac Lab overview.
8. Isaac Lab "add new robot".
9. Rudy `docs/runbooks/isaac_lab.md`.
10. Rudy `docs/decisions/0009-simulation-ladder-and-sim-to-sim.md`.

## Success checks

Jetson readiness:

- Can reach Pi over Tailscale.
- Can run a small inference/container demo.
- Can call a read-only `cortex` endpoint.
- Does not have CAN access.

Simulation readiness:

- Rudy model imports.
- Right-arm joints move in expected signs.
- Joint limits match repo limits.
- A tiny replay emits machine-readable trace.
- Trace can be compared against expected thresholds.
