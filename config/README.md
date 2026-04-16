# Workspace config

Non-ROS shared configuration lives here (actuator specs, sim-wide constants, developer tooling).

| Path | Purpose |
|------|---------|
| `actuators/robstride_rs03.yaml` | Canonical Robstride RS03 limits + protocol metadata (must stay aligned with URDF + tests) |

ROS node parameters still belong under each package’s `config/` directory (see `murphy_bringup`).
