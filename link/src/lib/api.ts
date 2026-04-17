// Thin fetch wrapper that injects the bearer token on every request. The
// actual server-state caching lives in TanStack Query (see `./query.ts`).
//
// `VITE_RUDYD_URL` is honored for "offsite laptop" workflows where the SPA is
// served from something other than rudyd itself. The default is same-origin,
// which is how the production Pi deploy works (rudyd embeds the built SPA).

import { clearToken, getToken } from "./auth";

const BASE_URL: string = (import.meta.env.VITE_RUDYD_URL as string | undefined) ?? "";

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
  const token = getToken();
  const headers = new Headers(init.headers);
  if (!headers.has("Content-Type") && init.body) {
    headers.set("Content-Type", "application/json");
  }
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }

  const url = `${BASE_URL}${path}`;
  const res = await fetch(url, { ...init, headers });

  if (res.status === 401) {
    // Token is stale or wrong - bounce to login.
    clearToken();
    window.location.assign("/login");
    throw new ApiError("unauthorized", 401, null);
  }

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
  config: () => apiFetch<import("@/api/generated/ServerConfig").ServerConfig>("/api/config"),
  listMotors: () =>
    apiFetch<import("@/api/generated/MotorSummary").MotorSummary[]>("/api/motors"),
  getMotor: (role: string) =>
    apiFetch<import("@/api/generated/MotorSummary").MotorSummary>(`/api/motors/${encodeURIComponent(role)}`),
  getParams: (role: string) =>
    apiFetch<import("@/api/generated/ParamSnapshot").ParamSnapshot>(
      `/api/motors/${encodeURIComponent(role)}/params`,
    ),
  writeParam: (
    role: string,
    name: string,
    body: import("@/api/generated/ParamWrite").ParamWrite,
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
};
