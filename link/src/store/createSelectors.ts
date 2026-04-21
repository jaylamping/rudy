import type { StoreApi, UseBoundStore } from "zustand";

/**
 * Auto-generates `store.use.someField()` hooks from a Zustand store, so callers
 * don't have to hand-write a selector for every primitive field.
 *
 * Usage:
 *   const useUiStore = createSelectors(create<UiState>()(...));
 *   const sidebarOpen = useUiStore.use.sidebarOpen();
 *
 * Each generated hook calls `useStore(s => s[key])`, so it subscribes to one
 * field only — no `useShallow` needed for primitive reads. For derived
 * objects/arrays, keep writing explicit selectors with `useShallow`.
 */
type WithSelectors<S> = S extends { getState: () => infer T }
  ? S & { use: { [K in keyof T]: () => T[K] } }
  : never;

export function createSelectors<S extends UseBoundStore<StoreApi<object>>>(
  store: S,
): WithSelectors<S> {
  const withSelectors = store as WithSelectors<S>;
  withSelectors.use = {} as WithSelectors<S>["use"];

  for (const key of Object.keys(store.getState()) as Array<
    keyof ReturnType<S["getState"]>
  >) {
    (withSelectors.use as Record<string, unknown>)[key as string] = () =>
      store((s) => s[key as keyof typeof s]);
  }

  return withSelectors;
}
