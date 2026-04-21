import { create } from "zustand";
import { persist, createJSONStorage } from "zustand/middleware";
import { useShallow } from "zustand/react/shallow";

import { createSelectors } from "./createSelectors";
import { createAuthSlice } from "./slices/authSlice";
import { createUiSlice } from "./slices/uiSlice";
import type { RootState } from "./types";

/**
 * Bump this when the persisted shape changes in a way old clients can't read.
 * Pair it with a `migrate` function below.
 */
const PERSIST_VERSION = 1;

/**
 * Single root store, composed from independent slices. We deliberately keep
 * one store rather than many — actions in one slice can read/write another
 * via `get()`, and consumers only need one provider-less import.
 *
 * If a slice's data ever needs its own lifecycle (separate persistence key,
 * eager hydration, isolated reset, etc.), promote it to its own `create()` —
 * but start here.
 */
const useStoreBase = create<RootState>()(
  persist(
    (...a) => ({
      ...createAuthSlice(...a),
      ...createUiSlice(...a),
    }),
    {
      name: "cortex-console", // localStorage key
      version: PERSIST_VERSION,
      storage: createJSONStorage(() => localStorage),

      // Whitelist what hits disk. Default is "everything", which leaks
      // transient UI flags and (worse) ephemeral auth status into storage.
      // Pick fields explicitly so adding new state is opt-in to persist.
      partialize: (state) => ({
        token: state.token,
        user: state.user,
        theme: state.theme,
        sidebarOpen: state.sidebarOpen,
      }),

      // Skip hydration during SSR / tests where `window` is absent. Vite is
      // CSR-only today but this makes the store safe to import from Node.
      skipHydration: typeof window === "undefined",

      // Example migration scaffold — delete the body until you actually need
      // to bump `PERSIST_VERSION`.
      migrate: (persistedState, version) => {
        if (version < 1) {
          // shape changes go here
        }
        return persistedState as RootState;
      },
    },
  ),
);

/**
 * Public store. Prefer the auto-generated `useStore.use.foo()` selectors for
 * primitive reads — they subscribe to a single field and never re-render on
 * unrelated changes.
 *
 * For derived objects/arrays or multi-field picks, use the explicit hooks
 * exported below (see `useAuth` / `useUi`) which wrap `useShallow` for you.
 */
export const useStore = createSelectors(useStoreBase);

/**
 * Non-reactive access. Use sparingly — inside event handlers, effects, or
 * outside React entirely. Reading via `useStore.getState()` does NOT
 * subscribe to changes.
 */
export const getState = useStoreBase.getState;
export const setState = useStoreBase.setState;
export const subscribe = useStoreBase.subscribe;

// ---------------------------------------------------------------------------
// Curated multi-field selector hooks
// ---------------------------------------------------------------------------
// Use these when a component needs more than one field at once. `useShallow`
// makes the equality check shallow over the returned object, so the component
// only re-renders when one of the picked fields actually changes — not every
// time the store updates.

export const useAuth = () =>
  useStore(
    useShallow((s) => ({
      user: s.user,
      token: s.token,
      status: s.status,
      isAuthenticated: s.status === "authenticated",
    })),
  );

export const useAuthActions = () =>
  useStore(
    useShallow((s) => ({
      signIn: s.signIn,
      signOut: s.signOut,
      setStatus: s.setStatus,
    })),
  );

export const useUi = () =>
  useStore(
    useShallow((s) => ({
      theme: s.theme,
      sidebarOpen: s.sidebarOpen,
      commandPaletteOpen: s.commandPaletteOpen,
    })),
  );

export const useUiActions = () =>
  useStore(
    useShallow((s) => ({
      setTheme: s.setTheme,
      toggleSidebar: s.toggleSidebar,
      setSidebarOpen: s.setSidebarOpen,
      openCommandPalette: s.openCommandPalette,
      closeCommandPalette: s.closeCommandPalette,
    })),
  );

// Re-export types for ergonomic imports from `@/store`.
export type { RootState } from "./types";
export type { AuthSlice, User } from "./slices/authSlice";
export type { UiSlice, Theme } from "./slices/uiSlice";
