#!/usr/bin/env bash
# First-time Raspberry Pi 5 setup (Ubuntu 24.04 aarch64): ROS 2 Jazzy base, CycloneDDS, chrony hints, CAN service.
# Run on the Pi as a user with sudo. Review and edit CAN overlays in /boot/firmware/config.txt first.
set -euo pipefail

echo "== Rudy Pi 5 setup =="
echo "Ensure Ubuntu sources include noble-updates and noble-backports (ROS Jazzy on arm64)."
sudo apt-get update
sudo apt-get install -y chrony can-utils iproute2 curl gnupg lsb-release software-properties-common

if ! dpkg -l | grep -q ros-jazzy-ros-base; then
  sudo curl -sSL https://raw.githubusercontent.com/ros/rosdistro/master/ros.key -o /usr/share/keyrings/ros-archive-keyring.gpg
  echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/ros-archive-keyring.gpg] http://packages.ros.org/ros2/ubuntu $(. /etc/os-release && echo $UBUNTU_CODENAME) main" | sudo tee /etc/apt/sources.list.d/ros2.list > /dev/null
  sudo apt-get update
  sudo apt-get install -y ros-jazzy-ros-base ros-jazzy-rmw-cyclonedds-cpp ros-jazzy-ros2cli
fi

echo "export RMW_IMPLEMENTATION=rmw_cyclonedds_cpp" >> ~/.bashrc || true
echo "source /opt/ros/jazzy/setup.bash" >> ~/.bashrc || true

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
sudo install -m 0755 "${SCRIPT_DIR}/can_setup.sh" /usr/local/bin/robot-can-setup.sh
sudo install -m 0644 "${SCRIPT_DIR}/robot-can.service" /etc/systemd/system/robot-can.service
sudo systemctl daemon-reload
sudo systemctl enable robot-can.service

echo "Done. Reboot after editing /boot/firmware/config.txt for CAN overlays, then: sudo systemctl start robot-can"
