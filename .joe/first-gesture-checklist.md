# First gesture day checklist

Use this at the bench. Check boxes in order. If any item is uncertain, stop and fix that item.

## 1. Robot state

- [ ] Right arm is mechanically supported.
- [ ] No fingertip/hand payload installed unless intentionally tested.
- [ ] No fingers, cables, or loose tools inside the motion envelope.
- [ ] PSU current limit is conservative.
- [ ] CAN_H/CAN_L termination is correct for the current harness.
- [ ] E-stop is visible in the operator console.

## 2. Pi and CAN

- [ ] `cortex.service` is running.
- [ ] `robot-can` is running.
- [ ] CAN interface for the joint is `ERROR-ACTIVE`.
- [ ] `cortex` logs show healthy startup.
- [ ] Telemetry updates live in the UI.
- [ ] No active `fault_sta`.
- [ ] `warn_sta` is understood and non-fatal.

Useful Pi checks:

```bash
sudo systemctl status cortex.service
ip -details -statistics link show can0
ip -details -statistics link show can1
journalctl -u cortex -n 80 --no-pager
```

## 3. Inventory

- [ ] Role matches the physical joint.
- [ ] `can_bus` and `can_id` match wiring.
- [ ] `present: true`.
- [ ] `verified: true`.
- [ ] `limb: right_arm`.
- [ ] `joint_kind` matches the physical joint.
- [ ] `travel_limits` are narrow for the first test.
- [ ] `commissioned_zero_offset` matches firmware.

First-test travel band suggestion:

```yaml
travel_limits:
  min_rad: -0.12
  max_rad: 0.12
```

Only use this if the physical neutral pose and hard stops make it safe.

## 4. Operator console test

- [ ] Open actuator page.
- [ ] Confirm latest position is inside travel band.
- [ ] Run home to `0.0` or the chosen safe center.
- [ ] Run tiny joint oscillation with amplitude <= `0.08 rad`.
- [ ] Run speed <= `0.05 rad/s`.
- [ ] Stop after 5-10 seconds.
- [ ] Confirm final status says stopped.
- [ ] Confirm telemetry still live.
- [ ] Confirm no new faults, current trips, or thermal concerns.

## 5. Jetson replay

- [ ] Jetson can `curl https://rudy-pi/api/config`.
- [ ] Jetson sends only allowed low-level actions.
- [ ] Jetson uses `X-Rudy-Session`.
- [ ] Same tiny amplitude and speed as console test.
- [ ] Stop command issued even if motion looked fine.

## 6. Pass/fail log

Record:

- Date/time:
- Joint role:
- CAN bus/id:
- Firmware version:
- Travel band:
- Commanded center:
- Commanded amplitude:
- Commanded speed:
- Duration:
- Peak observed current:
- Peak observed temperature:
- Fault/warn after test:
- Video path:
- Notes:

Pass means: smooth slow oscillation, clean stop, no new fault, no runaway, no bus degradation.
