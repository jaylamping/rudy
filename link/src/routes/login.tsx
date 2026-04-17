import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useState } from "react";
import { useAuth } from "@/lib/hooks/useAuth";
import { apiFetch } from "@/lib/api";

export const Route = createFileRoute("/login")({
  component: LoginScreen,
});

function LoginScreen() {
  const navigate = useNavigate();
  const { setToken } = useAuth();
  const [token, setTokenInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError(null);

    // Persist the token so `apiFetch` picks it up, then try a gated endpoint.
    // If it 200s we're in; if it 401s we wipe it and show the error.
    setToken(token);
    try {
      await apiFetch("/api/config");
      navigate({ to: "/telemetry" });
    } catch (e: unknown) {
      const msg = e instanceof Error ? e.message : "login failed";
      setError(msg);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex min-h-full items-center justify-center p-6">
      <form
        onSubmit={onSubmit}
        className="w-full max-w-sm space-y-5 rounded-lg border border-border bg-card p-6 shadow-sm"
      >
        <div>
          <h1 className="text-xl font-semibold">Rudy operator console</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            Sign in with the operator token. Tailscale-only.
          </p>
        </div>
        <label className="block space-y-1.5">
          <span className="text-sm font-medium">Token</span>
          <input
            className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring"
            type="password"
            autoComplete="off"
            value={token}
            onChange={(e) => setTokenInput(e.target.value)}
            placeholder="paste operator token"
          />
        </label>
        {error && (
          <div className="rounded-md border border-destructive/40 bg-destructive/10 p-2 text-sm text-destructive">
            {error}
          </div>
        )}
        <button
          type="submit"
          disabled={busy || token.length === 0}
          className="w-full rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground disabled:opacity-60"
        >
          {busy ? "Signing in..." : "Sign in"}
        </button>
        <p className="text-xs text-muted-foreground">
          Tokens rotate via the runbook. If you are running rudydae with{" "}
          <code className="font-mono">dev_allow_no_token = true</code>, any value works.
        </p>
      </form>
    </div>
  );
}
