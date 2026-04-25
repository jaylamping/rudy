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
// `ServerConfig` is mostly static (URL adverts, feature flags). The nested
// `deployment` object changes on the Pi as the background poller refreshes
// GitHub `latest.json` + systemd state — 60s stale here lines up with that
// poll. Default `staleTime: 2_000`
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
 * `/api/config`. Mostly static — see `CONFIG_STALE_MS` — except
 * `deployment` (stale release / Pi updater) which the server refreshes
 * on its own cadence. Pass `refetchInterval` (e.g. 30_000) when you want
 * the browser to re-query often while mounted (Connection card on Overview).
 *
 * Use this everywhere instead of `useQuery({ queryKey: ['config'], ... })`
 * so the daemon doesn't see four near-simultaneous refetches every time
 * the dashboard cold-mounts.
 *
 * `enabled`, `refetchInterval`, and `refetchIntervalInBackground` are the
 * call-site overrides; staleTime and the queryFn stay canonical.
 */
export function useConfigQuery(
  opts?: {
    enabled?: boolean;
    refetchInterval?: number | false;
    refetchIntervalInBackground?: boolean;
  },
) {
  const o = opts ?? {};
  return useQuery({
    ...configQueryOptions(),
    ...(o.enabled !== undefined && { enabled: o.enabled }),
    ...(o.refetchInterval !== undefined && { refetchInterval: o.refetchInterval }),
    ...(o.refetchIntervalInBackground !== undefined && {
      refetchIntervalInBackground: o.refetchIntervalInBackground,
    }),
  });
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

// ---------------------------------------------------------------------------
// /api/settings
// ---------------------------------------------------------------------------

const SETTINGS_STALE_MS = 15_000;

export const settingsQueryOptions = () =>
  queryOptions({
    queryKey: queryKeys.settings.all(),
    queryFn: () => api.settings.get(),
    staleTime: SETTINGS_STALE_MS,
  });
