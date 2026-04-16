# murphy_control

`ros2_control` integration for Murphy.

## Current state

- `MurphyTopicLoopbackHardware` — a minimal `hardware_interface::SystemInterface` that exposes a single
  `murphy_loopback_joint` and mirrors commanded position into state (for CI + early bring-up).

## Next steps

- Replace/extend with a **topic bridge** `SystemInterface` that relays commands/state to `murphy_driver_node` (Rust).

## Config

See `config/controllers.yaml` (placeholder controller manager parameters).
