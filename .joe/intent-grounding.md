# Intent grounding: how Rudy learns what "wave" means

Key correction: a fresh robot does not know what a wave is, and Rudy should not store a hardware behavior named after that word.

It only knows:

- its robot model,
- installed joints,
- limits and current telemetry,
- low-level primitives humans have defined,
- demonstrations or policies humans have decomposed into those primitives.

The missing layer is **intent decomposition / grounding**: converting a human phrase like "wave" into constituent actions that can be validated and executed.

## Layer stack

```text
Human request
  "wave right arm slowly"

Language / VLA layer
  parse phrase, scene, target, modifiers
  output: candidate intent, not motor commands

Intent decomposition / grounding layer
  infer constituent actions
  bind arguments: requested_side=right, style=slow, audience/front frame
  check capabilities and allowed substitutions
  reject unknown or unsafe requests

Primitive / action library
  position_arm, hold_pose, oscillate_joint, return_home, stop
  parameter schemas and allowed ranges

Kinematics / planning layer
  optional Cartesian target -> IK / joint trajectory
  collision and joint-limit checks

cortex runtime
  verified motors, fresh telemetry, travel limits, faults, current, audit
  final CAN authority
```

## Where the word should live

For V1, the word should exist only in human language input, transcripts, and maybe annotations. It should not be a primitive name, preset name, new API surface, or model-chosen behavior label.

Example decomposition:

```json
{
  "input_text": "wave right arm slowly",
  "constituent_actions": [
    {
      "action": "select_limb",
      "limb": "right_arm"
    },
    {
      "action": "choose_available_joint",
      "preferred_joint_kinds": ["wrist_yaw", "wrist_roll", "forearm_roll"]
    },
    {
      "action": "home_joint",
      "role": "right_arm.wrist_yaw",
      "target_rad": 0.0
    },
    {
      "action": "oscillate_joint",
      "role": "right_arm.wrist_yaw",
      "center_rad": 0.0,
      "amplitude_rad": 0.08,
      "speed_rad_s": 0.05,
      "duration_s": 8.0
    },
    {
      "action": "stop_joint",
      "role": "right_arm.wrist_yaw"
    }
  ]
}
```

The model can propose this decomposition. The grounding layer clamps or rejects values against allowed action schemas. `cortex` still performs runtime validation before motion.

## Capability-aware substitution

Natural language often contains both a **goal** and a **preference**.

Example:

```text
"wave your right hand"
```

Possible parse:

```json
{
  "goal": "perform_greeting_gesture",
  "preferred_limb": "right_arm",
  "acceptable_substitutions": ["left_arm"],
  "must_use_requested_limb": false
}
```

If the right arm is unavailable but the left arm can satisfy the social goal, Rudy should prefer a safe left-arm decomposition over total failure.

Decision shape:

```text
1. Decompose phrase into goal + constraints.
2. Query capability state:
   - installed limbs and joints,
   - verified motors,
   - fresh telemetry,
   - travel limits,
   - boot state / limb quarantine,
   - faults, current trips, temperature.
3. Try requested limb.
4. If requested limb fails and substitution is allowed, try equivalent limb.
5. Tell operator what changed.
6. Execute only after approval / policy permits.
```

Example output:

```json
{
  "input_text": "wave your right hand",
  "goal": "perform_greeting_gesture",
  "requested_limb": "right_arm",
  "selected_limb": "left_arm",
  "substitution_reason": "right_arm unavailable: limb_quarantined",
  "requires_operator_notice": true,
  "constituent_actions": [
    { "action": "home_joint", "role": "left_arm.wrist_yaw", "target_rad": 0.0 },
    {
      "action": "oscillate_joint",
      "role": "left_arm.wrist_yaw",
      "center_rad": 0.0,
      "amplitude_rad": 0.08,
      "speed_rad_s": 0.05,
      "duration_s": 8.0
    },
    { "action": "stop_joint", "role": "left_arm.wrist_yaw" }
  ]
}
```

Some requests should **not** substitute:

- "pick up the cup with your right hand" when the right hand is specified for task geometry,
- "show me your broken right arm" where the target is diagnostic,
- "move only the right arm" where the side is an explicit constraint,
- any action where the alternate limb would collide, overreach, or violate limits.

So substitution needs a policy field:

```yaml
substitution_policy:
  social_gesture: allowed_with_notice
  diagnostic_motion: forbidden
  object_manipulation: requires_replan_and_confirmation
  safety_recovery: forbidden
```

## VLA layer vs grounding layer

VLA is useful when the task depends on perception:

- "wave at Joey" -> detect Joey / face / front direction.
- "point to the red cup" -> identify object and target frame.
- "reach toward the handle" -> object pose and affordance.

For the first arm gesture, VLA is not needed. No visual target is required. The grounding layer can decompose the phrase into joint-space actions.

Use VLA later as a **context provider**:

```text
VLA: Joey is in front-left.
Grounding: decompose request; bind audience_frame=front_left.
Planner: maybe orient torso/head or choose right/left limb later.
cortex: execute only validated primitives.
```

## Joint-space vs Cartesian decomposition

There are two ways to decompose the phrase:

### Joint-space decomposition

Good for first hardware:

- pick one joint,
- oscillate inside narrow travel limits,
- no IK,
- no end-effector frame required,
- easy to test and stop.

This is what first Rudy should do.

### Cartesian decomposition

Better long-term human meaning:

- define hand/palm/wrist end-effector frame,
- move hand to a pose near shoulder/head,
- oscillate hand left-right or wrist yaw around that pose,
- solve IK for joint trajectory,
- check collisions and joint limits,
- send candidate to `cortex` for validation/execution.

Cartesian decomposition requires:

- stable URDF frames,
- hand/palm frame, even if fingertip is not built,
- IK solver,
- collision model,
- trajectory time parameterization,
- sim replay,
- hardware shadow mode.

Do not start here.

## How Rudy gains grounded behavior

### 1. Hand-authored decomposition rules

Fastest path. Human writes a rule that expands a phrase into lower-level primitives with safe parameter ranges. Validate in UI, script, sim, then hardware.

### 2. Demonstration capture

Operator teleops or manually guides a motion.

Record:

- joint positions,
- timestamps,
- velocities,
- current/torque estimates,
- video,
- label/annotation: user intended a greeting gesture.

Then compress into lower-level representation:

- keyframes,
- spline,
- dynamic movement primitive,
- or primitive sequence.

### 3. Sim refinement

Replay the decomposed action sequence in Isaac/MuJoCo.

Check:

- joint signs,
- limits,
- velocity,
- acceleration,
- collision/contact,
- torque/current estimate,
- repeatability.

### 4. Promotion

A decomposition path becomes callable only after it has:

- schema,
- parameter limits,
- preconditions,
- sim trace,
- hardware test note,
- rollback/stop behavior.

## Practical next build target

Build an **intent decomposition registry** separate from the model:

```yaml
decompositions:
  social_arm_slow_oscillation_template:
    matches:
      - wave
      - wave right arm
      - wave slowly
      - say hi
    goal: perform_greeting_gesture
    requested_limb: right_arm
    substitution_policy: social_gesture
    fallback_limb_order: [right_arm, left_arm]
    requires:
      joints:
        any_of:
          - right_arm.wrist_yaw
          - left_arm.wrist_yaw
      approval: true
    limits:
      max_amplitude_rad: 0.10
      max_speed_rad_s: 0.08
      max_duration_s: 10
    actions:
      - action: home_joint
        role: "${selected_limb}.wrist_yaw"
        target_rad: 0.0
      - action: oscillate_joint
        role: "${selected_limb}.wrist_yaw"
        center_rad: 0.0
        amplitude_rad: 0.08
        speed_rad_s: 0.05
        duration_s: 8.0
      - action: stop_joint
        role: "${selected_limb}.wrist_yaw"
```

Then the model's job is small:

```text
"wave right arm slowly" -> [home_joint, oscillate_joint, stop_joint]
```

If no decomposition is valid for installed hardware, Rudy asks for clarification or refuses.

## Clean answer to the disconnect

The robot does not know "wave" from the box, and Rudy should not hide that behind a magic named behavior.

Humans teach Rudy how to decompose that phrase into constituent actions. The LLM/VLA layer can recognize the phrase and provide context. The grounding layer produces low-level actions with safe parameters. Kinematics/planning maps Cartesian targets when needed. `cortex` remains the only layer that can execute on hardware.
