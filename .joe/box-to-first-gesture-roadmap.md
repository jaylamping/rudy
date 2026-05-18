# Box-to-first-gesture roadmap

Goal: get from opening the Jetson Orin Nano box to a slow, boring, repeatable right-arm gesture without weakening Rudy's safety model.

Important distinction: the robot should not contain an executable behavior named after the user phrase. A phrase like "wave" is natural language input. The system should decompose it into lower-level actions such as position arm, hold, oscillate a wrist/forearm joint, return, and stop.

## Phase 0: Pick the first physical motion

Use one joint first, not the whole arm.

Preferred order:

1. `right_arm.wrist_yaw` or `right_arm.wrist_roll` if installed and mechanically low load.
2. `right_arm.forearm_roll` / lower-arm yaw equivalent if that is the wrist-down joint available today.
3. `right_arm.elbow_pitch`, only with the arm supported.
4. Shoulder joints last, because gravity load and stored energy are higher.

Keep first amplitude tiny: 5-7 degrees. Keep first speed tiny: 3-6 degrees/sec.

## Phase 1: Bring up Jetson as compute only

1. Flash the official Jetson image for the exact board you receive.
2. Boot, create user, update packages.
3. Install Tailscale and join the same tailnet as the Pi.
4. Install repo basics: `git`, `curl`, Python tooling, and whatever model runtime you want to test.
5. Prove Jetson can reach `cortex` before touching motors:

```bash
curl https://rudy-pi/api/config
```

If this fails, fix networking first. Do not debug motion while networking is uncertain.

## Phase 2: Keep hardware authority on the Pi

Rudy's near-term topology stays:

```text
Jetson: inference, speech, cameras, high-level app logic
Pi 5: cortex, SocketCAN, operator console, audit log, motor authority
```

The Jetson should only call approved HTTPS/WebTransport surfaces exposed by `cortex`.

For the first gesture, the Jetson does not need ROS, MoveIt, or a model. A plain `curl` command is enough.

## Phase 3: Commission each actuator

For every motor involved:

1. Confirm firmware version.
2. Confirm CAN ID and bus.
3. Set mechanical zero at the installed neutral pose.
4. Write conservative firmware limits.
5. Save to flash.
6. Power-cycle.
7. Verify values persisted.
8. Set `present: true` and `verified: true`.
9. Set `limb: right_arm` and correct `joint_kind`.
10. Set narrow `travel_limits`.

Do not move with `travel_limits: null` or `verified: false`.

## Phase 4: Register right-arm roles cleanly

Use canonical role names for new installed joints:

```yaml
role: right_arm.wrist_yaw
limb: right_arm
joint_kind: wrist_yaw
```

Other likely roles:

- `right_arm.shoulder_pitch`
- `right_arm.shoulder_roll`
- `right_arm.upper_arm_yaw`
- `right_arm.elbow_pitch`
- `right_arm.forearm_roll`
- `right_arm.wrist_pitch`
- `right_arm.wrist_roll`

The current seed inventory still has `shoulder_actuator_a` as a bench-era role. Treat that as transitional unless it has been physically mapped and renamed.

## Phase 5: Dry-run through cortex

Use the operator console first:

1. Open actuator page.
2. Confirm live telemetry: position, velocity, bus voltage, temperature, fault status.
3. Confirm travel band.
4. Home the joint.
5. Start a tiny joint oscillation.
6. Stop.

Expected failures are useful:

- `not_verified`: finish commissioning.
- `stale_telemetry`: fix CAN or telemetry freshness.
- `travel_limit_violation`: fix band or center/amplitude.
- `limb_quarantined`: fix sibling joint before moving this one.
- `motor_fault`: clear or investigate drive fault before moving.

## Phase 6: First hardware gesture

Physical setup:

- Arm supported.
- No fingers near pinch points.
- PSU current limit conservative.
- E-stop visible in browser.
- One operator at the robot.
- One person calling out test steps if possible.

Start with the operator console. Only after that works, repeat from Jetson using HTTPS.

Example command shape for the first constituent action, a tiny joint oscillation:

```bash
SESSION="joe-gesture-$(date +%s)"
ROLE="right_arm.wrist_yaw"

curl -sS -X POST "https://rudy-pi/api/motors/${ROLE}/home" \
  -H "content-type: application/json" \
  -H "X-Rudy-Session: ${SESSION}" \
  -d '{"target_rad":0.0}'

curl -sS -X POST "https://rudy-pi/api/motors/${ROLE}/motion/wave" \
  -H "content-type: application/json" \
  -H "X-Rudy-Session: ${SESSION}" \
  -d '{"center_rad":0.0,"amplitude_rad":0.08,"speed_rad_s":0.05}'

sleep 8

curl -sS -X POST "https://rudy-pi/api/motors/${ROLE}/motion/stop" \
  -H "X-Rudy-Session: ${SESSION}"
```

Current endpoint name is `/motion/wave` because the existing API predates this naming decision. Treat it as an implementation detail for "bounded joint oscillation." Longer term, replace it with a neutral primitive name such as `oscillate_joint`.

Adjust `ROLE`, center, and band to the real installed joint.

## Phase 7: Add model language only after motion is boring

After manual and scripted motion both work:

1. Jetson runs model.
2. Model output becomes structured decomposition, not a named behavior.
3. Grounding layer maps parts to lower-level primitives.
4. Human approves.
5. Adapter calls `cortex`.

Example decomposition:

```json
{
  "request": "wave right arm slowly",
  "decomposition": [
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

If the phrase cannot be decomposed into allowed low-level actions, Rudy asks for clarification or refuses.
