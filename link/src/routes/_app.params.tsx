import { createFileRoute } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { api } from "@/lib/api";
import { ParamRow } from "@/components/params";
import type { ParamSnapshot } from "@/lib/types/ParamSnapshot";
import type { ParamValue } from "@/lib/types/ParamValue";

export const Route = createFileRoute("/_app/params")({
  validateSearch: (s: Record<string, unknown>): { role?: string } => ({
    role: typeof s.role === "string" ? s.role : undefined,
  }),
  component: ParamsPage,
});

function ParamsPage() {
  const { role: roleInUrl } = Route.useSearch();
  const navigate = Route.useNavigate();

  const motorsQ = useQuery({ queryKey: ["motors"], queryFn: () => api.listMotors() });
  const roles = motorsQ.data?.map((m) => m.role) ?? [];
  const role = roleInUrl ?? roles[0];

  const paramsQ = useQuery({
    queryKey: ["params", role],
    queryFn: () => (role ? api.getParams(role) : Promise.reject("no role")),
    enabled: !!role,
  });

  return (
    <div className="space-y-4">
      <header className="flex items-baseline justify-between">
        <h1 className="text-2xl font-semibold">Parameters</h1>
        <select
          className="rounded-md border border-input bg-background px-2 py-1 text-sm"
          value={role ?? ""}
          onChange={(e) => navigate({ search: { role: e.target.value } })}
        >
          {roles.map((r) => (
            <option key={r} value={r}>
              {r}
            </option>
          ))}
        </select>
      </header>

      {paramsQ.isPending && <div className="text-muted-foreground">Loading...</div>}
      {paramsQ.isError && (
        <div className="text-destructive">Error: {(paramsQ.error as Error).message}</div>
      )}
      {paramsQ.data && role && (
        <ParamTable role={role} snap={paramsQ.data} />
      )}
    </div>
  );
}

function ParamTable({ role, snap }: { role: string; snap: ParamSnapshot }) {
  const entries = useMemo(() => Object.values(snap.values).filter((p): p is ParamValue => p !== undefined), [snap]);
  // Split on the spec section the param came from (`writable`),
  // not on the presence of `hardware_range`. Several firmware-limit
  // params (`run_mode`, `can_timeout`, `zero_sta`, `damper`,
  // `add_offset`) are writable but have no numeric range — they're
  // enums or counters — so the old `hardware_range`-based gate
  // misclassified them as observables. The cortex `PUT
  // /api/motors/:role/params/:name` handler validates against
  // `spec.firmware_limits` directly, so this flag is the canonical
  // "the server will accept a write to me" signal.
  const editable = entries.filter((p) => p.writable);
  const observables = entries.filter((p) => !p.writable);

  return (
    <div className="space-y-6">
      <section>
        <h2 className="mb-2 text-sm font-medium uppercase tracking-wide text-muted-foreground">
          Firmware limits (writable)
        </h2>
        <div className="overflow-hidden rounded-lg border border-border bg-card">
          <table className="w-full text-sm">
            <thead className="bg-muted/30 text-xs uppercase tracking-wide text-muted-foreground">
              <tr>
                <th className="px-3 py-2 text-left font-medium">Name</th>
                <th className="px-3 py-2 text-left font-medium">Index</th>
                <th className="px-3 py-2 text-left font-medium">Value</th>
                <th className="px-3 py-2 text-left font-medium">Range</th>
                <th className="px-3 py-2 text-left font-medium">Unit</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {editable.map((p) => (
                <ParamRow key={p.name} role={role} param={p} />
              ))}
              {editable.length === 0 && (
                <tr>
                  <td colSpan={6} className="px-3 py-6 text-center text-muted-foreground">
                    No writable parameters in the spec.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </div>
      </section>

      <section>
        <h2 className="mb-2 text-sm font-medium uppercase tracking-wide text-muted-foreground">
          Observables (read-only)
        </h2>
        <div className="overflow-hidden rounded-lg border border-border bg-card">
          <table className="w-full text-sm">
            <thead className="bg-muted/30 text-xs uppercase tracking-wide text-muted-foreground">
              <tr>
                <th className="px-3 py-2 text-left font-medium">Name</th>
                <th className="px-3 py-2 text-left font-medium">Index</th>
                <th className="px-3 py-2 text-left font-medium">Value</th>
                <th className="px-3 py-2 text-left font-medium">Unit</th>
              </tr>
            </thead>
            <tbody>
              {observables.map((p) => (
                <tr key={p.name} className="border-t border-border/60">
                  <td className="px-3 py-2 font-mono">{p.name}</td>
                  <td className="px-3 py-2 font-mono text-muted-foreground">
                    0x{p.index.toString(16).toUpperCase().padStart(4, "0")}
                  </td>
                  <td className="px-3 py-2 font-mono tabular-nums">{JSON.stringify(p.value)}</td>
                  <td className="px-3 py-2 text-muted-foreground">{p.units ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  );
}
