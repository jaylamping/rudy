import type { SliceCreator } from "../types";

/**
 * Example "auth" slice — kept intentionally minimal so it can be deleted /
 * replaced. The pattern to copy:
 *
 *   1. Define <Name>State (data) and <Name>Actions (functions) separately.
 *   2. Combine them into <Name>Slice.
 *   3. Export an `initial<Name>State` constant — handy for `reset()` and tests.
 *   4. Export `create<Name>Slice: SliceCreator<...>` that returns
 *      `{ ...initialState, ...actions }`.
 */

export type User = {
  id: string;
  email: string;
  displayName: string;
};

type AuthState = {
  user: User | null;
  token: string | null;
  // Transient: not persisted (see partialize in `index.ts`).
  status: "idle" | "loading" | "authenticated" | "error";
  error: string | null;
};

type AuthActions = {
  signIn: (user: User, token: string) => void;
  signOut: () => void;
  setStatus: (status: AuthState["status"], error?: string | null) => void;
  reset: () => void;
};

export type AuthSlice = AuthState & AuthActions;

const initialAuthState: AuthState = {
  user: null,
  token: null,
  status: "idle",
  error: null,
};

export const createAuthSlice: SliceCreator<AuthSlice> = (set) => ({
  ...initialAuthState,

  signIn: (user, token) =>
    set(
      { user, token, status: "authenticated", error: null },
      false, // `replace`: false → merge, don't clobber other slices
    ),

  signOut: () => set({ ...initialAuthState }, false),

  setStatus: (status, error = null) => set({ status, error }, false),

  reset: () => set({ ...initialAuthState }, false),
});
