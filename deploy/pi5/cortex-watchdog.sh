#!/usr/bin/env bash
# Probe cortex health from loopback and restart on sustained failures.
set -euo pipefail

STATE_DIR="/var/lib/rudy"
STATE_FILE="${STATE_DIR}/watchdog.fails"
THRESHOLD="${CORTEX_WATCHDOG_THRESHOLD:-4}"
API_URL="http://127.0.0.1:8443/api/health"
ROOT_URL="http://127.0.0.1:8443/"

mkdir -p "${STATE_DIR}"

fails=0
if [[ -f "${STATE_FILE}" ]]; then
  raw="$(<"${STATE_FILE}")"
  if [[ "${raw}" =~ ^[0-9]+$ ]]; then
    fails="${raw}"
  fi
fi

ok=1
if ! health_json="$(curl -fsS --max-time 5 "${API_URL}")"; then
  ok=0
  logger -t cortex-watchdog "health probe failed: ${API_URL} unreachable"
elif ! grep -Eq '"status"[[:space:]]*:[[:space:]]*"ok"' <<< "${health_json}"; then
  ok=0
  logger -t cortex-watchdog "health probe failed: non-ok payload from ${API_URL}"
fi

if ! root_html="$(curl -fsS --max-time 5 "${ROOT_URL}")"; then
  ok=0
  logger -t cortex-watchdog "spa probe failed: ${ROOT_URL} unreachable"
elif ! grep -Eiq '<div[[:space:]]+id="root"' <<< "${root_html}"; then
  ok=0
  logger -t cortex-watchdog "spa probe failed: root shell marker missing"
fi

if [[ "${ok}" -eq 1 ]]; then
  echo 0 > "${STATE_FILE}"
  exit 0
fi

fails=$((fails + 1))
echo "${fails}" > "${STATE_FILE}"
logger -t cortex-watchdog "cortex unhealthy (${fails}/${THRESHOLD})"

if (( fails >= THRESHOLD )); then
  logger -t cortex-watchdog "restarting cortex.service after sustained failure"
  systemctl restart cortex.service
  echo 0 > "${STATE_FILE}"
fi
