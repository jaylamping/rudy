// Firmware tab: the full parameter catalog for one motor.
//
// Reuses `ParamRow` (writable + observables) from `_app.params.tsx`'s
// extracted version, plus a sticky "Save to flash" button at the bottom for
// the once-per-batch persistence step.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog, ParamRow } from "@/components/params";
import type { ParamValue } from "@/lib/types/ParamValue";

export function ActuatorFirmwareTab({ role }: { role: string }) {
  const qc = useQueryClient();
  const paramsQ = useQuery({
    queryKey: ["params", role],
    queryFn: () => api.getParams(role),
  });
  const [confirmSave, setConfirmSave] = useState(false);

  const saveAll = useMutation({
    mutationFn: () => api.saveToFlash(role),
    onSuccess: () => {
      setConfirmSave(false);
      qc.invalidateQueries({ queryKey: ["params", role] });
    },
  });

  const entries = useMemo(
    () =>
      Object.values(paramsQ.data?.values ?? {}).filter(
        (p): p is ParamValue => p !== undefined,
      ),
    [paramsQ.data],
  );
  const editable = entries.filter(
    (p) => p.hardware_range !== null && p.hardware_range !== undefined,
  );
  const observables = entries.filter((p) => !p.hardware_range);

  if (paramsQ.isPending) {
    return <div className="text-muted-foreground">Loading parameters...</div>;
  }
  if (paramsQ.isError) {
    return (
      <div className="text-destructive">
        Error: {(paramsQ.error as Error).message}
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">
            Firmware limits (writable)
          </CardTitle>
        </CardHeader>
        <CardContent className="px-0 pb-0">
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
                  <td
                    colSpan={6}
                    className="px-3 py-6 text-center text-muted-foreground"
                  >
                    No writable parameters in the spec.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">
            Observables (read-only)
          </CardTitle>
        </CardHeader>
        <CardContent className="px-0 pb-0">
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
                  <td className="px-3 py-2 font-mono tabular-nums">
                    {JSON.stringify(p.value)}
                  </td>
                  <td className="px-3 py-2 text-muted-foreground">
                    {p.units ?? ""}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </CardContent>
      </Card>

      <div className="sticky bottom-2 z-10 flex items-center justify-end gap-2 rounded-md border border-border bg-card/80 p-2 backdrop-blur">
        <span className="text-xs text-muted-foreground">
          RAM writes are volatile. Save once per batch to persist across power.
        </span>
        <Button
          variant="destructive"
          size="sm"
          disabled={saveAll.isPending}
          onClick={() => setConfirmSave(true)}
        >
          {saveAll.isPending ? "Saving..." : "Save all to flash"}
        </Button>
        {saveAll.isError && (
          <span className="text-xs text-destructive">
            {(saveAll.error as ApiError).message}
          </span>
        )}
      </div>

      {confirmSave && (
        <ConfirmDialog
          title="Save parameters to flash"
          description={
            <>
              Issue type-22 to <code className="font-mono">{role}</code>. Every
              RAM-resident parameter on the motor will be written to its
              non-volatile store. Survives power cycles.
            </>
          }
          confirmLabel="Save"
          confirmVariant="destructive"
          onCancel={() => setConfirmSave(false)}
          onConfirm={() => saveAll.mutate()}
        />
      )}
    </div>
  );
}
