#!/usr/bin/env bash
# Bring up SocketCAN interfaces for Robstride (1 Mbps). Run as root or via systemd.
set -euo pipefail

BR="${1:-1000000}"

missing=0
for iface in can0 can1; do
  if ip link show "$iface" &>/dev/null; then
    ip link set "$iface" down || true
    # Explicitly clear loopback/listen-only/triple-sampling/one-shot so that a
    # previously-set diagnostic mode (e.g. an internal loopback test) cannot
    # silently persist across service restarts.  Without this, `ip link set up`
    # alone inherits whatever mode the interface was configured in last.
    #
    # `restart-ms 100` lets the kernel auto-recover from BUS-OFF without
    # needing a manual `ip link` cycle (typical cause: a node is hot-swapped
    # or the bus loses termination briefly during a connector reseat).
    # ERROR-PASSIVE recovers automatically once ACKs resume; this only helps
    # the harder BUS-OFF case, but the cost is zero.
    ip link set "$iface" type can bitrate "$BR" restart-ms 100 \
      loopback off listen-only off triple-sampling off one-shot off
    ip link set "$iface" up
    ip link set "$iface" txqueuelen 65536
    echo "robot-can: $iface up @ ${BR} bps (loopback off, restart-ms 100)"
  else
    echo "robot-can: $iface not found (check HAT / overlays / boot config)" >&2
    missing=1
  fi
done

# Fail the unit if either expected interface is missing — silently degrading
# to "only can1 came up" leads to confusing 'discovered=0' scans hours later.
exit "$missing"
