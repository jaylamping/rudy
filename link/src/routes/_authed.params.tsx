import { createFileRoute } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { api, ApiError } from "@/lib/api";
import type { ParamSnapshot } from "@/lib/types/ParamSnapshot";
import type { JsonValue } from "@/lib/types/serde_json/JsonValue";
import type { ParamValue } from "@/lib/types/ParamValue";

export const Route = createFileRoute("/_authed/params")({
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
  const entries = useMemo(() => Object.values(snap.values), [snap]);
  const editable = entries.filter((p) => p.hardware_range !== null && p.hardware_range !== undefined);
  const observables = entries.filter((p) => !p.hardware_range);

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

function ParamRow({ role, param }: { role: string; param: ParamValue }) {
  const qc = useQueryClient();
  const [draft, setDraft] = useState<string>(String(param.value ?? ""));
  const [confirm, setConfirm] = useState<null | { save: boolean }>(null);

  const write = useMutation({
    mutationFn: async ({ save }: { save: boolean }) => {
      const value = parseValue(draft, param.type);
      return api.writeParam(role, param.name, { value, save_after: save });
    },
    onSuccess: () => {
      setConfirm(null);
      qc.invalidateQueries({ queryKey: ["params", role] });
    },
  });

  const [lo, hi] = param.hardware_range ?? [null, null];

  return (
    <tr className="border-t border-border/60 align-middle">
      <td className="px-3 py-2 font-mono">{param.name}</td>
      <td className="px-3 py-2 font-mono text-muted-foreground">
        0x{param.index.toString(16).toUpperCase().padStart(4, "0")}
      </td>
      <td className="px-3 py-2">
        <input
          className="w-32 rounded-md border border-input bg-background px-2 py-1 font-mono text-sm"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
        />
      </td>
      <td className="px-3 py-2 font-mono text-muted-foreground">
        {lo !== null && hi !== null ? `[${lo}, ${hi}]` : "-"}
      </td>
      <td className="px-3 py-2 text-muted-foreground">{param.units ?? ""}</td>
      <td className="px-3 py-2">
        <div className="flex gap-2">
          <button
            className="rounded-md border border-border px-2 py-1 text-xs hover:bg-accent"
            onClick={() => setConfirm({ save: false })}
            disabled={write.isPending}
          >
            Write RAM
          </button>
          <button
            className="rounded-md border border-destructive/50 bg-destructive/10 px-2 py-1 text-xs text-destructive hover:bg-destructive/20"
            onClick={() => setConfirm({ save: true })}
            disabled={write.isPending}
          >
            Save to flash
          </button>
        </div>
        {write.isError && (
          <div className="mt-1 text-xs text-destructive">
            {(write.error as ApiError).message}
          </div>
        )}
        {confirm && (
          <ConfirmDialog
            paramName={param.name}
            role={role}
            value={draft}
            units={param.units ?? ""}
            save={confirm.save}
            onCancel={() => setConfirm(null)}
            onConfirm={() => write.mutate({ save: confirm.save })}
          />
        )}
      </td>
    </tr>
  );
}

function parseValue(s: string, ty: string): JsonValue {
  if (ty.startsWith("u") || ty === "uint8" || ty === "uint16" || ty === "uint32") {
    const n = Number(s);
    if (!Number.isFinite(n)) throw new Error(`expected integer, got ${s}`);
    return Math.trunc(n);
  }
  const n = Number(s);
  if (!Number.isFinite(n)) throw new Error(`expected number, got ${s}`);
  return n;
}

function ConfirmDialog({
  role,
  paramName,
  value,
  units,
  save,
  onConfirm,
  onCancel,
}: {
  role: string;
  paramName: string;
  value: string;
  units: string;
  save: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const phrase = save ? `save ${paramName}` : `write ${paramName}`;
  const [typed, setTyped] = useState("");
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
      <div className="w-full max-w-md space-y-4 rounded-lg border border-border bg-card p-6">
        <div>
          <h3 className="text-lg font-semibold">
            {save ? "Save to flash" : "Write to RAM"}
          </h3>
          <p className="mt-1 text-sm text-muted-foreground">
            You are about to {save ? "save " : "write "}
            <code className="font-mono">{paramName}</code> ={" "}
            <code className="font-mono">{value}</code> {units} on{" "}
            <code className="font-mono">{role}</code>.
            {save && " This persists across power cycles."}
          </p>
        </div>
        <label className="block space-y-1.5 text-sm">
          <span>
            Type <code className="font-mono">{phrase}</code> to confirm:
          </span>
          <input
            className="w-full rounded-md border border-input bg-background px-2 py-1 font-mono text-sm"
            value={typed}
            onChange={(e) => setTyped(e.target.value)}
            autoFocus
          />
        </label>
        <div className="flex justify-end gap-2">
          <button
            className="rounded-md border border-border px-3 py-1.5 text-sm hover:bg-accent"
            onClick={onCancel}
          >
            Cancel
          </button>
          <button
            className="rounded-md bg-destructive px-3 py-1.5 text-sm text-destructive-foreground disabled:opacity-50"
            disabled={typed !== phrase}
            onClick={onConfirm}
          >
            {save ? "Save" : "Write"}
          </button>
        </div>
      </div>
    </div>
  );
}
