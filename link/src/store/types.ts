import type { StateCreator } from "zustand";
import type { AuthSlice } from "./slices/authSlice";
import type { UiSlice } from "./slices/uiSlice";

/**
 * The fully-composed root state. Every slice contributes its keys here, and
 * `set`/`get` inside any slice are typed against this union — that's what lets
 * one slice safely read/update fields owned by another.
 */
export type RootState = AuthSlice & UiSlice;

/**
 * Middleware stack we apply to the root store, listed outer-most first.
 * Keep this in sync with the `create()` call in `index.ts`. If you add
 * `devtools`, `immer`, or `subscribeWithSelector`, append the matching tuple
 * here so slice creators stay correctly typed.
 *
 * Currently: persist (outer)  →  raw store
 */
export type StoreMutators = [["zustand/persist", unknown]];

/**
 * Helper alias for writing slice creators with the right middleware mutators
 * baked in. Use this instead of bare `StateCreator` so `set`/`get` know about
 * the full `RootState` and persist's partializer infers cleanly under strict.
 *
 * Example:
 *   export const createUiSlice: SliceCreator<UiSlice> = (set) => ({ ... });
 */
export type SliceCreator<Slice> = StateCreator<
  RootState,
  StoreMutators,
  [],
  Slice
>;
