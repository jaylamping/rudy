---
name: polymorphic device inventory
overview: Replace the flat RS03-assumed `motors: Vec<Motor>` schema with a polymorphic `devices: Vec<Device>` schema where `Device` is an enum of `Actuator | Sensor | Battery`, `Actuator` is `{ common: ActuatorCommon, family: ActuatorFamily }`, and `ActuatorFamily` is `Robstride { model: RobstrideModel } | ...future families`. The `RobstrideModel` enum (`RS01 | RS02 | RS03 | RS04`) drives per-model spec lookup (`config/actuators/robstride_rs0X.yaml`) and protocol dispatch through the existing-but-unused `RsActuator` trait in `ros/src/driver/src/robstride/mod.rs`. Sensor variants start with concrete types for the planned hardware (motion/IMU sensors like BNO085, force/torque sensors, gyros, with cameras/LIDAR scaffolded but not implemented). Migration is a HARD CUT: bump `schema_version: 1 → 2`, refuse to load v1 files at boot with a clear migration error, ship a one-shot migration script (`tools/migrate_inventory_v1_to_v2.py` or rust binary) that the operator runs once. Discovery: passive listener on the bus worker accumulates `seen_can_ids: HashMap<(bus, can_id), LastSeen>` populated from every received frame; active scan endpoint probes a configurable ID range using the protocol-specific probe registered by each device family (RobStride uses `read_param(firmware_version)`, future sensors register their own); a unified `GET /api/hardware/unassigned` returns the diff between seen-or-probed IDs and inventory. New `/hardware` page in the SPA presents Assigned + Unassigned views, launches an onboarding wizard that walks family-identification → role assignment → limb/joint_kind (if actuator) → travel limits (if actuator) → predefined home (if actuator) → commissioning (if actuator) on commit. Builds DIRECTLY on the commissioned-zero plan (`.cursor/plans/quick-home_commissioned_zero_boot.plan.md`) — must merge AFTER that plan completes, since onboarding's commissioning step calls the new `/commission` endpoint.
todos:
  - id: device-enum-design
    content: "Design and document the polymorphic schema in `crates/rudydae/src/inventory.rs`. Top-level: `enum Device { Actuator(Actuator), Sensor(Sensor), Battery(Battery) }` with `#[serde(tag = \"kind\", rename_all = \"snake_case\")]` for tagged JSON (`{ \"kind\": \"actuator\", ... }`). `struct Actuator { #[serde(flatten)] common: ActuatorCommon, family: ActuatorFamily }`. `struct ActuatorCommon { role: String, can_bus: String, can_id: u8, present: bool, verified: bool, commissioned_at: Option<String>, travel_limits: Option<TravelLimits>, commissioned_zero_offset: Option<f32>, predefined_home_rad: Option<f32>, limb: Option<String>, joint_kind: Option<JointKind>, firmware_version: Option<String> }`. `enum ActuatorFamily { #[serde(rename = \"robstride\")] Robstride { model: RobstrideModel } }` (extensible to other vendors later). `enum RobstrideModel { Rs01, Rs02, Rs03, Rs04 }` (matches the existing `driver::robstride::RsModel`). `struct Sensor { common: SensorCommon, family: SensorFamily }` with `SensorFamily` carrying initial variants for the planned hardware (see `sensor-family-variants` todo). `struct Battery { common: BatteryCommon, family: BatteryFamily }` with at least one placeholder variant. The `kind` discriminator is a top-level union; the `family` discriminator is per-kind; concrete model is per-family. Three nested levels matches the actual taxonomy (it's not just one flat list). Add doc comments on every type explaining the layering. RFC the design in this todo's commit message before implementing — comments-only commit so reviewers can push back."
    status: completed
  - id: sensor-family-variants
    content: "Define initial `SensorFamily` variants based on operator-confirmed planned hardware. `enum SensorFamily { #[serde(rename = \"motion\")] Motion { model: MotionSensorModel }, #[serde(rename = \"force\")] Force { model: ForceSensorModel }, #[serde(rename = \"gyro\")] Gyro { model: GyroSensorModel }, #[serde(rename = \"camera\")] Camera { model: CameraModel }, #[serde(rename = \"lidar\")] Lidar { model: LidarModel } }`. For each, define a model enum with at least one placeholder variant (e.g. `enum MotionSensorModel { Bno085 }`, `enum CameraModel { Placeholder }`). Sensors do NOT need `boot_state`, `travel_limits`, `commissioned_zero_offset`, or `predefined_home_rad` (these are actuator concepts). `SensorCommon` has: `role`, `can_bus` (or `i2c_bus`/`spi_bus` later — for now CAN-only), `can_id`, `present`, `verified`, `commissioned_at`, `notes`. The schema is *defined* in this PR but no code path actually USES sensor entries yet — that's a separate plan once concrete sensors arrive. The variants exist so v2 inventory.yaml has a stable shape from day one and operators can hand-add sensors as placeholders."
    status: completed
  - id: schema-version-enforcement
    content: "Bump `Inventory::schema_version` default to `Some(2)`. Update `Inventory::load` (`crates/rudydae/src/inventory.rs:166-174`) to REQUIRE `schema_version == 2`; loading a v1 file returns a structured error: `InventoryError::SchemaVersionMismatch { found: u32, required: u32, migration_hint: String }` where the hint says 'run `cargo run --bin migrate_inventory` (see docs/operator-guide/inventory-v2-migration.md)'. Daemon refuses to start until inventory.yaml is migrated. This is the 'hard cut' guardrail — there is no transparent v1→v2 fallback because the schema change is too large for sane runtime conversion. Update `Inventory::validate` (`crates/rudydae/src/inventory.rs:180-218`) to walk `devices` (Vec<Device>) instead of `motors` (Vec<Motor>); per-actuator validations remain (role format, limb/joint_kind consistency, per-limb joint uniqueness); add per-device validation: unique `(can_bus, can_id)` across ALL devices regardless of kind (no two devices can share an ID on the same bus)."
    status: completed
  - id: migration-binary
    content: "New `crates/rudydae/src/bin/migrate_inventory.rs`. Reads `config/actuators/inventory.yaml` (assumes v1), writes `config/actuators/inventory.yaml.v2` (operator manually swaps in after review). Mapping: every entry in v1 `motors:` becomes a `Device::Actuator` with `family: Robstride { model: Rs03 }` (the only model present today), `common` populated from v1 fields, and ANY v1 fields that landed in `extra` (the `serde(flatten) BTreeMap`) preserved into a new `notes_yaml: Option<String>` field on `ActuatorCommon` so nothing is silently lost. Print a per-motor diff to stdout so operator can audit. Refuse to overwrite an existing `.v2` file. Document usage in `docs/operator-guide/inventory-v2-migration.md`. Add `migration_round_trip` integration test that loads the existing repo `inventory.yaml`, runs migration, parses the v2 output, and asserts every original field is represented (including `extra` payload preservation)."
    status: completed
  - id: refactor-by-role-by-can-id
    content: "Update `Inventory::by_role(role) -> Option<&Device>` and `by_can_id(bus, can_id) -> Option<&Device>` to return the polymorphic `Device`. Add convenience methods: `actuator_by_role(role) -> Option<&Actuator>` (returns Some only if the device is an actuator), `actuators() -> impl Iterator<Item = &Actuator>` (filter+unwrap), `sensors() -> impl Iterator<Item = &Sensor>`, `batteries() -> impl Iterator<Item = &Battery>`. Most existing call sites (50+ across `api/`, `can/`, `motion/`) want the actuator convenience methods; very few want the polymorphic `Device`. The polymorphic accessors stay available for the new Hardware page and discovery code which DO care about all kinds."
    status: completed
  - id: per-device-spec-resolution
    content: "Replace the global `state.spec: Arc<ActuatorSpec>` (`crates/rudydae/src/state.rs`, single field loaded from `cfg.paths.actuator_spec`) with `state.specs: HashMap<RobstrideModel, Arc<ActuatorSpec>>`. Loading: at daemon start, scan `config/actuators/robstride_*.yaml`, parse each, key by `actuator_model` field. Today only `robstride_rs03.yaml` exists; the loader should not fail if other model files are absent. Add `state.spec_for(model: RobstrideModel) -> Arc<ActuatorSpec>` lookup with a clear panic-on-missing message. Update every `state.spec.*` consumer (5+ files per the exploration: `telemetry.rs`, `api/params.rs`, `api/config_route.rs`, `can/linux.rs::seed_boot_low_limits`, `tests/common/mod.rs`) to first resolve the actuator's family/model and look up the appropriate spec. The `api/config_route.rs` exposed `actuator_model` becomes a list of supported models. NEW: ensure `Spec` parsing covers `protocol`, `hardware`, `op_control_scaling`, `thermal` sections (currently silently ignored — the polymorphism will need them as soon as we have a non-RS03 model with a different op_control range or different gear ratio)."
    status: pending
  - id: extend-spec-struct
    content: "Extend `ActuatorSpec` (`crates/rudydae/src/spec.rs:13-24`) to actually parse the YAML sections it currently ignores: `protocol` (id_layout, comm_types map, bitrate), `hardware` (gear_ratio, encoder_resolution_bits, torque_constant_nm_per_arms, etc.), `op_control_scaling` (position/velocity/kp/kd/torque_ff ranges — used for MIT mode), `thermal` (derating curves). Add validation: `actuator_model` field must match the filename (e.g. `robstride_rs03.yaml` must contain `actuator_model: RS03`) so the spec loader's filename→model mapping can't go wrong. Add a `RobstrideSpec` newtype that wraps `ActuatorSpec` plus the additional protocol/hardware sections, since (long-term) non-RobStride actuator families will need their own spec shape and we want the type system to catch family-mismatch at compile time."
    status: completed
  - id: rsactuator-trait-adoption
    content: "rudydae's CAN paths (`bus_worker.rs`, `linux.rs`) currently `use driver::rs03::{session, feedback, frame, params}` directly — RS03 is hardcoded by import. Refactor to dispatch through the existing-but-unused `driver::robstride::RsActuator` trait (already defined in `ros/src/driver/src/robstride/mod.rs`). Each `Actuator` value in the inventory resolves to a concrete impl: `Robstride { model: Rs03 }` → `driver::rs03::Rs03`, `Robstride { model: Rs04 }` → `driver::rs04::Rs04` (when that module exists). The bus worker's hardcoded `params::RUN_MODE = 2` and `params::SPD_REF` calls become `actuator.run_mode_velocity()` and `actuator.spd_ref_index()` trait methods. This is a non-trivial refactor of bus_worker.rs and linux.rs but is the structural change that makes the polymorphism real. AUDIT scope before starting: every `driver::rs03::*` use statement in `crates/rudydae/src/` must either move behind the trait or be deleted. Document this audit in the PR description."
    status: pending
  - id: travel-rail-from-spec
    content: "`crates/rudydae/src/can/travel.rs:18-24` hardcodes `HARDWARE_*_RAD = ±4π` as the outer rail, with a comment 'matches the RS03 spec.protocol.position_min_rad/position_max_rad'. Move this to `RobstrideSpec` (read from `op_control_scaling.position.range` per model) and resolve per-actuator via the new `state.spec_for(model)`. Different models may have different MIT position ranges; today's hardcoding is a latent bug that this todo eliminates. Add `travel_rail_from_spec_per_model` test."
    status: pending
  - id: passive-seen-ids-tracker
    content: "On `AppState`, add `pub seen_can_ids: Arc<RwLock<HashMap<(String, u8), SeenInfo>>>` where `SeenInfo { first_seen_ms: i64, last_seen_ms: i64, frame_count: u64 }`. In `bus_worker::handle_frame`, BEFORE the existing `apply_type2` / type-17 dispatch, extract source motor ID from the arbitration ID (existing helper) and update `seen_can_ids[(bus, src_motor)]`. Cost: one HashMap update per received frame; benchmark first to ensure no regression vs. the current per-frame budget. The map is unbounded by ID space (max 127 entries per bus); no eviction needed. Used by `GET /api/hardware/unassigned` to surface IDs that have been seen on the bus but aren't in inventory."
    status: pending
  - id: device-probe-trait
    content: "New `crates/rudydae/src/discovery.rs` module. Define `pub trait DeviceProbe: Send + Sync { fn family_name(&self) -> &'static str; async fn probe(&self, bus: &str, can_id: u8, core: &LinuxCanCore) -> Option<DiscoveredDevice>; }` where `DiscoveredDevice { bus: String, can_id: u8, family_hint: String, identification_payload: Option<serde_json::Value> }`. Register probes in a `DeviceProbeRegistry` (Vec<Box<dyn DeviceProbe>>). Initial registrations: `RobstrideProbe` (sends `read_param(firmware_version)` with 50ms timeout, returns Some on any response, attempts to extract firmware_version from response payload as bonus). Future sensor probes register here too. The registry is keyed by family so each scan iteration tries each registered probe sequentially; whichever returns Some first wins. Operator confirmed: probe doesn't have to *understand* the response — presence is enough."
    status: pending
  - id: active-scan-endpoint
    content: "New `POST /api/hardware/scan` endpoint (`crates/rudydae/src/api/hardware_scan.rs`). Body: `{ bus: Option<String> (default: scan all configured buses), id_range: Option<(u8, u8)> (default: 0x01..=0x7F), timeout_ms: Option<u64> (default: 50) }`. Iterates each bus × each ID; for each (bus, id) iterates `DeviceProbeRegistry` until one probe returns Some. Returns `Vec<DiscoveredDevice>` plus a per-(bus,id) summary of which probes were tried and what they returned. Concurrency: probes run sequentially per (bus, id) but parallel across (bus, id) up to a configurable max — but only one frame on the wire at a time per bus to avoid response collisions. Uses the existing bus worker's request/response infrastructure; doesn't need a new CAN socket. Idempotent. Auto-runs once on daemon startup gated by `safety.scan_on_boot: bool` (default true) — uses `tokio::spawn` so it doesn't block the listener startup. **Stub shipped 2026-04-19:** `POST /api/hardware/scan` in `api/hardware.rs` returns `ok: true`, empty `discovered`, explanatory `message`."
    status: pending
  - id: unassigned-list-endpoint
    content: "New `GET /api/hardware/unassigned` endpoint. Computes the union of `(seen_can_ids ∪ last_active_scan_results)` minus `inventory.devices.iter().map(|d| (d.can_bus, d.can_id))`. Returns `Vec<UnassignedDevice>` where `UnassignedDevice { bus: String, can_id: u8, source: \"passive\" | \"active_scan\" | \"both\", first_seen_ms: i64, last_seen_ms: i64, family_hint: Option<String> (from the most recent active probe), identification_payload: Option<serde_json::Value> }`. The endpoint is read-only and cheap — no CAN traffic. Cached via the standard `[hardware]` query key in the SPA. Updated whenever `seen_can_ids` changes (passive) or scan completes (active) — push via the existing safety event SSE channel. **Stub shipped 2026-04-19:** route + JSON type exist; always returns `[]` until `passive-seen-ids-tracker` + scan cache."
    status: pending
  - id: hardware-page-route
    content: "New `link/src/routes/_app.hardware.tsx` route. Layout: header with global health summary (reuses the global health bar from the boot-orchestrator plan's `ui-troublemaker-identification` todo), then two main sections: (1) **Assigned** — list of all `Device`s in inventory grouped by kind (Actuators, Sensors, Batteries) and within Actuators by limb. Each row shows role, can_bus, can_id, family, model, BootState badge (actuators only), and quick-link to the existing detail page. (2) **Unassigned** — list of discovered-but-not-inventoried devices from `GET /api/hardware/unassigned`. Each row shows bus, can_id, source (passive/active/both), family hint, last seen timestamp, and a 'Onboard' button that launches the wizard. A persistent 'Discover' button at the top of the Unassigned section triggers `POST /api/hardware/scan` and shows progress. Default sort: Unassigned section first when non-empty (operator's eye is drawn there), Assigned second. Empty-state copy on Unassigned: 'No new devices detected. Click Discover to actively scan, or plug in a new device and wait for it to transmit.'"
    status: completed
  - id: hardware-page-extensibility
    content: "Structure the hardware page with a `<HardwareSection title=\"Actuators\" items={...} renderRow={...} />` component pattern in `link/src/components/hardware/hardware-section.tsx` so future Sensor / Battery / Camera / Lidar sections drop in without restructuring. Each row renderer is a per-kind component (`<ActuatorRow>`, `<SensorRow>`, etc.) so each can show kind-appropriate details. Today only `<ActuatorRow>` has real content; the others render placeholder rows with role + can_id + 'Configuration UI coming soon' until concrete consumers exist. The Unassigned section uses a single `<UnassignedRow>` regardless of family hint."
    status: completed
  - id: onboarding-wizard
    content: "New `link/src/components/hardware/onboarding-wizard.tsx`. Modal/sheet launched from clicking 'Onboard' on an Unassigned row. Steps (skipping non-applicable steps based on family): (1) **Confirm device family** — pre-filled from probe's family hint, operator can override (dropdown of registered families); (2) **Confirm model** — for actuators, dropdown of models within the family; for sensors, dropdown of models within the sensor family; (3) **Assign role** — text input with validation against the canonical role format; for actuators, encourages the `{limb}.{joint_kind}` form; (4) **Assign limb + joint_kind** (actuators only) — dropdowns with the existing `JointKind` enum values; (5) **Set travel limits** (actuators only) — same UI as the existing travel tab, defaulting to a conservative ±30°; (6) **Set predefined home** (actuators only) — defaults to 0.0 rad, must be inside the band; (7) **Commission zero** (actuators only) — operator physically positions joint, clicks 'Commission', system runs the commission flow from the boot-orchestrator plan; (8) **First auto-home** — observe the boot orchestrator drive the joint to predefined home; success → motor is now Homed and operational. Each step persists incrementally to inventory.yaml via `inventory::write_atomic` so the operator can stop and resume between sessions; partial state is shown as 'partially configured' on the Hardware page Assigned section. Add `onboarding_wizard_persists_per_step` integration test."
    status: pending
  - id: reassign-can-id-endpoint
    content: "New `POST /api/hardware/:bus/:current_id/reassign_can_id` endpoint (`crates/rudydae/src/api/reassign_can_id.rs`). Body: `{ new_can_id: u8 }`. Validates `new_can_id` is not already in use on that bus and not in the reserved range (0x00 = broadcast/uninitialized?). Issues type-7 SetCanId to `current_id`; sleeps 500ms to allow firmware to commit and likely reset; probes the new_id with a `read_param(firmware_version)` — if that succeeds, reports success; if not, returns a clear error envelope ('reassignment may have succeeded but new ID did not respond — power-cycle the actuator and retry probe'). NOTE: this is delicate because the response could come back from EITHER the old or new ID depending on firmware behavior; the endpoint must not assume which. Audit-logged. Used by the onboarding wizard when the operator's brand-new actuator is at the factory-default ID and needs reassignment before joining the main bus. Adds a 'Reassign CAN ID' optional pre-step in the wizard, surfaced when the Unassigned device's can_id matches a configurable `factory_default_can_id` (default unknown — needs hardware verification, see `factory-default-research` todo)."
    status: pending
  - id: factory-default-research
    content: "Verify the RS03's factory-default CAN ID by reading the RobStride RS03 user manual (`docs/vendor/rs03-user-manual-260112.pdf`) or testing on real hardware. Document in `docs/decisions/0002-rs03-protocol-spec.md` and as a comment in `config/actuators/robstride_rs03.yaml`. If the default is well-defined and consistent, the wizard can detect 'this looks like a factory-fresh actuator' and surface the reassign step automatically. If the default varies (e.g. operator pre-configures via Motor Assistant before plugging into the robot bus), this todo just documents it without auto-detection. THIS TODO IS A LITERAL READ — assigning to a coding agent will produce no output; the operator should answer this themselves and then update the plan with the answer before implementation begins."
    status: pending
  - id: ts-bindings-regen
    content: "Regenerate every TS-rs binding affected by the schema change: `Device.ts`, `Actuator.ts`, `ActuatorCommon.ts`, `ActuatorFamily.ts`, `RobstrideModel.ts`, `Sensor.ts`, `SensorFamily.ts`, `Battery.ts`, `MotorSummary.ts` (becomes `ActuatorSummary` — see `motor-summary-rename`), plus `DiscoveredDevice.ts`, `UnassignedDevice.ts` from the discovery module. The TS-rs export pattern is documented somewhere in `crates/rudydae` — find it and follow it. Verify the resulting JSON shape with the `serde(tag = \"kind\")` discriminator matches what TS-rs generates for tagged unions (it should produce `{ kind: 'actuator', ... } | { kind: 'sensor', ... } | { kind: 'battery', ... }` discriminated unions in TS)."
    status: pending
  - id: motor-summary-rename
    content: "Rename `MotorSummary` (used by `GET /api/motors`) to `ActuatorSummary` (`GET /api/actuators`). Add `GET /api/sensors` and `GET /api/batteries` returning their respective summaries (placeholder summaries for now since no real consumers exist). Add `GET /api/devices` returning the polymorphic `Vec<DeviceSummary>` for the Hardware page's Assigned section. The existing `GET /api/motors` is RETAINED as a backwards-compat alias that returns only the actuator subset (so the boot-orchestrator plan's UI doesn't break during this migration); add a `Deprecation: ...` header. Remove the alias in a follow-up plan once all SPA call sites have migrated. Update the SPA `api.listMotors()` to `api.listActuators()` and `api.listDevices()`; update the `[\"motors\"]` query key to `[\"actuators\"]` everywhere (the exploration found 12+ consumer files — every one needs updating)."
    status: pending
  - id: spa-consumer-migration
    content: "Mechanical update of all SPA files that use the `[\"motors\"]` query key or `MotorSummary` type. Per the exploration: `_app.telemetry.tsx`, `_app.actuators.$role.tsx`, `_app.params.tsx`, `dashboard/actuator-status-card.tsx`, `components/telemetry-grid.tsx`, `motor-chart.tsx`, `components/actuator/*` (all 7 files), `components/viz/use-joint-states.ts`, `WebTransportBridge.tsx`, `wtReducers.ts`, `WebTransportBridge.test.tsx`, `api.contract.test.ts`. None of these care about non-actuator devices today, so they all become `[\"actuators\"]` consumers. The Hardware page is the only `[\"devices\"]` consumer."
    status: pending
  - id: bus-worker-passive-listener-test
    content: "Add `passive_listener_records_unknown_can_ids` test in `crates/rudydae/tests/`. Inject a fake type-2 frame from CAN ID 0x55 on a bus with no inventory entry for 0x55; assert (a) the frame is silently dropped (existing behavior preserved — `apply_type2` returns early), (b) `state.seen_can_ids[(bus, 0x55)]` is populated with `frame_count: 1`. Send another frame from same ID, assert `frame_count: 2`. Verify ZERO bus traffic was emitted by the listener (passive must mean passive)."
    status: pending
  - id: scan-endpoint-tests
    content: "Tests for the active scan endpoint: (1) `scan_finds_inventoried_motor` — mock CAN with one inventoried motor responding, scan returns it; (2) `scan_finds_unknown_id` — mock CAN with a responder at an ID not in inventory, scan returns it as an unassigned device; (3) `scan_skips_silent_id` — mock CAN with no responder at an ID, scan does not include it; (4) `scan_attributes_by_family` — mock two probes (Robstride + a fake sensor family), assert the right family hint is returned for each; (5) `scan_idempotent` — running scan twice produces the same result; (6) `scan_respects_id_range` — body specifies `id_range: (0x10, 0x20)`, scan only probes that range; (7) `scan_on_boot_runs_once` — daemon startup with `scan_on_boot: true` triggers exactly one scan."
    status: pending
  - id: onboarding-wizard-tests
    content: "Tests for the onboarding flow: (1) `onboarding_actuator_full_flow` — start with an unassigned ID, walk every step, assert inventory.yaml is mutated correctly at each step and a fully-configured actuator exists at the end; (2) `onboarding_resume_after_partial` — complete steps 1-4, restart daemon, verify the partial entry persists and the wizard resumes from step 5; (3) `onboarding_sensor_skips_actuator_steps` — onboarding a sensor skips travel limits / predefined home / commission steps; (4) `onboarding_role_validation` — invalid role format rejected with clear error; (5) `onboarding_role_collision` — operator picks a role that's already in use, rejected; (6) `onboarding_can_id_collision` — somehow the same (bus, can_id) ends up onboarded twice (race condition?), `Inventory::validate` rejects on save. Add fixture helpers in `tests/common/mod.rs`: `unassigned_actuator()`, `unassigned_sensor()`, `partial_onboarding_state()`."
    status: pending
  - id: docs-and-runbook
    content: "Update operator-facing docs: (1) `docs/operator-guide/inventory-v2-migration.md` (NEW) — step-by-step migration walkthrough; (2) `docs/operator-guide/onboarding-new-hardware.md` (NEW) — Hardware page tour, when to use Discover vs. wait for passive, walking through the wizard, recovery if something goes wrong mid-onboarding; (3) `docs/runbooks/operator-console.md` — update inventory-related sections for v2 schema; (4) `docs/decisions/0002-rs03-protocol-spec.md` — add note that the protocol is now per-RobStride-model, not RS03-specific, and document the SetCanId (type-7) wire format used by reassign-can-id; (5) `config/actuators/inventory.yaml` header comments — rewrite for v2 schema, document the device kinds and family layering; (6) NEW `docs/decisions/0005-polymorphic-device-inventory.md` ADR explaining the schema decision (why a three-level taxonomy, why hard-cut, why not trait-objects, etc.) for future maintainers."
    status: pending
  - id: implementation-order
    content: "RECOMMENDED ORDER (each phase is a separate PR; phases must merge in order). Phase A (foundations, no behavior change): device-enum-design (RFC commit) → sensor-family-variants → schema-version-enforcement → migration-binary → factory-default-research (operator answers, can run in parallel with Phase A code work). Phase B (refactor existing code to polymorphic accessors, schema still single-actuator-family in practice): refactor-by-role-by-can-id → motor-summary-rename → ts-bindings-regen → spa-consumer-migration. Phase C (per-model spec resolution): extend-spec-struct → per-device-spec-resolution → travel-rail-from-spec → rsactuator-trait-adoption. Phase D (discovery infrastructure, new behavior): passive-seen-ids-tracker → device-probe-trait → active-scan-endpoint → unassigned-list-endpoint. Phase E (Hardware page UI): hardware-page-route → hardware-page-extensibility → onboarding-wizard → reassign-can-id-endpoint. Phase F (validation): bus-worker-passive-listener-test → scan-endpoint-tests → onboarding-wizard-tests. Phase G: docs-and-runbook. CRITICAL: this entire plan is a follow-on to `.cursor/plans/quick-home_commissioned_zero_boot.plan.md`. The boot-orchestrator plan must complete first because the onboarding wizard's commissioning step calls the new `/commission` endpoint that plan introduces. Do not start Phase A of this plan until that plan is fully merged and validated on hardware."
    status: pending
isProject: false
---

## Progress (update as work lands)

**Last updated:** 2026-04-19 (`extend-spec-struct` landed).

**Current phase:** **Phase C — per-model spec resolution**. Next todo: `per-device-spec-resolution` → `travel-rail-from-spec` → `rsactuator-trait-adoption`.

### Phase C handoff (for new chat / new session)

1. **`extend-spec-struct`** — **done** (`crates/rudydae/src/spec.rs`): `protocol`, `hardware`, `op_control_scaling`, `thermal`, optional `manual_ref` / `notes`; filename check for `robstride_*.yaml`; `RobstrideSpec` wrapper + unit tests.

2. **`per-device-spec-resolution`** — `crates/rudydae/src/state.rs` (+ `AppState::new`)  
   Replace single `spec: Arc<ActuatorSpec>` with `specs: HashMap<RobstrideModel, Arc<ActuatorSpec>>` (or `Arc<RobstrideSpec>` once step 1 exists). Load all `config/actuators/robstride_*.yaml` at startup; add `spec_for(model: RobstrideModel)`. Update call sites: `telemetry.rs`, `api/params.rs`, `api/config_route.rs`, `can/linux.rs` (e.g. `seed_boot_low_limits`), `tests/common/mod.rs`. Expose supported models from `/api/config` if the SPA needs them.

3. **`travel-rail-from-spec`** — `crates/rudydae/src/can/travel.rs`  
   Replace hardcoded `HARDWARE_*_RAD` with per-model rails from `op_control_scaling` (or equivalent) via `state.spec_for`.

4. **`rsactuator-trait-adoption`** — `bus_worker`, `can/linux.rs`, `ros/src/driver/.../robstride`  
   Route `driver::rs03::*` use through `RsActuator` trait; dispatch per `RobstrideModel` from inventory.

**Deferred (not Phase C):** Phase B items `motor-summary-rename`, `spa-consumer-migration`, and full `ts-bindings-regen` (beyond existing inventory TS) remain optional follow-ups — they can run in parallel with Phase C only if you want cleaner API names; **not required** to start Phase C.

**Already shipped (do not redo):** Phase A–B (v2 inventory, migration, accessors). **Phase E partial:** `/hardware` page, `GET /api/devices`, stub `GET /api/hardware/unassigned` + `POST /api/hardware/scan` (real discovery = Phase D: `passive-seen-ids-tracker` onward).

**After Phase C:** Phase D (discovery), then finish Phase E (onboarding wizard, reassign CAN ID), then Phase F tests and Phase G docs.

---

## Polymorphic device inventory

### What this enables

Today the daemon assumes every entity on the CAN bus is an RS03 actuator — `inventory.yaml` has a flat `motors:` list, `state.spec` is a single global RS03 spec, `bus_worker` imports `driver::rs03::*` directly, and the Hardware/discovery surface doesn't exist. Operator has confirmed the next 6-12 months bring multiple RobStride actuator models (RS01, RS02, RS04 alongside the existing RS03), motion/force/gyro sensors (BNO085 and similar), and eventually cameras and possibly LIDAR. The current schema and code paths can't accommodate any of that without invasive surgery at the moment of introduction.

This plan does that surgery proactively, on a hard-cut migration, while only RS03 actuators are in production. Specific outcomes:

- **`inventory.yaml` v2** with a polymorphic `devices:` list. Each device declares its `kind` (actuator | sensor | battery), its `family` (within actuators: robstride; future: other vendors), and its concrete `model` (RS01-04 today; new variants land as new hardware arrives).
- **Per-model RobStride spec resolution** — different models have different gear ratios, encoder resolutions, MIT scaling ranges, parameter index layouts. The daemon resolves the right spec per actuator instead of assuming RS03.
- **Discovery infrastructure** — passive listener accumulates seen CAN IDs from every received frame; active scan endpoint probes a configurable ID range with protocol-specific probes; unassigned list endpoint surfaces the diff. Hardware page in the SPA presents this to the operator with an onboarding wizard.
- **Sensor / battery scaffolding** — schema and types are defined for the planned hardware families even though no behavior is implemented yet, so when the first BNO085 arrives there's a place to put it.

### Why three-level taxonomy (kind → family → model)

The exploration confirmed that today's coupling has three distinct concerns:

1. **What kind of thing is this** (actuator vs sensor vs battery). Drives the highest-level UI decisions: is there a `boot_state`, are there `travel_limits`, is there a `commissioned_zero_offset`? Sensors don't have any of these; actuators always do.
2. **Which protocol family** (RobStride for all current actuators; future: other vendors). Drives wire-protocol selection — frame layout, comm types, byte order.
3. **Which concrete model** (RS01 vs RS02 vs RS03 vs RS04). Drives spec lookup — gear ratio, encoder resolution, MIT scaling ranges.

A flatter scheme (just `model: Rs03` everywhere, no kind/family layer) would conflate these and force RS03-specific code paths to spread back into the codebase. A taller scheme (kind → family → model → submodel) would over-engineer for differences that don't exist (RS03 has no submodels). Three levels matches the actual taxonomy the hardware reality presents.

### Why hard-cut migration

A backwards-compatible v1+v2 deserializer was considered and rejected. The schema change is too large for sane runtime coexistence: the v1 `motors` list flattens fields that v2 splits across `common` + `family` + a tagged `kind` discriminator. A `serde(untagged)` fallback would silently misparse v1 files in non-obvious ways (the `extra: BTreeMap` flatten field on today's `Motor` would swallow unknown v2 fields). Better to refuse v1 files at boot with a clear migration error, ship a one-shot migration script the operator runs once, and have a single source of schema truth from then on.

The risk window is small: there are exactly two actuators in the current `inventory.yaml` (one fully configured, one a placeholder). Migration is essentially a manual exercise the script automates.

### What this depends on

This plan is a follow-on to `.cursor/plans/quick-home_commissioned_zero_boot.plan.md` and must merge AFTER it. Specific dependencies:

- The onboarding wizard's commissioning step calls `POST /api/motors/:role/commission` which the boot-orchestrator plan introduces.
- The Hardware page surfaces `BootState` per actuator, including the new `OffsetChanged` / `AutoHoming` / `HomeFailed` variants.
- The global health bar (boot-orchestrator plan's `ui-troublemaker-identification` todo) is reused as the Hardware page header.
- The `commissioned_zero_offset` and `predefined_home_rad` fields on `ActuatorCommon` come from the boot-orchestrator plan's schema additions.

If the boot-orchestrator plan is delayed, this plan can technically start Phase A in parallel (the device-enum design and migration binary are independent), but Phases B onward will conflict with in-flight changes from the other plan.

### What this changes about existing files

#### Daemon

| File | Change |
|------|--------|
| `crates/rudydae/src/inventory.rs` | Polymorphic `Device` enum replaces flat `Motor`; `Inventory::devices: Vec<Device>` replaces `motors`; convenience `actuators()`, `sensors()`, `batteries()` iterators; new `actuator_by_role()` accessor |
| `crates/rudydae/src/spec.rs` | Extend `ActuatorSpec` to parse `protocol`, `hardware`, `op_control_scaling`, `thermal`; add `RobstrideSpec` newtype |
| `crates/rudydae/src/state.rs` | `state.spec` becomes `state.specs: HashMap<RobstrideModel, Arc<ActuatorSpec>>`; new `seen_can_ids: Arc<RwLock<HashMap<(String, u8), SeenInfo>>>` |
| `crates/rudydae/src/discovery.rs` | NEW — `DeviceProbe` trait, `DeviceProbeRegistry`, initial `RobstrideProbe` impl |
| `crates/rudydae/src/api/hardware_scan.rs` | NEW — `POST /api/hardware/scan` |
| `crates/rudydae/src/api/hardware_unassigned.rs` | NEW — `GET /api/hardware/unassigned` |
| `crates/rudydae/src/api/devices.rs` | NEW — `GET /api/devices` polymorphic listing |
| `crates/rudydae/src/api/actuators.rs` | NEW (renamed from `motors.rs`) — `GET /api/actuators` |
| `crates/rudydae/src/api/sensors.rs` | NEW — `GET /api/sensors` |
| `crates/rudydae/src/api/batteries.rs` | NEW — `GET /api/batteries` |
| `crates/rudydae/src/api/reassign_can_id.rs` | NEW — `POST /api/hardware/:bus/:current_id/reassign_can_id` |
| `crates/rudydae/src/api/motors.rs` | RETAINED as backwards-compat alias for `GET /api/actuators` with a deprecation header |
| `crates/rudydae/src/api/mod.rs` | Register new routes |
| `crates/rudydae/src/can/bus_worker.rs` | Refactor `use driver::rs03::*` to dispatch through `RsActuator` trait; passive listener splice in `handle_frame` to populate `seen_can_ids` |
| `crates/rudydae/src/can/linux.rs` | Same RS03→trait refactor; `seed_boot_low_limits` resolves spec per actuator's model |
| `crates/rudydae/src/can/travel.rs` | Replace hardcoded `±4π` rail with per-model `op_control_scaling.position.range` from spec |
| `crates/rudydae/src/main.rs` | Spec loader walks `config/actuators/robstride_*.yaml` instead of single path |
| `crates/rudydae/src/bin/migrate_inventory.rs` | NEW — one-shot v1→v2 migration tool |
| `crates/rudydae/src/config.rs` | Add `safety.scan_on_boot: bool` (default true) |

#### Driver crate

| File | Change |
|------|--------|
| `ros/src/driver/src/lib.rs` | Re-export future `rs01`, `rs02`, `rs04` modules alongside existing `rs03` |
| `ros/src/driver/src/robstride/mod.rs` | Operationalize the existing `RsActuator` trait — add per-model factory function `for_model(model: RsModel) -> Box<dyn RsActuator>` |
| `ros/src/driver/src/rs01/`, `rs02/`, `rs04/` | NEW STUB MODULES — minimal skeleton mirroring `rs03/` layout; concrete protocol details land when first actuator of that model is acquired and tested |

#### Tests

| File | Change |
|------|--------|
| `crates/rudydae/tests/common/mod.rs` | Update fixtures to construct `Device::Actuator(...)` instead of `Motor`; add `unassigned_actuator()`, `unassigned_sensor()`, `partial_onboarding_state()` helpers |
| `crates/rudydae/tests/api_contract.rs` | Update existing motor tests for renamed routes; add new tests for `/devices`, `/sensors`, `/batteries`, `/hardware/scan`, `/hardware/unassigned`, `/hardware/:bus/:current_id/reassign_can_id` |
| `crates/rudydae/tests/discovery_lifecycle.rs` | NEW — passive listener + active scan + onboarding wizard integration tests |
| `crates/rudydae/tests/inventory_migration.rs` | NEW — v1→v2 migration round-trip test |

#### UI

| File | Change |
|------|--------|
| `link/src/lib/types/Device.ts`, `Actuator.ts`, `ActuatorCommon.ts`, `ActuatorFamily.ts`, `RobstrideModel.ts`, `Sensor.ts`, `SensorFamily.ts`, `Battery.ts`, `DiscoveredDevice.ts`, `UnassignedDevice.ts` | NEW — regenerated from ts-rs |
| `link/src/lib/types/MotorSummary.ts` → `ActuatorSummary.ts` | Renamed |
| `link/src/lib/api.ts` | Add `listActuators`, `listSensors`, `listBatteries`, `listDevices`, `scanHardware`, `listUnassigned`, `reassignCanId`; deprecate `listMotors` (kept as alias) |
| `link/src/routes/_app.hardware.tsx` | NEW — Hardware page route |
| `link/src/components/hardware/hardware-section.tsx` | NEW — extensible per-kind section component |
| `link/src/components/hardware/actuator-row.tsx` | NEW |
| `link/src/components/hardware/sensor-row.tsx` | NEW (placeholder content) |
| `link/src/components/hardware/battery-row.tsx` | NEW (placeholder content) |
| `link/src/components/hardware/unassigned-row.tsx` | NEW |
| `link/src/components/hardware/onboarding-wizard.tsx` | NEW |
| All 12+ files using `["motors"]` query key | Mechanical rename to `["actuators"]` |
| `link/src/components/app-shell.tsx` (or wherever nav lives) | Add "Hardware" nav entry |

### Migration playbook (for the operator)

1. **Backup**: copy `config/actuators/inventory.yaml` to `inventory.yaml.v1.bak`.
2. **Run migration tool**: `cargo run --bin migrate_inventory --release`. Outputs `config/actuators/inventory.yaml.v2` and prints a per-motor diff.
3. **Audit the diff**: visually verify every existing motor entry's fields are preserved in the v2 output. Pay special attention to fields that lived in v1's `extra` (BTreeMap flatten) — they should land in `notes_yaml` on the v2 entry.
4. **Swap files**: `mv inventory.yaml.v2 inventory.yaml`.
5. **Daemon restart**: rudydae now refuses v1 files; restart should succeed against the v2 file. If the daemon still fails to start, the v2 file is malformed — restore from backup, fix the migration tool, retry.
6. **Smoke test**: open the SPA, verify all existing actuators appear under the Hardware page → Assigned → Actuators section. Verify each actuator's role / can_id / model match the backup. Verify the dashboard, telemetry, and per-actuator detail pages still work.
7. **Validate with the boot orchestrator**: power-cycle one actuator, watch the orchestrator auto-home it. The migration is non-destructive to commissioned-zero state — `commissioned_zero_offset` and `predefined_home_rad` fields are carried through.

**Rollback**: copy `inventory.yaml.v1.bak` back to `inventory.yaml`, downgrade rudydae to the previous tag (the v2-required check is in this PR; previous versions accept v1).

### Why we operationalize the existing `RsActuator` trait

The driver crate already has `ros/src/driver/src/robstride/mod.rs` defining `RsActuator` as a trait and `RsModel` as an enum including `Rs04`. This was scaffolded in anticipation of multi-model support and never used. Building on it (rather than inventing parallel infrastructure) means:

- The trait surface is already designed by someone who thought about the protocol.
- The `Rs03` impl in `ros/src/driver/src/rs03/actuator.rs` already implements the trait, so we have a working reference.
- New models (RS04 first, when operator acquires one) become new impls of the existing trait — the rudydae integration only learns to dispatch.

The trade-off: rudydae's CAN paths today bypass the trait and call `driver::rs03::session::*` directly (per the exploration). Adopting the trait is a non-trivial refactor of `bus_worker.rs` and `linux.rs`. This is captured in the `rsactuator-trait-adoption` todo and is the largest single piece of work in the plan.

### Out of scope

- **Sensor protocol implementations.** The schema reserves `Sensor` variants for motion/force/gyro/camera/lidar but no actual driver code lands in this plan. When the first BNO085 (or similar) arrives, it gets its own plan that implements the wire protocol, the probe, the telemetry handling, and the UI surface.
- **Non-CAN sensor buses.** I2C and SPI sensors will need bus-type abstractions (`SensorCommon` currently assumes `can_bus` + `can_id`). Defer until concrete non-CAN sensors are picked.
- **URDF / ros2_control integration for new actuator models.** The repo has URDF files that reference the RS03 spec; those will need updating when a different model joins the kinematic chain. Out of scope for this plan; the inventory migration is daemon-only.
- **Cross-bus device migration** (moving an actuator from `can0` to `can1` without re-onboarding). The current rename/assign endpoints handle role changes, not bus changes; multi-bus orchestration is its own design problem.
- **Discovery on non-CAN buses.** The `DeviceProbe` trait could generalize to other transports, but for now CAN is the only bus and the trait's signature reflects that. Generalize when needed.
- **Telemetry / live data for sensors.** Sensors today are inventory entries only; no telemetry pipeline. When real sensors arrive their plans add this.
- **Battery management protocol.** `Battery` is scaffolded as a kind variant but no concrete model or behavior lands. When the operator picks a BMS the plan for it adds this.
