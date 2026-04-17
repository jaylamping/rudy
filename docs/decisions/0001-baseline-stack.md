# ADR 0001: Baseline software stack (2026-04)

## Status

Accepted

## Context

Rudy is an upper-body humanoid using Robstride RS03 actuators, with **sim-first** development and **Isaac Lab** for sim-to-real. The onboard computer is a **Raspberry Pi 5** with a **Waveshare 2-CH CAN HAT**.

## Decision

- **ROS 2**: Jazzy on Ubuntu 24.04 (desktop + Pi).
- **DDS**: CycloneDDS as default RMW for Pi friendliness.
- **Languages**: Rust for CAN/driver logic, C++ only where `ros2_control` requires it, Python for Isaac Lab and tooling.
- **Monorepo**: single ROS 2 workspace with `colcon` packages; cross-compile Rust/aarch64 in CI.
- **Telemetry**: diagnostics + rosbag-first; expand to OpenTelemetry later if needed.

## Consequences

- `ros2_control` hardware plugins remain C++ (`pluginlib`), with a planned topic bridge to Rust.
- Isaac Sim is **not** expected to run on the Pi; training is desktop-hosted.