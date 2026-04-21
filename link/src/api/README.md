# `@/api` — TanStack Query conventions

This folder centralizes the parts of our TanStack Query usage that
**multiple call sites should agree on**. It does **not** try to wrap every
query — see "What this folder doesn't do" below.

The `QueryClient` itself still lives in `@/lib/query.ts`. The low-level
fetch wrapper and per-endpoint API methods still live in `@/lib/api.ts`.

## Layout

```
src/api/
├── index.ts        ← public re-exports
├── queryKeys.ts    ← `queryKeys.*` factory: single source of truth for
│                     every query key shape used in the SPA
├── queries.ts      ← `*QueryOptions()` builders + `use*Query()` hooks for
│                     resources where call sites should agree on options
└── README.md       ← this file
```

## Conventions

### 1. Query keys go through the `queryKeys` factory

Every cache key in the SPA is built via the `queryKeys` factory in
`queryKeys.ts`:

```ts
import { queryKeys } from "@/api";

useQuery({ queryKey: queryKeys.motors.all(), queryFn: () => api.listMotors() });
useQuery({ queryKey: queryKeys.params.byRole(role), queryFn: () => api.getParams(role) });
queryClient.invalidateQueries({ queryKey: queryKeys.motors.all() });
```

Why one factory instead of inline `["motors"]` literals everywhere:

- Renaming or restructuring a key is one edit, not a grep across 20+ files.
- The WebTransport reducers (`@/lib/hooks/wtReducers.ts`) write to the same
  keys components subscribe to. Routing both through the factory keeps WT
  writers and hook readers from drifting apart.
- `as const` returns preserve literal types so
  `queryClient.invalidateQueries({ queryKey: queryKeys.motors.all() })` stays
  fully typed.

The one rule: **the WT push path and the REST pull path must use the same
factory call**. The reducers in `wtReducers.ts` write to
`queryKeys.motors.all()` / `queryKeys.system()` / `queryKeys.logs.live()`;
component `useQuery` keys must call the same factory entry.

### 2. Cache shape mirrors the wire shape

Whatever the daemon returns from `/api/foo` is what the corresponding
cache key holds. The WT reducers depend on this — they `setQueryData`
in the wire shape (`MotorSummary[]`, `SystemSnapshot`, `LogEntry[]`).
If a component wants a derived view, derive in render or use TanStack's
`select` option — never write a reshaped value back into the cache.

### 3. Mutations invalidate; they don't `setQueryData`

Default to `queryClient.invalidateQueries({ queryKey: queryKeys.foo.all() })`
in mutation `onSuccess`. The next render refetches from the daemon, and
the cache stays a *fetch target*, not a write target.

**Documented exceptions** (don't add new ones without comment):

- **WebTransport reducers** (`@/lib/hooks/wtReducers.ts`) write directly
  via `setQueryData` because they ARE the live data source — round-tripping
  through REST after every datagram would defeat the point of the stream.
- **`level-control.tsx`** writes the daemon's canonical response into
  `queryKeys.logs.level()` after a `PUT` because the response IS the new
  state and a refetch would be wasteful.
- **`_app.logs.tsx` clear** writes `[]` into `queryKeys.logs.live()` because
  the REST `DELETE /api/logs` clears the on-disk store — the cache should
  match immediately, not wait for the next REST poll.

### 4. Use `queryOptions()`, not bare objects, for shared queries

When defining a query that's used in more than one place — including
"used by both `useQuery` and `queryClient.fetchQuery`" — wrap it with
`queryOptions()` from TanStack. This gives one definition, two call sites:

```ts
import { configQueryOptions } from "@/api";

// In a component:
const { data } = useQuery(configQueryOptions());

// In a route loader or non-React module:
const cfg = await queryClient.fetchQuery(configQueryOptions());
```

This is what the WT bridge does for `queryKeys.config()`.

### 5. Set `staleTime` per-resource, not globally

The default in `lib/query.ts` (`staleTime: 2_000`) is appropriate for
"refetch frequently because data turns over". For data that's effectively
static (like `/api/config`), define an explicit `staleTime` in the
`queryOptions` builder and have everyone use it via the wrapper hook.

### 6. WT-driven queries don't need a `refetchInterval`

If a query key is the target of a WT reducer (currently
`queryKeys.motors.all()`, `queryKeys.system()`, `queryKeys.logs.live()`),
REST polling is a *safety net* for when WT is disconnected — not a primary
data path. Use `useLiveInterval()` to pick a slow cadence when WT is up
and a fast one when it's down. Hooks that don't care about the safety net
(e.g. `useLimbHealth`) can omit `refetchInterval` entirely.

## What this folder *doesn't* do

We deliberately resisted the urge to add a `useFooQuery()` wrapper for
every endpoint. Here's why each was considered and skipped:

- **`useMotorsQuery`**: every call site has a legitimately different
  polling profile (`live: 30_000, fallback: 1_000` vs `2_000` vs no poll
  at all). A single wrapper would force these into a sprawl of variants
  (`useMotorsQuery`, `useMotorsQueryNoPoll`, `useMotorsQueryFastPoll`...)
  which is worse than the call-site explicitness we have today.
- **`useDevicesQuery`, `useUnassignedHardwareQuery`**: only used in
  `_app.devices.tsx`. No centralization value.
- **`useInventoryQuery(role)`, `useParamsQuery(role)`, `useTravelLimitsQuery(role)`**:
  used in 1-2 places each, all in the actuator detail tabs. Wrapping
  would move code without removing it.
- **Loader convention rollout**: most routes don't need pre-fetching
  (the components handle pending state fine). The one route that uses a
  loader (`_app.actuators.$role`) does so because it needs to 404 on
  unknown roles before render — a real semantic reason, not a convention.

If you're tempted to add a wrapper here, ask: *would multiple call sites
disagree on the options today?* If yes, wrap. If no, leave it inline.

## Adding a new resource

1. Add the key to `queryKeys.ts` (e.g. `foo: { all: () => ["foo"] as const }`).
2. Use it inline at the call site:
   `useQuery({ queryKey: queryKeys.foo.all(), queryFn: () => api.foo() })`.
3. If it's used in multiple places **and they should agree on options**
   (e.g. `staleTime`, `select`), add a `fooQueryOptions()` builder and
   `useFooQuery()` hook to `queries.ts`.
4. If a WebTransport stream pushes to it, add a reducer to
   `wtReducers.ts` that writes via
   `setQueryData(queryKeys.foo.all(), ...)`. The reducer's key must match
   every consumer's `useQuery` key — that's the whole point of the factory.
