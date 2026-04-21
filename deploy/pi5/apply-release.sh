#!/usr/bin/env bash
# Apply a staged Rudy Pi release tarball into /opt/rudy and restart the daemon.
# Called by rudy-update.sh; never invoked directly under normal operation.
#
# Argument: path to extracted release tree containing
#   bin/cortex                       (SPA embedded inside the binary)
#   config/...
#   deploy/pi5/cortex.service
#   deploy/pi5/rudy-watchdog.sh
#   deploy/pi5/rudy-watchdog.service
#   deploy/pi5/rudy-watchdog.timer
#   deploy/pi5/render-cortex-toml.sh
#   docs/runbooks/operator-console.md
set -euo pipefail

STAGE="${1:?stage dir required}"

if [[ ! -x "${STAGE}/bin/cortex" ]]; then
  echo "apply-release: missing ${STAGE}/bin/cortex" >&2
  exit 1
fi

if ! id -u rudy >/dev/null 2>&1; then
  useradd --system --home /var/lib/rudy --create-home --shell /usr/sbin/nologin rudy
fi
usermod -a -G netdev rudy || true

install -d -m 0755 /opt/rudy/bin /opt/rudy/config /etc/rudy /etc/rudy/docs/runbooks
install -d -o rudy -g rudy -m 0755 /var/lib/rudy

# One-shot migration from legacy rudyd.service + rudyd.toml + /var/lib/rudyd/tailscale
# (pre-cortex rename). Idempotent — safe on fresh installs. See docs/runbooks/pi5.md.
if systemctl list-unit-files 2>/dev/null | grep -q '^rudyd\.service'; then
  systemctl stop rudyd.service 2>/dev/null || true
  systemctl disable rudyd.service 2>/dev/null || true
  rm -f /etc/systemd/system/rudyd.service
fi
if [[ -f /etc/rudy/rudyd.toml ]] && [[ ! -f /etc/rudy/cortex.toml ]]; then
  mv /etc/rudy/rudyd.toml /etc/rudy/cortex.toml
fi
if [[ -d /var/lib/rudyd/tailscale ]] && [[ ! -d /var/lib/rudy/cortex/tailscale ]]; then
  mkdir -p /var/lib/rudy/cortex
  mv /var/lib/rudyd/tailscale /var/lib/rudy/cortex/tailscale
  rmdir /var/lib/rudyd 2>/dev/null || true
fi

install -d -o rudy -g rudy -m 0750 /var/lib/rudy/cortex/tailscale

install -m 0755 "${STAGE}/bin/cortex" /opt/rudy/bin/cortex
setcap cap_net_raw,cap_net_admin=eip /opt/rudy/bin/cortex

rsync -a --delete "${STAGE}/config/" /opt/rudy/config/

install -m 0644 "${STAGE}/docs/runbooks/operator-console.md" /etc/rudy/docs/runbooks/operator-console.md
install -m 0644 "${STAGE}/deploy/pi5/cortex.service" /etc/systemd/system/cortex.service
install -m 0755 "${STAGE}/deploy/pi5/rudy-watchdog.sh" /usr/local/bin/rudy-watchdog.sh
install -m 0644 "${STAGE}/deploy/pi5/rudy-watchdog.service" /etc/systemd/system/rudy-watchdog.service
install -m 0644 "${STAGE}/deploy/pi5/rudy-watchdog.timer" /etc/systemd/system/rudy-watchdog.timer

# Always re-render cortex.toml so config-schema changes ship in releases land
# on the Pi. A timestamped backup of the previous file is kept alongside.
bash "${STAGE}/deploy/pi5/render-cortex-toml.sh" /etc/rudy/cortex.toml

# Ensure `tailscale serve` is fronting cortex on https://<host>/. Idempotent.
# `tailscale serve` state is persistent across reboots; we re-assert on every
# release so a Tailscale config drift (or a fresh tailnet identity) heals
# itself the next time we ship.
if command -v tailscale >/dev/null; then
  if tailscale status >/dev/null 2>&1; then
    # `tailscale serve --bg --https=443 http://127.0.0.1:8443` is the modern
    # form; older Tailscale versions used `tailscale serve https / proxy ...`.
    # Both end up writing to the same serve config; the modern form is a no-op
    # if the same mapping is already active.
    tailscale serve --bg --https=443 http://127.0.0.1:8443 \
      || echo "apply-release: tailscale serve setup failed (cortex is still up on 127.0.0.1:8443; rerun bootstrap.sh)" >&2
  else
    echo "apply-release: tailscale not logged in; skipping tailscale serve setup" >&2
  fi
else
  echo "apply-release: tailscale CLI not found; skipping tailscale serve setup" >&2
fi

systemctl daemon-reload
systemctl enable cortex.service >/dev/null
systemctl enable rudy-watchdog.timer >/dev/null
systemctl restart cortex.service
systemctl restart rudy-watchdog.timer

# Smoke-check startup health before accepting the release. This catches
# "process is running but API/UI is not serving" failures that `is-active`
# alone can't detect.
health_ok=0
for _ in {1..10}; do
  if systemctl is-active --quiet cortex.service; then
    if health_json="$(curl -fsS --max-time 5 http://127.0.0.1:8443/api/health)"; then
      if grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"' <<< "${health_json}"; then
        health_ok=1
        break
      fi
    fi
  fi
  sleep 2
done

if [[ "${health_ok}" -ne 1 ]]; then
  echo "apply-release: cortex FAILED health checks after restart" >&2
  systemctl status cortex.service --no-pager -n 5 >&2 || true
  echo "--- watchdog status ---" >&2
  systemctl status rudy-watchdog.timer --no-pager -n 5 >&2 || true
  echo "--- last 30 journal lines ---" >&2
  journalctl -u cortex.service -n 30 --no-pager >&2 || true
  echo "--- last 20 watchdog journal lines ---" >&2
  journalctl -u rudy-watchdog.service -n 20 --no-pager >&2 || true
  exit 1
fi

echo "apply-release: cortex restarted and healthy"
