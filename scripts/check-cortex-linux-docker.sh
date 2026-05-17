#!/usr/bin/env bash
# Run the same cortex checks as GitHub Actions (Linux host), from macOS or Windows
# (via Git Bash/WSL) with Docker. Host `cargo clippy` skips `#[cfg(target_os = "linux")]`
# code such as `crates/cortex/src/can/linux.rs`.
#
# Usage (from repo root):
#   ./scripts/check-cortex-linux-docker.sh
#
# Requires: Docker

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE="${RUDY_CORTEX_LINUX_IMAGE:-rudy-cortex-linux-check:bookworm}"
REGISTRY_VOL="${RUDY_CARGO_REGISTRY_VOLUME:-rudy-cortex-cargo-registry}"
GIT_VOL="${RUDY_CARGO_GIT_VOLUME:-rudy-cortex-cargo-git}"
TARGET_VOL="${RUDY_CARGO_TARGET_VOLUME:-rudy-cortex-cargo-target}"

docker build \
  -q \
  -t "${IMAGE}" \
  -f "${ROOT}/scripts/check-cortex-linux.Dockerfile" \
  "${ROOT}/scripts" >/dev/null

docker run --rm \
  -v "${ROOT}:/work:rw" \
  -v "${REGISTRY_VOL}:/usr/local/cargo/registry" \
  -v "${GIT_VOL}:/usr/local/cargo/git" \
  -v "${TARGET_VOL}:/cargo-target" \
  -w /work/crates \
  -e CORTEX_NO_EMBED=0 \
  -e CARGO_TARGET_DIR=/cargo-target \
  "${IMAGE}" \
  bash -c '
    set -euo pipefail
    # Login shells on this image drop rustup from PATH; keep cargo on PATH explicitly.
    export PATH="/usr/local/cargo/bin:${PATH}"
    mkdir -p /work/link/dist
    printf "%s" "<!doctype html><title>link stub</title>" > /work/link/dist/index.html
    cargo fmt --check
    cargo clippy -p cortex --all-targets -- -D warnings
    cargo test -p cortex
  '
