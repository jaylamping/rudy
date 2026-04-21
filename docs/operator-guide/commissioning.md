# Commissioning actuators (commissioned zero)

This guide is for operators using **Rudy’s operator console** (`cortex` + Link SPA) with RobStride RS03 hardware. Canonical protocol detail lives in [ADR 0002: RS03 CAN protocol](../decisions/0002-rs03-protocol-spec.md), especially **Commissioned mechanical zero (cortex)**.

## What “commissioned zero” means

The firmware stores a mechanical position offset (`add_offset`, parameter `0x702B`). After you **commission**, the daemon records that value in `config/actuators/inventory.yaml` as `commissioned_zero_offset`. On every boot it compares live readback to that record:

- **Match** (within tolerance): normal boot flow; if the joint is in band and `auto_home_on_boot` is on, the **boot orchestrator** can home-ramp to `predefined_home_rad` (default `0`) and reach **Homed** without clicking Verify & Home. The drive then stays in **profile-position (PP) hold** at that angle (torque applied); expect quiet holding current and normal actuator whine until you jog, stop, or e-stop.
- **Mismatch**: **`OffsetChanged`** — motion is blocked until you **re-commission** or **restore offset** (writes the stored value back to firmware and saves).

## Before you start

1. **Travel limits** — Set a sensible soft band (`PUT /api/motors/:role/travel_limits` or Travel tab). Homing and orchestration enforce this band.
2. **Inventory** — Motor must exist in `inventory.yaml` with correct `can_bus`, `can_id`, and `present: true` for live hardware.
3. **Verified** — Production configs often require `verified: true` before motion; commission may still be allowed as a recovery path depending on your gates.

## One-time commissioning (SPA)

1. Physically move the joint to the pose you want as **neutral** (mechanical + logical zero for this joint).
2. Open the actuator **Controls** tab and use **Commission Zero** (calls `POST /api/motors/:role/commission`).
3. Confirm success in the UI and check the audit log if needed.
4. **Restart `cortex`** (or power-cycle). On boot you should see either:
   - **Auto-homing** briefly (`AutoHoming` boot state), then **Homed**, or
   - **In band** / **Verify & Home** if the motor was never commissioned (`commissioned_zero_offset` null) or auto-home is disabled.

## Optional: predefined home ≠ commissioned zero

If the joint’s comfortable resting pose is not the same as the angle where you commissioned:

- Set **`predefined_home_rad`** via `PUT /api/motors/:role/predefined_home` (Travel tab when exposed). The value must lie inside `travel_limits`. On boot, the orchestrator ramps to this angle instead of `0`.

## Recovery scenarios

| Symptom | What to do |
|--------|------------|
| **OffsetChanged** | Someone changed zero in firmware without updating inventory. Either **Commission** again at the new neutral, or **Restore offset** to push the recorded value back to the drive. |
| **OutOfBand** | Position is outside `travel_limits`. Move the joint manually into band, then **Verify & Home** (or let orchestrator run once in band). There is **no** daemon auto-ramp from out-of-band. |
| **HomeFailed** (after auto-home) | Investigate stall or tracking error; use **Verify & Home** or single-motor **home** to retry. |
| **Limb quarantined** | Another motor on the same `limb` is failed or out of band; fix the sibling first. |

## Bench / Motor Studio

For raw CAN bring-up and parameter dumps, see `tools/robstride/commission.md` and the bench scripts under `tools/robstride/`. The SPA **Commission** path is the supported persistence workflow for the daemon.

## Related endpoints (summary)

| Endpoint | Role |
|----------|------|
| `POST .../commission` | Persist zero: type-6 + type-22 + readback → `commissioned_zero_offset` |
| `POST .../set_zero` | Advanced / RAM diagnostic; does not update commissioned baseline by itself |
| `POST .../restore_offset` | Flash stored `commissioned_zero_offset` back when firmware disagrees |
| `PUT .../predefined_home` | Boot target angle inside travel limits |
| `POST .../home` | Manual home-ramp homer to requested target |
| `POST .../home_all` | Limb-ordered batch homing (requires `limb` set on motors) |
