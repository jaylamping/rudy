# Raspberry Pi 5 deployment (Rudy)

Scripts in this directory target **Ubuntu 24.04 (aarch64)** on a Raspberry Pi 5 with the **Waveshare 2-CH CAN HAT** (MCP2515).

## Files

| File | Purpose |
|------|---------|
| `Dockerfile.pi5` | Cross-compilation image (Rust + aarch64 toolchain) |
| `setup_pi5.sh` | One-time Pi setup: ROS 2 Jazzy base, CycloneDDS, CAN systemd unit |
| `can_setup.sh` | Bring up `can0` / `can1` at 1 Mbps |
| `rudy-can.service` | systemd unit (installed to `/etc/systemd/system/`) |
| `deploy.sh` | `rsync` of `install/` + `config/` to the Pi |
| `config.txt.example` | Example `/boot/firmware/config.txt` lines for SPI + MCP2515 overlays |

## Quick start

1. Flash **Ubuntu 24.04** for Pi 5, ensure `noble-updates` / `noble-backports` are enabled (ROS Jazzy arm64).
2. Copy CAN overlay lines from `config.txt.example` into `/boot/firmware/config.txt`, reboot.
3. On the Pi: `sudo bash deploy/pi5/setup_pi5.sh`
4. On your desktop: `colcon build`, then `./deploy/pi5/deploy.sh user@pi.local`

See [docs/runbooks/pi5.md](../../docs/runbooks/pi5.md) for full details.
