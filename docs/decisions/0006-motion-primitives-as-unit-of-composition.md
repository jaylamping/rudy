# ADR 0006: Motion primitives as the unit of composition (2026-04)
 
## Status
 
Proposed
 
## Context
 
The current motion module in [`crates/cortex/src/motion/`](../../crates/cortex/src/motion/) ships one Rust module per named pattern — `sweep`, `wave`, `jog` — each with its own REST endpoint (`POST /motors/:role/motion/{sweep,wave,jog,stop}`) and its own bespoke controller implementation. `MotionRegistry` enforces "one controller per motor at a time" over the top of this, per the 2026-04-19 addendum to [ADR-0004](./0004-operator-console.md).
 
This shape was right for the original bring-up plan: a small menu of hand-written patterns invoked from the operator console's Tests tab. It is wrong for the direction the project is headed. Two concrete pressures argue for a different abstraction:
 
**1. The pattern catalog will grow, the boilerplate will not pay off.** A serious demonstration robot needs more than `wave` and `sweep`. `reach_to`, `point`, `shake`, `nod`, `oscillate_wrist`, `arm_drop`, and the many minor variations ("wave slowly", "wave enthusiastically") will accumulate. Implementing each as its own module, controller type, and endpoint produces a combinatorial explosion of thinly-differentiated code. The useful unit is not "a wave controller" but "a parameterized oscillation applied to a chosen joint with a chosen amplitude and frequency for a chosen number of cycles."
 
**2. Natural-language driven motion requires a toolkit, not a menu.** The stated V1 demo is "wave your right arm" issued via voice, interpreted by an LLM. The realistic architecture for this is the *LLM-as-trajectory-composer* approach: the LLM composes motion from a small vocabulary of parameterized primitives, and `cortex` executes them with full safety enforcement. That architecture *requires* the primitives to exist as a first-class, schema-described, compositional vocabulary. A menu of hardcoded `POST /motion/wave` endpoints cannot be composed by an LLM — or, rather, making it work requires a translation layer that becomes its own primitive API with extra steps.
 
Today `MotionRegistry::start()` takes a concrete controller type per pattern. We need it to take a primitive + parameters, where primitives are extensible without touching the registry, the REST surface, or the WebTransport wiring.
 
## Decision
 
### D1. Introduce a `Primitive` trait as the unit of motion
 
A primitive is a self-describing, parameterized motion behavior. Each primitive carries:
 
- A **schema** (JSON Schema, derived via `schemars` from the param type) describing its parameters for external consumers (LLMs, external scripts, SPA forms).
- A **validation** step that checks parameters against runtime state (soft travel limits, robot availability, parameter ranges) and produces a validated value.
- An **execution** function that runs the closed-loop motion using the existing `MotionContext` (bus handle, registry lock, status broadcast, stop signal).
```rust
pub trait Primitive: Send + Sync + 'static {
    /// Stable snake_case identifier used in the wire protocol
    /// (e.g. "move_joint", "oscillate", "sequence").
    fn name(&self) -> &'static str;
 
    /// Human-readable description served to the LLM / UI.
    fn description(&self) -> &'static str;
 
    /// JSON schema of the parameters, derived via schemars.
    fn schema(&self) -> &'static RootSchema;
 
    /// Pure param validation against the current robot state.
    /// Errors here become 400s with structured reasons.
    fn validate(
        &self,
        params: &Value,
        state: &RobotState,
    ) -> Result<ValidatedParams, PrimitiveError>;
 
    /// Which motor roles will this primitive drive? Read before execute
    /// so the registry can enforce per-motor concurrency without the
    /// primitive body participating in locking.
    fn motors_claimed(&self, params: &ValidatedParams) -> SmallVec<[Role; 8]>;
 
    /// Execute the motion. Called after MotionRegistry has claimed the
    /// motors this primitive declared.
    fn execute<'a>(
        &'a self,
        params: ValidatedParams,
        ctx: MotionContext<'a>,
    ) -> BoxFuture<'a, Result<(), MotionError>>;
}
```
 
`MotionRegistry::start()` is reshaped to take `(Arc<dyn Primitive>, Value)` plus the session id. Everything else about the registry — per-motor concurrency, supersede semantics, stop propagation, `motion_status` WT broadcast — is unchanged.
 
### D2. One execute endpoint, one discovery endpoint
 
The per-pattern endpoints collapse into a single surface:
 
```
POST   /api/motion/execute
       body: { "primitive": "move_joint", "params": { ... } }
       -> 202 Accepted { "motion_id": "mt_01hx..." }
 
GET    /api/motion/execute/:motion_id
       -> { "status": "running" | "completed" | "superseded" | "failed", ... }
 
POST   /api/motion/execute/:motion_id/stop
       -> 204 No Content
 
GET    /api/motion/primitives
       -> [
            {
              "name": "move_joint",
              "description": "...",
              "schema": { ... JSON schema ... }
            },
            { "name": "oscillate", ... },
            ...
          ]
```
 
`GET /api/motion/primitives` is the LLM's toolkit — the same schema catalog that ships in code is served to the client. No second source of truth, no schema drift. The existing `motion_status` WT broadcast stream continues to carry live status; this ADR does not change that stream's shape, only what can appear in the `primitive` field.
 
### D3. Composition via `Sequence` and `Parallel` as primitives
 
`Sequence` and `Parallel` are themselves primitives whose parameters contain other primitive invocations:
 
```json
{
  "primitive": "sequence",
  "params": {
    "steps": [
      { "primitive": "move_joint", "params": { "joint": "r_shoulder_roll", "target_deg": 45, "duration_s": 0.8 } },
      { "primitive": "oscillate",  "params": { "joint": "r_wrist_roll",    "amplitude_deg": 20, "freq_hz": 2.0, "cycles": 3 } },
      { "primitive": "move_joint", "params": { "joint": "r_shoulder_roll", "target_deg": 0,  "duration_s": 1.0 } }
    ]
  }
}
```
 
`Sequence` runs steps in order, stops the whole thing on first failure, and propagates stop requests down to the currently-executing step. `Parallel` launches its children concurrently, claims all their motors up-front (rejecting at validate-time if two children contend for the same motor), completes when all children complete, and fails and stops siblings on first child failure. Both are implemented once in `motion/combinators.rs` and never grow per-pattern logic.
 
The recursive shape is what gives the LLM compositional reach with a small vocabulary. "Wave your right arm" becomes a `Sequence` of `MoveJoint` + `Oscillate` + `MoveJoint`; "wave with both arms" becomes a `Parallel` of two such sequences; "wave three times then rest" is a parameter change on the inner `Oscillate`.
 
### D4. Safety enforcement is unchanged
 
The primitive refactor reshapes *what* gets called; it does not alter *where safety lives*. All existing gates stay:
 
- **Server-side travel-limit check** against the per-motor soft limits in `config/actuators/inventory.yaml`. Enforced in `MoveJoint::validate`, `Oscillate::validate`, etc., with no code path from `/execute` to the bus that bypasses the validator.
- **Enable gate** refusing un-verified motors. Enforced by the bus worker, not by the primitive.
- **Single-operator 423 Locked** on `/api/motion/execute`, same as every other mutating endpoint. Checked once per execute call, not per constituent primitive inside a `Sequence`.
- **Dead-man / heartbeat semantics** for interactive primitives (the hold-to-jog case) continue to live on the WT bidi stream. EOF-as-`ClientGone` stops the running motion. Interactive mode is a primitive *invocation channel*, not a separate endpoint or a separate primitive.
- **Firmware envelope** (position limits, `canTimeout`, velocity ramping) and mechanical hard stops are below all of this and unchanged.
The audit log records `{primitive, params}` in addition to the existing endpoint/session/motor_id fields. An LLM-generated `Sequence` of six steps is one audit entry (its top-level invocation); the constituent steps are not separately audited because validation happened atomically at the top. The top-level `params` are already verbose enough to reconstruct what ran.
 
### D5. V1 vocabulary is joint-space only
 
The first primitives shipped are intentionally joint-space, not Cartesian:
 
| Primitive | Purpose |
| --- | --- |
| `move_joint` | Single joint to a target angle, with duration or velocity. Supersedes the motion portion of the current `jog` endpoint. |
| `oscillate` | Sinusoidal motion of a joint — amplitude, frequency, cycles. Covers `sweep` and `wave` semantically. |
| `home` | Move all declared joints to their `home_deg` per `inventory.yaml`. |
| `hold` | Maintain current position with enable asserted for a duration. Useful inside `Sequence` between moves. |
| `stop` | Immediate `cmd_stop` to a joint set. Degenerate primitive — exists for audit symmetry and so the LLM has a representable "stop everything" call. |
| `sequence` | Run primitives in order. |
| `parallel` | Run primitives concurrently. |
 
Cartesian primitives (`reach_to`, `point_at`, `track`) require an IK solver and a validated end-effector frame. Neither exists in `cortex` yet. They are explicitly out of scope for this ADR and will be introduced by a later ADR when a concrete use case demands them. The V1 wave demo does not need IK.
 
### D6. Backward compatibility via deprecated wrappers
 
The existing endpoints (`POST /motors/:role/motion/{sweep,wave,jog,stop}`) are retained as thin wrappers that construct the equivalent primitive invocation and forward internally to the execute path. They emit a deprecation header (`Deprecation: true`, `Sunset: <date>`) and log a warning on call.
 
The `link` SPA migrates surface-by-surface: hold-to-jog first (most frequently exercised, best stress test), then Tests tab bench routines, then the dashboard pattern buttons. When the last SPA caller is migrated, the deprecated endpoints are removed in a follow-up ADR.
 
[`bench_tool`](../../tools/robstride/)'s direct-CAN path (`--direct`) is unaffected; it doesn't go through `cortex` and never will. A future `--via-cortex` mode (mentioned as deferred in ADR-0004) would target the new `/api/motion/execute` surface directly.
 
### D7. The LLM integration is a consumer of this ADR, not this ADR
 
This ADR does not adopt an LLM, choose a model, define an intent-parsing layer, or specify a voice pipeline. Those are downstream of the primitive API and belong in their own ADR. What this ADR promises is that when the LLM integration arrives, it is a thin translation layer: the LLM receives `GET /api/motion/primitives` as its tool schema and emits `POST /api/motion/execute` calls. No changes to cortex's core abstractions are required to make that work.
 
## Migration plan
 
Phased so each step ships independently and nothing destabilizes the operator console mid-flight:
 
1. **Scaffolding PR.** Introduce the `Primitive` trait, `MotionContext`, `PrimitiveError`, and the registry changes that let `start()` take a primitive. Port `MoveJoint` and `Oscillate` as the first two primitives. Add `POST /api/motion/execute` and `GET /api/motion/primitives`. No SPA changes. Old endpoints untouched. `Sequence` / `Parallel` not included yet.
2. **First SPA migration.** Move the hold-to-jog dead-man through `/api/motion/execute` via the existing WT bidi stream (reusing the `ClientFrame::MotionJog` shape — the frame just carries a primitive invocation now). Old `/jog` endpoint stays but unused from the SPA. Proves the primitive path end-to-end under the most stressed real-world usage.
3. **Composition PR.** Add `Sequence`, `Parallel`, `Hold`, `Home`, `Stop`. Migrate `wave` and `sweep` to primitive expressions (thin shim handlers compose the equivalent `Sequence` and forward).
4. **Full SPA migration.** Tests tab, dashboard pattern buttons, actuator detail page Controls tab all switch to `/api/motion/execute`.
5. **Deprecation window.** Old endpoints warn in logs for two weeks of soak.
6. **Removal** (future ADR). Old endpoints deleted, per-pattern modules removed.
## Consequences
 
### Positive
 
- **Composable motion vocabulary.** "Wave slowly" vs "wave fast" is a parameter change. "Wave with both arms" is a `Parallel`. New behaviors do not require new modules or new endpoints.
- **LLM integration becomes a drop-in.** The schema catalog at `GET /api/motion/primitives` is exactly what function-calling APIs (OpenAI, Anthropic, local tool-using models) want. Adding the LLM layer is writing a client, not rewriting `cortex`.
- **Single audited API surface.** One endpoint to version, one schema to document, one log format to parse.
- **Safety properties centralize.** Every primitive invocation passes through the same gates; no per-endpoint drift. Travel-limit changes in `inventory.yaml` propagate automatically because every primitive's `validate()` reads from the same state.
- **External scripting falls out for free.** A Python or shell script POSTing `{primitive, params}` works without any API carve-out. `bench_tool --via-cortex` becomes a one-afternoon job later.
### Negative / trade-offs
 
- **Indirection cost.** An extra trait dispatch and schema validation on every call. Expected to measure in the tens of microseconds on `move_joint` — negligible against CAN round-trip, but nonzero. Should be benchmarked in the scaffolding PR.
- **Schema maintenance.** JSON Schema is derived via `schemars` from Rust types. This works cleanly for most primitive param shapes. The edge case is recursive primitive parameters in `Sequence` / `Parallel`, where the schema needs to reference itself. `schemars` supports `$ref` for this but it requires careful setup. Pinned by a wire-format test analogous to `wt_codec.rs`.
- **Recursive stop/supersede semantics require care.** `Sequence::stop()` must interrupt the currently-running child cleanly and not leak the remainder of the step list. `Parallel::stop()` must fan out concurrently. The motion module tests gain a fixture-heavy combinator suite.
- **Debugging changes shape.** Instead of "the `wave` handler did X", traces show "the primitive dispatcher routed to `Oscillate` which did X". Tracing needs to record the primitive name and the motion_id at span level. Worth doing regardless for the LLM-driven case where the `Sequence` → child primitive trace is how you'll read what the LLM chose to do.
- **The SPA's Tests tab UI becomes generic.** Today each pattern has a bespoke card. Post-refactor, the tab is a primitive picker + a schema-driven form (already a pattern the SPA uses for firmware params). This is a net improvement for extensibility but is a nontrivial SPA PR separate from the server-side work.
### Deferred
 
- **LLM integration itself.** Separate ADR when the first real implementation lands. Expected scope: model choice, STT pipeline, intent validation, prompt design, safety of LLM-generated parameter values, what happens when the LLM hallucinates a joint name.
- **Cartesian primitives.** `reach_to`, `point_at`, `track`. Require an IK solver plus an end-effector frame definition in URDF. Separate ADR when a demo demands them.
- **Trajectory generator primitives.** Time-parameterized spline trajectories (quintic, minimum-jerk) as distinct objects that primitives consume. Currently every primitive implies its own trajectory shape; when the vocabulary grows, factoring trajectory generation out makes sense. Not yet.
- **Primitive discovery from external crates.** All primitives live in `crates/cortex/src/motion/primitives/`. A plugin system for external primitives (a `rudy-primitive-*` crate pattern) is conceivable but speculative; the current inventory is small enough that inline is fine.
- **`ros2_control` integration.** When `driver_node` arrives (per ADR-0005, when that's written), it will consume cortex's primitive API from above or expose a compatible one on the ROS side. The ros2_control hardware interface is *lower* in the stack than primitives — primitives generate joint commands, the hardware interface writes them. This ADR does not alter that layering.
## Alternatives considered
 
1. **Keep per-pattern endpoints, add an LLM dispatcher layer on top.** Rejected. The dispatcher rapidly grows into exactly the primitive API this ADR proposes, but less principled (schemas implicit in dispatcher code, no discovery endpoint, no external-script story, no audit symmetry). If you're going to build it, build it.
2. **Python scripting sandbox for motion.** Rejected. Adds a Python runtime dependency on the Pi, gives up Rust's type safety on the motion path, and requires its own safety sandbox. The primitive API is already a very simple DSL; a scripting language is strictly more scope.
3. **Full DSL for motion scripts.** Rejected as overkill. The primitive + `Sequence` / `Parallel` shape *is* a DSL — JSON-shaped, composable, typed by JSON Schema. Adding a parser, a standalone grammar, and a second serialization format buys nothing the JSON form doesn't already provide.
4. **Wait until the LLM integration is concretely in progress, design primitives from there.** Rejected. The current pattern modules entrench with every week of use, and the SPA forms, audit log, and WT status broadcast all calcify against the per-pattern shape. Cheaper to refactor now with three patterns than later with fifteen.
5. **Use `ros2_control`'s `joint_trajectory_controller` as the primitive layer.** Rejected for Phase 1. It's the right tool once ros2_control is driving the robot, but it covers trajectory execution only — not oscillation, not composition, not the LLM tool schema. And it's not available today; `control` ships a loopback `SystemInterface`. This ADR defines the primitive layer that would eventually sit above (or alongside) `joint_trajectory_controller`, not inside it.
## Follow-ups
 
- Runbook: author `docs/runbooks/motion-primitives.md` covering how to add a primitive, the trait contract, the testing pattern, the schema-generation workflow, and the deprecation path for old endpoints.
- ADR (when relevant): LLM-driven motion — model, pipeline, safety of generated params, hallucination handling.
- ADR (when relevant): removal of deprecated per-pattern endpoints.
- ADR (when relevant): Cartesian primitives and IK.