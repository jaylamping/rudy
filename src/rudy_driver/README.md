# rudy_driver

Rust CAN stack for Robstride RS03 actuators over **SocketCAN** (Linux).

## Layout

- `src/protocol.rs` — MIT-style frame encode/decode (verify against firmware)
- `src/socketcan_bus.rs` — blocking CAN I/O (**Linux**); non-Linux builds use a stub for compile-only workflows
- `src/state_machine.rs` — actuator lifecycle state machine
- `src/main.rs` — `rudy_driver_node` CLI scaffold (ROS wiring comes next)

## Build (Rust only)

```bash
cd src/rudy_driver
cargo test
```

## Cross-check (aarch64)

```bash
rustup target add aarch64-unknown-linux-gnu
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
cargo check --target aarch64-unknown-linux-gnu
```

## ROS 2 packaging

This package is also an `ament_cmake` package that runs `cargo build --release` during `colcon build` and installs `rudy_driver_node` into `lib/rudy_driver/`.
