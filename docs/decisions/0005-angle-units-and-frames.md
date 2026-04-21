# ADR 0005: Angular units and frames in cortex

## Status

Accepted

## Context

Motion and safety code must agree on whether an `f32` angle is in radians vs degrees, and whether it is a **principal-angle** value in (−π, π] (used for soft travel limits and inventory home targets) or an **unwrapped** value on the encoder’s current continuity branch (used for home-ramp setpoint integration).

## Decisions

1. **Units** — All internal angles are **radians**; API surfaces use `*_rad` / `*_rad_s` suffixes on types and fields. Degrees appear only in the operator SPA, converted at the REST boundary.

2. **Principal-angle frame** — Soft limits (`travel_limits.min_rad` / `max_rad`), boot-state band classification, and `LOC_REF` written defensively as a position-hold setpoint use the principal-angle convention (−π, π] after `wrap_to_pi`. Path-aware checks use [`enforce_position_with_path`](../../crates/cortex/src/can/travel.rs), which wraps both endpoints before testing the band.

3. **Unwrapped branch** — The home-ramp integrates toward a target on the **same multi-turn branch** as the current telemetry sample: `unwrapped_target = from + shortest_signed_delta(from, target_principal)` (see [`home_ramp::run_with_overrides`](../../crates/cortex/src/can/home_ramp.rs)). The virtual setpoint advances in this unwrapped scalar; band checks still reduce positions to principal angles internally.

4. **Type system** — Frame-aware newtypes (`PrincipalAngle`, `UnwrappedAngle`, `Radians` for signed deltas) live in [`crates/cortex/src/can/angle.rs`](../../crates/cortex/src/can/angle.rs) and are used on motion / homing / boot paths; inventory YAML and HTTP DTOs remain bare `f32` radians at the edge, converted at call sites.

## Consequences

- Call sites must not mix principal and unwrapped values without an explicit conversion.
- Documentation for RS03 `MECH_POS` vs `LOC_REF` wire semantics remains in [0002-rs03-protocol-spec.md](./0002-rs03-protocol-spec.md).
