//! Route inventory checklist for SPA parity.
//!
//! Sanity check: the URL paths exercised by the other `tests/api/*` contract
//! suites should match what the SPA's `link/src/lib/api.ts` constructs. If
//! someone renames a route in `crates/cortex/src/api/mod.rs`, update the list
//! below; a harder pin lives in `link/scripts/smoke-contract.mjs`.
#[test]
fn endpoint_inventory_documented() {
    // Single source of truth listing every route hit by the SPA today. When
    // adding a new route to `link/src/lib/api.ts`, add it here too AND write a
    // contract test above. The string is deliberately not parsed ΓÇö it's a
    // checklist for code reviewers.
    let _spa_endpoints = [
        "GET    /api/config",
        "GET    /api/system",
        "GET    /api/devices",
        "DELETE /api/devices/:role",
        "GET    /api/hardware/unassigned",
        "POST   /api/hardware/scan",
        "POST   /api/hardware/onboard/robstride",
        "GET    /api/motors",
        "GET    /api/motors/:role",
        "GET    /api/motors/:role/feedback",
        "GET    /api/motors/:role/params",
        "PUT    /api/motors/:role/params/:name",
        "POST   /api/motors/:role/enable",
        "POST   /api/motors/:role/stop",
        "POST   /api/motors/:role/save",
        "POST   /api/motors/:role/set_zero",
        "POST   /api/motors/:role/commission",
        "POST   /api/motors/:role/restore_offset",
        "GET    /api/motors/:role/travel_limits",
        "PUT    /api/motors/:role/travel_limits",
        "PUT    /api/motors/:role/predefined_home",
        "PUT    /api/motors/:role/homing_speed",
        "POST   /api/motors/:role/jog",
        "GET    /api/motors/:role/motion",
        "POST   /api/motors/:role/motion/sweep",
        "POST   /api/motors/:role/motion/wave",
        "POST   /api/motors/:role/motion/jog",
        "POST   /api/motors/:role/motion/stop",
        "POST   /api/motors/:role/home",
        "POST   /api/motors/:role/rename",
        "POST   /api/motors/:role/assign",
        "POST   /api/home_all",
        "POST   /api/motors/:role/tests/:name",
        "GET    /api/motors/:role/inventory",
        "PUT    /api/motors/:role/verified",
        "POST   /api/estop",
        "GET    /api/reminders",
        "POST   /api/reminders",
        "PUT    /api/reminders/:id",
        "DELETE /api/reminders/:id",
        "GET    /api/logs",
        "DELETE /api/logs",
        "GET    /api/logs/level",
        "PUT    /api/logs/level",
        "GET    /api/settings",
        "PUT    /api/settings/:key",
        "POST   /api/settings/reset",
        "POST   /api/settings/reseed",
        "POST   /api/settings/recovery/ack",
        "GET    /api/settings/profiles",
        "POST   /api/settings/profiles",
        "POST   /api/settings/profiles/apply/:name",
    ];
}
