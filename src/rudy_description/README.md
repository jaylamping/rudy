# rudy_description

Robot model for Rudy (URDF + xacro).

## Contents

- `urdf/rudy.urdf.xacro` — Full kinematic tree: `base_link`, torso, waist yaw, left/right arms (4 DOF each). Joint limits match the prior `mr_robot` `robot.yaml` (RobStride RS03 nominal torque/speed caps).

## Generate URDF for tools

```bash
source install/setup.bash
ros2 run xacro xacro $(ros2 pkg prefix rudy_description)/share/rudy_description/urdf/rudy.urdf.xacro -o /tmp/rudy.urdf
check_urdf /tmp/rudy.urdf
```

## Public interfaces

This package exposes the robot model only (no ROS topics). Downstream packages load it via `robot_description` or `xacro` includes.
