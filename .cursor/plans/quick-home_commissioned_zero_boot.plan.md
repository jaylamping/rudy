---
name: commissioned-zero quick-home boot
overview: Replace the per-power-cycle "Verify & Home ritual" with a commissioned-zero model. Operator commissions each actuator once (mechanically position joint at neutral, click "Set zero & save", daemon issues type-6 SetZero followed by type-22 SaveParams, reads back add_offset (0x702B) and stores it as `commissioned_zero_offset` in `inventory.yaml`). On every subsequent boot the telemetry loop, on first valid read per motor, reads back the firmware's current `add_offset` and verifies it matches the stored commissioned value (Class 1 shenanigan detection). If matched, computes `wrap_to_pi(mech_pos_rad)`; if the wrapped value is inside `travel_limits`, automatically runs the existing slow-ramp homer toward a new per-motor `predefined_home_rad` (default 0.0) and on success marks `Homed` — no operator click. If the wrapped value is outside the band, leaves `OutOfBand` and waits for the operator to physically move the joint into range, after which the OutOfBand→InBand transition retriggers the same auto-home. If the offset readback disagrees with the stored value, leaves a new `OffsetChanged { stored, current }` state and refuses to home until the operator explicitly re-commissions or restores. Layer 6 auto-recovery (`crates/rudydae/src/can/auto_recovery.rs`) is fully removed — operator handles physical OutOfBand recovery manually. The slow-ramp homer (`crates/rudydae/src/api/home.rs`) is retained as the auto-home executor and remains exposed as a manual diagnostic for post-maintenance / weird-tracking debugging.
todos:
  - id: set-zero-clarify-semantics
    content: "STANDALONE PRECURSOR (independently valuable, lands first): the existing `POST /api/motors/:role/set_zero` is RAM-only by design (ADR-0002 line 213-219: type-6 alone is not flash-persistent), but today's API docs and UI imply persistence. This is a documentation+UX bug, not a wire-protocol bug. Fix it without changing wire behavior: (a) update the endpoint's docstring to explicitly say 'RAM only; survives until power cycle. For persistence use POST /commission'; (b) update `actuator-tests-tab.tsx`'s set_zero bench action label and confirmation copy to say 'set zero (RAM only — does not persist)'; (c) update `actuator-controls-tab.tsx` set_zero label similarly; (d) update audit log entry detail to include `persisted: false`. No CAN protocol changes. The ACTUAL persistence path is the new `commission` endpoint, which adds the type-22 SaveParams. This todo is the safety net so any operator using set_zero before the full plan ships is not silently confused. Add a `set_zero_audit_records_not_persisted` contract test."
    status: completed
  - id: read-add-offset-helper
    content: "New `LinuxCanCore::read_add_offset(motor) -> anyhow::Result<f32>` thin wrapper over `read_param_value(motor, params::ADD_OFFSET)`. Used by both commissioning (read-back-after-save) and boot orchestrator (drift detection). Mock-CAN equivalent returns `Ok(0.0)` so contract tests don't need a real bus."
    status: completed
  - id: motor-commissioning-fields
    content: "Extend `inventory::Motor` with `commissioned_zero_offset: Option<f32>` (the readback from add_offset at commissioning time, in radians) and `predefined_home_rad: Option<f32>` (the per-motor target the boot orchestrator drives toward; default 0.0 when None). Both `#[serde(default)]`. Update `link/src/lib/types/Motor.ts` via `cargo test --features ts-export` (or whatever the existing pattern is). Migration: existing inventory entries get `commissioned_zero_offset: null` and must be re-commissioned before they can quick-home; until then the boot orchestrator skips them with a clear log message."
    status: completed
  - id: commission-endpoint
    content: "New `POST /api/motors/:role/commission` endpoint. Flow: control-lock check → validate `motor.present` → send type-6 SetZero → send type-22 SaveParams → sleep 100ms (give firmware time to flush flash) → call `read_add_offset` → write `commissioned_zero_offset` and bump `commissioned_at` in inventory.yaml via `inventory::write_atomic` → emit `SafetyEvent::Commissioned { role, offset_rad, t_ms }` → audit-log the readback value with result=Ok. On ANY step failure (CAN error, readback mismatch with expected ~0.0, inventory write failure), leave inventory unchanged and return a clear error envelope: `{ error: 'commission_failed', detail: 'step X: ...', readback_rad: Option<f32> }`. The plain `set_zero` endpoint stays available for advanced/diagnostic use but is gated (see `set-zero-gate` todo)."
    status: completed
  - id: boot-state-new-variants
    content: "Update `BootState` enum in `crates/rudydae/src/boot_state.rs`: REMOVE `AutoRecovering` variant (Layer 6 going away). ADD `OffsetChanged { stored_rad: f32, current_rad: f32 }` (Class 1 shenanigan detected). ADD `AutoHoming { from_rad: f32, target_rad: f32, progress_rad: f32 }` (boot orchestrator is currently driving the slow-ramp homer toward predefined home). ADD `HomeFailed { reason: String, last_pos_rad: f32 }` (auto-home aborted; operator intervention needed). Update `permits_enable` to allow only `Homed`. Update ts-rs export."
    status: completed
  - id: boot-orchestrator-module
    content: "DEPENDS ON: `extract-run-homer`, `read-add-offset-helper`, `motor-commissioning-fields`, `boot-state-new-variants`. New `crates/rudydae/src/boot_orchestrator.rs`. Public `pub async fn maybe_run(state: SharedState, role: String)` that the telemetry hook spawns on each transition into `InBand` from `Unknown` (first telemetry) or from `OutOfBand`. Idempotent: tracks per-role `boot_orchestrator_attempted: Arc<Mutex<HashSet<String>>>` on `AppState` to avoid double-running within one daemon lifetime. Steps: (1) gate on `state.cfg.safety.auto_home_on_boot` — if false, return early with an info log; (2) load motor from inventory; (3) if `commissioned_zero_offset` is None, log info ('skipping orchestrator: motor uncommissioned, run POST /commission first') and skip; (4) call `read_add_offset` over CAN; on CAN failure log warn and retry once after 200ms, then give up (do NOT force_set anything — let the next telemetry tick retrigger); (5) compare readback against stored within `commission_readback_tolerance_rad`; mismatch → force_set `OffsetChanged { stored, current }`, audit-log, emit `SafetyEvent::OffsetChanged`, and return; (6) read latest `mech_pos_rad` from `state.latest`; if missing or stale (> `max_feedback_age_ms`) skip and let next tick retrigger; (7) compute `wrapped = wrap_to_pi(mech_pos_rad)`; if `wrapped ∉ travel_limits` → do nothing (classifier already set OutOfBand) and clear the `attempted` flag so a future InBand transition retriggers this orchestrator; (8) force_set `AutoHoming { from_rad: mech_pos_rad, target_rad: predefined_home_rad.unwrap_or(0.0), progress_rad: 0.0 }`; (9) call `slow_ramp::run` (the extracted homer body) toward `predefined_home_rad.unwrap_or(0.0)`, with a progress callback that updates the `AutoHoming::progress_rad` field via a new `update_auto_homing_progress` helper; (10) on success → `mark_homed`, audit-log, emit `SafetyEvent::Homed`; (11) on failure → `force_set HomeFailed { reason, last_pos_rad }`, audit-log, emit `SafetyEvent::HomeFailed { role, reason }`."
    status: completed
  - id: extract-run-homer
    content: "Refactor `crates/rudydae/src/api/home.rs::run_homer` so the loop body is callable from outside the HTTP handler. Move it (plus `MAX_HOMER_VEL_RAD_S`) to `crates/rudydae/src/can/slow_ramp.rs`. The HTTP handler becomes a thin wrapper that does preflight + audit + calls `slow_ramp::run`. The boot orchestrator calls the same `slow_ramp::run` directly. This is a pure refactor in one commit — no behavior change — so it can be reviewed in isolation."
    status: completed
  - id: telemetry-hook
    content: "In `crates/rudydae/src/can/bus_worker.rs::apply_type2` and `crates/rudydae/src/can/linux.rs::merge_aux_into_latest`, after the existing `boot_state::classify` call, inspect the `ClassifyOutcome`. On `Changed { prev: Unknown, new: InBand }` OR on `MergeOutcome::Seeded` for a motor whose state is currently `InBand`, call `boot_orchestrator::maybe_run(state, role)`. Same for the `Changed { prev: OutOfBand{..}, new: InBand }` transition. Use `tokio::spawn` so the telemetry tick is never blocked by orchestrator I/O."
    status: completed
  - id: remove-layer-6
    content: "Delete `crates/rudydae/src/can/auto_recovery.rs` and `mod auto_recovery;` from `can/mod.rs`. Remove `BootState::AutoRecovering`, `boot_state::mark_auto_recovering`, `boot_state::update_auto_recovery_progress`, `BootState::is_auto_recovering`. Remove the `auto_recovery_attempted: Arc<Mutex<HashSet<String>>>` field from `AppState` and the `auto_recovery_max_rad`, `recovery_margin_rad`, `auto_recovery_enabled` fields from `SafetyConfig` (and their defaults). Remove every `is_auto_recovering` gate from `api/jog.rs`, `api/control.rs`, `api/home.rs`, `motion/preflight.rs`, `motion/controller.rs`. Remove the `maybe_spawn_recovery` calls from `bus_worker.rs` and `linux.rs`. Update `link/src/lib/types/BootState.ts` and remove all `auto_recovering` UI rendering across actuator tabs."
    status: completed
  - id: home-all-real-implementation
    content: "Shipped: `home_all` runs `slow_ramp::run` per motor toward `predefined_home_rad`, torso phase sequential then per-limb parallel with `LimbResult` map; limb quarantine pre-check; failures set `HomeFailed` + SafetyEvent. Follow-up (not shipped): SSE/streaming progress — endpoint remains single JSON response."
    status: completed
  - id: limb-quarantine-gate
    content: "New `crates/rudydae/src/limb_health.rs` module exposing `pub fn limb_status(state: &SharedState, limb: &str) -> LimbStatus { Healthy | Quarantined { failed_motors: Vec<(String, BootState)> } }`. A limb is Quarantined if ANY motor assigned to that limb is in `HomeFailed`, `OffsetChanged`, or `OutOfBand`. New `pub fn require_limb_healthy(state, role) -> Result<(), ApiError>` helper that resolves the role's limb (via `inventory.by_role(role).and_then(|m| m.limb.clone())`) and returns 409 `limb_quarantined { limb, failed_motors: [(role, state_kind), ...] }` if not healthy. Apply this gate at the API boundary in EVERY motion-issuing endpoint: `enable`, `jog`, `motion/move`, `motion/stop`, `home`, `home_all` (per-limb), `tests/*` (the bench tests that move motors). Do NOT apply to read-only paths: GET endpoints, `params` reads, `feedback` subscription, `set_zero` (the operator may need to re-zero a quarantined motor as part of recovery), `commission`, `restore_offset`. Motors that have no `limb` assigned are treated as their own single-motor limb (i.e., a failure on an unlimbed motor only quarantines that motor itself, matching today's behavior). Add `limb_quarantine_blocks_sibling_jog` and `limb_quarantine_allows_recovery_actions` contract tests."
    status: completed
  - id: ui-commissioning-flow
    content: "On `actuator-controls-tab.tsx` (or a new dedicated 'Commissioning' card), replace/augment the existing 'Set zero' button with 'Commission zero (saves to flash)'. Two-step confirm dialog spelling out: 'this will (1) tell the firmware that the joint's CURRENT physical position is the new zero, (2) save that to firmware flash so it persists across power cycles, (3) record the offset in inventory.yaml. After this, every boot will auto-home this actuator to its predefined home position.' On success show the readback `add_offset` value. The plain (non-flash) `set_zero` action gets demoted to an 'advanced' disclosure under the same card with its own warning."
    status: completed
  - id: ui-boot-state-new-variants
    content: "Update `BootStateBadge` in `link/src/routes/_app.actuators.$role.tsx` to render the new variants: `OffsetChanged` (red, with the stored vs current values and a 'Re-commission' / 'Restore offset' action), `AutoHoming` (blue/spinner, with progress bar from `from_rad`/`target_rad`/`progress_rad`), `HomeFailed` (amber, with the reason and a 'Retry' button that calls the existing manual-homer endpoint). Remove `AutoRecovering` rendering. Update `actuator-overview-tab.tsx`'s `GoHomeBar` (just added) to also surface during `AutoHoming`."
    status: completed
  - id: ui-restore-offset-action
    content: "On `OffsetChanged` state, expose 'Restore offset to {stored} rad' as an action: new `POST /api/motors/:role/restore_offset` endpoint that writes the stored value back to firmware via `write_param_f32(ADD_OFFSET, stored)` followed by type-22 SaveParams. This is the recovery path for 'someone ran set_zero from the bench tool by accident' without forcing the operator to re-mechanically position the joint."
    status: completed
  - id: predefined-home-ui
    content: "On `actuator-travel-tab.tsx`, add a 'Predefined home' field next to the min/max sliders. Defaults to 0°, must be inside the travel band, persisted via the same `inventory::write_atomic` pattern. Validation: rejected if outside `[min_rad, max_rad]`. Also requires a new `PUT /api/motors/:role/predefined_home` endpoint (parallels the existing travel_limits PUT)."
    status: completed
  - id: set-zero-gate
    content: "Update `POST /api/motors/:role/set_zero` (the raw, RAM-only diagnostic endpoint) to require an explicit `confirm_advanced: true` flag in the JSON body. Without the flag, return 400 `requires_confirmation` with a body explaining 'this is the diagnostic endpoint that does NOT save to flash and DOES NOT update inventory; the operator likely wants POST /commission instead'. With the flag, behave as today (send type-6, reset BootState to Unknown, audit-log with `set_zero_advanced` action name to distinguish from commission). Rationale: the operator confirmed `set_zero` should remain available, but a misclick from the UI shouldn't silently shift a commissioned motor's frame; the explicit flag forces an intentional choice. The `actuator-controls-tab.tsx` 'Set zero (advanced)' disclosure passes the flag automatically; ad-hoc curl/CLI usage requires the flag."
    status: completed
  - id: ui-troublemaker-identification
    content: "OPERATOR-CRITICAL: when something is wrong, the operator must be able to identify exactly which actuator in <5 seconds from anywhere in the UI. Specific requirements: (1) Header bar across every page surfaces a global health summary: '✓ all 12 actuators healthy' or 'X 2 issues: left_arm.shoulder_pitch (HomeFailed), right_leg.knee_pitch (OffsetChanged)'. Each named motor in the summary is a clickable link to that actuator's page on the troublesome tab. (2) `link/src/components/dashboard/actuator-status-card.tsx` already lists actuators on the dashboard — extend it to color-code by BootState (green Homed, blue AutoHoming, amber OutOfBand/HomeFailed, red OffsetChanged) and include the role label prominently in the same color. Sort order: failed states first, then AutoHoming, then OutOfBand, then Homed (least urgent last). (3) `link/src/routes/_app.actuators.$role.tsx` BootStateBadge already exists; ensure it includes the limb name in its label when limb is assigned ('left_arm.shoulder_pitch — HomeFailed: tracking_error at 0.42 rad'). (4) The new global header health summary lives in `link/src/components/app-shell.tsx` (or wherever the persistent chrome is — check `_app.tsx` route) and reads from the same `[motors]` cache the dashboard uses, so no extra polling. (5) A failed motor's audit log entries should be one-click accessible from the badge ('view audit history for this motor'); reuse the existing audit log page filter by role. Confirm filter exists, add if missing."
    status: completed
  - id: ui-limb-quarantine-feedback
    content: "When the daemon returns `409 limb_quarantined { limb, failed_motors: [...] }` from any motion endpoint, the UI must clearly explain to the operator (1) which command was refused, (2) which limb is quarantined, (3) which specific motor(s) caused the quarantine, (4) one-click navigation to the troublemaker. New `link/src/components/quarantine-toast.tsx` shown by the global toast layer when ANY motion mutation returns this error code. Toast format: 'Cannot jog right_arm.elbow_pitch: limb right_arm is quarantined because right_arm.shoulder_pitch is in HomeFailed. [Open shoulder_pitch →]'. Also: every motion-issuing button in the actuator UI (`enable`, `jog`, the dead-man button, `move`, `tests/*` triggers) must check the limb quarantine state from the `[motors]` cache and pre-disable itself with a tooltip explaining why ('Limb right_arm quarantined: see right_arm.shoulder_pitch') so the operator gets the feedback BEFORE clicking. Implement a new `useLimbHealth(role)` hook in `link/src/lib/hooks/useLimbHealth.ts` that returns `{ healthy: bool, quarantined_by: string[] }` for any role; every motion button uses it."
    status: completed
  - id: contract-tests-orchestrator
    content: "New tests in `crates/rudydae/tests/api_contract.rs` (or a new `boot_orchestrator_lifecycle.rs`): (1) `boot_orchestrator_skips_uncommissioned_motor` — motor with `commissioned_zero_offset: None` boots, orchestrator logs and leaves state alone; (2) `boot_orchestrator_quick_homes_in_band` — commissioned motor boots in-band, orchestrator transitions Unknown→InBand→AutoHoming→Homed without operator action; (3) `boot_orchestrator_detects_offset_change` — commissioned motor boots, mock CAN reports `add_offset` differing from stored value, state lands in `OffsetChanged`; (4) `boot_orchestrator_leaves_out_of_band` — commissioned motor boots outside band, state stays OutOfBand, orchestrator does not run; (5) `boot_orchestrator_retriggers_on_in_band_transition` — motor boots OutOfBand, then telemetry shows it moved into band, orchestrator runs and reaches Homed; (6) `boot_orchestrator_failure_lands_in_home_failed` — slow-ramp aborts (mock CAN forced tracking error), state lands in `HomeFailed`; (7) `restore_offset_writes_and_saves` — endpoint round-trips correctly."
    status: completed
  - id: contract-tests-commissioning
    content: "Tests for the new flash-persistent commissioning: (1) `commission_endpoint_writes_inventory` — done; (2) `commission_endpoint_can_failure_leaves_inventory_clean` (non-Linux stub `RealCanHandle`, step 3 fails, inventory untouched) + unknown_role/motor_absent tests — done; (3) wire-level type-6→type-22 ordering — deferred (no vcan/frame harness in repo; run on hardware or add Linux+vcan CI later)."
    status: completed
  - id: set-zero-wire-contract-deferred
    content: "OPTIONAL / DEFERRED: SocketCAN or Pi integration test asserting commission emits type-6 then type-22 on the bus. Not required for merge; software path covered by commission handler + stub tests."
    status: cancelled
  - id: defaults-and-config
    content: "Add `commission_readback_tolerance_rad: f32` to `SafetyConfig` (default 1e-3, the threshold for the orchestrator's offset-match check). Add `auto_home_on_boot: bool` to `SafetyConfig` (default TRUE per operator decision — the whole point is to make the safe path automatic; the escape hatch exists in case someone needs to disable the orchestrator during a hardware investigation). Document in `config/rudyd.toml` with a `[safety]` block comment explaining each."
    status: completed
  - id: audit-log-coverage
    content: "Audit-log every state transition the orchestrator initiates: 'boot_orchestrator: detected offset change for {role}: stored={x} current={y}', 'boot_orchestrator: auto-homed {role}: from={x} to={y} ticks={n}', 'boot_orchestrator: auto-home failed for {role}: reason={...}'. Same audit envelope as existing entries (`AuditEntry` in `crates/rudydae/src/audit.rs`)."
    status: completed
  - id: safety-event-variants
    content: "Extend `SafetyEvent` enum in `crates/rudydae/src/types.rs` with: `Commissioned { t_ms, role, offset_rad }` (emitted by commission endpoint), `OffsetChanged { t_ms, role, stored_rad, current_rad }` (emitted by orchestrator when readback mismatches), `HomeFailed { t_ms, role, reason, last_pos_rad }` (emitted by orchestrator on slow-ramp abort), `AutoHomed { t_ms, role, from_rad, target_rad, ticks }` (emitted by orchestrator on success — distinct from existing `Homed` which is operator-initiated). Remove `AutoRecoveryAttempted` and any related variants when Layer 6 is removed. These events flow through the existing safety-event SSE channel that the dashboard already subscribes to, so the global health bar (todo `ui-troublemaker-identification`) can update in real time without polling."
    status: completed
  - id: docs-and-adr
    content: "Update ADR-0002 (`docs/decisions/0002-rs03-protocol-spec.md`) with a new section explaining the commissioned-zero invariant and its dependence on the type-22 SaveParams call after type-6 SetZero. Mark the existing `.cursor/plans/boot-time_travel-band_gate_170c48af.plan.md` as superseded at the top with a pointer to this plan. Update `config/actuators/inventory.yaml` header comments explaining the new `commissioned_zero_offset` and `predefined_home_rad` fields and the commissioning workflow. Update `crates/rudydae/src/boot_state.rs` module docstring to reflect the new semantics (no longer per-power-cycle; commissioned motors auto-home on boot). Add a `docs/operator-guide/commissioning.md` (or extend an existing operator doc if one exists) walking through the first-time commissioning workflow with screenshots: position joint, click Commission Zero, observe success, restart daemon, observe auto-home."
    status: completed
  - id: implementation-order
    content: "RECOMMENDED ORDER (each item is a separate reviewable commit; a dependency arrow means the predecessor must merge first). Phase A (precursors, can land standalone): set-zero-clarify-semantics → set-zero-gate → read-add-offset-helper. Phase B (commissioning groundwork): motor-commissioning-fields → commission-endpoint + ui-commissioning-flow. Phase C (orchestrator core): extract-run-homer → boot-state-new-variants → safety-event-variants → defaults-and-config → boot-orchestrator-module + telemetry-hook → audit-log-coverage. Phase D (recovery and homing target): ui-restore-offset-action → predefined-home-ui. Phase E (limb safety): limb-quarantine-gate → ui-limb-quarantine-feedback. Phase F (UI completion): ui-boot-state-new-variants → ui-troublemaker-identification. Phase G (orchestration consumers): home-all-real-implementation. Phase H (cleanup): remove-layer-6 → docs-and-adr. Phase I (validation): contract-tests-orchestrator + contract-tests-commissioning (can run in parallel with Phase F-H but must complete before merge). Note `remove-layer-6` is intentionally LATE: the gates it removes are still in place during the migration, which is fine — the new orchestrator coexists with Layer 6 because they don't overlap (Layer 6 only fires on OutOfBand and orchestrator only fires after a SUCCESSFUL classification). Removing Layer 6 only after the orchestrator has been validated against the operator's actual hardware is the safer order."
    status: completed
isProject: false
---

**Active plan:** Commissioned-zero boot orchestration and operator workflows documented here supersede the Layer 6 / auto-recovery portions of [boot-time_travel-band_gate_170c48af.plan.md](boot-time_travel-band_gate_170c48af.plan.md) (that file has a **Superseded** banner). ADR: [docs/decisions/0002-rs03-protocol-spec.md](../docs/decisions/0002-rs03-protocol-spec.md) (**Commissioned mechanical zero**). Operator walkthrough: [docs/operator-guide/commissioning.md](../docs/operator-guide/commissioning.md).

**Status:** Implementation complete in `main` (Phases A–I). Optional follow-ups tracked as cancelled/deferred todos: wire-level type-6→type-22 capture test; `home_all` live progress streaming.

## Commissioned-zero quick-home boot

### What this replaces

Today every motor requires an operator to click "Verify & Home" on every power-cycle, because `BootState` is per-power-cycle by design and only `Homed` permits enable. The original design (see [.cursor/plans/boot-time_travel-band_gate_170c48af.plan.md](.cursor/plans/boot-time_travel-band_gate_170c48af.plan.md)) was conservative about persisting any cross-boot position information because it didn't trust that the firmware's reported position would still be mechanically meaningful after a power-off (the multi-turn-encoder-loses-count problem).

That conservatism was correct given the assumptions of that plan. This plan changes the assumptions:

- Operator confirms that the only realistic "shenanigans during power-off" that matter are detectable by reading back the firmware's stored zero offset (Class 1: zero changed) and that joint motion within the mechanical envelope while powered off is *fine* (Class 3: not shenanigans, just physics).
- Operator confirms that boot-time auto-recovery from OutOfBand is overkill for their deployment context — they will be physically present and can manhandle the joint into range when needed.
- Operator confirms that on every boot, they want each commissioned motor to physically drive itself to a predefined neutral pose, not just sit wherever it was at last shutdown.

These assumptions enable a much simpler model: commission once, trust the readback, auto-home on every boot.

### The commissioned-zero invariant

**Once an operator has commissioned a motor:**
1. The firmware's `add_offset` (parameter 0x702B) is set such that the joint's neutral mechanical position reads as 0.0 rad.
2. That offset is saved to firmware flash (type-22).
3. The same offset value is recorded in `inventory.yaml` as `commissioned_zero_offset`.

**On every subsequent boot, the daemon:**
1. Reads back the firmware's current `add_offset` over CAN.
2. Compares it to the stored `commissioned_zero_offset` within `commission_readback_tolerance_rad` (default 1e-3).
3. Mismatch → `BootState::OffsetChanged { stored, current }`. Refuse all motion. Surface in UI with two recovery actions: re-commission (operator agrees the new position is the new neutral) or restore (operator wants the old offset back; daemon writes it and saves it).
4. Match → continue with the auto-home flow.

**The auto-home flow:**
1. Read latest `mech_pos_rad` and compute `wrapped = wrap_to_pi(mech_pos_rad)`.
2. If `wrapped ∉ travel_limits` → leave `OutOfBand`. Operator must physically move the joint into the band; the next telemetry classification will retrigger this flow.
3. If `wrapped ∈ travel_limits` → spawn the slow-ramp homer toward `predefined_home_rad` (default 0.0). State transitions Unknown → InBand → AutoHoming → Homed.
4. If the slow-ramp aborts (tracking error, fault, timeout) → `HomeFailed { reason }`. Operator can retry via the existing manual `Verify & Home` endpoint, or investigate.

### What we keep

- **Travel limits** — unchanged. The soft band the daemon enforces on every commanded move.
- **Per-step ceiling** — unchanged. Max rad/tick on commanded moves while not Homed.
- **Slow-ramp homer (`api/home.rs`)** — kept as the auto-home executor (called by the new orchestrator) and as a manual diagnostic tool. Refactored so its loop body lives in `crates/rudydae/src/can/slow_ramp.rs` and can be invoked by both the HTTP handler and the orchestrator.
- **`BootState::OutOfBand`** — still gates enable, still surfaces in the UI.
- **`BootState::Homed`** — still the only state that permits enable.

### What we remove

- **Layer 6 auto-recovery** (`crates/rudydae/src/can/auto_recovery.rs`, `BootState::AutoRecovering`, all the related config fields and gates). Operator handles physical OutOfBand recovery manually.
- **The "Verify & Home is required every boot" UX** — replaced with the auto-home orchestrator. The button becomes a diagnostic / retry action only.

### What we add

- **`commissioned_zero_offset` and `predefined_home_rad`** on `Motor`.
- **`POST /api/motors/:role/commission`** — combined SetZero + SaveParams + readback + inventory update.
- **`POST /api/motors/:role/restore_offset`** — recovery path for the `OffsetChanged` state.
- **`boot_orchestrator` module** — the per-motor "first valid telemetry" hook that runs the readback + auto-home flow.
- **`BootState::OffsetChanged`, `BootState::AutoHoming`, `BootState::HomeFailed`** variants.

### What this changes about existing files

#### Daemon

| File | Change |
|------|--------|
| `crates/rudydae/src/api/control.rs` | `set_zero` handler: clarify docstring (RAM-only by design), gate on `confirm_advanced: true` flag, audit-log as `set_zero_advanced` with `persisted: false` detail; remove `is_auto_recovering` gate. The `commission` endpoint is the persistent path — raw set_zero remains the diagnostic path |
| `crates/rudydae/src/api/home.rs` | Extract `run_homer` body to `can/slow_ramp.rs`; this file becomes a thin HTTP wrapper; remove `is_auto_recovering` gate |
| `crates/rudydae/src/api/home_all.rs` | Replace placeholder `mark_homed` with real per-limb-sequential / cross-limb-parallel orchestration using `slow_ramp::run` |
| `crates/rudydae/src/api/jog.rs` | Remove `is_auto_recovering` gate |
| `crates/rudydae/src/api/mod.rs` | Add `commission`, `restore_offset`, `predefined_home` (PUT) routes |
| `crates/rudydae/src/api/commission.rs` | NEW — POST /commission handler |
| `crates/rudydae/src/api/restore_offset.rs` | NEW — POST /restore_offset handler |
| `crates/rudydae/src/api/predefined_home.rs` | NEW — PUT /predefined_home handler |
| `crates/rudydae/src/api/motors.rs` | Surface new BootState variants in `summary_for` (no shape change beyond enum additions) |
| `crates/rudydae/src/boot_state.rs` | Drop `AutoRecovering`; add `OffsetChanged`, `AutoHoming`, `HomeFailed`; drop `mark_auto_recovering`, `update_auto_recovery_progress`, `is_auto_recovering`; add `force_set_offset_changed`, `force_set_auto_homing`, `force_set_home_failed` helpers |
| `crates/rudydae/src/boot_orchestrator.rs` | NEW — see todo `boot-orchestrator-module` |
| `crates/rudydae/src/can/auto_recovery.rs` | DELETE |
| `crates/rudydae/src/can/bus_worker.rs` | Remove `maybe_spawn_recovery` call after `classify`; replace with `boot_orchestrator::maybe_run` |
| `crates/rudydae/src/can/linux.rs` | Same as bus_worker; add `read_add_offset` helper |
| `crates/rudydae/src/can/mod.rs` | Drop `mod auto_recovery`; add `mod slow_ramp` |
| `crates/rudydae/src/can/slow_ramp.rs` | NEW — extracted `run_homer` body from `api/home.rs` |
| `crates/rudydae/src/config.rs` | Drop `auto_recovery_max_rad`, `recovery_margin_rad`, `auto_recovery_enabled`; add `commission_readback_tolerance_rad`, `auto_home_on_boot` |
| `crates/rudydae/src/inventory.rs` | Add `commissioned_zero_offset`, `predefined_home_rad` to `Motor` |
| `crates/rudydae/src/limb_health.rs` | NEW — `limb_status`, `require_limb_healthy` for per-limb quarantine gate |
| `crates/rudydae/src/motion/preflight.rs` | Remove `is_auto_recovering` gate; add `require_limb_healthy` call |
| `crates/rudydae/src/motion/controller.rs` | Remove `is_auto_recovering` reference |
| `crates/rudydae/src/state.rs` | Add `boot_orchestrator_attempted: Arc<Mutex<HashSet<String>>>`; remove `auto_recovery_attempted` |
| `crates/rudydae/src/types.rs` | Add `SafetyEvent::Commissioned`, `OffsetChanged`, `HomeFailed`, `AutoHomed` variants; remove `AutoRecoveryAttempted` and related |

#### Tests

| File | Change |
|------|--------|
| `crates/rudydae/tests/api_contract.rs` | Add commissioning + restore_offset + new home_all tests; remove auto_recovery_* tests |
| `crates/rudydae/tests/boot_orchestrator_lifecycle.rs` | NEW — orchestrator state transitions |
| `crates/rudydae/tests/common/mod.rs` | Add `set_commissioned_offset` fixture; remove `force_auto_recovering` if it exists |
| `crates/rudydae/src/boot_state.rs` | Update unit tests for new enum variants |

#### UI

| File | Change |
|------|--------|
| `link/src/lib/types/BootState.ts` | Regenerate from ts-rs |
| `link/src/lib/types/Motor.ts` | Regenerate from ts-rs |
| `link/src/lib/types/MotorSummary.ts` | Regenerate from ts-rs |
| `link/src/lib/api.ts` | Add `commissionMotor`, `restoreOffset`; update `homeMotor` docstring (now diagnostic) |
| `link/src/routes/_app.actuators.$role.tsx` | Update `BootStateBadge` for `OffsetChanged`, `AutoHoming`, `HomeFailed`; remove `AutoRecovering` |
| `link/src/components/actuator/actuator-controls-tab.tsx` | Add Commission Zero card; demote raw set_zero to "advanced" |
| `link/src/components/actuator/actuator-travel-tab.tsx` | Add `predefined_home_rad` field; remove `auto_recovering` references; keep `VerifyAndHomeCard` (now diagnostic, with updated copy) |
| `link/src/components/actuator/actuator-overview-tab.tsx` | Update `GoHomeBar` to surface during AutoHoming |
| `link/src/components/dashboard/actuator-status-card.tsx` | Surface `OffsetChanged` and `HomeFailed` as warnings; color-code by BootState; sort failed states first |
| `link/src/components/quarantine-toast.tsx` | NEW — toast shown when motion endpoints return `409 limb_quarantined` |
| `link/src/components/global-health-bar.tsx` | NEW — header bar across every page surfacing a global actuator health summary with one-click navigation to troublemakers |
| `link/src/lib/hooks/useLimbHealth.ts` | NEW — hook returning `{ healthy, quarantined_by }` for any role; consumed by every motion-issuing button to pre-disable with tooltip |
| `link/src/components/actuator/dead-man-jog.tsx` | Pre-disable when `useLimbHealth(role).healthy === false` |
| `link/src/components/actuator/actuator-tests-tab.tsx` | Update set_zero bench label to clarify RAM-only; pre-disable motion-issuing tests via `useLimbHealth` |

### Migration / rollout

This change ships with `auto_home_on_boot: true` per operator decision — the goal is maximum flight hours with the new behavior to surface bugs early. The migration is forward-compatible because the orchestrator skips uncommissioned motors:

- **Uncommissioned motors** (existing inventory entries with `commissioned_zero_offset: null`) — orchestrator skips them with a clear log message (`'skipping orchestrator: motor uncommissioned, run POST /commission first'`). These motors continue to require the manual `Verify & Home` flow on every boot, exactly as today. The UI surfaces this as a yellow "Not commissioned" badge on the actuator page.
- **Commissioned motors** — orchestrator runs on first valid telemetry. Operator sees the joint physically drive to its `predefined_home_rad` automatically, with no clicks.

Recommended operator rollout:

1. Daemon ships with the changes; nothing changes for any existing motor until explicitly commissioned.
2. Operator picks one low-risk actuator (a shoulder is a good first choice — wide travel band, easy to manually move, low consequence if homing misbehaves).
3. Operator physically positions the joint at neutral, clicks "Commission Zero (saves to flash)", confirms the dialog, observes the readback value match expected ~0.0 rad.
4. Operator restarts the daemon (or power-cycles the motor) and watches that motor auto-home to neutral on boot.
5. Repeat for remaining motors over subsequent sessions, prioritizing motors used in the most common workflows.
6. Optional: once all priority motors are commissioned, the operator can leave the rest uncommissioned indefinitely if there's a reason (e.g., a motor under active development whose neutral pose isn't yet finalized).

**Rollback:** if the auto-home orchestrator misbehaves in a way that needs immediate disabling, set `auto_home_on_boot = false` in `config/rudyd.toml` and restart the daemon. All commissioned motors revert to the manual `Verify & Home` flow until the orchestrator is re-enabled. No data loss; commissioning state on disk is unaffected.

### Safety analysis

**The four classes of "what could go wrong" and how this plan handles each:**

| Class | Mechanism | Detection | Response |
|-------|-----------|-----------|----------|
| 1. Zero offset changed in flash since commissioning | `add_offset` register readback comparison | Definitive | `OffsetChanged` state, refuse motion, expose Re-commission / Restore actions |
| 2. Joint moved past mechanical envelope while powered off (multi-turn drift) | Wrapped position outside `travel_limits` | Heuristic but reliable for narrow bands | `OutOfBand` state, refuse motion, operator manually moves joint |
| 3. Joint moved within mechanical envelope while powered off | None (and intentionally so) | N/A | Auto-home drives joint to `predefined_home_rad`; operator sees expected motion |
| 4. Joint mechanically wedged inside band but encoder reads free | Slow-ramp homer's tracking-error abort | Definitive (motor stalls) | `HomeFailed` state, operator investigates |

**The remaining uncovered case:** Class 4 with the wedge happening to be at exactly `predefined_home_rad`. Auto-home thinks the move succeeded (no tracking error because no motion was needed). This is fundamentally undetectable from the encoder alone and requires the same care that Class 4-elsewhere requires: don't put hard stops inside your declared travel band. The same hazard exists in the current system; this plan does not make it worse.

### Resolved decisions (operator-confirmed)

- **`commission` is a new endpoint.** The raw `set_zero` endpoint stays available for advanced/diagnostic use (the operator may want to re-zero a motor mid-session for reasons that aren't "redefine the persisted neutral"); the commissioning UX uses the new combined endpoint.
- **`auto_home_on_boot` defaults to `true`.** Operator wants maximum flight hours with the new behavior to surface bugs early.
- **`home_all` failure isolation is per-limb.** A single motor failure quarantines its entire limb (any further motion command to any motor in that limb is refused) until the failed motor returns to `Homed`. Other limbs continue to operate. Motors with no `limb` assignment are treated as their own single-motor limb.

### Out of scope (explicit non-goals)

- Per-joint `mechanical_range: bounded_360 | continuous` flag for joints that don't fit the "narrower than 360°" assumption. Anticipated for wheels/turrets later; none today.
- Persisting `last_known_position_rad` to inventory.yaml for cross-boot delta checks. The commissioned-zero model makes this unnecessary for the failure modes we care about.
- Per-motor `predefined_home_rad` profiles (e.g. "stowed pose" vs "ready pose"). For now there is one home position per motor; multi-pose orchestration is a future feature on top of `home_all`.
- Auto-restoring boot-time low-torque RAM writes after fault clear / E-stop release (already an open follow-up from the original plan and remains so).
