# RobStride Motor Studio (a.k.a. Motor Assistant / Motor Tool) — v1.0.3

## What this is

Windows GUI tool for configuring RobStride RS-series actuators over CAN-USB.
Used for reading/writing the parameter table, setting mechanical zero, assigning
CAN IDs, and flashing motor firmware.

Officially distributed by RobStride on GitHub; **not** bundled with the motor.

## Why we staged it in-repo

- The version that shipped on the USB stick with joey's motors is **0.0.13**,
  a pre-release that predates the v1.0.x line and cannot successfully flash
  firmware 0.3.1.x (errors out with "Failed to send bin file information"
  during the bin-info handshake).
- v1.0.3 is the current official release as of 2024-11-22, and fixes the
  bootloader handshake against modern RS03 firmware.
- Keeping the exact binary in-repo means the commissioning runbook
  (`tools/robstride/commission.md`) is reproducible and auditable: a future
  contributor (or forensic investigator) can see exactly which Motor Studio
  build was used to write each motor's firmware and parameter table.

## Source

- GitHub release: <https://github.com/RobStride/MotorStudio/releases/tag/v1.0.3>
- Released: 2024-11-22
- Release notes: "更新上位机 / update motorstudio"
- Asset: `motor_toolV13.zip` (20.4 MiB)
- Upstream SHA-256: downloaded live — see `motor_toolV13.zip.sha256` below

## Local copy

- File: `motor_toolV13.zip`
- Size: 21,391,561 bytes
- SHA-256: `77533A7E79440E467A25C87254272D15BA28628C6EB03EE58F881701E1950966`
- Extracted to: `extracted/motor_toolV13/`
- Entry point: `extracted/motor_toolV13/motor_tool.exe` (portable, no installer)

## How to run (Windows desktop)

1. Close any older Motor Assistant / Motor Tool that's currently running.
2. From this repo, double-click:
   `docs\vendor\tools\motor-studio-1.0.3\extracted\motor_toolV13\motor_tool.exe`
3. In the title bar you should see the version string — confirm it's not
   0.0.13 anymore before doing anything with the motor.
4. Proceed with `tools/robstride/commission.md` Step 0.5 (firmware update).

## Known differences vs. 0.0.13

- New English UI (0.0.13 is Chinese-only).
- Correct bootloader handshake for 0.3.1.x firmware line.
- Parameter table layout updated to match 2025–2026 firmware ranges.
- Linux build also available in the same release (not used here).

## License / redistribution

Redistributed under the assumption that the public GitHub release grants a
redistribution license. If upstream objects, delete the `.zip` and
`extracted/` directory; the README and SHA record alone are sufficient for
reproducibility (the binary can always be re-downloaded from the URL above).
