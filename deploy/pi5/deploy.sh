#!/usr/bin/env bash
# Sync the source tree needed for a Pi-native cortex build, then run install.sh remotely.
# Usage: ./deploy.sh user@pi-host [remote_repo_dir]
set -euo pipefail

TARGET="${1:?usage: ./deploy.sh user@pi-host [remote_repo_dir]}"
REMOTE_USER="${TARGET%@*}"
if [[ "${REMOTE_USER}" == "${TARGET}" ]]; then
  REMOTE_USER="${USER}"
fi
REMOTE_REPO="${2:-/home/${REMOTE_USER}/rudy}"
if [[ "${REMOTE_REPO}" == "~/"* ]]; then
  REMOTE_REPO="/home/${REMOTE_USER}/${REMOTE_REPO#~/}"
elif [[ "${REMOTE_REPO}" == "~" ]]; then
  REMOTE_REPO="/home/${REMOTE_USER}"
elif [[ "${REMOTE_REPO}" != /* ]]; then
  REMOTE_REPO="/home/${REMOTE_USER}/${REMOTE_REPO}"
fi
WS="$(cd "$(dirname "$0")/../.." && pwd)"

sync_dir() {
  local src_rel="$1"
  local dst_rel="$2"
  shift 2

  if command -v rsync >/dev/null 2>&1; then
    rsync -avz "$@" "${WS}/${src_rel}/" "${TARGET}:${REMOTE_REPO}/${dst_rel}/"
    return
  fi

  echo "rsync not found locally; falling back to tar+ssh for ${src_rel}"
  ssh "${TARGET}" "mkdir -p '${REMOTE_REPO}/${dst_rel}'"
  tar -C "${WS}/${src_rel}" -cf - "$@" . | ssh "${TARGET}" "tar -xf - -C '${REMOTE_REPO}/${dst_rel}'"
}

echo "== Syncing Rudy repo subset to ${TARGET}:${REMOTE_REPO} =="
sync_dir "crates" "crates" --exclude=target
sync_dir "ros/src/driver" "ros/src/driver" --exclude=target
sync_dir "link" "link" --exclude=node_modules --exclude=dist
sync_dir "config" "config"
sync_dir "deploy" "deploy"
sync_dir "docs/runbooks" "docs/runbooks"

echo "== Running Pi installer =="
ssh -t "${TARGET}" "cd '${REMOTE_REPO}' && sed -i 's/\r$//' deploy/pi5/*.sh && sudo bash deploy/pi5/install.sh"
