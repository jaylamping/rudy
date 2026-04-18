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
  ];

  for (const [name, call, expectedUrl, expectedMethod] of cases) {
    it(name, async () => {
      await call();
      expect(captured?.url).toBe(expectedUrl);
      expect(captured?.method).toBe(expectedMethod);
    });
  }
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
