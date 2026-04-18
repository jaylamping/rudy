#!/usr/bin/env bash
# First-time Raspberry Pi 5 setup: chrony, CAN tools, robot-can systemd unit.
# ROS 2 on the Pi is deferred — see deploy/pi5/README.md.
# Run on the Pi as a user with sudo. Edit /boot/firmware/config.txt for CAN overlays, reboot, then run this.
set -euo pipefail

echo "== Rudy Pi 5 setup (no ROS on device) =="

# Remove a stale ROS apt source if a previous attempt added one on an unsupported
# Ubuntu series (ROS Jazzy targets noble only).
if [[ -f /etc/apt/sources.list.d/ros2.list ]]; then
  echo "Removing /etc/apt/sources.list.d/ros2.list (ROS on Pi not installed by this script)."
  sudo rm -f /etc/apt/sources.list.d/ros2.list
  sudo rm -f /usr/share/keyrings/ros-archive-keyring.gpg
fi

sudo apt-get update
sudo apt-get install -y chrony can-utils iproute2 curl gnupg lsb-release

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sudo install -m 0755 "${SCRIPT_DIR}/can_setup.sh" /usr/local/bin/robot-can-setup.sh
sudo install -m 0644 "${SCRIPT_DIR}/robot-can.service" /etc/systemd/system/robot-can.service
sudo systemctl daemon-reload
sudo systemctl enable robot-can.service

echo "Done. After CAN overlays in /boot/firmware/config.txt and a reboot: sudo systemctl start robot-can"
