# RobStride RS03 commissioning runbook

Purpose: bring a **newly-received** RS03 actuator into a known-good, known-safe
state before it is ever wired into Rudy. This runbook writes firmware-level
limits that survive power cycles — those are Rudy's innermost software safety
layer, beneath anything our driver or `ros2_control` does.

Every motor must pass this runbook before `config/actuators/inventory.yaml`
flips its `verified` flag to `true`. The Rust driver refuses to enable
unverified motors.

Tool: **RobStride Motor Studio** (a.k.a. Motor Assistant / Motor Tool) for
Windows, **v1.0.3 or newer**, with the vendor-supplied CAN-to-USB adapter
(CH340-based). Driver CH341SER is in the vendor GitHub.

> **Do not use the 0.0.13 build that ships on the USB stick with many
> storefront-sourced motors.** It predates the v1.0.x line and silently fails
> the bootloader handshake against firmware 0.3.1.x with the error
> "Failed to send bin file information!" Use the mirrored copy at
> `docs/vendor/tools/motor-studio-1.0.3/extracted/motor_toolV13/motor_tool.exe`
> or a newer release from [https://github.com/RobStride/MotorStudio/releases](https://github.com/RobStride/MotorStudio/releases).

Authoritative reference: `docs/decisions/0002-rs03-protocol-spec.md` and
`docs/vendor/rs03-user-manual-260112.pdf` §3.3 "Motor settings" and §4.1.14
"Read and write a single parameter list".

---

## Safety preconditions

- Motor output shaft is **disconnected from any mechanical load** (out of the
shoulder housing, off the bench, nothing coupled to it).
- Rotor is free to spin a full revolution without hitting anything.
- Power supply is **current-limited** to ~2 A. If the motor tries to do
something stupid because the factory parameters are wrong, we want it to brown
out, not rip its own wiring off.
- Voltage ≤ 24 V during commissioning. 48 V is rated but pointless here.
- Only **one motor on the bus** at a time while commissioning. This avoids CAN
ID collisions when a storefront ships motors with the same default ID.

> **⚠️ Do NOT click "Factory Reset" (恢复出厂设置) in Motor Studio.** It lives
> uncomfortably close to "Write Para" and "Export Parameters" in the UI.
> Clicking it wipes:
> - `MechOffset` → back to factory (your zero is gone)
> - `limit_spd` → back to 10 rad/s (factory)
> - `limit_cur` → back to 43 A (factory hardware ceiling!)
> - `CAN_TIMEOUT` → back to 0 (watchdog disabled!)
>
> `CAN_ID`, `CAN_MASTER`, `baud`, `damper`, and `zero_sta` survive, but the
> safety-critical quartet does not. If you click this by accident, redo
> Steps 5 (zero), 6 (limits), 7 (persistence verify) before using the motor.

---

## Step 0 — Record-keeping

For each motor, open `config/actuators/inventory.yaml` and add a stub entry:

```yaml
- serial: "<whatever you can read off the label, or Sharpie something>"
  role: <e.g. right_shoulder_pitch>  # can be tentative
  sourced_from: <aliexpress | ebay | direct | ...>
  verified: false
```

Every later step fills in fields under this entry.

## Step 0.5 — Firmware update (optional but recommended once per motor)

Only skip this if the motor is already on the latest firmware from the
vendor's public releases. To check latest:

- Upstream: `https://github.com/RobStride/Product_Information/releases/latest`
- Mirror in this repo (verified sha256): `docs/vendor/firmware/`

### Preconditions

1. Motor on the bench, **output shaft free** (firmware notes say zero must be
  recalibrated after upgrade, and occasionally motor behavior is surprising
   right after a flash — don't have it coupled to anything).
2. PSU **stable**, ideally on a UPS or at minimum not shared with equipment
  that draws big transients. Voltage ≥ 24 V recommended.
3. **Only one motor on the CAN bus.** If multiple motors share IDs
  momentarily during boot (bootloader uses CAN_ID 0x00 by default), you can
   have collisions.
4. **Nothing else connected via the same USB hub that might steal bandwidth**
  mid-flash. CH340 is slow and sensitive.

### Flash procedure (Motor Assistant → Motor Upgrade Module)

1. **Take a pre-flash parameter export.** Motor Assistant → Parameter Settings
  → **Export**. Save as
   `config/actuators/factory-dumps/<role>_<can_id>_fw<old_version>_pre-flash.xlsx`.
   (If you already have a `_pre-commission.xlsx`, copy it to `_pre-flash.xlsx`.)
2. In Motor Assistant, click **Open File** in the Motor Upgrade module and
  select the `.bin` from `docs/vendor/firmware/rs03-`*. Verify the filename
   starts with `rs03-` — the manual specifically warns that the `rs-0x` prefix
   must match the motor type.
3. **Verify SHA256** of the file you're about to flash against the hash in
  the matching `README.md`. The manual doesn't explicitly call this out, but
   if you skip it and the binary got corrupted on download, you can brick a
   motor.
4. Click **Start Upgrade**. The motor will enter "upgrade preparation". Wait
  for the green text "Device has entered upgrade mode".
5. Click the **(second) Start Upgrade** button. A green progress bar advances.
6. **Do not touch anything.** Expected flash time: 20–60 seconds.
  - If the bar stalls halfway, click **Stop Upgrade** (per manual), power-cycle
   the motor, and retry from step 4. The vendor explicitly states the motor
   is recoverable after a failed flash — the bootloader survives.
7. Wait for green "Upgrade Successfully".
8. **Power-cycle the motor.** (Manual implies it, but some firmware versions
  don't fully re-init without one.)
9. Reconnect in Motor Assistant, **Refresh Parameter Table**. Verify
  `0x1003 AppCodeVersion` matches the new firmware string.
10. **Take a post-flash parameter export.** Save as
  `…_fw<new_version>_post-flash.xlsx`.
11. Diff the pre-flash and post-flash exports. Most `0x20xx` stored
  parameters should carry over; if any don't, decide whether to re-set or
    accept the new default. Commit both files.
12. Note: **mechanical zero is expected to be invalidated** after firmware
  upgrade (per RS03 0.3.1.3 release notes). Proceed to Step 5 to re-zero.

### What could go wrong


| symptom                                                 | likely cause                                                    | remedy                                                                           |
| ------------------------------------------------------- | --------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Device not detected after flash                         | Bootloader stuck, CAN ID reset                                  | Power-cycle. Try detecting at ID `0x7F` (factory default)                        |
| Progress bar stalls                                     | CAN bit error / USB stall                                       | Stop, power-cycle, retry                                                         |
| "Upgrade failed" red text                               | Bad file or wrong motor type                                    | Re-check SHA256, confirm `rs03-` prefix                                          |
| New firmware runs but motor won't enable                | `canTimeout` carried over as 0 and new firmware is stricter     | Re-read, if needed write new value via commissioning Step 6                      |
| Kt (`0x303c`), rated_i (`0x302d`) change after flash    | New firmware has different factory data                         | Update `inventory.yaml:baseline` to match                                        |
| "Failed to send bin file information!" on Start Upgrade | Ancient Motor Assistant (0.0.x) can't talk to modern bootloader | Close it, launch Motor Studio v1.0.3 from `docs/vendor/tools/`, re-detect, retry |


## Step 1 — Power up + connect

1. Wire the motor to PSU (24 V, current-limited), CAN-H / CAN-L to the
  CAN-to-USB dongle. Dongle DIP-2 ON (enables its 120 Ω terminator), DIP-1
   OFF (not in boot mode).
2. In Motor Assistant: **Refresh Serial Port → Open Serial Port → Detect
  Device**. You should see green text identifying the motor type as RS03
   with its current CAN ID.
3. If no device is detected, stop. Fix cabling / termination / power.

## Step 2 — Read firmware version

1. Click **Refresh Parameter Table**.
2. Find parameter `0x1003 AppCodeVersion`. Record it verbatim in
  `inventory.yaml:firmware_version`.
3. Cross-reference against the changelog in
  `.firecrawl/robstride-product-info.md` (or the upstream GitHub README). In
   particular:

  | feature you want                 | minimum firmware              |
  | -------------------------------- | ----------------------------- |
  | Zero-setting without motion leap | ≥ 0.3.1.9                     |
  | `add_offset`                     | ≥ 0.3.1.9                     |
  | `damper` disable                 | ≥ 0.3.1.4                     |
  | `can_status` terminator flag     | RS03_APP_V0311_V1001_20250507 |

   If below those, either (a) upgrade firmware via Motor Assistant (Motor
   Upgrade Module, pick the `.bin` from the GitHub "Product Literature/RS03"
   directory), or (b) make a conscious decision to live without the feature and
   write it in `inventory.yaml:notes`.

## Step 3 — Read CAN status flag

1. Find parameter `0x3041 can_status`. Record in
  `inventory.yaml:can_status`.
2. 0 → motor has an onboard 240 Ω terminator. When it sits at the end of the
  bus, the external 120 Ω you toggle on your board is **in parallel** with
   that 240 Ω, giving ~80 Ω. Disable the external one OR keep at most one
   terminator on each end.
3. 1 → no onboard terminator. The external 120 Ω is required when this motor
  is at an end of the bus.
4. Note: many gray-market units ship as flag=0 even when physically the SMD
  resistor is absent. If the bus is flaky, put a scope on CAN-H/CAN-L idle
   voltage and confirm (~2.5 V both lines, ~50 Ω DC impedance between
   CAN-H/L).

## Step 4 — Read current parameters (before changing anything)

In Motor Assistant → Parameter Settings → **Export** the full parameter table
to CSV. Commit the CSV under `config/actuators/factory-dumps/<serial>.csv`
(gitignore PSU-specific info; only parameter values are of interest).

This gives us a forensic baseline if firmware limits are ever accidentally
overwritten.

## Step 5 — Set the mechanical zero

**Only after the motor is physically at the desired zero position.** For a
joint that will live in a shoulder, this typically means "hold the rotor at
its mid-range of motion with your hand, motor unpowered, then power on".

> **Use the Pi for this step, not the Windows GUI.** Motor Studio v0.0.13
> (the build on vendor USB sticks) has no "save parameters" button, so zeros
> set via its GUI live only in RAM and are lost on the next power cycle.
> The script below sends both the zero and the save frame via `can0` on the
> Pi, which is also how production provisioning will work.

### Preferred path — from the Pi

One motor on `can0` at its assigned CAN ID. Motor unpowered mechanically
(no load on the shaft), electrically powered from the PSU.

```bash
# Read-only sanity check (no writes sent).
sudo ./tools/robstride/bench_set_zero_and_save.py \
    --iface can0 --motor-id 0x08 --host-id 0xFD --read-only

# Actually set zero and save to flash.
sudo ./tools/robstride/bench_set_zero_and_save.py \
    --iface can0 --motor-id 0x08 --host-id 0xFD --set-zero --save
```

Expected state transitions:

1. **Before**: `MechOffset` = current stored value. `mechPos` = wherever the
   shaft physically is. `faultSta` = 0, `mechVel` ≈ 0.
2. **After Set Zero (RAM)**: `MechOffset` has moved to make `mechPos` ≈ 0.
3. **After Save to Flash**: values unchanged, but now persisted.

**Power-cycle the motor**, then re-run with `--read-only`. `MechOffset`
MUST match the post-save value. If it reverted to the pre-zero value, the
save did not take — do not proceed.

Record the new `0x2005 MechOffset` in `inventory.yaml:mech_offset_rad`.

### Alternative path — Motor Studio v1.0.3 (Windows)

If for some reason you can't run the Pi path, v1.0.3 *does* have the save
button (labelled "Save" or "Write to Flash" depending on locale) in the
Parameter Settings toolbar:

1. Switch the motor mode to **Operation (MIT)** or **CSP** (see manual §4.2.6
   — PP mode refuses to zero).
2. Click **Set Zero Position** in the Motor Configuration panel.
3. Read back `0x3016 mechPos`. It should be within a few mrad of 0.
4. Click **Save / Write to Flash** in the Parameter Settings toolbar.
5. Power-cycle, reconnect, re-read — verify persistence.

Do NOT use Motor Studio v0.0.13 for this step. Its GUI accepts the Set Zero
click but gives you no way to commit it to flash, leading to silent loss on
the next power cycle.

## Step 6 — Write starter-conservative firmware limits

Values come from `config/actuators/robstride_rs03.yaml` under
`commissioning_defaults`. These are intentionally **much** smaller than
hardware maxima so that during bring-up, a bug or bus glitch cannot produce a
dangerous motion.

For each parameter, use Motor Assistant's **Write Parameters** flow:


| parameter      | index    | value              |
| -------------- | -------- | ------------------ |
| `limit_torque` | `0x700B` | **5.0** Nm         |
| `limit_spd`    | `0x7017` | **2.0** rad/s      |
| `limit_cur`    | `0x7018` | **10.0** A         |
| `canTimeout`   | `0x7028` | **20000** (= 1 s)  |
| `damper`       | `0x702A` | **0** (damping ON) |
| `zero_sta`     | `0x7029` | 1 (report -π..π)   |


Values can be loosened later per-joint as commissioning advances.

## Step 7 — SAVE TO FLASH (very important)

Motor Assistant → Parameter Settings → **Save Parameters** (or equivalently
send communication type 22 from the command box). Without this, every limit
you just wrote is lost the next time the motor loses power.

**Cycle power** to the motor. Re-connect in Motor Assistant. Re-read each of
the parameters from Step 6. They MUST still have the values you wrote. If
any reverted to factory, the save did not stick — DO NOT proceed.

## Step 8 — Assign a unique CAN ID

1. Default from factory is usually `0x7F` (127) on some units and `0x01` on
  others; this is a common footgun for multi-motor buses.
2. Use Motor Assistant **Set ID** (comm type 7) to write the motor's designated
  ID — e.g. `0x08` for shoulder_actuator_a.
3. Power-cycle, reconnect, confirm it answers on its new ID. Some firmwares
  require re-detect after a reboot.
4. Record the new ID in `inventory.yaml:can_id`.

## Step 9 — Jog test (still on the bench)

**Canonical path (Pi, Rust):** `src/driver` `bench_tool` per ADR-0003 and
`docs/decisions/0002-rs03-protocol-spec.md` — velocity mode first, software-capped
on top of firmware limits. Run from the driver crate (release binary optional:
`cargo build --release --bin bench_tool` then `sudo ./target/release/bench_tool`).

**Fallback (Python):** deprecated frozen scripts under `tools/robstride/` — same
caps and flow; use only if `bench_tool` is not yet built on the Pi.

1. **Dry-run** (prints the plan, touches the bus only for type-17 reads):

   ```bash
   cd ~/rudy/src/driver
   sudo cargo run --bin bench_tool -- smoke --iface can1 --motor-id 0x08 --host-id 0xFD -v

   sudo cargo run --bin bench_tool -- jog --iface can1 --motor-id 0x08 --host-id 0xFD \
       --target-vel 0.2 --duration 2.0 -v
   ```

   Fallback:

   ```bash
   cd ~/rudy
   sudo python3 ./tools/robstride/bench_enable_disable.py \
       --iface can1 --motor-id 0x08 --host-id 0xFD -v

   sudo python3 ./tools/robstride/bench_jog_velocity.py \
       --iface can1 --motor-id 0x08 --host-id 0xFD \
       --target-vel 0.2 --duration 2.0 -v
   ```

2. **Smoke test — enable with `spd_ref = 0` (expect no motion):**

   ```bash
   cd ~/rudy/src/driver
   sudo cargo run --bin bench_tool -- smoke --iface can1 --motor-id 0x08 --host-id 0xFD --go -v
   ```

   Fallback:

   ```bash
   sudo python3 ./tools/robstride/bench_enable_disable.py \
       --iface can1 --motor-id 0x08 --host-id 0xFD --go -v
   ```

   PASS criteria: tool exits 0, peak `|mechVel|` stays below 0.1 rad/s while
   enabled, cleanup leaves `run_mode = 0`.

3. **First jog — velocity ramp (expect slow smooth motion):**

   ```bash
   cd ~/rudy/src/driver
   sudo cargo run --bin bench_tool -- jog --iface can1 --motor-id 0x08 --host-id 0xFD \
       --target-vel 0.2 --duration 2.0 --go -v
   ```

   Fallback:

   ```bash
   sudo python3 ./tools/robstride/bench_jog_velocity.py \
       --iface can1 --motor-id 0x08 --host-id 0xFD \
       --target-vel 0.2 --duration 2.0 --go -v
   ```

   PASS criteria: exit 0, shaft moves in the commanded direction, no watchdog
   trip (`|mechVel|` must stay below 1 rad/s with the stock caps).

4. **Limit enforcement — prove `limit_spd` clamps in firmware:**

   ```bash
   cd ~/rudy/src/driver
   sudo cargo run --bin bench_tool -- jog --iface can1 --motor-id 0x08 --host-id 0xFD \
       --go --test-overlimit -v
   ```

   Fallback:

   ```bash
   sudo python3 ./tools/robstride/bench_jog_velocity.py \
       --iface can1 --motor-id 0x08 --host-id 0xFD --go --test-overlimit -v
   ```

   PASS criteria: exit 0, peak `|mechVel|` lands in ~[2.5, 3.2] rad/s while
   `spd_ref` is commanded to 20 rad/s (tool fails if speed exceeds 3.5 rad/s).

5. Record completion in `inventory.yaml` (see Step 10).

<details>
<summary>Fallback: Motor Assistant GUI jog (Windows + CAN dongle)</summary>

1. In Motor Assistant → Control Demo → select Operation Mode.
2. Click **JOG+** / **JOG-**. The motor should turn slowly (~1 rad/s) and stop
   when released.
3. Try to command something that would violate the firmware limits (e.g. lower
   `limit_spd` in parameters, then command a faster jog). The motor should
   refuse to exceed the configured limit. If it does exceed the limit, the save
   did not stick or the firmware does not enforce that parameter — STOP, do not
   deploy the motor.

</details>

## Step 10 — Mark verified

Only after **every** step above passed — including Pi Step 9 substeps — set:

- `inventory.yaml:enable_disable_verified: true` (after §9.2 smoke test)
- `inventory.yaml:jog_verified: true` (after §9.3 velocity jog)
- `inventory.yaml:limit_spd_enforcement_verified: true` (after §9.4 overlimit)

Then set `inventory.yaml:verified: true` and
`commissioned_at: <ISO 8601 timestamp>`.

Commit the inventory change with a message like
`commission: verify shoulder_actuator_a (FW 0.3.1.10, ID 0x08)`.

---

## Known footguns

- **Type-18 write is RAM-only.** A pristine-looking motor in Motor Assistant
may revert limits the next time it power-cycles. Always run Step 7 +
verify.
- **Zero in PP mode is silently refused.** The upper computer shows "OK" but
nothing changes.
- **Zero in old firmware jumps toward the commanded position.** Keep motors
unloaded until at least one zero has been set on new firmware.
- **Default `canTimeout = 0` means "no timeout".** If you leave this at 0,
a wedged ROS controller leaves the motor frozen at its last setpoint
indefinitely. Always set it non-zero in production.
- `**run_mode` change without stopping the motor first is forbidden** by the
manual's Precautions §2. Stop → change mode → enable → run.

