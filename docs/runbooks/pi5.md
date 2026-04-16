# Runbook: Raspberry Pi 5 (Murphy onboard)

## Preconditions

- **OS**: Ubuntu 24.04 LTS for Raspberry Pi (aarch64)
- **ROS**: Jazzy (`ros-jazzy-ros-base` is enough on the Pi)
- **RMW**: `rmw_cyclonedds_cpp` (recommended for Wi‑Fi/Ethernet mixed networks)
- **Time sync**: `chrony` enabled (important for TF + DDS)

## CAN HAT (Waveshare 2-CH, MCP2515)

1. Confirm SPI + MCP2515 overlays match your board revision (oscillator + IRQ GPIOs).
2. Use the example lines in [`deploy/pi5/config.txt.example`](../../deploy/pi5/config.txt.example).
3. After reboot, verify interfaces:

```bash
ip link show can0
ip link show can1
```

4. Install and enable the systemd unit (via `deploy/pi5/setup_pi5.sh`) or run:

```bash
sudo ./deploy/pi5/can_setup.sh 1000000
```

## ROS environment on the Pi

```bash
source /opt/ros/jazzy/setup.bash
source ~/murphy/install/setup.bash  # after deploy
export RMW_IMPLEMENTATION=rmw_cyclonedds_cpp
```

## Deploying new builds from desktop

```bash
colcon build --symlink-install
./deploy/pi5/deploy.sh ubuntu@murphy-pi.local
```

## CAN debugging checklist

- Termination enabled only at bus ends.
- `candump can0` shows traffic when actuators are powered and addressed correctly.
- `ip -details -statistics link show can0` for error counters / bus-off recovery.
