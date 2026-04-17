// Tiny token store. Persisted in localStorage so refresh doesn't log you out;
// cleared via the `logout()` helper. Tailscale-bounded reachability + single
// operator = this is fine.

const KEY = "rudyd.token";

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
  } catch {
    // Private-mode fallback: ignore.
  }
}

export function clearToken() {
  try {
    localStorage.removeItem(KEY);
  } catch {
    // ignore
  }
}

export function isAuthed(): boolean {
  return getToken() !== null;
}
