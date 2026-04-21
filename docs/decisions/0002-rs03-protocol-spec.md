# ADR 0002: RobStride RS03 CAN protocol canonical spec (2026-04)

## Status

Accepted.

## Context

Rudy uses Chinese-market RobStride RS03 quasi-direct-drive integrated servos
sourced from a mix of AliExpress and eBay storefronts. Firmware versions and
minor hardware revisions (e.g. presence of onboard 240 Ω CAN terminator) vary
between units. Any driver or safety layer we write must be pinned to the
vendor-documented protocol, not forum posts or generic "Mi-Mini-Cheetah
derivative" assumptions.

The authoritative source is `docs/vendor/rs03-user-manual-260112.pdf`, the
January 12 2026 RS03 User Manual published by RobStride on their official
GitHub (`RobStride/Product_Information`).

## Decision

The driver and the actuator config treat the following as **canonical, verbatim
from the vendor manual**. All other values in the codebase must either match
this or call out the deviation in the same file with a rationale.

### Physical layer

| property        | value                                              |
|-----------------|----------------------------------------------------|
| Bus             | Classical CAN 2.0B, **extended 29-bit** identifier |
| Bit rate        | **1 Mbps** (configurable: 1M / 500K / 250K / 125K) |
| Data length     | 8 bytes                                            |
| Termination     | RS03 has two pass-through CAN connectors; only one has the onboard 240 Ω in-circuit. `0x3041 can_status` reports the *currently-wired* state (0 = terminated via this connector, 1 = not). Plan bus topology accordingly. |
| Rated voltage   | 48 VDC (op range 24–60 VDC)                        |
| Peak torque     | 60 Nm                                              |
| Peak phase cur. | 43 Apk                                             |
| Rated phase cur.| 13 Apk                                             |

### CAN arbitration ID layout (29-bit extended)

```
bits 28..24 : communication type  (5 bits)
bits 23..8  : data area 2         (16 bits, usually host CAN_ID + aux)
bits  7..0  : destination address (8 bits, motor CAN_ID)
```

### Communication types (types we care about)

| type dec | type hex | purpose                                                  |
|---------:|:--------:|----------------------------------------------------------|
|  0       | 0x00     | Get device ID + 64-bit MCU UID                           |
|  1       | 0x01     | Operation (MIT) control: pos, vel, kp, kd, tff           |
|  2       | 0x02     | Motor feedback frame (response)                          |
|  3       | 0x03     | Enable motor run                                         |
|  4       | 0x04     | Stop motor. `byte[0]=1` clears fault                     |
|  6       | 0x06     | Set mechanical zero. `byte[0]=1`                         |
|  7       | 0x07     | Change motor CAN_ID (immediate)                          |
| 17       | 0x11     | Read single parameter                                    |
| 18       | 0x12     | Write single parameter (**RAM only, lost on power-off**) |
| 21       | 0x15     | Fault feedback frame                                     |
| 22       | 0x16     | **Save parameters to flash (0x20xx range)**              |
| 23       | 0x17     | Change baud rate (effective after power cycle)           |
| 24       | 0x18     | Active reporting enable/disable (default 10 ms interval; `cortex` applies this at boot and persists via type-22 on first successful setup) |
| 25       | 0x19     | Change protocol: 0=private / 1=CANopen / 2=MIT           |
| 26       | 0x1A     | Version number read                                      |

### Operation-control (MIT) frame encoding

Type 1 frame, 8 bytes, big-endian within each field:

| byte(s) | field      | raw range     | physical range       |
|--------:|------------|---------------|----------------------|
| ID b16-23 | torque ff | 0..65535     | −60..+60 Nm          |
| 0..1    | target pos | 0..65535      | −4π..+4π rad         |
| 2..3    | target vel | 0..65535      | −20..+20 rad/s       |
| 4..5    | kp         | 0..65535      | 0..5000              |
| 6..7    | kd         | 0..65535      | 0..100               |

**Prior code in this repo used kp [0,500] and kd [0,5]. That is wrong by 10× and
20×. It MUST be fixed before sending MIT frames.**

### Enable motor run (type 3)

Per vendor manual §4.1.4, the host sends comm type `0x03` with arbitration ID
`[0x03][host_id][motor_id]`. The **8-byte data payload is don't-care** (all-zero
padding is fine on transmit); the motor enters the enabled run state according
to the current `run_mode` and RAM setpoints (`spd_ref`, `loc_ref`, MIT stream,
etc.).

Bench tooling: `tools/robstride/rs03_can.py:cmd_enable()`.

### Motor feedback frame (type 2)

Emitted while the motor is active (after enable). **Canonical field packing is
vendor manual §4.1.3**; the table below is what
`tools/robstride/rs03_can.py:decode_motor_feedback()` implements for Pi-side
commissioning scripts.

**Empirical check (required on first bring-up):** run
`tools/robstride/bench_enable_disable.py --go --verbose` and compare decoded
`pos_rad` / `vel_rad_s` against type-17 reads of `0x7019` / `0x701B` at the same
instant. If the scaling or byte order is wrong, fix `decode_motor_feedback()`
and update this table.

**Reply arbitration ID** (29-bit extended, as seen on the host RX path — motor
is source):

```
bits 28..24 : 0x02               (comm type)
bits 23..16 : source motor_id (8 bits)
bits 15..8  : status            (fault/mode bits; see manual §4.1.3 / §3.3.7)
bits  7..0  : destination host_id
```

**Data payload** (8 bytes, **big-endian 16-bit words**):

| bytes | field        | encoding |
|------:|--------------|----------|
| 0..1  | mechPos raw | `uint16` BE → linear map −4π..+4π rad |
| 2..3  | mechVel raw  | `uint16` BE → linear map −20..+20 rad/s |
| 4..5  | torque raw   | `uint16` BE → linear map −60..+60 Nm |
| 6..7  | MOS temp raw | `uint16` BE → physical °C = raw / 10 |

### Run modes (`run_mode`, parameter `0x7005`, uint8)

| value | mode                          |
|------:|-------------------------------|
| 0     | Operation (MIT)               |
| 1     | Position mode (PP, profiled)  |
| 2     | Velocity mode                 |
| 3     | Current mode                  |
| 5     | Position mode (CSP, cyclic)   |

**Cortex (post-home hold).** After a successful home-ramp, `cortex` leaves the
joint in **PP** (`run_mode = 1`) with `loc_ref` set to the principal-angle home
target (`wrap_to_pi` of `predefined_home_rad` / HTTP target — see
[ADR 0005](./0005-angle-units-and-frames.md)). Sequence on the bus worker:
`cmd_stop` → `RUN_MODE=1` → `LOC_REF` → `cmd_enable`. The next operator jog
(`spd_ref`) forces a velocity-mode re-arm (`run_mode = 2`) via
`state.position_hold` + smart re-arm in `Cmd::SetVelocity`.

### Safety-critical parameters

Writable via type 18, **persisted to flash only after type 22 save**. Firmware
loses uncommitted type-18 writes on every power cycle — this is the #1 footgun
when configuring firmware-level joint limits.

| index    | name         | type   | range         | role                                          |
|:--------:|--------------|--------|---------------|-----------------------------------------------|
| `0x7005` | run_mode     | uint8  | 0,1,2,3,5     | Control mode select                           |
| `0x7006` | iq_ref       | float  | −43..43 A     | Current-mode setpoint                         |
| `0x700A` | spd_ref      | float  | −20..20 rad/s | Velocity-mode setpoint                        |
| `0x700B` | **limit_torque** | float | 0..60 Nm  | **Hard torque clamp, all modes**              |
| `0x7016` | loc_ref      | float  | rad           | Position-mode setpoint                        |
| `0x7017` | **limit_spd**| float  | 0..20 rad/s   | **Hard speed clamp, CSP / operation modes**   |
| `0x7018` | **limit_cur**| float  | 0..43 A       | **Hard phase-current clamp, vel/pos modes**   |
| `0x7022` | acc_rad      | float  | rad/s²        | Velocity-mode accel                           |
| `0x7024` | vel_max      | float  | rad/s         | PP-mode max speed                             |
| `0x7025` | acc_set      | float  | rad/s²        | PP-mode accel                                 |
| `0x7028` | canTimeout   | uint32 | counts (20000 = 1 s) | **Motor fails to reset if no CAN cmd in N** |
| `0x7029` | zero_sta     | uint8  | 0 or 1        | 0 → reports 0..2π, 1 → reports −π..π          |
| `0x702A` | damper       | uint8  | 0 or 1        | 1 = disable post-power-off backdrive braking  |
| `0x702B` | add_offset   | float  | rad           | Zero-point offset                             |

Observables (read-only) relevant to bring-up (0x70xx shadow = type-17 readable,
0x30xx original = type-17 **NOT** readable, see "Parameter-read scope" below):

| shadow idx | orig idx | name           | type   | meaning                                  |
|:----------:|:--------:|----------------|--------|------------------------------------------|
| `0x7019`   | `0x3016` | mechPos        | float  | load-end mechanical angle (post-gearbox) |
| `0x701B`   | `0x3017` | mechVel        | float  | load-end speed                           |
| `0x701A`   | `0x301E` | iqf            | float  | filtered q-axis current                  |
| `0x701C`   | `0x300C` | VBUS           | float  | bus voltage                              |
| n/a        | `0x3022` | faultSta       | uint32 | fault bit field (see manual §3.3.7)      |
| n/a        | `0x3041` | can_status     | uint8  | 0 = onboard 240 Ω in-circuit via currently-wired connector |
| n/a        | `0x1003` | AppCodeVersion | str    | firmware version (e.g. "0.3.1.41")       |

### Parameter-read scope (type 17): 0x70xx only

**This is the #2 footgun after flash-vs-RAM writes, and it bit us in Step 9.**

Type-17 "Read single parameter" does NOT expose the full Motor Studio
parameter table.  It can ONLY address indices in the **0x70xx namespace**
listed in vendor manual §4.1.14.  Anything else (0x20xx stored config,
0x30xx observables, 0x10xx version strings) reports as a failed read:

- Reply frame still arrives, correctly source-swapped, with the requested
  index echoed back in bytes 0-1.
- But value bytes 4-7 are zero.
- And bit 16 of the reply arbitration ID is set (`[0x11][0x01][motor][host]`
  instead of `[0x11][0x00][motor][host]`).  That bit is the motor's
  "read failed" status flag, documented as "0 = read OK, 1 = read failed"
  in §4.1.6 (description truncated in the PDF but confirmed empirically).

Observable-parameter access paths by namespace:

| namespace | what's in it               | type-17 single read? | other access                     |
|-----------|----------------------------|----------------------|----------------------------------|
| `0x10xx`  | firmware version strings   | no                   | type-26 version-read frame       |
| `0x20xx`  | stored config (MechOffset, CAN_ID, limit_*, baud, …) | no                   | Motor Studio type-0x13 bulk export; individual fields via type-7 / type-23 / type-25 as documented |
| `0x30xx`  | runtime observables        | no (use 0x70xx shadow) | Motor Studio type-0x13 bulk export |
| `0x70xx`  | runtime observables + settable safety params | **yes** (§4.1.14 is the canonical list) | also via Motor Studio |

Implication for our driver: the type-17 shadow at `0x70xx` is our only single-
frame read path from the Pi.  We use it for observability (mechPos, mechVel,
iqf, VBUS) and for verifying that firmware-level limits written via type-18
were accepted.  `faultSta` is NOT reachable via type-17; we depend on the
type-21 fault-feedback frame or the type-2 motor-feedback frame (embedded
fault bits in bits 16..21 of the reply arb ID) for fault visibility.

### Parameter-write frame layout (type 18)

```
ID:   [type=0x12][host_id][motor_can_id]     (29-bit)
data: [idx_lo][idx_hi][0x00][0x00][val0][val1][val2][val3]
```

Value bytes are little-endian within the 4-byte payload (floats are IEEE-754
single-precision).

### Zero-calibration rules

- Only supported in **Operation (MIT)** and **CSP** modes. PP mode refuses.
- On firmware `>= 0.3.1.9`, zeroing in CSP/MIT atomically updates the setpoint
  to 0 so the motor does not leap. On older firmware it will leap toward the
  prior commanded position.
- After zeroing, issue a type-22 save or the zero is lost on power cycle.
  For Rudy’s operator console, the persisted commissioning path is
  `POST /api/motors/:role/commission` (type-6 + type-22 + `add_offset` readback),
  not ad-hoc type-6 alone — see **Commissioned mechanical zero (cortex)** below.

### Commissioned mechanical zero (cortex)

The RS03 exposes the running position offset as parameter **`0x702B`
`add_offset`** (radians). Together with **type-6 Set mechanical zero** and
**type-22 Save parameters to flash**, this is how a joint’s “commissioned
neutral” survives power cycles.

**Invariant.** After commissioning, the daemon stores the firmware’s confirmed
`add_offset` in `config/actuators/inventory.yaml` as
`commissioned_zero_offset`. On every subsequent boot it re-reads `0x702B` over
CAN. If the live value differs from the stored baseline by more than
`safety.commission_readback_tolerance_rad` in `config/cortex.toml`, the motor
enters `BootState::OffsetChanged` and motion is refused until the operator
either re-runs commission (new baseline) or calls
`POST /api/motors/:role/restore_offset` (write stored value back to firmware
and save).

**Operator persistence path.** `POST /api/motors/:role/commission` in `cortex`
sequences: type-6 SetZero → type-22 SaveParams → type-17 readback of
`add_offset` → atomic inventory write. This is the **only** supported way to
persist a new zero through the SPA “Commission Zero” flow.

**Diagnostic RAM path.** `POST /api/motors/:role/set_zero` issues type-6 (and
related RAM writes) **without** type-22 unless the operator uses a separate
save path. It is gated as an advanced/diagnostic action and does **not** update
`commissioned_zero_offset`. Operators should prefer **Commission** for anything
intended to survive reboot.

**Boot orchestration.** When `commissioned_zero_offset` is set and readback
matches, and the wrapped position is inside `travel_limits`, and
`safety.auto_home_on_boot` is true, `cortex` may drive the joint to
`predefined_home_rad` (default `0.0`) via the same home-ramp loop as manual
`POST /home`, then mark `BootState::Homed`. Motors with no commissioned baseline
still require the manual **Verify & Home** ritual each boot.

**Out-of-band behavior.** If the joint boots outside `travel_limits`, it stays
`OutOfBand` until the operator moves it into band; the daemon **does not**
auto-ramp from out-of-band (historical “Layer 6” auto-recovery was removed in
favor of explicit operator recovery and the commissioned-zero path).

### Firmware-version gating

| feature                                      | requires         |
|---------------------------------------------- |------------------|
| Zero-position setting without motion leap    | ≥ 0.3.1.9        |
| `add_offset` position-offset parameter       | ≥ 0.3.1.9        |
| Disable reverse-drive damping (`damper=1`)   | ≥ 0.3.1.4        |
| CAN-terminator flag (`can_status`)           | ≥ `APP_V0311_V1001_20250507` |
| Zero-point dead zone                         | ≥ 0.3.1.6        |
| `limit_torque` / `limit_spd` / `limit_cur`   | all documented revisions |
| MIT-mode parameter read/write + save         | ≥ 0.3.1.41       |
| Cogging-torque calibration                   | ≥ 0.3.1.41       |
| Backup parameter-storage region              | ≥ 0.3.1.41       |

### Cogging-torque calibration (≥ 0.3.1.41)

A one-shot per-motor calibration introduced in 0.3.1.41 that measures
per-angle cogging torque so the firmware can compensate for it in
MIT / operation mode. Runs unloaded; the motor spins the shaft itself.

Parameters involved (all present in this unit's 0.3.1.41 dump):

| index   | name             | type  | role                                              |
|---------|------------------|-------|---------------------------------------------------|
| 0x2028  | `alveolous_open` | uint8 | Runtime toggle: 1 = apply cogging compensation    |
| 0x2029  | `iq_test`        | uint8 | Arm flag: set to 1 then reboot before calibrating |
| 0x304a  | `max_alve`       | float | Read-only: nonzero after a successful calibration |
| 0x304b  | `max_alve2`      | float | Read-only, secondary metric (undocumented)        |

Procedure (from `docs/vendor/firmware/rs03-0.3.1.41/README.md`):

1. Unload the shaft mechanically (must be free to rotate).
2. Write `iq_test = 1`, save, power-cycle.
3. Trigger "Cogging Calibration" from Motor Studio. Motor spins
   autonomously; do not touch.
4. On completion, verify `max_alve` is nonzero. If zero, calibration
   failed -- repeat.
5. Write `alveolous_open = 1` to enable compensation in runtime.
6. Power-cycle and verify both values persist.

The driver crate MUST NOT trigger calibration at runtime. It is a
commissioning-time-only procedure gated behind the commissioning
runbook. The driver MAY read `max_alve` at bring-up and refuse to
enable the motor if `alveolous_open=1` but `max_alve=0` (indicating
a corrupt or partial calibration).

### `warnSta` (0x3023) advisory bit catalogue

The vendor manual does not publish the `warnSta` bitfield. Observed
behavior on firmware 0.3.1.41, AppCodeName `z_motor`:

| bit | value | observed behavior                                                        |
|-----|-------|--------------------------------------------------------------------------|
| 5   | 32    | Set persistently. Survives PSU cycle. Cleared briefly by factory-reset   |
|     |       | + re-zero, but returned on next PSU cycle. Working hypothesis: "cogging  |
|     |       | calibration not run" advisory -- consistent with `max_alve=0` on every   |
|     |       | observed dump. Unverified. Motor jogs normally with this bit set.        |

We tolerate bit 5 for now. The driver MAY log it at bring-up but MUST
NOT refuse to enable on it alone. Any *other* bits in `warnSta` (i.e.
`warnSta != 0` AND `warnSta != 32`) ARE a real warning and SHOULD
block enable until investigated.

## Consequences

- **`cortex` commissioning** MUST treat type-6 + type-22 + `0x702B` readback as
  the persisted zero workflow; see **Commissioned mechanical zero (cortex)**
  above and `docs/operator-guide/commissioning.md`.
- The driver crate MUST encode/decode these frames per the table above, with
  unit tests covering the exact worked examples in §4.4 of the manual.
- Before any firmware-level limits are relied upon for safety, per-motor
  firmware version and `can_status` must be recorded in
  `config/actuators/inventory.yaml`.
- Firmware-level limits are set via the RobStride **Motor Assistant** GUI (or
  equivalent CAN commands), never silently re-written from our driver at
  runtime. They are the bottom-most software safety layer and must be auditable.
- The anti-backdrive damping default (`damper=0`, i.e. damping ON when
  unpowered) is an additional passive safety feature. Do not change it without
  explicit reason.
- `canTimeout` (`0x7028`) should be set to a non-zero value in production so
  that a controller crash or network stall forces the motor into reset mode,
  not frozen at last command.
- **Kt and rated current are per-unit empirical values**, not constants. The
  first motor we inspected (FW 0.3.1.21, AppCodeName `z_motor`) reports
  `Kt_Nm/A = 1.53` and `rated_i = 9 A`, vs. manual §1.3 values of 2.36 Nm/Arms
  and 13 Apk. Driver torque↔current conversions must read `0x303c Kt_Nm/Amp`
  from each motor, cache it in `inventory.yaml`, and reject motors whose Kt
  differs unexpectedly between inventory and live read.
- The CAN protocol reserves 8 bits for "host CAN_ID". The motor stores its
  expected host in `0x200A CAN_MASTER` (default 0xFD = 253). The driver must
  send frames with this host ID or writes/reads may be silently ignored on
  firmware revisions that filter.
- Some gray-market motors report `AppCodeName = z_motor` instead of `rs-03`.
  This is tolerated but logged; if two motors in the same inventory disagree
  on `AppCodeName`, that is a commissioning red flag (possible aftermarket
  firmware) and warrants manual investigation.
- **`can_status` (0x3041) reflects the *currently-wired* termination state,
  not just what's physically on the PCB.** The RS03 has two pass-through CAN
  connectors on opposite sides of the housing; only one of them has the
  onboard 240 Ω terminator in-circuit. Moving the harness from one side of a
  motor to the other causes `can_status` to flip between 0 and 1. This is a
  feature, not a bug: it means the firmware tells you whether the motor is
  currently acting as an end-of-bus termination, which you want to audit at
  commissioning time. Topology rule for a multi-motor bus:
  - CAN-USB adapter at one end, its own 120 Ω terminator on.
  - All motors *except* the far-end motor wired on their NON-terminated side
    (`can_status == 1`), i.e. acting as pass-throughs.
  - Far-end motor wired on its terminated side (`can_status == 0`).
  Record `can_status` per-motor in `inventory.yaml` *after the harness is in
  its final configuration*, not from a standalone bench dump.
- **Firmware flashing requires Motor Studio v1.0.3+**, not the 0.0.13 build
  that ships on vendor USB sticks. The older tool fails the bootloader
  handshake for firmware 0.3.1.x with "Failed to send bin file information!".
  Use the mirrored copy at `docs/vendor/tools/motor-studio-1.0.3/`.

## References

- `docs/vendor/rs03-user-manual-260112.pdf` (RobStride, 2026-01-12).
- `https://github.com/RobStride/Product_Information` (firmware changelog).
