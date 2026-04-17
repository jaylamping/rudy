// Tiny token store. Persisted in localStorage so refresh doesn't log you out;
// cleared via `clearToken()` or the `useAuth()` hook. Tailscale-bounded
// reachability + single operator = this is fine.

import { useMemo, useSyncExternalStore } from "react";

const KEY = "rudyd.token";

type Listener = () => void;
const listeners = new Set<Listener>();

function emit() {
  for (const l of listeners) l();
}

function subscribe(listener: Listener) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

export function getToken(): string | null {
  try {
    return localStorage.getItem(KEY);
  } catch {
    return null;
  }
}

export function setToken(token: string) {
  try {
    localStorage.setItem(KEY, token);
    emit();
  } catch {
    // Private-mode fallback: ignore.
  }
}

export function clearToken() {
  try {
    localStorage.removeItem(KEY);
    emit();
  } catch {
    // ignore
  }
}

export function isAuthed(): boolean {
  return getToken() !== null;
}

export function useAuth() {
  const token = useSyncExternalStore(subscribe, getToken, getToken);

  return useMemo(
    () => ({
      token,
      setToken,
      clearToken,
      isAuthed: token !== null,
    }),
    [token],
  );
}