// Single source of truth for every TanStack Query key used by the SPA.
//
// Why a factory:
//   - Renaming a resource is now one edit, not a grep across 20+ files.
//   - The WebTransport reducers (`wtReducers.ts`) write to the same keys
//     that components subscribe to. Centralizing keeps WT writers and
//     hook readers from drifting apart.
//   - `as const` everywhere preserves literal types so
//     `queryClient.invalidateQueries({ queryKey: queryKeys.motors.all() })`
//     stays typed.
//
// Convention: scoped namespaces with `all()` for the collection key and
// `byX(id)` for keyed entries. Sub-namespaces (e.g. `queryKeys.logs.live()`)
// keep related keys grouped without forcing every consumer to know the
// layout of the underlying URL.
//
// Do NOT re-shape cache entries here. Whatever the daemon returns from
// `/api/foo` is what `queryKeys.foo.*` keys should hold — that lets WT
// reducers and REST queries share the same cache slot. If a component
// wants a derived view, derive in render or via `select`, not by writing
// a reshaped value into the cache.

export const queryKeys = {
  config: () => ["config"] as const,
  system: () => ["system"] as const,

  motors: {
    all: () => ["motors"] as const,
  },

  params: {
    byRole: (role: string) => ["params", role] as const,
  },

  inventory: {
    byRole: (role: string) => ["inventory", role] as const,
  },

  travelLimits: {
    byRole: (role: string) => ["travel_limits", role] as const,
  },

  devices: {
    all: () => ["devices"] as const,
    unassigned: () => ["hardware", "unassigned"] as const,
  },

  reminders: {
    all: () => ["reminders"] as const,
  },

  settings: {
    all: () => ["settings"] as const,
  },

  logs: {
    /**
     * The "live tail" slot. The WebTransport `log_event` reducer writes here
     * and the Logs page subscribes via
     * `useQuery({ queryKey: queryKeys.logs.live() })`. The cache stores
     * `LogEntry[]` newest-first, capped at `LIVE_LOG_CAP`.
     */
    live: () => ["logs", "live"] as const,
    level: () => ["logs", "level"] as const,
  },

  assets: {
    manifest: () => ["asset-manifest"] as const,
    cacheStats: () => ["asset-cache-stats"] as const,
  },
} as const;
