#!/usr/bin/env bash
# Bring up SocketCAN interfaces for Robstride (1 Mbps). Run as root or via systemd.
set -euo pipefail

BR="${1:-1000000}"

for iface in can0 can1; do
  if ip link show "$iface" &>/dev/null; then
    ip link set "$iface" down || true
    ip link set "$iface" up type can bitrate "$BR"
    ip link set "$iface" txqueuelen 65536
    echo "robot-can: $iface up @ ${BR} bps"
  else
    echo "robot-can: $iface not found (check HAT / overlays / boot config)" >&2
  fi
done
