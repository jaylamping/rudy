// Thin fetch wrapper. Server-state caching lives in TanStack Query (see
// `./query.ts`). Requests are same-origin (Vite dev proxy or rudydae serving
// the built SPA). No auth: the console is tailnet/localhost-only.

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
