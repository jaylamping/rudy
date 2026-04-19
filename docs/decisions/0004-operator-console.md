# ADR 0004: Operator console (`rudydae` + `link`) (2026-04)

## Status

Accepted

## Context

Rudy needs a single browser-reachable surface for day-to-day operation:

- Live telemetry (per-motor `mechPos`, `mechVel`, `vbus`, `faultSta`, and later `joint_states`).
- Firmware parameter editor — must replace Motor Studio for RS03 commissioning
  (see [tools/robstride/commission.md](../../tools/robstride/commission.md)).
  This is explicitly write-capable: the final "hard joint limit" work
  (`limit_torque`, `limit_spd`, `limit_cur`, `canTimeout`) is landed through
  this UI per the plan that prompted this ADR.
- Jog/enable controls with a dead-man switch (Phase 2).
- URDF 3D view driven by reconstructed `joint_states` (Phase 2).
- Log tail (journald for the daemon, `dmesg` for kernel CAN errors) (Phase 2).

Today the driver story is a one-shot CLI (`bench_tool`) that grabs the CAN
socket per-invocation, and the ROS 2 `driver_node` promised in
[docs/architecture.md](../architecture.md) does not yet exist (`control`
currently ships a loopback `SystemInterface`). Any live console needs
*something* to continuously own the bus and fan out telemetry.

The operator is a single human on a LAN, with Tailscale as the boundary for
any remote access.

## Decision

### D1. One daemon owns the bus: `rudydae` (new crate at `crates/rudydae/`)

A long-lived Rust daemon takes exclusive ownership of `can0` / `can1` on the
Pi and exposes typed HTTP + streaming APIs to the `link/` SPA. The daemon is
not a ROS 2 node. When a ROS 2 `driver_node` is eventually written, it will
be a **sibling consumer** of the same `rudydae` CAN handle (likely via ROS
topics that `rudydae` also publishes), not a competitor for the socket.
Rationale: we need one writer to the bus; two would race.

### D2. Dual-listener architecture

`rudydae` runs two network listeners in the same process, sharing in-process
state via `tokio::sync::broadcast` channels:

- **axum on `:8443` (HTTPS/1.1+2)** — all CRUD + embedded SPA static assets.
  Curlable, cacheable, TanStack-Query-friendly on the client.
- **wtransport on `:4433/udp` (HTTP/3 / QUIC)** — the telemetry + log
  firehose. Unreliable datagrams for high-rate signals (`mechPos`, `mechVel`,
  `vbus`); reliable unidirectional streams for fault/warn events and journald
  log lines.

Rationale: WebTransport is the user's chosen streaming transport (see plan
discussion). `axum` does not serve HTTP/3 today, so two listeners is the
pragmatic path. CRUD and streaming have different latency/reliability needs
anyway and split cleanly.

### D3. TLS via Tailscale HTTPS

No self-signed cert rotation glue. The Pi provisions a real Let's Encrypt
cert via `tailscale cert` (see [deploy/pi5/tailscale-cert.md](../../deploy/pi5/tailscale-cert.md)),
`rudydae` binds only Tailscale-local addresses, and access from outside
Tailscale yields no response. Browsers accept both the HTTPS and WebTransport
endpoints without cert pinning or developer-mode flags.

### D4. Wire format

One `serde` struct per API concept, encoded two ways:

- **JSON** on the REST side for debuggability.
- **CBOR** on WebTransport datagrams for throughput.

TypeScript types are generated from the Rust structs via `ts-rs` into
`link/src/lib/types/` when running `cargo test -p rudydae export_bindings` (see `crates/.cargo/config.toml` for `TS_RS_EXPORT_DIR`) followed by `python scripts/fix-ts-rs-imports.py` (or `npm run gen:types` in `link/`). No second source of truth.

### D5. Auth: none (network-bounded)

`rudydae` does not authenticate requests. Reachability is gated entirely by
the network: Tailscale ACLs in production, localhost in dev. Every mutating
REST request and every WebTransport session open/close still writes an entry
to `~/.rudyd/audit.jsonl` (append-only) so we have a record of who did what.

Rationale: single operator, Tailscale-bounded reachability — even a shared
bearer token was ceremony without a threat model to match. If a second
operator or non-tailnet access ever lands, revisit (see deleted `auth.rs` in
git history for the shared-token starting point).

### D6. Safety model: `rudydae` is strictly outside the firmware envelope

The firmware layering in
[docs/robotics-best-practices-reference.md](../robotics-best-practices-reference.md)
holds. On top of it, `rudydae` adds:

- **Write confirmation.** Every `PUT /api/motors/:id/params/:index` is
  server-side range-checked against
  `config/actuators/robstride_rs03.yaml:firmware_limits.hardware_range` and
  (for commissioning-relevant parameters)
  `commissioning_defaults`. The UI additionally requires a typed-confirm
  dialog.
- **Save-to-flash is a separate button.** Matches the Step 6 / Step 7 split
  in [tools/robstride/commission.md](../../tools/robstride/commission.md).
  RAM writes and flash saves are distinct endpoints
  (`PUT …/params/:index` vs `POST …/save`).
- **Enable gating.** `POST /api/motors/:id/enable` refuses unless the motor's
  `config/actuators/inventory.yaml` entry has `verified: true`. Same gate the
  Rust driver already enforces.
- **Single-operator lock.** `rudydae` runs a lightweight implicit lock on
  mutating endpoints (jog, home, params, travel-limits, verified, tests,
  rename). The first mutator from a fresh `X-Rudy-Session` silently claims
  it; a second concurrent session is refused with 423 Locked. There is no
  UI surface — see the 2026-04-18 addendum below for why this collapsed
  from the original "explicit Take control / Take over" UX.
- **Dead-man jog.** Holding a jog key sends commands at ≥ 20 Hz; releasing
  (or disconnecting) causes `rudydae` to issue `cmd_stop`. The firmware
  `canTimeout` is the backstop if `rudydae` itself hangs.
- **Append-only audit log.** Every mutating action is recorded with ISO 8601
  timestamp, session id, motor id, endpoint, and pre/post values. Survives
  restarts; rotation is the operator's problem (logrotate config shipped in
  `deploy/pi5/`).

### D7. Repository layout (reorganized as Phase 0 of this work)

```
rudy/
├── ros/src/…       ROS 2 colcon packages (driver stays here for now)
├── crates/         Cargo workspace: rudyd (and future non-ROS crates)
├── link/           Vite + React + TS SPA
├── config/ deploy/ docs/ tools/ scripts/ tests/
```

See [docs/architecture.md](../architecture.md) for the full table.

### D8. `driver` crate stays as a hybrid ament/cargo package (for now)

The `ros/src/driver/` package is both a Rust crate and a ROS 2 ament package
(has `package.xml` + `CMakeLists.txt`). `rudydae` depends on it via a relative
Cargo path:

```toml
driver = { path = "../../ros/src/driver" }
```

We do **not** split it today into a pure `crates/driver/` library + a thin
`ros/src/driver_node/` ament wrapper. Rationale: the ROS wrapper doesn't
exist yet, so splitting now would be busywork that precedes its trigger. When
`driver_node` is actually written (to bridge `ros2_control` to `rudydae`), that
is the moment to revisit; the split becomes ADR-0005 then.

## Consequences

### Positive

- Lands the long-lived CAN-owning process the architecture doc has been
  promising — unblocks both this console and future `driver_node`.
- `link/` as a standalone Vite project can be iterated on without `cargo`,
  and can later be deployed offboard (e.g. a laptop during offsite debugging)
  by pointing `VITE_RUDYD_URL` at the Pi over Tailscale.
- Parameter writes become a first-class, audited, safety-gated UI action
  rather than a footgun in Motor Studio.
- WebTransport gives room to grow: Phase 3's Isaac Lab ghost overlay and
  full 1 kHz joint-state recording are already in budget.

### Negative / trade-offs

- Two network listeners in one process — more surface area than a single
  WebSocket server. Mitigated by their sharing an in-process core and
  identical (none) auth posture.
- WebTransport debuggability is thinner than WebSocket (no `wscat`, DevTools
  support younger). Accepted; telemetry is secondary to the REST surface
  during bring-up.
- Tailscale is now a hard runtime dependency for the console. Accepted; the
  operator is already a Tailscale user and the Pi is already on the tailnet.
- Browser support: Chrome/Edge fully; Firefox partial; Safari experimental.
  Operator uses Chrome/Edge — acceptable. No WebSocket fallback (explicit
  decision).
- One more process to supervise on the Pi (`systemctl enable rudyd.service`).
  Offset by removing the ad-hoc `bench_tool` invocations.

### Deferred (explicitly not in scope)

- Multi-operator / federated auth. We'll revisit when >1 person uses Rudy.
- Remote (non-Tailscale) access. If needed, either tailnet-funnel or a
  proper reverse-proxy + OIDC; neither is ADR-0004.
- Splitting the `driver` package (see D8) — future ADR.
- `bench_tool` routing through `rudydae`. For now, `bench_tool` keeps direct
  CAN access (`--direct`) as a rescue path when the daemon is crashed; a
  `--via-rudyd` mode may be added in Phase 2 so `bench_tool` can respect the
  single-operator lock.

## Alternatives considered

1. **Single axum process, WebSocket for streaming.** Rejected: user
   specifically chose WebTransport for future growth and has accepted the
   dual-listener cost.
2. **Put `rudydae` under `ros/src/`.** Rejected: `rudydae` is not a ROS 2 package
   and forcing it into colcon's world adds ament overhead with no ROS
   integration in return. Living in `crates/` is honest about what it is.
3. **Separate repo for `link/`.** Rejected: `link/` and `rudydae` move together
   on safety-relevant changes (param schemas, auth, lock semantics). Atomic
   commits across the API boundary matter more than repo purity.
4. **Let `ros2_control` own the bus and `rudydae` subscribe via DDS.**
   Rejected for Phase 1: adds a ROS dependency to the operator console for
   no current benefit, and the `driver_node` that would be the DDS owner
   doesn't exist yet.

## Follow-ups

- ADR-0005 (future): splitting `driver` when `driver_node` is written.
- Runbook: [docs/runbooks/operator-console.md](../runbooks/operator-console.md).
- Runbook: [deploy/pi5/tailscale-cert.md](../../deploy/pi5/tailscale-cert.md).

## Addendum 2026-04-18: TLS via `tailscale serve`

The original D3 had `rudydae` terminating TLS itself for both surfaces (REST
on `:8443`, WebTransport on `:4433`) using `tailscale cert`-issued PEM
files. We are amending: the REST + SPA surface now runs **plaintext on
`127.0.0.1:8443`** and is fronted by `tailscale serve --bg --https=443
http://127.0.0.1:8443`. The HTTPS URL becomes the short MagicDNS form,
`https://<host>/` (no port, no `.ts.net` suffix).

WebTransport keeps doing its own TLS on `<tailnet-ip>:4433` because
`tailscale serve` is HTTP/1.1+HTTP/2 only — it cannot proxy HTTP/3 / QUIC.
The WT cert is still the same `tailscale cert`-issued pair.

### Why

- Auto-renewing cert for the main UI: `tailscale serve` reuses the
  Tailscale daemon's continuously-rotated Let's Encrypt cert. We deleted
  the manual `tailscale cert` step from the REST/SPA bring-up, and a
  follow-up `rudyd-cert-renew.timer` only needs to handle the WT cert.
- Shorter URL: `https://rudy-pi/` is materially nicer to type and bookmark
  than `https://rudy-pi.tail-abc123.ts.net:8443/`.
- Smaller `rudydae`: removed the `axum-server tls-rustls` feature and the
  `[http.tls]` config block + branch in `server.rs`. One less dep, one
  less crash surface (rustls `CryptoProvider`-init panics still apply for
  the WT path; we keep the `install_default()` call for that).
- Firewall simplification: `tailscale serve` already binds tailnet-only,
  so we no longer need the nftables drop rule on `:8443`. The rule on
  `:4433/udp` (for WT) stays.

### Known limitations / follow-ups

- No HSTS / HPKP / cert pinning at the SPA layer. We rely on Tailscale
  trust + browser-native LE chain validation, same as before.
- If `tailscale serve` configuration drifts (e.g. a tailnet rejoin), the
  next `apply-release.sh` re-asserts the mapping. Manual recovery:
  `sudo tailscale serve --bg --https=443 http://127.0.0.1:8443`.

## Addendum 2026-04-18: WebTransport wire format + stream registry

This addendum supersedes the brief D4 description of the WT side. The REST
side (D4 first paragraph) is unchanged.

### Wire format

Every WT message — datagram or reliable-stream frame — is a CBOR-encoded
`WtEnvelope`:

```cbor
{
  "v":     1,                     # protocol version (WT_PROTOCOL_VERSION)
  "kind":  "motor_feedback",      # snake_case stream discriminator
  "seq":   12345,                 # per-stream monotonic sequence
  "t_ms":  1700000123456,         # daemon wallclock at emit, ms since epoch
  "data":  { ...payload... }      # nested, fully opaque to the envelope
}
```

Three properties matter:

1. **`data` is nested, not flattened.** Payload field names can never
   collide with envelope field names. A future payload that defines its
   own `kind` or `seq` field cannot silently corrupt the envelope.
2. **`v` is checked on every decode.** A daemon ↔ SPA version skew is a
   loud failure (`status.error`) instead of a silent decode-as-garbage.
3. **`seq` enables gap detection on the client.** The bridge logs (and
   exposes via `onGap`) when a kind's sequence skips ahead; this gives
   us a real signal when datagrams are being dropped on the network or
   dropped by an overloaded `broadcast` channel.

The wire shape is pinned by `crates/rudydae/tests/wt_codec.rs`. Any
breaking change trips the test + breaks the SPA decoder.

### Reliability tiers

QUIC offers two delivery primitives; we use both:

| Tier      | QUIC mechanism            | Use for                                 |
| --------- | ------------------------- | --------------------------------------- |
| Datagram  | `connection.send_datagram`| latest-wins telemetry (motor, system)   |
| Stream    | one uni-stream per session| events that must not be dropped         |

Reliable frames are written into a single per-session uni-stream as
`u32 BE length | cbor body`. Length-prefixing is necessary because QUIC
streams are byte streams, not message streams. The TS reader buffers
across `read()` chunks so a frame split mid-header reassembles cleanly.

### Stream registry

Adding a new "near-realtime" stream (Phase 2 candidates: faults, joint
state, log tail) is now a fixed-shape change:

1. Define the payload struct in `types.rs` with the usual derives:
   `Serialize, Deserialize, Clone, TS`.
2. Add a one-line entry to `declare_wt_streams!`:
   ```rust
   declare_wt_streams! {
       MotorFeedback   => MotorFeedback   { kind: "motor_feedback",   transport: Datagram, },
       SystemSnapshot  => SystemSnapshot  { kind: "system_snapshot",  transport: Datagram, },
       Fault           => Fault           { kind: "fault",            transport: Stream,   },
       //                ^^^^^                                        ^^^^^^^^^^^^^^^^^^^^
       //                payload type                                 reliability tier
   }
   ```
   The macro emits `impl WtPayload for Fault`, the `WtKind::Fault`
   discriminator, and a runtime metadata table.
3. Add a `broadcast::Sender<Fault>` field to `AppState` and a producer
   task somewhere (driver, telemetry loop, ...).
4. Add one `recv()` arm to `wt_router::run_session`. This is the only
   per-stream code in `wt_router.rs`; everything else (encoding,
   sequence numbering, transport dispatch, filtering) is generic.
5. (Optional, frontend) register a reducer in `wtReducers.ts`:
   ```ts
   const faultReducer: WtReducer<Fault, Fault[]> = {
     kind: "fault",
     initBucket: () => [],
     merge(bucket, env) { bucket.push(env.data); return true; },
     flush(bucket, queryClient) {
       queryClient.setQueryData<Fault[]>(["faults"], (prev) => [...(prev ?? []), ...bucket]);
     },
   };
   ```

What you do NOT need to touch: `useWebTransport.ts`, the bridge
component, `wt.rs`, the codec test (unless the new payload's CBOR
size is large enough to test specifically).

### Subscription protocol

A client may open a bidi stream right after session establishment and
send one CBOR `WtSubscribe`:

```cbor
{
  "kinds":   ["motor_feedback"],         # empty == "all kinds"
  "filters": { "motor_roles": ["arm_a"] } # per-kind narrowing
}
```

The server applies the new filter immediately. A session that **never**
sends a `WtSubscribe` gets every registered stream — this keeps dumb
clients (curl, future Python tooling) trivially functional and lets the
SPA evolve its filter without coordinating server-side rollouts.

### Observability

- `WT_STREAMS` is a public `&[WtStreamMeta]` slice — exposed for
  future runtime introspection by an `/api/wt/streams` endpoint or the
  dashboard's debug pane.
- The bridge logs a `console.warn` per detected sequence gap; consumers
  can override via the `onGap` prop for richer telemetry.

## Addendum 2026-04-18: solo-operator UX simplification

The original D6 took a multi-operator posture: an explicit `LockBadge`
component with **Take control** / **Take over** / **Release** buttons, a
`GET/POST/DELETE /api/lock` endpoint, and a typed-confirm dialog
(`ConfirmDialog` requiring you to type e.g. `limit shoulder_actuator_a`)
on every destructive action. This was overbuilt for the actual deployment.

Rudy is operated by exactly one human. The realistic failure modes that
remain are:

1. Two browser tabs of the operator's own session racing each other on the
   bus (e.g. an old tab on the phone whose dead-man jog timer fires while
   the laptop is driving).
2. A stale tab's mutator landing concurrently with the active tab's.

What was removed:

- `LockBadge` component, the `["lock"]` query, the `lock_changed`
  invalidation wiring in the SPA.
- `GET / POST / DELETE /api/lock` route handlers and `crates/rudydae/src/api/lock.rs`.
- `AppState::has_control / acquire_control / release_control`.
- The typed-phrase requirement on `ConfirmDialog` (no `phrase` prop;
  no input field; just Confirm / Cancel).

What stayed:

- The `control_lock: RwLock<Option<ControlLockHolder>>` field on `AppState`.
- The 423-Locked guard at the top of every mutating handler — but it now
  calls `AppState::ensure_control(session)` which **auto-claims the lock
  when free**. So a fresh tab "just works" on first click and only a
  *competing* concurrent session gets refused, which is exactly the
  failure mode that matters for solo operation.
- `LockChanged` SafetyEvent broadcast (still emitted on auto-acquire so
  the audit trail captures it; useful if a future second tab needs to
  diagnose "who took the lock from me").
- Audit entries: auto-acquires log as `control_lock_auto_acquire`, refused
  mutators audit-log per existing handler logic with `reason: "lock_held"`.

Rationale: the safety story for Rudy is the firmware envelope (travel
limits, `canTimeout`, enable-gating, dead-man jog watchdog), not modal
ceremony. The lock now exists purely as a tab-race interlock, not as a
permission system, and the dialogs exist to let the operator say "yes, I
meant to" without the cognitive overhead of typing a phrase that nobody
else is around to be tricked by. If a second human ever joins this
project, revisit — the multi-operator UX is git-recoverable.
