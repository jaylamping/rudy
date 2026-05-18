# Planning under uncertainty

This is the shared core behind vague instructions and unscripted events.

Examples:

```text
"wave your right hand"
"help me with that"
"come here"
tennis ball enters view
human steps into workspace
right arm faults mid-task
```

None of these are clean "command -> action" cases. Rudy needs a planner that reasons over uncertainty, risk, current capabilities, and time.

## Unified loop

```text
input/event
  -> situation model
  -> candidate goals
  -> constraints + preferences
  -> capability query
  -> candidate plans
  -> risk / utility / deadline scoring
  -> act, ask, hold, or refuse
  -> audit + learn from outcome
```

Input can be language, perception, telemetry, fault state, or a combination.

## What Rudy should estimate

For every situation:

- What changed?
- What are possible goals?
- How confident is each goal?
- What constraints are hard?
- What preferences are soft?
- What hardware is available now?
- What plans are feasible?
- What is the risk of each plan?
- How reversible is the action?
- How urgent is the decision?
- Is there time to ask?

## Response modes

### 1. Ask

Use when uncertainty is high and time allows.

Example:

```text
Human: "help me with that"
Rudy: "Do you want me to hold the part, move the arm out of the way, or open the gripper?"
```

### 2. Act conservatively

Use when goal is likely, risk is low, and action is reversible.

Example:

```text
Human: "come here"
Rudy: rotates head/torso or moves to a ready pose, but does not drive into a crowded path.
```

### 3. Re-plan with notice

Use when literal wording is infeasible but goal can be satisfied another way.

Example:

```text
Human: "wave your right hand"
Right arm: faulted
Left arm: healthy
Rudy: "Right arm is faulted; using left arm for greeting gesture."
```

### 4. Act immediately

Use when deadline is short and the safest goal is clear enough.

Example:

```text
Incoming ball, path intersects torso.
No time to ask.
Rudy chooses block/protect/avoid based on feasibility.
```

### 5. Refuse / hold / stop

Use when all feasible plans are unsafe or constraints conflict.

Example:

```text
Human: "move only the right arm"
Right arm: quarantined
Rudy: refuses and explains right arm is unavailable.
```

## Deadline changes everything

Same planner, different time budget:

| Situation | Time budget | Best response |
| --- | --- | --- |
| "what can you do?" | seconds | explain / ask |
| "wave right hand" with right fault | seconds | re-plan with notice |
| "catch this" before throw | seconds | prepare catch/block stance |
| ball already incoming | tens to hundreds ms | reactive block/catch/avoid |
| motor current spike | milliseconds | stop/hold safety path |

This is why thrown-ball autonomy and ambiguous commands are related. They both need goal inference and capability planning. They differ in latency and risk tolerance.

## Planner scoring sketch

A candidate plan can be scored like:

```text
score =
  goal_satisfaction
  - safety_risk
  - collision_risk
  - hardware_risk
  - time_risk
  - reversibility_penalty
  - preference_mismatch
```

Hard constraints still override score:

- no motion with unverified motors,
- no motion with stale telemetry,
- no path through travel limits,
- no collision with human/robot/self,
- no bypassing `cortex`.

## Capability model

The planner needs live capability state:

```json
{
  "limbs": {
    "right_arm": {
      "available": false,
      "reason": "motor_fault",
      "healthy_effectors": []
    },
    "left_arm": {
      "available": true,
      "healthy_effectors": ["left_arm.wrist_yaw", "left_arm.elbow_pitch"]
    }
  },
  "active_constraints": ["operator_lock", "travel_limits", "fresh_telemetry_required"]
}
```

This model is why Rudy can choose left-arm greeting when right arm is down, but refuse "diagnose right arm by moving only right arm."

## Relation to LLM/VLA

LLM/VLA can help produce:

- candidate goals,
- object labels,
- scene descriptions,
- semantic constraints,
- likely human intent.

Planner decides what to do.

`cortex` decides what can execute.

Fast events bypass slow language reasoning and use event-specific perception/policy, but they still output the same kind of goal/action/capability records.

## Design consequence

Do not design Rudy as:

```text
phrase -> command
```

Design Rudy as:

```text
situation -> goals -> feasible plans -> safest useful action
```

That lets the same architecture handle vague human requests, hardware faults, unexpected objects, and reactive motion.
