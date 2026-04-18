#!/usr/bin/env bash
# Apply a staged Rudy Pi release tarball into /opt/rudy and restart the daemon.
# Called by rudy-update.sh; never invoked directly under normal operation.
#
# Argument: path to extracted release tree containing
#   bin/rudydae                       (SPA embedded inside the binary)
#   config/...
#   deploy/pi5/rudyd.service
#   deploy/pi5/render-rudyd-toml.sh
#   docs/runbooks/operator-console.md
set -euo pipefail

STAGE="${1:?stage dir required}"

if [[ ! -x "${STAGE}/bin/rudydae" ]]; then
  echo "apply-release: missing ${STAGE}/bin/rudydae" >&2
  exit 1
fi

if ! id -u rudy >/dev/null 2>&1; then
  useradd --system --home /var/lib/rudy --create-home --shell /usr/sbin/nologin rudy
fi
usermod -a -G netdev rudy || true

install -d -m 0755 /opt/rudy/bin /opt/rudy/config /etc/rudy /etc/rudy/docs/runbooks
install -d -o rudy -g rudy -m 0755 /var/lib/rudy
install -d -o rudy -g rudy -m 0750 /var/lib/rudyd/tailscale

install -m 0755 "${STAGE}/bin/rudydae" /opt/rudy/bin/rudydae
setcap cap_net_raw,cap_net_admin=eip /opt/rudy/bin/rudydae

rsync -a --delete "${STAGE}/config/" /opt/rudy/config/

install -m 0644 "${STAGE}/docs/runbooks/operator-console.md" /etc/rudy/docs/runbooks/operator-console.md
install -m 0644 "${STAGE}/deploy/pi5/rudyd.service" /etc/systemd/system/rudyd.service

# Always re-render rudyd.toml so config-schema changes ship in releases land
# on the Pi. A timestamped backup of the previous file is kept alongside.
bash "${STAGE}/deploy/pi5/render-rudyd-toml.sh" /etc/rudy/rudyd.toml

# Ensure `tailscale serve` is fronting rudyd on https://<host>/. Idempotent.
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
      || echo "apply-release: tailscale serve setup failed (rudyd is still up on 127.0.0.1:8443; rerun bootstrap.sh)" >&2
  else
    echo "apply-release: tailscale not logged in; skipping tailscale serve setup" >&2
  fi
else
  echo "apply-release: tailscale CLI not found; skipping tailscale serve setup" >&2
fi

systemctl daemon-reload
systemctl enable rudyd.service >/dev/null
systemctl restart rudyd.service

echo "apply-release: rudyd restarted"
