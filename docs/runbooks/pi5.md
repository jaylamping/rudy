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

The Pi pulls prebuilt aarch64 releases from GitHub Actions on a 60-second
timer. You do not build on the Pi any more.

**One-time bootstrap on a fresh Pi (after Tailscale + cert):**

```bash
git clone https://github.com/jaylamping/rudy ~/rudy
sudo bash ~/rudy/deploy/pi5/bootstrap.sh
```

That installs `rudy-update.timer`, which polls the latest GitHub Release.
On every push to `main`, [`.github/workflows/release.yaml`](../../.github/workflows/release.yaml)
cross-builds `rudydae` for `aarch64-unknown-linux-gnu`, bundles the SPA,
and publishes the tarball + `latest.json` manifest. Within ~60s of a green
build, the Pi downloads it (sha256-verified) and restarts `rudyd`.

**Day-to-day:**

```bash
# Force an immediate update check
sudo systemctl start rudy-update

# Watch deploys land
journalctl -u rudy-update -f
journalctl -u rudyd -f

# What commit is running?
cat /opt/rudy/current.sha
```

**Emergency build-on-Pi path (offline, no GHA):**

```bash
sudo bash deploy/pi5/install.sh   # legacy; compiles locally
```

## CAN debugging checklist

- Termination enabled only at bus ends.
- `candump can0` shows traffic when actuators are powered and addressed correctly.
- `ip -details -statistics link show can0` for error counters / bus-off recovery.
