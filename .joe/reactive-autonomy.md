# Reactive autonomy: catching a thrown ball someday

Goal: Rudy notices an unscripted event, decides what matters, and acts fast enough without waiting for a human instruction.

Example:

```text
Tennis ball enters camera view.
Rudy estimates trajectory.
Rudy decides: catch, block, dodge, or ignore.
Rudy executes the safest feasible reaction.
```

This is not language planning. It is a real-time reaction stack.

See `planning-under-uncertainty.md` for the shared planner loop that also handles ambiguous human instructions.

## Core idea

The planner is not only for user commands. It should also handle **events**.

```text
external event
  -> perception
  -> state estimation / prediction
  -> goal inference
  -> capability planner
  -> reflex / policy / controller
  -> cortex safety gate
  -> actuators
```

For a thrown ball, the "intent" is not spoken. It is inferred from the event:

```json
{
  "event": "incoming_object",
  "object": "tennis_ball",
  "trajectory": "toward_robot_upper_body",
  "candidate_goals": ["catch", "block", "avoid", "ignore"]
}
```

## LLM/VLA role

No LLM should be in the tight loop.

Useful:

- scene understanding before/after event,
- labeling objects,
- explaining what happened,
- updating high-level preferences,
- offline skill review.

Not useful in the catch window:

- token-by-token reasoning,
- natural-language planning,
- generating joint values,
- choosing catch pose from scratch.

The catch window likely has only a few hundred milliseconds. That belongs to perception, prediction, and a trained/reactive controller.

## Latency budget thinking

Approximate target for a close throw:

```text
camera exposure + transfer       5-20 ms
object detection / tracking      5-25 ms
trajectory prediction            1-5 ms
capability / intercept planner   5-20 ms
policy / controller update       1-5 ms
transport + actuator response    variable, must be measured
```

If a ball is 3 m away and moving 10 m/s, impact is about 300 ms away. That leaves little room for slow reasoning or network round trips.

## Planner responsibility

Given an event, the planner should solve:

```text
What goal is appropriate?
What effectors are available?
Can any effector reach an intercept point in time?
Is catch safer than block or dodge?
What plan has lowest risk?
What must cortex refuse?
```

Candidate outputs:

```json
{
  "event": "incoming_ball",
  "selected_goal": "block",
  "selected_effector": "left_forearm",
  "reason": "catch infeasible: no hand/fingers; block feasible before impact",
  "deadline_ms": 180,
  "actions": [
    {
      "action": "move_effector_to_intercept",
      "effector": "left_forearm",
      "intercept_xyz_m": [0.35, 0.18, 0.62],
      "deadline_ms": 180
    },
    {
      "action": "compliant_hold",
      "effector": "left_forearm",
      "duration_ms": 250
    }
  ]
}
```

If no safe plan exists:

```json
{
  "event": "incoming_ball",
  "selected_goal": "avoid",
  "reason": "catch/block infeasible before impact",
  "actions": [
    { "action": "protect_head" },
    { "action": "stop_nonessential_motion" }
  ]
}
```

## Catch vs block vs dodge

Do not make "catch" the only success condition.

Rudy should rank goals:

1. Protect humans.
2. Protect robot.
3. Avoid unsafe motion.
4. Catch if feasible.
5. Block/deflect if catch infeasible.
6. Dodge/protect if block infeasible.
7. Ignore if object is not a threat or target.

Before hands/fingers exist, the right behavior might be "block with forearm," not catch.

## Required building blocks

Perception:

- low-latency camera stream,
- ball detector/tracker,
- depth or stereo estimate,
- trajectory fit,
- uncertainty estimate.

Robot state:

- fresh joint state,
- known available effectors,
- current boot/fault/quarantine state,
- collision geometry,
- current motion mode.

Planner:

- event-to-goal mapping,
- reachability check,
- time-to-intercept check,
- risk scoring,
- hard constraints,
- best feasible action sequence.

Controller:

- fast Cartesian or joint-space servo,
- compliance/impedance behavior,
- abort on stale perception,
- abort on cortex safety refusal.

Training/sim:

- Isaac ball trajectory scenarios,
- randomized throws,
- latency injection,
- camera noise,
- actuator lag,
- sim-to-sim replay,
- hardware shadow mode before motion.

## Training path

1. Build ball tracking offline from recorded video.
2. Simulate incoming ball in Isaac.
3. Train/evaluate block/catch policies in sim.
4. Export policy or controller to Jetson.
5. Run hardware shadow mode: perceive and plan, but do not move.
6. Low-speed foam ball tests.
7. Tennis ball only after many safe runs.

## Architecture principle

Reactive autonomy should share the same planner vocabulary as instructed actions.

```text
spoken request: "wave"
  -> goal + constraints
  -> action sequence

unscripted event: incoming ball
  -> goal + constraints
  -> action sequence
```

Different input source. Same safety authority.

`cortex` still owns final execution. For very fast reactions, this may eventually require a lower-latency real-time controller, but the safety boundary must remain explicit: no model gets raw motor authority.

## Near-term relevance

This is not needed for first gesture.

But it should influence design now:

- Log events, state, and actions in machine-readable form.
- Keep action schemas neutral and composable.
- Make planner query current capabilities, not assume perfect hardware.
- Preserve sim-to-real traces.
- Avoid hardcoding named behaviors that cannot generalize.
