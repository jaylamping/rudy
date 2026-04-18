#!/usr/bin/env bash
# Bootstrap a fresh Raspberry Pi for hands-off Rudy operation.
#
#   sudo bash deploy/pi5/bootstrap.sh
#
# What this does (idempotent):
#   1. Installs minimal runtime deps (no Rust/Node toolchains).
#   2. Ensures CAN HAT overlays are in /boot/firmware/config.txt.
#   3. Installs robot-can.service so can0/can1 come up at boot.
#   4. Installs the rudy-update updater + 1-minute systemd timer.
#   5. Triggers an immediate update so the daemon comes up now.
#
# After this, the Pi will auto-update on every push to main:
#   git push -> CI builds aarch64 release -> Pi pulls within ~60s.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SCRIPT_DIR="${REPO_ROOT}/deploy/pi5"

if [[ "$(id -u)" -ne 0 ]]; then
  exec sudo -E bash "$0" "$@"
fi

echo "== Rudy Pi bootstrap =="
echo "repo:   ${REPO_ROOT}"

if [[ -f /etc/apt/sources.list.d/ros2.list ]]; then
  echo "Removing stale ROS apt source from unsupported Ubuntu release."
  rm -f /etc/apt/sources.list.d/ros2.list /usr/share/keyrings/ros-archive-keyring.gpg
fi

apt-get update
apt-get install -y --no-install-recommends \
  ca-certificates \
  can-utils \
  chrony \
  curl \
  iproute2 \
  jq \
  libcap2-bin \
  openssl \
  rsync

if ! command -v tailscale >/dev/null; then
  echo "NOTE: tailscale not found. Install it first (curl -fsSL https://tailscale.com/install.sh | sh)"
  echo "      then re-run this bootstrap so rudyd binds to its tailnet IP and uses HTTPS."
fi

if [[ ! -f /etc/systemd/system/robot-can.service ]]; then
  install -m 0755 "${SCRIPT_DIR}/can_setup.sh" /usr/local/bin/robot-can-setup.sh
  install -m 0644 "${SCRIPT_DIR}/robot-can.service" /etc/systemd/system/robot-can.service
  systemctl daemon-reload
  systemctl enable robot-can.service
fi

if ! grep -qE '^dtoverlay=mcp2515-can0' /boot/firmware/config.txt; then
  echo "Adding MCP2515 overlays to /boot/firmware/config.txt"
  bash "${SCRIPT_DIR}/install_can_overlays.sh"
  echo
  echo "*** REBOOT REQUIRED to load CAN overlays. ***"
  echo "Re-run this script after reboot to finish bootstrap."
  exit 0
fi

if ! ip link show can0 >/dev/null 2>&1; then
  echo "can0 not present. Running robot-can.service..."
  systemctl restart robot-can.service || true
fi

if ! ip link show can0 >/dev/null 2>&1; then
  echo "ERROR: can0 still missing after robot-can. Check HAT seating, dmesg, and overlays." >&2
  exit 1
fi

install -m 0755 "${SCRIPT_DIR}/rudy-update.sh" /usr/local/bin/rudy-update.sh
install -m 0644 "${SCRIPT_DIR}/rudy-update.service" /etc/systemd/system/rudy-update.service
install -m 0644 "${SCRIPT_DIR}/rudy-update.timer" /etc/systemd/system/rudy-update.timer

systemctl daemon-reload
systemctl enable --now rudy-update.timer

echo
echo "Triggering first update (this builds nothing locally; just downloads the latest release):"
systemctl start rudy-update.service || true

echo
echo "== Bootstrap complete =="
echo
echo "Next steps:"
echo "  - Verify daemon:  systemctl status rudyd --no-pager"
echo "  - Tail logs:      journalctl -u rudyd -f"
echo "  - Tail updater:   journalctl -u rudy-update -f"
echo "  - Auth token:     cat /etc/rudy/rudyd.token"
echo "  - Open UI at:     https://\$(hostname).\$(tailscale status --json | jq -r .MagicDNSSuffix):8443/"
