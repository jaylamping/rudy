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

if [[ ! -f /etc/rudy/rudyd.toml ]]; then
  bash "${STAGE}/deploy/pi5/render-rudyd-toml.sh" /etc/rudy/rudyd.toml
fi

systemctl daemon-reload
systemctl enable rudyd.service >/dev/null
systemctl restart rudyd.service

echo "apply-release: rudyd restarted"
