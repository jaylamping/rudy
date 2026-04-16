# murphy_description

Robot model for Murphy (URDF + xacro).

## Contents

- `urdf/murphy.urdf.xacro` — Full kinematic tree: `base_link`, torso, waist yaw, left/right arms (4 DOF each). Joint limits match the prior `mr_robot` `robot.yaml` (RobStride RS03 nominal torque/speed caps).

## Generate URDF for tools

```bash
source install/setup.bash
ros2 run xacro xacro $(ros2 pkg prefix murphy_description)/share/murphy_description/urdf/murphy.urdf.xacro -o /tmp/murphy.urdf
check_urdf /tmp/murphy.urdf
```

## Public interfaces

This package exposes the robot model only (no ROS topics). Downstream packages load it via `robot_description` or `xacro` includes.
