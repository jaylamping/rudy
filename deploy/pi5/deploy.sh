#!/usr/bin/env bash
# Rsync local colcon install + config to Pi 5 and optionally restart services.
# Usage: ./deploy.sh user@pi-host [/path/to/workspace]
set -euo pipefail

TARGET="${1:?usage: deploy.sh user@pi-host [workspace]}"
WS="${2:-$(cd "$(dirname "$0")/../.." && pwd)}"

if [[ ! -d "${WS}/install" ]]; then
  echo "No install/ at ${WS}. Build on desktop first: colcon build --symlink-install" >&2
  exit 1
fi

rsync -avz --delete "${WS}/install/" "${TARGET}:murphy/install/"
rsync -avz "${WS}/config/" "${TARGET}:murphy/config/" || true

echo "Synced install/ to ${TARGET}:murphy/install/"
echo "On the Pi: source ~/murphy/install/setup.bash"
