# control

`ros2_control` integration for Rudy.

## Current state

- `TopicLoopbackHardware` — a minimal `hardware_interface::SystemInterface` that exposes a single
  `loopback_joint` and mirrors commanded position into state (for CI + early bring-up).

## Why is this package in C++?

The rest of Rudy's robotics code (`driver`, `cortex`, the RobStride CAN stack) is Rust. This
package is the one place we cannot follow that rule today, because:

- `controller_manager` discovers and instantiates hardware interfaces through `pluginlib`, which
  uses `dlopen` plus a registered C++ class loader (`PLUGINLIB_EXPORT_CLASS`) to construct
  subclasses of `hardware_interface::SystemInterface`. The plugin **must** be a C++ shared
  library implementing that base class — no other language can satisfy the loader contract.
- In ROS 2 Jazzy there is no first-party Rust path: `rclrs` is alpha and ships no `pluginlib`,
  `hardware_interface`, `controller_interface`, or `controller_manager` bindings.
- The standard "Rust + ros2_control" pattern, which we follow, is a thin C++ `SystemInterface`
  shim that bridges commands/state to a Rust process (`driver_node`) over ROS topics. See
  `docs/architecture.md` (`HWI -->|ROS_topics| RustNode[driver_node]`).

The C++ surface is therefore intentionally tiny (~60 lines of real code) and we expect it to
stay that way.

## Next steps

- Replace/extend `TopicLoopbackHardware` with a **topic bridge** `SystemInterface` that relays
  commands/state to `driver_node` (Rust). The C++ shape stays roughly the same; the loopback
  internals become two ROS pub/sub pairs.

## Config

See `config/controllers.yaml` (placeholder controller manager parameters).
