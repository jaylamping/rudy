// Centralized query definitions for resources where call sites currently
// disagree on options (staleTime, etc.). Each `queryOptions()` builder is
// the single source of truth for the queryKey + queryFn + canonical options
// of one resource — usable both inside React via `useQuery(...)` and
// outside via `queryClient.fetchQuery(...)` / `ensureQueryData(...)`.
//
// We deliberately do NOT wrap every query. Most call sites legitimately
// need to set their own `refetchInterval` (driven by `useLiveInterval`),
// so a one-size-fits-all `useMotorsQuery` would just push the variation
// into a sprawl of named variants. Wrap when call sites SHOULD agree;
// inline otherwise.

import { queryOptions, useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import { queryKeys } from "./queryKeys";

// ---------------------------------------------------------------------------
// /api/config
// ---------------------------------------------------------------------------
// `ServerConfig` is essentially static for the lifetime of the daemon
// (URL adverts, feature flags, capability bits). Default `staleTime: 2_000`
// from `lib/query.ts` is wrong for it: every component that mounts
// (`ConnectionCard`, `OnboardingWizard`, the viz route, the WT bridge)
// would re-fetch within seconds of each other on a cold cache.
//
// 60s matches what the WebTransport bridge was already requesting; this
// makes that the convention rather than a one-off.
const CONFIG_STALE_MS = 60_000;

export const configQueryOptions = () =>
  queryOptions({
    queryKey: queryKeys.config(),
    queryFn: () => api.config(),
    staleTime: CONFIG_STALE_MS,
  });

/**
 * `/api/config`. Effectively static during a session — see `CONFIG_STALE_MS`.
 *
 * Use this everywhere instead of `useQuery({ queryKey: ['config'], ... })`
 * so the daemon doesn't see four near-simultaneous refetches every time
 * the dashboard cold-mounts.
 *
 * `enabled` is the only option call sites may override — staleTime and
 * the queryFn are intentionally locked so the resource has one canonical
 * cadence across the app.
 */
export function useConfigQuery(opts?: { enabled?: boolean }) {
  return useQuery({ ...configQueryOptions(), enabled: opts?.enabled });
}

// ---------------------------------------------------------------------------
// /api/logs/level
// ---------------------------------------------------------------------------
// Read by `<LevelControl>` in two places (the Logs page header and via the
// shared component). The mutation in `level-control.tsx` writes to the same
// key with `setQueryData` after a successful PUT, which is fine because the
// daemon echoes the canonical state in the response.

export const logsLevelQueryOptions = () =>
  queryOptions({
    queryKey: queryKeys.logs.level(),
    queryFn: () => api.logs.getLevel(),
  });

export function useLogsLevelQuery() {
  return useQuery(logsLevelQueryOptions());
}
