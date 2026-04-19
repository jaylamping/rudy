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
#   5. Configures `tailscale serve` to front rudyd at https://<host>/.
#   6. Triggers an immediate update so the daemon comes up now.
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

# Pin each CAN interface's hard IRQ to the same CPU rudydae will pin
# its per-bus worker thread to. The kernel runs the SocketCAN softirq
# on whichever CPU received the hard IRQ, so co-locating IRQ + worker
# eliminates an inter-core hop on every received frame and keeps the
# RX path resident in the worker core's L1/L2.
#
# Mapping rule mirrors `crates/rudydae/src/can/bus_worker.rs::auto_assign_cpu`:
# the first iface (sorted) goes on core 1, the second on core 2, etc.,
# leaving core 0 for the kernel + tokio runtime + axum / WebTransport.
#
# Idempotent — safe to re-run on every bootstrap.
pin_can_irqs() {
  local cpu_count
  cpu_count=$(nproc)
  if [[ "${cpu_count}" -lt 2 ]]; then
    echo "Skipping CAN IRQ pinning: ${cpu_count} CPU(s) available."
    return 0
  fi

  local idx=0
  for iface in $(ip -o link show type can | awk -F': ' '{print $2}' | sort); do
    # Skip if iface didn't make it up.
    if ! ip link show "${iface}" >/dev/null 2>&1; then
      continue
    fi
    # /proc/interrupts rows look like:
    #   23:    1234567   ...   spi0.0   <iface>
    # Pull the leading IRQ number for the row whose final column is
    # this iface. Using `awk` rather than grep so multi-word iface
    # names can never confuse the match.
    local irq
    irq=$(awk -v want="${iface}" '$NF == want { sub(":", "", $1); print $1; exit }' /proc/interrupts)
    if [[ -z "${irq}" ]]; then
      echo "WARN: no /proc/interrupts row for ${iface}; skipping IRQ pin."
      continue
    fi
    local cpu=$(( 1 + (idx % (cpu_count - 1)) ))
    if [[ -w "/proc/irq/${irq}/smp_affinity_list" ]]; then
      echo "${cpu}" > "/proc/irq/${irq}/smp_affinity_list" || \
        echo "WARN: failed to pin IRQ ${irq} (${iface}) to CPU ${cpu}."
      echo "Pinned ${iface} (IRQ ${irq}) to CPU ${cpu}."
    else
      echo "WARN: /proc/irq/${irq}/smp_affinity_list not writable; skipping ${iface}."
    fi
    idx=$((idx + 1))
  done
}

pin_can_irqs

install -m 0755 "${SCRIPT_DIR}/rudy-update.sh" /usr/local/bin/rudy-update.sh
install -m 0644 "${SCRIPT_DIR}/rudy-update.service" /etc/systemd/system/rudy-update.service
install -m 0644 "${SCRIPT_DIR}/rudy-update.timer" /etc/systemd/system/rudy-update.timer

systemctl daemon-reload
systemctl enable --now rudy-update.timer

# Wire `tailscale serve` to terminate TLS at the tailnet IP and proxy to the
# rudyd plaintext loopback listener. `tailscale serve` config is persistent
# across reboots; it re-applies itself after `tailscaled` restarts. We also
# re-assert this from `apply-release.sh` on every release in case it drifted.
if command -v tailscale >/dev/null && tailscale status >/dev/null 2>&1; then
  echo "Configuring tailscale serve (https://\$(hostname)/ -> 127.0.0.1:8443)..."
  tailscale serve --bg --https=443 http://127.0.0.1:8443 || \
    echo "WARN: tailscale serve setup failed; rerun once the daemon is up."
else
  echo "WARN: tailscale not up; skipping \`tailscale serve\` setup."
  echo "      Once tailscale is logged in, run:"
  echo "        sudo tailscale serve --bg --https=443 http://127.0.0.1:8443"
fi

echo
echo "Triggering first update (this builds nothing locally; just downloads the latest release):"
systemctl start rudy-update.service || true

HOST_SHORT="$(hostname -s)"
TAILNET_URL="https://${HOST_SHORT}/"

echo
echo "== Bootstrap complete =="
echo
echo "Next steps:"
echo "  - Verify daemon:    systemctl status rudyd --no-pager"
echo "  - Tail logs:        journalctl -u rudyd -f"
echo "  - Tail updater:     journalctl -u rudy-update -f"
echo "  - Inspect serve:    tailscale serve status"
echo "  - Open UI at:       ${TAILNET_URL}"
echo "                      (from any device on this tailnet — MagicDNS"
echo "                       resolves \`${HOST_SHORT}\` to the Pi's 100.x IP)"
