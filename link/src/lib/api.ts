// Thin fetch wrapper. Server-state caching lives in TanStack Query (see
// `./query.ts`). Requests are same-origin (Vite dev proxy or rudydae serving
// the built SPA). No auth: the console is tailnet/localhost-only.
//
// Mutating requests carry an `X-Rudy-Session` header. The daemon uses it
// for the implicit single-operator lock (first mutator from a fresh session
// wins; a second concurrent session gets 423 Locked) and to attribute audit
// log entries. The session id is minted once per browser tab and stashed in
// `sessionStorage`; see `./session.ts`.

import { sessionId } from "./session";

export class ApiError extends Error {
  status: number;
  body: unknown;
  constructor(message: string, status: number, body: unknown) {
    super(message);
    this.status = status;
    this.body = body;
  }
}

export async function apiFetch<T>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const headers = new Headers(init.headers);
  if (!headers.has("Content-Type") && init.body) {
    headers.set("Content-Type", "application/json");
  }
  const method = (init.method ?? "GET").toUpperCase();
  if (method !== "GET" && method !== "HEAD" && !headers.has("X-Rudy-Session")) {
    headers.set("X-Rudy-Session", sessionId());
  }

  const res = await fetch(path, { ...init, headers });

  const text = await res.text();
  const body: unknown = text ? safeJson(text) : null;

  if (!res.ok) {
    const msg =
      (body && typeof body === "object" && "error" in body && typeof body.error === "string")
        ? body.error
        : res.statusText;
    throw new ApiError(msg, res.status, body);
  }

  return body as T;
}

function safeJson(s: string): unknown {
  try {
    return JSON.parse(s);
  } catch {
    return s;
  }
}

// Response shape for /api/motors/:role/rename and /api/motors/:role/assign.
// `auto_stopped` and `auto_reenabled` are omitted by the daemon when false
// (skip_serializing_if), so they're optional on the wire.
export interface RenameResp {
  ok: boolean;
  new_role: string;
  auto_stopped?: boolean;
  auto_reenabled?: boolean;
  auto_reenable_error?: string;
}

export const api = {
  config: () => apiFetch<import("@/lib/types/ServerConfig").ServerConfig>("/api/config"),
  system: () =>
    apiFetch<import("@/lib/types/SystemSnapshot").SystemSnapshot>("/api/system"),
  listMotors: () =>
    apiFetch<import("@/lib/types/MotorSummary").MotorSummary[]>("/api/motors"),
  getMotor: (role: string) =>
    apiFetch<import("@/lib/types/MotorSummary").MotorSummary>(`/api/motors/${encodeURIComponent(role)}`),
  getParams: (role: string) =>
    apiFetch<import("@/lib/types/ParamSnapshot").ParamSnapshot>(
      `/api/motors/${encodeURIComponent(role)}/params`,
    ),
  writeParam: (
    role: string,
    name: string,
    body: import("@/lib/types/ParamWrite").ParamWrite,
  ) =>
    apiFetch<{ ok: boolean; saved: boolean; role: string; name: string; value: unknown }>(
      `/api/motors/${encodeURIComponent(role)}/params/${encodeURIComponent(name)}`,
      { method: "PUT", body: JSON.stringify(body) },
    ),
  enable: (role: string) =>
    apiFetch<{ ok: boolean }>(`/api/motors/${encodeURIComponent(role)}/enable`, { method: "POST" }),
  stop: (role: string) =>
    apiFetch<{ ok: boolean }>(`/api/motors/${encodeURIComponent(role)}/stop`, { method: "POST" }),
  saveToFlash: (role: string) =>
    apiFetch<{ ok: boolean }>(`/api/motors/${encodeURIComponent(role)}/save`, { method: "POST" }),
  setZero: (role: string) =>
    apiFetch<{ ok: boolean }>(`/api/motors/${encodeURIComponent(role)}/set_zero`, { method: "POST" }),
  // Travel limits (added by the actuator-detail page work).
  getTravelLimits: (role: string) =>
    apiFetch<import("@/lib/types/TravelLimits").TravelLimits>(
      `/api/motors/${encodeURIComponent(role)}/travel_limits`,
    ),
  setTravelLimits: (
    role: string,
    body: { min_rad: number; max_rad: number },
  ) =>
    apiFetch<import("@/lib/types/TravelLimits").TravelLimits>(
      `/api/motors/${encodeURIComponent(role)}/travel_limits`,
      { method: "PUT", body: JSON.stringify(body) },
    ),
  // Hold-to-jog. The TTL is also a server-side watchdog: if no follow-up
  // jog frame arrives within ttl_ms the daemon issues `cmd_stop`.
  jog: (role: string, body: { vel_rad_s: number; ttl_ms: number }) =>
    apiFetch<{ ok: boolean }>(
      `/api/motors/${encodeURIComponent(role)}/jog`,
      { method: "POST", body: JSON.stringify(body) },
    ),
  // Slow-ramp homer. Validates current position is in band, then rolls
  // setpoints toward `target_rad` (default 0.0) at low torque/speed under
  // a tracking-error abort. On success transitions BootState -> Homed and
  // restores per-motor full torque/speed limits.
  homeMotor: (role: string, target_rad?: number) =>
    apiFetch<{ ok: boolean; final_pos_rad: number; ticks: number }>(
      `/api/motors/${encodeURIComponent(role)}/home`,
      {
        method: "POST",
        body: JSON.stringify(target_rad === undefined ? {} : { target_rad }),
      },
    ),
  // Run the multi-limb home orchestrator: sequential within each limb
  // (proximal-to-distal), parallel across limbs.
  homeAll: () =>
    apiFetch<{
      ok: boolean;
      results: Record<
        string,
        {
          status: string;
          homed: string[];
          failed_at: string | null;
          failure_reason: string | null;
        }
      >;
    }>(`/api/home_all`, { method: "POST" }),
  // Atomic rename: changes the inventory primary key, migrates in-memory
  // maps, audit-logs, broadcasts MotorRenamed safety event. If the motor
  // is currently enabled the daemon transparently stops it on the bus
  // before the rename and re-enables it under the new role afterward —
  // `auto_stopped` / `auto_reenabled` surface that round-trip so the SPA
  // can show a small "torque was briefly dropped" notice.
  renameMotor: (role: string, new_role: string) =>
    apiFetch<RenameResp>(
      `/api/motors/${encodeURIComponent(role)}/rename`,
      { method: "POST", body: JSON.stringify({ new_role }) },
    ),
  // Convenience: set limb + joint_kind on an unassigned motor and let
  // the daemon derive the canonical role. Same auto-stop/auto-reenable
  // behavior as `renameMotor` for already-assigned motors.
  assignMotor: (
    role: string,
    body: { limb: string; joint_kind: import("@/lib/types/JointKind").JointKind },
  ) =>
    apiFetch<RenameResp>(
      `/api/motors/${encodeURIComponent(role)}/assign`,
      { method: "POST", body: JSON.stringify(body) },
    ),
  // Bench routines. Returns a run_id that filters the test_progress stream.
  runTest: (
    role: string,
    name: import("@/lib/types/TestName").TestName,
    body: { save?: boolean; target_vel?: number; duration?: number },
  ) =>
    apiFetch<{ run_id: string }>(
      `/api/motors/${encodeURIComponent(role)}/tests/${encodeURIComponent(name)}`,
      { method: "POST", body: JSON.stringify(body) },
    ),
  // Inventory passthrough (raw `extra` map plus typed scalars).
  getInventory: (role: string) =>
    apiFetch<Record<string, unknown>>(
      `/api/motors/${encodeURIComponent(role)}/inventory`,
    ),
  setVerified: (role: string, body: { verified: boolean; note?: string }) =>
    apiFetch<{ ok: boolean; verified: boolean }>(
      `/api/motors/${encodeURIComponent(role)}/verified`,
      { method: "PUT", body: JSON.stringify(body) },
    ),
  // Global e-stop: fans cmd_stop to every present motor and emits a
  // safety_event WT frame.
  estop: () =>
    apiFetch<{ ok: boolean; stopped: number }>(`/api/estop`, { method: "POST" }),
  reminders: {
    list: () =>
      apiFetch<import("@/lib/types/Reminder").Reminder[]>("/api/reminders"),
    create: (body: import("@/lib/types/ReminderInput").ReminderInput) =>
      apiFetch<import("@/lib/types/Reminder").Reminder>("/api/reminders", {
        method: "POST",
        body: JSON.stringify(body),
      }),
    update: (
      id: string,
      body: import("@/lib/types/ReminderInput").ReminderInput,
    ) =>
      apiFetch<import("@/lib/types/Reminder").Reminder>(
        `/api/reminders/${encodeURIComponent(id)}`,
        { method: "PUT", body: JSON.stringify(body) },
      ),
    delete: (id: string) =>
      apiFetch<null>(`/api/reminders/${encodeURIComponent(id)}`, {
        method: "DELETE",
      }),
  },
};
