# Cortex crate layout

Single crate (`cortex`) for the operator-console daemon. Top-level modules are domain-oriented; `lib.rs` re-exports several legacy names (`wt`, `boot_state`, `inventory`, …) so tests and external tools keep stable paths until a cleanup pass removes them.

## Module map

- **`app/`** — `AppState`, daemon `bootstrap` (`run` from argv).
- **`api/`** — Axum REST: `meta/` (config, health, system, logs), `inventory/`, `motors/` (incl. bench tests), `motion/`, `ops/`, plus `error.rs` / `lock_gate.rs`.
- **`can/`** — `worker/` (per-bus SocketCAN thread), `handle/` (`LinuxCanCore`), math, travel, discovery, mocks.
- **`config/`** — TOML config (`Config::load`), split by HTTP / WT / CAN / safety / logs / telemetry.
- **`hardware/`** — Inventory YAML, actuator specs, boot state / orchestrator, limbs.
- **`http/`** — Plaintext HTTP server, SPA static bundle, session header helpers.
- **`motion/`** — Intents, controller, sweep/wave patterns, registry.
- **`observability/`** — Audit log, tracing capture, SQLite log store, telemetry param cache, host metrics poller, reminders.
- **`types/`** — `ts-rs` DTOs and wire enums (meta, motor, system, logs, WT, …).
- **`webtransport/`** — QUIC listener, per-session router, client frame codec.

Integration tests live under `crates/cortex/tests/`; they build `cortex::build_app` and exercise the REST surface without binding ports when possible. Large REST contract suites live under `tests/api/` (`meta.rs`, `inventory.rs`, `params.rs`, `control.rs`, `motion.rs`, `ops.rs`, `endpoints.rs`), each registered in `Cargo.toml` as `[[test]]` because Cargo only auto-discovers `tests/*.rs`. Mock-CAN / stub checks live under `tests/can/` (`stub.rs`, `feedback_broadcast.rs`), also via `[[test]]`. Fine-grained CAN behavior (`backoff`, `math`, `discovery`, travel, …) is covered by **unit tests** next to the library modules (`src/can/*_tests.rs`), not as separate integration binaries. Shared helpers are in `tests/common/` (`fixtures.rs` + `mod.rs`).
