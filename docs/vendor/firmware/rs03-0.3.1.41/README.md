# RS03 firmware 0.3.1.41 (release V26.04.07)

Vendor release source: https://github.com/RobStride/Product_Information/releases/tag/V26.04.07

Published: 2026-04-09

Files in this directory:

| file                    | sha256                                                             |
|-------------------------|--------------------------------------------------------------------|
| `rs03-0.3.1.41.bin`     | `979198DD56091F8CF8A385988BF66A5FAA6F41C56B98D71413F0E7287184B82F` |
| `update.pdf`            | `26D2A999FA81B85324A78FEF7657539E72D9C479B0FC720F2AADC3A393F76355` |

Verify in PowerShell:

```powershell
Get-FileHash docs\vendor\firmware\rs03-0.3.1.41\rs03-0.3.1.41.bin -Algorithm SHA256
```

## Meaningful changes for Rudy vs. 0.3.1.21

Translated from `update.pdf` (Chinese original).

### 0.3.1.29
1. Filter CANopen extended-frame interference.
2. Fix: intermittent zero-set failure when motor is under load.
3. Fix: first-time enable can fail if PSU ramps slowly.
4. Improved initial-position detection at boot.
5. Fix: CANopen enable + reset could break communication.
6. Fix: residual damping after fault-clear.
7. Fix: CANopen couldn't emit all 4 TPDOs simultaneously.
8. CANopen defaults for speed/accel/torque now set at init.
9. Manufacturer-code integrity check fixed.

### 0.3.1.30
- Fixed abnormal states in CANopen mode.

### 0.3.1.41 (current latest — what we are flashing)
1. **MIT mode feature parity** — active reporting, parameter save, and parameter
   read/write now available in MIT protocol. Feedback frame carries status info.
2. Fixed occasional collisions between active reports and feedback messages.
3. Enable/disable feedback frames now sent **after** execution completes (was
   before). Makes our driver's state machine easier to reason about.
4. New: initialization calibration — set `iq_test=1`, reboot; more accurate
   current-sampling baseline, reduces jitter in some conditions.
5. **New: cogging-torque calibration.** Process: `iq_test=1`, reboot,
   mechanically unload the motor, click "cogging calibration" in Motor
   Assistant. On success, `max_alve` becomes nonzero. Then set
   `alveolous_open=1` to enable cogging compensation in operation / MIT mode.
6. Better emergency-stop planning in PP (interpolated position) mode.
7. **New: CANopen watchdog** at OD index `0x6099:1` (nonzero = enabled).
8. **Safer parameter save.** On save, firmware now checks voltage stability
   first. **Added a backup storage region**: if power is lost mid-save, the
   backup is used on next boot and the motor emits a warning frame requesting
   re-save.
9. **New: split PP-mode accel/decel.** Set `acc_stutus=1` to treat them
   independently; deceleration stored at `0x702E`. Default 0 keeps backward
   behavior.
10. Optimized drive current for reduced driver heating; fixes some
    drive-fault edge cases.

## Consequences for Rudy

- **Must re-zero after flash** (per RS03 0.3.1.3 changelog). Old `MechOffset`
  is invalidated.
- **Saved parameters (0x20xx, 0x70xx):** The vendor statement is that firmware
  upgrade preserves stored parameters, but this is a trust-but-verify moment.
  Re-read every parameter from `config/actuators/factory-dumps/…_pre-commission.xlsx`
  after the flash and diff; commit the post-flash export as
  `…_post-flash.xlsx`.
- **New cogging calibration is attractive** — RS03 has a 9:1 gearbox, where
  cogging torque shows up as position tracking error at low speed. Worth
  performing once the motor is back to a known-good state.
- **MIT mode parameter save (0.3.1.41 item 1) is a driver feature we can now
  depend on** — previously we would have had to switch to Operation Control
  mode to save, then back to MIT. Cleaner now.
- **Backup storage region (item 8) lets us trust the "save to flash" step in
  the commissioning runbook.** Before this release, a PSU glitch during save
  could corrupt parameters silently.
