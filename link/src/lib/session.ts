// Per-tab session id used by the rudydae single-operator lock.
//
// Minted lazily on first use, cached in `sessionStorage` so a route swap
// doesn't churn the id. The id is opaque to the daemon — it just needs a
// stable string per browser tab so the lock-holder check in the control
// handlers can match repeat requests from the same operator.
//
// SSR-safe: returns a synthetic id off the browser (e.g. during vitest in
// jsdom-less contexts) so callers don't crash.

const KEY = "rudy.session_id";

let cached: string | null = null;

export function sessionId(): string {
  if (cached) return cached;
  try {
    const ss = window.sessionStorage;
    let id = ss.getItem(KEY);
    if (!id) {
      id = randomId();
      ss.setItem(KEY, id);
    }
    cached = id;
    return id;
  } catch {
    cached = randomId();
    return cached;
  }
}

function randomId(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) {
    return crypto.randomUUID();
  }
  // Fallback for older runtimes.
  const a = Math.random().toString(36).slice(2);
  const b = Date.now().toString(36);
  return `s-${b}-${a}`;
}
