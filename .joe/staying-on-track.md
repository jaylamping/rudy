# Staying on track

Use this when the project starts to sprawl.

## North star

First useful milestone:

> Jetson hears or receives "wave right arm slowly", maps it to a known safe preset, asks `cortex` to run it, and one right-arm joint waves slowly under full `cortex` safety checks.

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
6. Tiny wave from UI.
7. Tiny wave from Jetson script.
8. Tiny wave from approved intent preset.
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

## Safe preset policy

The model never chooses raw joint values for hardware.

Allowed:

```json
{ "intent": "wave_right_arm_slow" }
```

Not allowed:

```json
{
  "role": "right_arm.shoulder_roll",
  "amplitude_rad": 0.6,
  "speed_rad_s": 1.0
}
```

Preset values live in code/config, have names, have limits, and can be reviewed.

## Done means evidence

For each hardware run, keep:

- command or UI action used,
- relevant `cortex` log/audit lines,
- telemetry summary,
- video,
- pass/fail note.

If there is no evidence, it did not happen.
