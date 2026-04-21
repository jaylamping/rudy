#!/usr/bin/env bash
# Pi-side updater. Runs from systemd timer cortex-update.timer.
#
# Polls the GitHub repo's latest release manifest (latest.json), and if its
# commit_sha differs from /opt/rudy/current.sha, downloads the tarball,
# verifies sha256, hands off to apply-release.sh, and records the new sha.
#
# Designed to be safe to run every minute: a no-op when nothing changed.
set -euo pipefail

REPO="${RUDY_REPO:-jaylamping/rudy}"
STATE_DIR="/var/lib/rudy"
SHA_FILE="/opt/rudy/current.sha"
WORK_DIR="$(mktemp -d /tmp/cortex-update.XXXXXX)"
trap 'rm -rf "${WORK_DIR}"' EXIT

MANIFEST_URL="https://github.com/${REPO}/releases/latest/download/latest.json"
MANIFEST="${WORK_DIR}/latest.json"

log() { echo "cortex-update: $*"; }

if ! curl -fsSL --retry 3 --connect-timeout 5 -o "${MANIFEST}" "${MANIFEST_URL}"; then
  log "could not fetch ${MANIFEST_URL} (no network or no release yet); skipping"
  exit 0
fi

REMOTE_SHA="$(jq -r '.commit_sha' "${MANIFEST}")"
ASSET_URL="$(jq -r '.url' "${MANIFEST}")"
ASSET_NAME="$(jq -r '.asset' "${MANIFEST}")"
EXPECTED_SHA256="$(jq -r '.sha256' "${MANIFEST}")"

if [[ -z "${REMOTE_SHA}" || "${REMOTE_SHA}" == "null" ]]; then
  log "manifest missing commit_sha; skipping"
  exit 0
fi

CURRENT_SHA=""
if [[ -f "${SHA_FILE}" ]]; then
  CURRENT_SHA="$(cat "${SHA_FILE}")"
fi

if [[ "${CURRENT_SHA}" == "${REMOTE_SHA}" ]]; then
  exit 0
fi

log "update available: ${CURRENT_SHA:-<none>} -> ${REMOTE_SHA}"

ASSET_PATH="${WORK_DIR}/${ASSET_NAME}"
log "downloading ${ASSET_URL}"
curl -fsSL --retry 3 --connect-timeout 10 -o "${ASSET_PATH}" "${ASSET_URL}"

ACTUAL_SHA256="$(sha256sum "${ASSET_PATH}" | awk '{print $1}')"
if [[ "${ACTUAL_SHA256}" != "${EXPECTED_SHA256}" ]]; then
  log "sha256 mismatch (expected ${EXPECTED_SHA256}, got ${ACTUAL_SHA256}); aborting"
  exit 1
fi

STAGE_DIR="${WORK_DIR}/stage"
mkdir -p "${STAGE_DIR}"
tar -xzf "${ASSET_PATH}" -C "${STAGE_DIR}"

APPLY_SCRIPT="${STAGE_DIR}/deploy/pi5/apply-release.sh"
if [[ ! -f "${APPLY_SCRIPT}" ]]; then
  log "tarball missing apply-release.sh; aborting"
  exit 1
fi
# Don't require +x: older release tarballs (built before release.yaml
# learned to chmod the staged scripts) ship apply-release.sh as 0644.
# `bash <path>` runs fine regardless. We `chmod` it to be tidy and so
# downstream tooling that does want -x is happy.
chmod 0755 "${APPLY_SCRIPT}" || true

log "applying release ${REMOTE_SHA}"
sudo bash "${APPLY_SCRIPT}" "${STAGE_DIR}"

echo "${REMOTE_SHA}" | sudo tee "${SHA_FILE}" >/dev/null
log "done; running ${REMOTE_SHA}"
