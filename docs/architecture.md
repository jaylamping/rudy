# Murphy architecture

This document is the **living** system architecture for the Murphy monorepo. Update it whenever packages, data flows, or deployment topology change.

## Topology

Murphy uses a **two-machine** layout:

- **Desktop (x86_64 + NVIDIA GPU)**: development, `colcon` builds, Isaac Lab training, RViz, heavy logging.
- **Raspberry Pi 5 (aarch64 + CAN HAT)**: onboard runtime — CAN driver, `ros2_control`, telemetry, policy inference.

```mermaid
graph LR
  subgraph desktop [Desktop_x86]
    Build[colcon_build]
    Sim[IsaacLab]
    Train[Training]
    Build --> Sim --> Train
  end

  subgraph pi5 [Pi5_aarch64]
    Driver[murphy_driver_Rust]
    Control[murphy_control]
    Telem[murphy_telemetry]
    Driver --> CAN[SocketCAN]
    Control --> Driver
    Telem --> Driver
  end

  desktop -->|rsync_install| pi5
  desktop <-->|CycloneDDS| pi5
  CAN --> Act[Robstride_RS03]
```

## Packages (ROS 2)

| Package | Role |
|---------|------|
| `murphy_description` | URDF / xacro — kinematic source of truth |
| `murphy_bringup` | XML launch + YAML params |
| `murphy_msgs` | Custom messages (placeholder) |
| `murphy_driver` | Rust CAN stack + `murphy_driver_node` (SocketCAN; Linux-only I/O) |
| `murphy_control` | `ros2_control` hardware plugin(s) + controller YAML |
| `murphy_telemetry` | Diagnostics + rosbag launch helpers |
| `murphy_simulation` | Isaac Lab scaffold + sim config YAML |
| `murphy_tests` | `launch_testing` + parity tests |

## Data flow (target)

```mermaid
graph TD
  CM[controller_manager] --> HWI[murphy_control_plugin]
  HWI -->|ROS_topics| RustNode[murphy_driver_node]
  RustNode --> CAN[can0_can1]
  RustNode --> Diag[/diagnostics]
  RustNode --> JS[/joint_states]
```

Today: `murphy_control` ships a **loopback** `SystemInterface` for CI/bring-up. The topic bridge to the Rust driver is the next integration step.

## Configuration hierarchy

- **Actuator truth**: [`config/actuators/robstride_rs03.yaml`](../config/actuators/robstride_rs03.yaml)
- **URDF limits/dynamics**: must stay consistent with the actuator spec (enforced by `tests/`)
- **Sim randomization**: `src/murphy_simulation/configs/*.yaml`
- **ROS parameters**: per-package `config/` + `murphy_bringup`

## See also

- [Runbook: Pi 5](runbooks/pi5.md)
- [Runbook: Isaac Lab](runbooks/isaac_lab.md)
- [Decisions (ADR)](decisions/)
