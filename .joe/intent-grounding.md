# Intent grounding: how Rudy learns what "wave" means

Key correction: a fresh robot does not know what a wave is.

It only knows:

- its robot model,
- installed joints,
- limits and current telemetry,
- skills/presets humans have defined,
- demonstrations or policies humans have promoted into that skill library.

The missing layer is **intent grounding**: converting a human phrase like "wave" into a reviewed, executable skill invocation.

## Layer stack

```text
Human request
  "wave right arm slowly"

Language / VLA layer
  parse phrase, scene, target, modifiers
  output: candidate intent, not motor commands

Intent grounding layer
  choose known skill
  bind arguments: side=right, style=slow, audience/front frame
  reject unknown or unsafe requests

Skill / primitive library
  greeting_wave_right_slow
  raise_arm, oscillate_wrist, lower_arm
  parameter schemas and allowed ranges

Kinematics / planning layer
  optional Cartesian target -> IK / joint trajectory
  collision and joint-limit checks

cortex runtime
  verified motors, fresh telemetry, travel limits, faults, current, audit
  final CAN authority
```

## Where "wave" should live

For V1, `wave` should be a **skill**, not a thing the model invents.

Example skill record:

```json
{
  "name": "greeting_wave_right_slow",
  "description": "Small slow right-arm greeting wave for bench bring-up.",
  "inputs": {
    "side": "right",
    "style": "slow"
  },
  "preconditions": [
    "right arm installed",
    "target joints verified",
    "travel limits configured",
    "fresh telemetry",
    "operator approved"
  ],
  "implementation": {
    "type": "sequence",
    "steps": [
      { "primitive": "home", "role": "right_arm.wrist_yaw" },
      {
        "primitive": "oscillate",
        "role": "right_arm.wrist_yaw",
        "center_rad": 0.0,
        "amplitude_rad": 0.08,
        "speed_rad_s": 0.05,
        "duration_s": 8.0
      },
      { "primitive": "stop", "role": "right_arm.wrist_yaw" }
    ]
  }
}
```

The model can choose `greeting_wave_right_slow`. It cannot invent `0.6 rad` amplitude because it "feels friendly."

## VLA layer vs grounding layer

VLA is useful when the task depends on perception:

- "wave at Joey" -> detect Joey / face / front direction.
- "point to the red cup" -> identify object and target frame.
- "reach toward the handle" -> object pose and affordance.

For a first wave, VLA is not needed. No visual target is required. The grounding layer can map the phrase to a fixed preset.

Use VLA later as a **context provider**:

```text
VLA: Joey is in front-left.
Grounding: choose greeting_wave_right_slow, set audience_frame=front_left.
Planner: maybe orient torso/head later.
cortex: execute only validated primitives.
```

## Joint-space vs Cartesian wave

There are two versions of "wave":

### Joint-space wave

Good for first hardware:

- pick one joint,
- oscillate inside narrow travel limits,
- no IK,
- no end-effector frame required,
- easy to test and stop.

This is what first Rudy should do.

### Cartesian wave

Better long-term human meaning:

- define hand/palm/wrist end-effector frame,
- move hand to a pose near shoulder/head,
- oscillate hand left-right or wrist yaw around that pose,
- solve IK for joint trajectory,
- check collisions and joint limits,
- send candidate to `cortex` for validation/execution.

Cartesian wave requires:

- stable URDF frames,
- hand/palm frame, even if fingertip is not built,
- IK solver,
- collision model,
- trajectory time parameterization,
- sim replay,
- hardware shadow mode.

Do not start here.

## How Rudy gains skills

### 1. Hand-authored presets

Fastest path. Human writes safe `wave_right_slow` parameters. Validate in UI, script, sim, then hardware.

### 2. Demonstration capture

Operator teleops or manually guides a motion.

Record:

- joint positions,
- timestamps,
- velocities,
- current/torque estimates,
- video,
- label: `greeting_wave_slow`.

Then compress into a skill:

- keyframes,
- spline,
- dynamic movement primitive,
- or primitive sequence.

### 3. Sim refinement

Replay skill in Isaac/MuJoCo.

Check:

- joint signs,
- limits,
- velocity,
- acceleration,
- collision/contact,
- torque/current estimate,
- repeatability.

### 4. Promotion

A skill becomes callable only after it has:

- schema,
- parameter limits,
- preconditions,
- sim trace,
- hardware test note,
- rollback/stop behavior.

## Practical next build target

Build a **skill registry** separate from the model:

```yaml
skills:
  greeting_wave_right_slow:
    phrases:
      - wave
      - wave right arm
      - wave slowly
      - say hi
    requires:
      joints:
        - right_arm.wrist_yaw
      approval: true
    limits:
      max_amplitude_rad: 0.10
      max_speed_rad_s: 0.08
      max_duration_s: 10
    primitive:
      name: wave
      role: right_arm.wrist_yaw
      center_rad: 0.0
      amplitude_rad: 0.08
      speed_rad_s: 0.05
```

Then the model's job is small:

```text
"wave right arm slowly" -> greeting_wave_right_slow
```

If no skill matches, Rudy asks for clarification or refuses.

## Clean answer to the disconnect

The robot does not know "wave" from the box.

Humans teach Rudy "wave" by adding a reviewed skill. The LLM/VLA layer can recognize that a user asked for that skill, maybe add context, and maybe choose style. The grounding layer binds it to safe parameters. Kinematics/planning maps Cartesian targets when needed. `cortex` remains the only layer that can execute on hardware.
