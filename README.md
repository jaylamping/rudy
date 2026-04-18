# Rudy

<p align="center">
  <img src="docs/images/rudy_the_robot.png" alt="Rudy the robot: grey cylindrical body, blue screen face with simple line eyes and mouth, accordion arms, glowing light bulb on its head" width="320">
</p>

Monorepo for the Rudy upper-body humanoid (RobStride RS03 actuators, CAN bus, Isaac Lab sim-to-real, web operator console).

## Top-level layout

- [`ros/`](ros/) — ROS 2 **Jazzy** colcon workspace (packages under `ros/src/`):
  - `description` — URDF / xacro robot model (kinematic source of truth)
  - `bringup` — XML launch files and runtime parameters
  - `msgs` — Custom message / service / action definitions (placeholder for now)
  - `driver` — **Rust** CAN driver + protocol (`driver_node`, hybrid ament_cmake + Cargo)
  - `control` — `ros2_control` hardware plugin(s) + controller YAML (starts with a loopback `SystemInterface`)
  - `telemetry` — diagnostics + rosbag launch helpers
  - `simulation` — Isaac Lab scaffold + sim YAML configs
  - `tests` — `launch_testing` + parity tests
- [`crates/`](crates/) — Non-ROS Rust (Cargo workspace):
  - `rudydae` — operator-console daemon (axum HTTPS + WebTransport over Tailscale)
- [`link/`](link/) — Vite + React + TypeScript operator console UI (shadcn/ui, TanStack Query, WebTransport)
- [`config/`](config/) — Workspace-wide configuration (actuator specs, `rudyd.toml`, inventory)
- [`deploy/pi5/`](deploy/pi5/) — Raspberry Pi 5 bring-up + systemd units + deploy scripts
- [`docs/`](docs/README.md) — Architecture, ADRs, runbooks, robotics reference, research exports, MCP stack
- [`tools/`](tools/) — RobStride bench scripts, Motor Studio exports, diagnostic helpers
- [`scripts/`](scripts/) — Repo-level helpers (URDF validation, etc.)
- [`tests/`](tests/) — Cross-cutting parity tests (URDF ↔ actuator spec)
- [`.devcontainer/`](.devcontainer/) — Desktop dev container (ROS Jazzy + Rust + cross tools)

## Prerequisites

- **Desktop**: ROS 2 **Jazzy** (`desktop` or `desktop-full`), `colcon`, Rust (`cargo`, `rustfmt`, `clippy`), Node 20+, plus `xacro` for tests.
- **Pi 5**: Ubuntu LTS aarch64, [`deploy/pi5/setup_pi5.sh`](deploy/pi5/setup_pi5.sh) for SocketCAN + `chrony` (no ROS on the device yet). Operator console: `rudydae` + Tailscale 1.60+.

## Build

```bash
# ROS 2 workspace
cd ros
source /opt/ros/jazzy/setup.bash
rosdep install --from-paths src --ignore-src -r -y
colcon build --symlink-install
source install/setup.bash

# rudydae daemon
cd ../crates && cargo build --release -p rudydae

# link frontend
cd ../link && npm install && npm run build
```

## Tests

```bash
# Cross-cutting Python parity / gold-standard tests (URDF + actuator spec)
python3 -m pip install -U pytest pyyaml xacro urdfdom-py
python3 -m pytest -q tests

# Rust unit tests (driver crate, inside ROS workspace)
(cd ros/src/driver && cargo test)

# Rust unit tests (rudydae + future crates)
(cd crates && cargo test)

# ROS package tests (after colcon build inside ros/)
(cd ros && colcon test && colcon test-result --verbose)

# link frontend
(cd link && npm run lint && npm run typecheck && npm run build)
```

## Visualize the model

```bash
cd ros && source install/setup.bash
ros2 launch bringup display_model.launch.xml
```

## Validate URDF (without full ROS)

```bash
brew install urdfdom graphviz   # macOS example
python3 -m venv .venv && .venv/bin/pip install xacro urdfdom-py

PATH="$PWD/.venv/bin:$PATH" xacro ros/src/description/urdf/robot.urdf.xacro > /tmp/robot.urdf
check_urdf /tmp/robot.urdf
PATH="$PWD/.venv/bin:$PATH" python3 scripts/validate_urdf.py
```

## Operator console

The `rudydae` daemon plus `link` SPA together form the operator console — live
telemetry, firmware parameter editor, jog/enable controls, URDF 3D view, log
tail. Reachable over Tailscale only. See:

- [ADR-0004](docs/decisions/0004-operator-console.md) — architecture + safety model
- [Runbook](docs/runbooks/operator-console.md) — start/stop, token rotation, audit log
- [Tailscale cert runbook](deploy/pi5/tailscale-cert.md)

## CI

GitHub Actions workflow: [`.github/workflows/ci.yaml`](.github/workflows/ci.yaml) — separate jobs for `ros` (Rust + aarch64 `cargo check`, `colcon` build/test, pytest), `rudydae` (crates workspace fmt/clippy/test), `link` (lint + typecheck + build), and `docs-links`.

## License

Apache-2.0 (see `LICENSE`).
