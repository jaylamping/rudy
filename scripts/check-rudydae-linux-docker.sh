#!/usr/bin/env bash
# Run the same rudydae checks as GitHub Actions (Linux host), from macOS or Windows
# (via Git Bash/WSL) with Docker. Host `cargo clippy` skips `#[cfg(target_os = "linux")]`
# code such as `crates/rudydae/src/can/linux.rs`.
#
# Usage (from repo root):
#   ./scripts/check-rudydae-linux-docker.sh
#
# Requires: Docker

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

docker run --rm \
  -v "${ROOT}:/work:rw" \
  -w /work/crates \
  -e RUDYD_NO_EMBED=0 \
  rust:bookworm \
  bash -c '
    set -euo pipefail
    # Login shells on this image drop rustup from PATH; keep cargo on PATH explicitly.
    export PATH="/usr/local/cargo/bin:${PATH}"
    apt-get update -qq
    apt-get install -y --no-install-recommends pkg-config libssl-dev
    rustup component add rustfmt clippy
    mkdir -p /work/link/dist
    printf "%s" "<!doctype html><title>link stub</title>" > /work/link/dist/index.html
    cargo fmt --check
    cargo clippy -p rudydae --all-targets -- -D warnings
    cargo test -p rudydae
  '
