# description

Robot model for Rudy (URDF + xacro).

## Contents

- `urdf/robot.urdf.xacro` — Full kinematic tree: `base_link`, torso, waist yaw, left/right arms (5 DOF each: shoulder pitch/roll, upper-arm yaw, elbow pitch, lower-arm yaw) plus `*_arm_tip` nub frames. Joint limits match the prior `mr_robot` `robot.yaml` where available (RobStride RS03 nominal torque/speed caps).

## Generate URDF for tools

```bash
source install/setup.bash
ros2 run xacro xacro $(ros2 pkg prefix description)/share/description/urdf/robot.urdf.xacro -o /tmp/robot.urdf
check_urdf /tmp/robot.urdf
```

## Public interfaces

This package exposes the robot model only (no ROS topics). Downstream packages load it via `robot_description` or `xacro` includes.
