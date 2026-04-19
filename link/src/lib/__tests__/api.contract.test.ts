// Contract tests for `link/src/lib/api.ts`.
//
// We don't boot a real backend here — that's what `scripts/smoke-contract.mjs`
// (and the Rust integration tests under `crates/rudydae/tests/`) do. Instead,
// these tests pin the URL, HTTP method, and request body shape that each
// `api.*` call produces, so a frontend-only refactor can't silently drift
// from the routes the Rust server registers in `crates/rudydae/src/api/mod.rs`.
//
// If a route name changes on the backend, the matching Rust integration test
// will fail. If the frontend stops calling that route by the right name, this
// file fails. Together they sandwich the contract.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, ApiError } from "@/lib/api";

interface Captured {
  url: string;
  method: string;
  body: unknown;
  headers: Record<string, string>;
}

let captured: Captured | null = null;
let nextResponse: { status?: number; body?: unknown } = {};

function installFetchSpy() {
  globalThis.fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const url = typeof input === "string" ? input : input.toString();
    const method = (init?.method ?? "GET").toUpperCase();
    const headers: Record<string, string> = {};
    new Headers(init?.headers).forEach((v, k) => {
      headers[k] = v;
    });
    let body: unknown = null;
    if (typeof init?.body === "string") {
      try {
        body = JSON.parse(init.body);
      } catch {
        body = init.body;
      }
    }
    captured = { url, method, body, headers };
    const status = nextResponse.status ?? 200;
    // 204 No Content disallows a body per the Fetch spec; constructing
    // `new Response(body, {status: 204})` throws. Mirror the daemon's
    // behavior by sending null on 204 so callers exercising the
    // `api.motion.current` 204 branch see what they'd see in prod.
    if (status === 204) {
      return new Response(null, { status });
    }
    const responseBody =
      nextResponse.body === undefined ? { ok: true } : nextResponse.body;
    return new Response(JSON.stringify(responseBody), {
      status,
      headers: { "content-type": "application/json" },
    });
  }) as unknown as typeof fetch;
}

beforeEach(() => {
  captured = null;
  nextResponse = {};
  installFetchSpy();
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("REST contract — URL + method per call", () => {
  // Each row is [human-readable name, () => api.someCall(), expected URL, expected method].
  const cases: Array<[string, () => Promise<unknown>, string, string]> = [
    ["GET /api/config", () => api.config(), "/api/config", "GET"],
    ["GET /api/motors", () => api.listMotors(), "/api/motors", "GET"],
    [
      "GET /api/motors/:role",
      () => api.getMotor("shoulder_actuator_a"),
      "/api/motors/shoulder_actuator_a",
      "GET",
    ],
    [
      "GET /api/motors/:role/params",
      () => api.getParams("shoulder_actuator_a"),
      "/api/motors/shoulder_actuator_a/params",
      "GET",
    ],
    [
      "POST /api/motors/:role/enable",
      () => api.enable("shoulder_actuator_a"),
      "/api/motors/shoulder_actuator_a/enable",
      "POST",
    ],
    [
      "POST /api/motors/:role/stop",
      () => api.stop("shoulder_actuator_a"),
      "/api/motors/shoulder_actuator_a/stop",
      "POST",
    ],
    [
      "POST /api/motors/:role/save",
      () => api.saveToFlash("shoulder_actuator_a"),
      "/api/motors/shoulder_actuator_a/save",
      "POST",
    ],
    [
      "POST /api/motors/:role/set_zero",
      () => api.setZero("shoulder_actuator_a"),
      "/api/motors/shoulder_actuator_a/set_zero",
      "POST",
    ],
    [
      "POST /api/motors/:role/commission",
      () => api.commissionMotor("shoulder_actuator_a"),
      "/api/motors/shoulder_actuator_a/commission",
      "POST",
    ],
    [
      "POST /api/motors/:role/restore_offset",
      () => api.restoreOffset("shoulder_actuator_a"),
      "/api/motors/shoulder_actuator_a/restore_offset",
      "POST",
    ],
    [
      "PUT /api/motors/:role/predefined_home",
      () =>
        api.setPredefinedHome("shoulder_actuator_a", {
          predefined_home_rad: 0,
        }),
      "/api/motors/shoulder_actuator_a/predefined_home",
      "PUT",
    ],
    [
      "POST /api/motors/:role/motion/sweep",
      () =>
        api.motion.sweep("shoulder_actuator_a", {
          speed_rad_s: 0.1,
        }),
      "/api/motors/shoulder_actuator_a/motion/sweep",
      "POST",
    ],
    [
      "POST /api/motors/:role/motion/wave",
      () =>
        api.motion.wave("shoulder_actuator_a", {
          center_rad: 0,
          amplitude_rad: 0.2,
          speed_rad_s: 0.1,
        }),
      "/api/motors/shoulder_actuator_a/motion/wave",
      "POST",
    ],
    [
      "POST /api/motors/:role/motion/jog",
      () => api.motion.jog("shoulder_actuator_a", { vel_rad_s: 0.25 }),
      "/api/motors/shoulder_actuator_a/motion/jog",
      "POST",
    ],
    [
      "POST /api/motors/:role/motion/stop",
      () => api.motion.stop("shoulder_actuator_a"),
      "/api/motors/shoulder_actuator_a/motion/stop",
      "POST",
    ],
  ];

  for (const [name, call, expectedUrl, expectedMethod] of cases) {
    it(name, async () => {
      await call();
      expect(captured?.url).toBe(expectedUrl);
      expect(captured?.method).toBe(expectedMethod);
    });
  }
});

describe("POST /api/motors/:role/set_zero", () => {
  // The daemon now requires `confirm_advanced: true` in the JSON body
  // so a misclick or copy-pasted curl can't silently shift a
  // commissioned motor's frame (see Phase A.2 of the commissioned-zero
  // plan). The SPA's only call site is the explicit "Set zero (RAM
  // only)" diagnostic disclosure under typed-confirm — it's safe for
  // `api.setZero` to pass the flag automatically. If a future refactor
  // strips the flag, this contract test fails loudly and the daemon
  // would (correctly) start refusing the request with 400
  // `requires_confirmation`.
  it("always sends confirm_advanced:true in a JSON body", async () => {
    await api.setZero("shoulder_actuator_a");
    expect(captured?.url).toBe("/api/motors/shoulder_actuator_a/set_zero");
    expect(captured?.method).toBe("POST");
    expect(captured?.headers["content-type"]).toBe("application/json");
    expect(captured?.body).toEqual({ confirm_advanced: true });
  });
});

describe("GET /api/motors/:role/motion", () => {
  it("returns null when the server replies 204", async () => {
    nextResponse = { status: 204, body: "" };
    // The fetch spy returns "" as the body; the api.motion.current path
    // checks status === 204 first and returns null.
    const result = await api.motion.current("shoulder_actuator_a");
    expect(captured?.url).toBe("/api/motors/shoulder_actuator_a/motion");
    expect(captured?.method).toBe("GET");
    expect(result).toBeNull();
  });

  it("parses the JSON snapshot when the server replies 200", async () => {
    nextResponse = {
      status: 200,
      body: {
        run_id: "abc",
        role: "shoulder_actuator_a",
        kind: "sweep",
        started_at_ms: 1000,
        intent: { kind: "sweep", speed_rad_s: 0.1, turnaround_rad: 0.05 },
      },
    };
    const result = await api.motion.current("shoulder_actuator_a");
    expect(result?.run_id).toBe("abc");
    expect(result?.kind).toBe("sweep");
  });
});

describe("PUT /api/motors/:role/params/:name", () => {
  it("uses PUT, JSON content-type, and serialises the ParamWrite body", async () => {
    await api.writeParam("shoulder_actuator_a", "limit_torque", {
      value: 12.5,
      save_after: false,
    });
    expect(captured?.url).toBe(
      "/api/motors/shoulder_actuator_a/params/limit_torque",
    );
    expect(captured?.method).toBe("PUT");
    expect(captured?.headers["content-type"]).toBe("application/json");
    expect(captured?.body).toEqual({ value: 12.5, save_after: false });
  });

  it("URL-encodes role and name segments", async () => {
    await api.writeParam("with space", "slash/in/name", { value: 1, save_after: false });
    expect(captured?.url).toBe("/api/motors/with%20space/params/slash%2Fin%2Fname");
  });
});

describe("Error envelope handling", () => {
  it("throws ApiError with status + parsed body when server returns an ApiError", async () => {
    nextResponse = {
      status: 400,
      body: { error: "out_of_range", detail: "9999 not in [0, 60]" },
    };
    await expect(
      api.writeParam("x", "limit_torque", { value: 9999, save_after: false }),
    ).rejects.toBeInstanceOf(ApiError);

    nextResponse = {
      status: 404,
      body: { error: "unknown_motor", detail: "no motor with role=ghost" },
    };
    try {
      await api.getMotor("ghost");
      throw new Error("expected to throw");
    } catch (e) {
      expect(e).toBeInstanceOf(ApiError);
      const err = e as ApiError;
      expect(err.status).toBe(404);
      expect(err.message).toBe("unknown_motor");
      expect(err.body).toEqual({
        error: "unknown_motor",
        detail: "no motor with role=ghost",
      });
    }
  });

  it("falls back to statusText when the body is not an ApiError", async () => {
    nextResponse = { status: 503, body: "service unavailable" };
    try {
      await api.config();
      throw new Error("expected to throw");
    } catch (e) {
      expect(e).toBeInstanceOf(ApiError);
      const err = e as ApiError;
      expect(err.status).toBe(503);
      // safeJson() returns the raw string when the body isn't JSON.
      expect(err.body).toBe("service unavailable");
    }
  });
});
