# Raspberry Pi 5 deployment (Rudy)

Scripts in this directory target **Ubuntu LTS (aarch64)** on a Raspberry Pi 5 with the **Waveshare 2-CH CAN HAT** (MCP2515).

**Onboard today:** `rudydae` + SocketCAN + systemd (`robot-can`, `rudyd`). **ROS 2 on the Pi is not installed** by these scripts; it will return when `driver_node` / `ros2_control` integration is implemented (desktop `ros/` workspace unchanged).

## Files


| File                 | Purpose                                                              |
| -------------------- | -------------------------------------------------------------------- |
| `Dockerfile.pi5`     | Cross-compilation image (Rust + aarch64; Jazzy base matches CI)     |
| `setup_pi5.sh`       | One-time Pi setup: chrony, CAN utils, `robot-can` systemd unit       |
| `can_setup.sh`       | Bring up `can0` / `can1` at 1 Mbps                                   |
| `robot-can.service`  | systemd unit (installed to `/etc/systemd/system/`)                   |
| `install.sh`         | Builds frontend + `rudydae` on the Pi and installs `/opt/rudy` + `/etc/rudy` |
| `deploy.sh`          | Syncs repo sources to the Pi and runs `install.sh` remotely          |
| `config.txt.example` | Example `/boot/firmware/config.txt` lines for SPI + MCP2515 overlays |
| `install_can_overlays.sh` | Idempotent append of those lines to `/boot/firmware/config.txt` (run on Pi) |


## Quick start

1. Flash **Ubuntu LTS** for Pi 5 (24.04 recommended; newer LTS/non‑LTS works for `rudydae`-only).
2. On the Pi: `sudo bash deploy/pi5/install_can_overlays.sh` (or merge `config.txt.example` into `/boot/firmware/config.txt`), then reboot.
3. On the Pi: `sudo bash deploy/pi5/setup_pi5.sh`
4. Either on the Pi: `sudo bash deploy/pi5/install.sh`, or from your desktop: `./deploy/pi5/deploy.sh user@pi.local`

See [docs/runbooks/pi5.md](../../docs/runbooks/pi5.md) for full details.

## Gotcha: `can0` / `can1` vs silkscreen labels

On the Waveshare 2-CH CAN HAT, the **silkscreen labels on the PCB** ("CAN0" / "CAN1" near the screw terminals) are *not* guaranteed to match the **Linux interface names** (`can0` / `can1`). The Linux names come from the order in which the two `mcp2515-canX` device tree overlays are registered, whereas the silkscreen labels come from the PCB designer's intent.

Empirically on **our** Pi 5 with the overlays in `config.txt.example` (`mcp2515-can0,interrupt=23` + `mcp2515-can1,interrupt=25`), the mapping is:


| silkscreen label | SPI CE | interrupt GPIO | **Linux iface** |
| ---------------- | ------ | -------------- | --------------- |
| `CAN0`           | CE0    | GPIO 23        | `**can1**`      |
| `CAN1`           | CE1    | GPIO 25        | `**can0**`      |


**Translation: if you plug your wires into the screw terminals labeled "CAN0" on the board, talk to `can1` in software.** This is counter-intuitive but matches what a `candump` sanity test confirmed 2026-04-17 (full RobStride RS03 parameter export received on `can1`, nothing on `can0`, with wires in the silkscreen-"CAN0" terminal).

If you move the wires to the other terminal, swap the iface name. If you re-order the overlays in `config.txt`, the Linux names swap too. There is no single "right" mapping — always verify with `candump` against a known-talking device on the bus.

Our convention going forward: **shoulder_actuator_a is on Linux `can1`** (silkscreen "CAN0").
`inventory.yaml` is the source of truth for which iface each motor is on.
