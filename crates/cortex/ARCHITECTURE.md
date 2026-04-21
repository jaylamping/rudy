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

Integration tests live under `crates/cortex/tests/`; they build `cortex::build_app` and exercise the REST surface without binding ports when possible.
