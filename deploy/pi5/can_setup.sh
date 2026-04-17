#!/usr/bin/env bash
# Bring up SocketCAN interfaces for Robstride (1 Mbps). Run as root or via systemd.
set -euo pipefail

BR="${1:-1000000}"

for iface in can0 can1; do
  if ip link show "$iface" &>/dev/null; then
    ip link set "$iface" down || true
    # Explicitly clear loopback/listen-only/triple-sampling/one-shot so that a
    # previously-set diagnostic mode (e.g. an internal loopback test) cannot
    # silently persist across service restarts.  Without this, `ip link set up`
    # alone inherits whatever mode the interface was configured in last.
    ip link set "$iface" type can bitrate "$BR" \
      loopback off listen-only off triple-sampling off one-shot off
    ip link set "$iface" up
    ip link set "$iface" txqueuelen 65536
    echo "robot-can: $iface up @ ${BR} bps (loopback off)"
  else
    echo "robot-can: $iface not found (check HAT / overlays / boot config)" >&2
  fi
done
