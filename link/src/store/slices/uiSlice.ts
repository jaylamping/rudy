import type { SliceCreator } from "../types";

/**
 * Example "UI" slice — owns ephemeral but user-tweakable presentation state
 * (theme, sidebar, etc.). Mostly persisted (see `partialize` in `index.ts`),
 * except for one-off transient flags like `commandPaletteOpen`.
 */

export type Theme = "light" | "dark" | "system";

type UiState = {
  theme: Theme;
  sidebarOpen: boolean;
  // Transient: don't persist modal/menu state across reloads.
  commandPaletteOpen: boolean;
};

type UiActions = {
  setTheme: (theme: Theme) => void;
  toggleSidebar: () => void;
  setSidebarOpen: (open: boolean) => void;
  openCommandPalette: () => void;
  closeCommandPalette: () => void;
};

export type UiSlice = UiState & UiActions;

const initialUiState: UiState = {
  theme: "system",
  sidebarOpen: true,
  commandPaletteOpen: false,
};

export const createUiSlice: SliceCreator<UiSlice> = (set, get) => ({
  ...initialUiState,

  setTheme: (theme) => set({ theme }, false),

  // Functional updates via `get()` keep the action self-contained — preferred
  // over `set((s) => ...)` when you only need to read one field, since it's
  // more explicit about what you're depending on.
  toggleSidebar: () => set({ sidebarOpen: !get().sidebarOpen }, false),

  setSidebarOpen: (open) => set({ sidebarOpen: open }, false),

  openCommandPalette: () => set({ commandPaletteOpen: true }, false),
  closeCommandPalette: () => set({ commandPaletteOpen: false }, false),
});
