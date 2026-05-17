# moveit_config

MoveIt 2 scaffold for Rudy arm-tip planning.

## Scope

- Planning group: `right_arm`
- Joint chain: shoulder pitch/roll, upper-arm yaw, elbow pitch, lower-arm yaw
- Tip frame: `r_arm_tip` (the nub at the end of the arm)
- IK policy: position-first (`position_only_ik: true`); strict 6D hand pose is out of scope for the 5-DOF arm
- Execution: fake MoveIt controller only; hardware execution will bridge planned `JointTrajectory` into `cortex`

## Demo

```bash
cd ros
source /opt/ros/jazzy/setup.bash
colcon build --symlink-install --packages-select description moveit_config
source install/setup.bash
ros2 launch moveit_config demo.launch.xml
```

## Design Notes

`cortex` remains the single CAN owner. This package is for kinematics/planning and sim-first validation. The first hardware bridge should send a complete, validated joint trajectory to `cortex`; it should not let MoveIt or `ros2_control` write CAN directly.
