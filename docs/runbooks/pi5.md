# Runbook: Raspberry Pi 5 (Rudy onboard)

## Preconditions

- **OS**: Ubuntu LTS for Raspberry Pi (aarch64). 24.04 LTS is the documented baseline; newer releases work for **`rudydae` + SocketCAN** (no ROS packages on the Pi for now).
- **Time sync**: `chrony` installed by `deploy/pi5/setup_pi5.sh` (important for logs and any future distributed stack).

**ROS 2 on the Pi:** deferred. The desktop [`ros/`](../../ros/) workspace and CI still use Jazzy; onboard ROS/`ros2_control` will return when the Pi `driver_node` path is implemented. Do not add `packages.ros.org` apt sources on the Pi unless you are intentionally matching a supported Ubuntu series for Jazzy (currently **noble**).

## CAN HAT (Waveshare 2-CH, MCP2515)

1. Confirm SPI + MCP2515 overlays match your board revision (oscillator + IRQ GPIOs).
2. On the Pi, either run `sudo bash deploy/pi5/install_can_overlays.sh` or merge the lines from [`deploy/pi5/config.txt.example`](../../deploy/pi5/config.txt.example) into `/boot/firmware/config.txt`.
3. Reboot, then verify interfaces:

```bash
ip link show can0
ip link show can1
```

4. Install and enable the systemd unit:

```bash
sudo bash deploy/pi5/setup_pi5.sh
sudo systemctl start robot-can
```

Or run the setup script once, then bring up manually:

```bash
sudo ./deploy/pi5/can_setup.sh 1000000
```

## Install / deploy `rudydae`

```bash
sudo bash deploy/pi5/install.sh
```

Or sync from your desktop and run the installer remotely:

```bash
./deploy/pi5/deploy.sh jaylamping@rudy-pi.local
```

This path builds the frontend and `rudydae` on the Pi, installs the binary to
`/opt/rudy/bin/rudydae`, writes `/etc/rudy/rudyd.toml`, installs
`rudyd.service`, and starts it.

## CAN debugging checklist

- Termination enabled only at bus ends.
- `candump can0` shows traffic when actuators are powered and addressed correctly.
- `ip -details -statistics link show can0` for error counters / bus-off recovery.
