# bringup

Runtime launch and configuration for Rudy.

## Launches

| File | Purpose |
|------|---------|
| `launch/display_model.launch.xml` | `robot_state_publisher` + `joint_state_publisher_gui` + RViz for URDF validation |

## Parameters

Shared non-secret defaults live under `config/` (see `display.yaml`). Prefer YAML parameters over hard-coded values in launch files per Henki ROS 2 best practices.

## Public interfaces

This package launches nodes; see each launch file for topics (`/joint_states`, `/robot_description`, TF).
