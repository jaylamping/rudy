# Murphy

ROS 2 workspace for the Murphy humanoid (RobStride RS03 actuators, sim-to-real stack).

## Layout

- `src/murphy_description` — URDF / xacro robot model
- `src/murphy_bringup` — XML launch files and runtime parameters
- `src/murphy_msgs` — Custom message / service / action definitions (placeholder for now)
- `config/` — Workspace-level shared configuration (non-ROS)
- [`docs/`](docs/README.md) — Robotics best-practices reference + offline research exports (Firecrawl snapshots) + [MCP research stack](docs/mcp-research-stack.md) for Cursor

## Prerequisites

- ROS 2 (Humble or newer) with `robot_state_publisher`, `joint_state_publisher_gui`, `rviz2`, `xacro`

## Build

```bash
cd /path/to/murphy
source /opt/ros/$ROS_DISTRO/setup.bash
colcon build --symlink-install
source install/setup.bash
```

## Visualize the model

With ROS 2 sourced and the workspace built:

```bash
ros2 launch murphy_bringup display_model.launch.xml
```

Use the joint state publisher GUI to move each revolute joint and confirm motion stays within the hard limits shown in RViz (RobotModel + TF). The packaged RViz layout sets **Fixed Frame** to `base_link`.

## Validate URDF (without colcon)

Install tools once (Homebrew macOS example):

```bash
brew install urdfdom graphviz
python3 -m venv .venv && .venv/bin/pip install xacro urdfdom-py
```

Then:

```bash
PATH="$PWD/.venv/bin:$PATH" xacro src/murphy_description/urdf/murphy.urdf.xacro > /tmp/murphy.urdf
check_urdf /tmp/murphy.urdf
urdf_to_graphviz /tmp/murphy.urdf /tmp/murphy_tree && dot -Tpng /tmp/murphy_tree.gv -o /tmp/murphy_tree.png
PATH="$PWD/.venv/bin:$PATH" python3 scripts/validate_urdf.py
```

## License

Apache-2.0 (see `LICENSE`).