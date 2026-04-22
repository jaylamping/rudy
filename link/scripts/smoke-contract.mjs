#!/usr/bin/env node
// End-to-end smoke test of the link <-> cortex REST contract.
//
// Hits every endpoint `link/src/lib/api.ts` calls against a running cortex
// (mock CAN is fine), validates the JSON shapes, and exits non-zero on the
// first mismatch. Designed to be both a `npm run smoke` for humans and a CI
// gate ("does the SPA's view of the API still match the binary?").
//
// Usage:
//   node scripts/smoke-contract.mjs                # hits http://127.0.0.1:8443
//   CORTEX_URL=http://127.0.0.1:9999 node scripts/smoke-contract.mjs
//   node scripts/smoke-contract.mjs --spawn        # spawns `cargo run -p cortex`
//                                                  # against ../config/cortex.toml
//
// Notes:
//   - WebTransport is intentionally NOT exercised here. The contract this
//     script pins is REST + the WT *advert* shape, not the QUIC transport
//     itself.
//   - The script is dependency-free (built-in fetch / child_process / process
//     only) so it can run on any Node 20+ install without adding a test
//     framework.

import { spawn } from "node:child_process";
import { setTimeout as sleep } from "node:timers/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..", "..");

const BASE = process.env.CORTEX_URL ?? "http://127.0.0.1:8443";
const SPAWN = process.argv.includes("--spawn");

let failures = 0;
const results = [];

function record(name, ok, detail = "") {
  results.push({ name, ok, detail });
  if (!ok) failures += 1;
  process.stdout.write(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? `  -  ${detail}` : ""}\n`);
}

function assertHas(obj, keys, label) {
  for (const k of keys) {
    if (obj == null || !Object.prototype.hasOwnProperty.call(obj, k)) {
      throw new Error(`${label}: missing key "${k}"`);
    }
  }
}

async function http(method, p, body) {
  const res = await fetch(`${BASE}${p}`, {
    method,
    headers: body ? { "content-type": "application/json" } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  });
  const text = await res.text();
  let json = null;
  try {
    json = text ? JSON.parse(text) : null;
  } catch {
    /* non-JSON body */
  }
  return { status: res.status, body: json, raw: text };
}

async function waitForServer() {
  // Generous deadline so a cold `cargo build` (~30s on my box, more on CI)
  // still has time to finish before we declare the server unreachable.
  const timeoutMs = SPAWN ? 180_000 : 10_000;
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const r = await http("GET", "/api/config");
      if (r.status === 200) return;
    } catch {
      /* not up yet */
    }
    await sleep(500);
  }
  throw new Error(`cortex at ${BASE} did not respond within ${timeoutMs / 1000}s`);
}

async function maybeSpawnDaemon() {
  if (!SPAWN) return null;
  // Note: this assumes mock CAN. The default config/cortex.toml has mock=true.
  // cortex resolves relative paths in cortex.toml (actuator_spec, inventory,
  // audit_log) against its CWD, so we run from the repo root.
  process.stdout.write(`# spawning: cargo run --manifest-path crates/Cargo.toml -p cortex (cwd=${repoRoot})\n`);
  const child = spawn(
    "cargo",
    [
      "run",
      "-q",
      "--manifest-path",
      "crates/Cargo.toml",
      "-p",
      "cortex",
      "--",
      "config/cortex.toml",
    ],
    {
      cwd: repoRoot,
      stdio: ["ignore", "inherit", "inherit"],
      env: { ...process.env, RUST_LOG: "cortex=warn" },
    },
  );
  child.on("exit", (code) => {
    if (code !== 0 && code !== null) {
      process.stderr.write(`# cortex exited with ${code}\n`);
    }
  });
  return child;
}

async function main() {
  const child = await maybeSpawnDaemon();
  try {
    await waitForServer();

    // 1. /api/config — fields the useWebTransport hook + telemetry page read.
    {
      const r = await http("GET", "/api/config");
      try {
        if (r.status !== 200) throw new Error(`status=${r.status}`);
        assertHas(r.body, ["version", "actuator_models", "webtransport", "features", "paths", "deployment"], "ServerConfig");
        assertHas(r.body.webtransport, ["enabled", "url"], "WebTransportAdvert");
        assertHas(r.body.features, ["mock_can", "require_verified"], "ServerFeatures");
        assertHas(r.body.paths, ["inventory"], "ServerPaths");
        assertHas(r.body.deployment, ["build", "latest", "is_stale", "latest_manifest_ok", "updater"], "DeploymentInfo");
        assertHas(r.body.deployment.build, ["commit_sha", "short_sha", "built_at", "package_version"], "BuildIdentity");
        assertHas(r.body.deployment.latest, ["commit_sha", "short_sha", "built_at", "manifest_error"], "ChannelLatest");
        assertHas(r.body.deployment.updater, ["systemd_probed", "last_check", "last_applied", "timer_active", "service_failed", "healthy"], "UpdaterStatus");
        if (r.body.webtransport.enabled) {
          if (typeof r.body.webtransport.url !== "string") {
            throw new Error("webtransport.enabled=true but url is not a string");
          }
          if (r.body.webtransport.url.includes("HOSTPLACEHOLDER")) {
            throw new Error(
              "webtransport.url still contains HOSTPLACEHOLDER — config_route::get_config regressed",
            );
          }
          if (!r.body.webtransport.url.startsWith("https://")) {
            throw new Error(
              `webtransport.url must be https://; got ${r.body.webtransport.url}`,
            );
          }
        }
        record("GET /api/config", true);
      } catch (e) {
        record("GET /api/config", false, e.message);
      }
    }

    // 2. /api/motors — list. Must be an array of MotorSummary.
    let firstRole = null;
    {
      const r = await http("GET", "/api/motors");
      try {
        if (r.status !== 200) throw new Error(`status=${r.status}`);
        if (!Array.isArray(r.body)) throw new Error("not an array");
        if (r.body.length === 0) throw new Error("inventory is empty");
        for (const m of r.body) {
          assertHas(m, ["role", "can_bus", "can_id", "verified"], "MotorSummary");
        }
        firstRole = r.body[0].role;
        record(`GET /api/motors (${r.body.length} motors)`, true);
      } catch (e) {
        record("GET /api/motors", false, e.message);
      }
    }

    if (!firstRole) {
      process.stdout.write("# no motor in inventory; skipping per-motor tests\n");
    } else {
      // 3. /api/motors/:role
      {
        const r = await http("GET", `/api/motors/${encodeURIComponent(firstRole)}`);
        try {
          if (r.status !== 200) throw new Error(`status=${r.status}`);
          assertHas(
            r.body,
            [
              "role",
              "can_bus",
              "can_id",
              "verified",
              "homing_speed_rad_s",
              "default_homing_speed_rad_s",
            ],
            "MotorSummary",
          );
          record(`GET /api/motors/${firstRole}`, true);
        } catch (e) {
          record(`GET /api/motors/${firstRole}`, false, e.message);
        }
      }

      // 4. /api/motors/:role/feedback (after a tick or two, mock CAN seeds it)
      // Allow up to 2s for the first mock tick to land.
      {
        let ok = false;
        let lastErr = "";
        const fbDeadline = Date.now() + 2000;
        while (Date.now() < fbDeadline && !ok) {
          const r = await http("GET", `/api/motors/${encodeURIComponent(firstRole)}/feedback`);
          if (r.status === 200) {
            try {
              assertHas(
                r.body,
                [
                  "t_ms",
                  "role",
                  "can_id",
                  "mech_pos_rad",
                  "mech_vel_rad_s",
                  "torque_nm",
                  "vbus_v",
                  "temp_c",
                  "fault_sta",
                  "warn_sta",
                ],
                "MotorFeedback",
              );
              if (typeof r.body.t_ms !== "number") {
                throw new Error(`t_ms must be a JSON number (the SPA reads it via JSON.parse); got ${typeof r.body.t_ms}`);
              }
              ok = true;
            } catch (e) {
              lastErr = e.message;
              break;
            }
          } else {
            lastErr = `status=${r.status}`;
            await sleep(100);
          }
        }
        record(`GET /api/motors/${firstRole}/feedback`, ok, ok ? "" : lastErr);
      }

      // 5. /api/motors/:role/params
      {
        const r = await http("GET", `/api/motors/${encodeURIComponent(firstRole)}/params`);
        try {
          if (r.status !== 200) throw new Error(`status=${r.status}`);
          assertHas(r.body, ["role", "values"], "ParamSnapshot");
          if (typeof r.body.values !== "object" || r.body.values === null) {
            throw new Error("values must be an object map");
          }
          for (const [name, p] of Object.entries(r.body.values)) {
            assertHas(p, ["name", "index", "type", "value"], `ParamValue ${name}`);
          }
          record(`GET /api/motors/${firstRole}/params`, true);
        } catch (e) {
          record(`GET /api/motors/${firstRole}/params`, false, e.message);
        }
      }

      // 6. PUT /api/motors/:role/params/:name — out-of-range path, no
      // hardware mutation (the value 9999 is rejected before any CAN write).
      // Skip if the motor exposes no firmware_limits param.
      {
        const snap = await http("GET", `/api/motors/${encodeURIComponent(firstRole)}/params`);
        const target = snap.body && Object.values(snap.body.values ?? {}).find(
          (p) => Array.isArray(p.hardware_range),
        );
        if (target) {
          const [, hi] = target.hardware_range;
          const r = await http(
            "PUT",
            `/api/motors/${encodeURIComponent(firstRole)}/params/${encodeURIComponent(target.name)}`,
            { value: hi + 1_000_000, save_after: false },
          );
          const ok = r.status === 400 && r.body?.error === "out_of_range";
          record(
            `PUT /api/motors/${firstRole}/params/${target.name} (out-of-range -> 400)`,
            ok,
            ok ? "" : `status=${r.status} body=${JSON.stringify(r.body)}`,
          );
        } else {
          process.stdout.write(
            `# no firmware_limits param on ${firstRole}; skipping out-of-range PUT test\n`,
          );
        }
      }

      // 7. Unknown motor -> 404 + ApiError envelope.
      {
        const r = await http("GET", "/api/motors/__definitely_not_a_role__");
        const ok = r.status === 404 && r.body?.error === "unknown_motor";
        record(
          "GET /api/motors/<unknown> (404 + ApiError envelope)",
          ok,
          ok ? "" : `status=${r.status} body=${JSON.stringify(r.body)}`,
        );
      }
    }

    // 8. /api/logs — paginated history. The store always has at least
    // the "loaded inventory" tracing line by now, so `entries` should
    // be non-empty. We also accept an empty list (e.g. immediately
    // after a clear) — the shape check is what we're really pinning.
    {
      const r = await http("GET", "/api/logs?limit=10");
      try {
        if (r.status !== 200) throw new Error(`status=${r.status}`);
        assertHas(r.body, ["entries", "next_before_id"], "LogsListResponse");
        if (!Array.isArray(r.body.entries)) throw new Error("entries is not an array");
        for (const e of r.body.entries) {
          assertHas(
            e,
            ["id", "t_ms", "level", "source", "target", "message", "fields", "span"],
            "LogEntry",
          );
          if (typeof e.id !== "number") {
            throw new Error(`LogEntry.id must be a JSON number; got ${typeof e.id}`);
          }
          if (typeof e.t_ms !== "number") {
            throw new Error(`LogEntry.t_ms must be a JSON number; got ${typeof e.t_ms}`);
          }
          if (!["trace", "debug", "info", "warn", "error"].includes(e.level)) {
            throw new Error(`LogEntry.level out of vocabulary: ${e.level}`);
          }
          if (!["tracing", "audit"].includes(e.source)) {
            throw new Error(`LogEntry.source out of vocabulary: ${e.source}`);
          }
        }
        record("GET /api/logs (shape)", true);
      } catch (e) {
        record("GET /api/logs (shape)", false, e.message);
      }
    }

    // 9. /api/logs/level — runtime EnvFilter snapshot.
    {
      const r = await http("GET", "/api/logs/level");
      try {
        if (r.status !== 200) throw new Error(`status=${r.status}`);
        assertHas(r.body, ["default", "directives", "raw"], "LogFilterState");
        if (!["trace", "debug", "info", "warn", "error"].includes(r.body.default)) {
          throw new Error(`LogFilterState.default out of vocabulary: ${r.body.default}`);
        }
        if (!Array.isArray(r.body.directives)) {
          throw new Error("directives is not an array");
        }
        for (const d of r.body.directives) {
          assertHas(d, ["target", "level"], "LogFilterDirective");
        }
        if (typeof r.body.raw !== "string" || r.body.raw.length === 0) {
          throw new Error("raw must be a non-empty string");
        }
        record("GET /api/logs/level (shape)", true);
      } catch (e) {
        record("GET /api/logs/level (shape)", false, e.message);
      }
    }

    // 10. PUT /api/logs/level — invalid directive must come back 400
    // with our error envelope. We don't exercise the success path here
    // because that would mutate the running daemon's filter and risk
    // slowing later checks; the unit tests in `crates/cortex` cover it.
    {
      const r = await http("PUT", "/api/logs/level", { raw: "this is not a filter directive!!!" });
      const ok =
        r.status === 400 && (r.body?.error === "invalid_filter" || r.body?.error === "empty_filter");
      record(
        "PUT /api/logs/level (invalid -> 400)",
        ok,
        ok ? "" : `status=${r.status} body=${JSON.stringify(r.body)}`,
      );
    }
  } finally {
    if (child) {
      // SIGINT on Windows interacts poorly with cargo's process tree (libuv
      // assertion at exit); SIGTERM is also unsupported. Use SIGKILL for a
      // clean teardown — this is a smoke runner, not a graceful shutdown
      // exercise.
      try {
        child.kill(process.platform === "win32" ? "SIGKILL" : "SIGINT");
      } catch {
        /* already dead */
      }
    }
  }

  process.stdout.write(
    `\n${results.length - failures}/${results.length} contract checks passed\n`,
  );
  process.exit(failures === 0 ? 0 : 1);
}

main().catch((e) => {
  process.stderr.write(`smoke runner crashed: ${e?.stack ?? e}\n`);
  process.exit(2);
});
