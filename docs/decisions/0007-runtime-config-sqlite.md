# ADR-0007: Runtime configuration in SQLite (operator console)

**Status:** accepted  
**Date:** 2026-04-24

## Context

`cortex` loaded motion/safety/telemetry tunables from `cortex.toml` and actuator state from `inventory.yaml`. Live tuning by editing files is risky; operators need a durable store and a Settings UI. ROS/sim YAML in `ros/src` stays package-level defaults, not the live console truth.

## Decision

- **Two layers:** In-repo TOML/YAML remain **seed defaults** and hardware truth (actuator spec, URDF parity). A SQLite database under a configurable path (e.g. `settings.db` / unified runtime DB) is the **authoritative operator state** after first successful import.
- **First boot:** If the DB is missing, create it, run migrations, and **ingest** seed values from `cortex.toml` (and, where applicable, other seed files), record metadata (paths, content hashes, schema version).
- **Later boots:** Load runtime tunables from the DB. Do **not** merge checked-in file changes over an existing healthy DB. Surface “seed file changed” only as a **warning** in `GET /api/settings`, not as automatic overwrite.
- **Corrupt/missing DB (recovery):** Quarantine the bad file, re-seed from TOML/YAML, set `recovery_pending`, **disable auto-home and motion** until the operator **acknowledges** via API/UI. Audit the event.
- **Control loops** never read SQLite in the hot path; they read a validated in-memory **effective snapshot** updated atomically after writes and cross-field validation.
- **Edits and modes:** Expose a registry: each key is `read_only` / `static_restart_required` / `runtime_immediate` / `applies_next_command` / `requires_stopped_motors` / `requires_restart` as appropriate. `GET /api/settings` returns the full list for a single **Settings** page that can **show** every value and **edit** only what policy allows. Cross-field checks run on the **whole** snapshot, not a single key in isolation.
- **Inventory (phase 2 of migration):** Store the serialized inventory v2 document in the same DB (or a dedicated table), keep `Inventory` as the in-memory projection, mirror to YAML for optional export/backup where needed.

## Consequences

- `deploy/pi5/render-cortex-toml.sh` must be narrowed so release applies **static** boot wiring and DB paths, not re-authoritative operator-tuned safety scalars.
- CI/URDF tests continue to use checked-in seed YAML; add round-trip or export tests when DB is authoritative.
- New failure modes: SD card wear, corrupt WAL — use WAL + integrity check + quarantine on failure (see `log_store` patterns).

## Rollback (operator / dev)

1. Stop `cortex` (or `systemctl stop rudy-cortex` on the Pi).
2. Back up then remove the runtime DB (default: `.cortex/runtime.db` in dev, `/var/lib/rudy/runtime.db` on Pi if using rendered config).
3. Optional: keep `cortex.toml` + `inventory.yaml` as your known-good seed; clear only the DB to force a first-boot re-import on next start.
4. Start `cortex` — it re-seeds from TOML/YAML; you may need `POST /api/settings/recovery/ack` if recovery mode blocks motion after a corrupt-DB re-seed.
5. For inventory-only: deleting the DB re-loads from `paths.inventory` on next boot; the first successful `write_atomic` rewrites `inventory_doc` when runtime is enabled.

## See also

- [ADR-0004: Operator console](0004-operator-console.md)
- `docs/architecture.md` — configuration hierarchy (updated)
