# `@/store` — Zustand store

A single root store composed from independent slices. Persisted to
`localStorage` under the key `rudy-link`.

## Layout

```
store/
├── index.ts            ← root store, persist config, curated hooks
├── types.ts            ← RootState + SliceCreator helper
├── createSelectors.ts  ← auto-generates `store.use.foo()` per field
└── slices/
    ├── authSlice.ts    ← example slice (data + actions + initial state)
    └── uiSlice.ts      ← example slice
```

## Reading state in a component

Pick the narrowest hook that does the job:

```tsx
import { useStore, useUi, useUiActions } from "@/store";

function ThemeBadge() {
  // Single primitive → auto-generated selector, zero boilerplate.
  const theme = useStore.use.theme();
  return <span>{theme}</span>;
}

function Sidebar() {
  // Multiple fields → curated `useShallow` hook, one re-render path.
  const { sidebarOpen, theme } = useUi();
  const { toggleSidebar } = useUiActions();
  return <aside data-open={sidebarOpen} data-theme={theme} />;
}
```

Avoid this — it re-renders on every store change:

```tsx
// ❌ subscribes to the entire state object
const { sidebarOpen } = useStore((s) => s);
```

## Writing state outside React

```ts
import { getState, setState } from "@/store";

// In an event handler, effect, or non-React module:
if (getState().status === "authenticated") {
  setState({ commandPaletteOpen: true });
}
```

## Adding a new slice

1. Create `slices/fooSlice.ts` mirroring `uiSlice.ts`:
   - `FooState`, `FooActions`, `FooSlice`
   - `initialFooState` constant
   - `createFooSlice: SliceCreator<FooSlice>`
2. Add `FooSlice` to `RootState` in `types.ts`.
3. Spread `createFooSlice(...a)` into the store body in `index.ts`.
4. Decide what (if anything) belongs in `partialize` — default is **don't
   persist** unless there's a reason.
5. Add a `useFoo` / `useFooActions` curated hook for multi-field reads.

## Migrations

Bump `PERSIST_VERSION` in `index.ts` and extend the `migrate` function. Old
clients with stale `localStorage` payloads will be upgraded on next load.

## Notes

- Actions are stable references — safe to put in `useEffect` deps and to pull
  out via `useUiActions()` without churn.
- All slices share one storage key by design. If a slice ever needs its own
  lifecycle, promote it to a separate `create()` call.
- `set(..., false)` is the default merge behavior; we pass it explicitly to
  document intent and to make the boolean visible when refactoring to
  `replace: true`.
