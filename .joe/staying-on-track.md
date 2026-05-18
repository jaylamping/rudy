# Staying on track

Use this when the project starts to sprawl.

## North star

First useful milestone:

> Jetson hears or receives "wave right arm slowly", decomposes it into allowed low-level actions, asks `cortex` to run those actions, and one right-arm joint oscillates slowly under full `cortex` safety checks.

Not part of the first milestone:

- Full hand or fingertip.
- Full learned policy.
- Cartesian reaching.
- Multi-joint choreography.
- Direct Jetson-to-CAN control.
- Direct LLM-to-motion control.
- ROS running on the Pi.

## Current first-demo stack

```text
Jetson
  - model runtime later
  - intent adapter
  - HTTPS client

Pi 5
  - cortex
  - operator console
  - CAN
  - audit log

Arm hardware
  - verified RS03 actuator
  - narrow travel limits
  - supported right arm
```

## Do one boring thing at a time

1. Network.
2. Telemetry.
3. Commissioning.
4. Travel limits.
5. Home.
6. Tiny joint oscillation from UI.
7. Tiny joint oscillation from Jetson script.
8. Tiny language-to-actions decomposition with approval.
9. Larger motion only after repeated clean logs.

## Red flags

Stop if any of these happen:

- "Let's just send CAN frames from the Jetson."
- "Travel limits are probably fine."
- "This motor is not verified but it moved before."
- "The warning is probably harmless" without notes.
- "The arm is unsupported but speed is low."
- "The model can choose the numbers."
- "We can skip audit because it is just a test."

## Safe decomposition policy

The model never chooses raw joint values for hardware.
The grounding layer may substitute an equivalent limb only when the goal allows it and it records the substitution reason.

Allowed:

```json
{
  "input_text": "wave right arm slowly",
  "actions": [
    { "action": "home_joint", "role": "right_arm.wrist_yaw", "target_rad": 0.0 },
    {
      "action": "oscillate_joint",
      "role": "right_arm.wrist_yaw",
      "center_rad": 0.0,
      "amplitude_rad": 0.08,
      "speed_rad_s": 0.05,
      "duration_s": 8.0
    },
    { "action": "stop_joint", "role": "right_arm.wrist_yaw" }
  ]
}
```

Not allowed:

```json
{
  "role": "right_arm.shoulder_roll",
  "amplitude_rad": 0.6,
  "speed_rad_s": 1.0
}
```

Action schemas and decomposition policies live in code/config, have limits, and can be reviewed.

Allowed substitution example:

```json
{
  "input_text": "wave your right hand",
  "requested_limb": "right_arm",
  "selected_limb": "left_arm",
  "substitution_reason": "right_arm unavailable: motor_fault",
  "actions": [
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

## Done means evidence

For each hardware run, keep:

- command or UI action used,
- relevant `cortex` log/audit lines,
- telemetry summary,
- video,
- pass/fail note.

If there is no evidence, it did not happen.
