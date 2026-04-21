// Firmware tab: the full parameter catalog for one motor.
//
// Renders the writable firmware-limit rows (via `ParamRow`) plus a
// read-only observables table, and offers bulk "Sync drifted" to push
// inventory desired values to the device.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ConfirmDialog, ParamRow } from "@/components/params";
import type { ParamValue } from "@/lib/types/ParamValue";
import {
  formatAngleDeg,
  formatAngularVelDeg,
  isAngleUnit,
  isAngularVelUnit,
} from "@/lib/units";

function observableValueCell(p: ParamValue): string {
  const n =
    typeof p.value === "number" && Number.isFinite(p.value) ? p.value : null;
  if (n != null && (isAngleUnit(p.units) || isAngularVelUnit(p.units))) {
    return isAngularVelUnit(p.units) && !isAngleUnit(p.units)
      ? formatAngularVelDeg(n, 4)
      : formatAngleDeg(n, 4);
  }
  return JSON.stringify(p.value);
}

function observableUnitCell(p: ParamValue): string {
  if (isAngleUnit(p.units) && !isAngularVelUnit(p.units)) return "°";
  if (isAngularVelUnit(p.units)) return "°/s";
  return p.units ?? "";
}

export function ActuatorFirmwareTab({ role }: { role: string }) {
  const qc = useQueryClient();
  const paramsQ = useQuery({
    queryKey: ["params", role],
    queryFn: () => api.getParams(role),
  });
  const [confirmSync, setConfirmSync] = useState(false);

  const syncDrifted = useMutation({
    mutationFn: () => api.syncParams(role, {}),
    onSuccess: () => {
      setConfirmSync(false);
      qc.invalidateQueries({ queryKey: ["params", role] });
      qc.invalidateQueries({ queryKey: ["motors"] });
    },
  });

  const entries = useMemo(
    () =>
      Object.values(paramsQ.data?.values ?? {}).filter(
        (p): p is ParamValue => p !== undefined,
      ),
    [paramsQ.data],
  );
  // Split on the spec section the param came from (`writable`), not on
  // the presence of `hardware_range`. Several firmware-limit params
  // (`run_mode`, `can_timeout`, `zero_sta`, `damper`, `add_offset`) are
  // writable but have no numeric range — they're enums or counters — so
  // a `hardware_range`-based gate would misclassify them as observables.
  // The cortex `PUT /api/motors/:role/params/:name` handler validates
  // against `spec.firmware_limits` directly, so this flag is the
  // canonical "the server will accept a write to me" signal.
  const editable = entries.filter((p) => p.writable);
  const observables = entries.filter((p) => !p.writable);
  const driftCount = useMemo(
    () => editable.filter((p) => p.drift).length,
    [editable],
  );

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
                    {observableValueCell(p)}
                  </td>
                  <td className="px-3 py-2 text-muted-foreground">
                    {observableUnitCell(p)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </CardContent>
      </Card>

      <div className="sticky bottom-2 z-10 flex flex-wrap items-center justify-end gap-2 rounded-md border border-border bg-card/80 p-2 backdrop-blur">
        <span className="text-xs text-muted-foreground">
          Saving writes to flash and updates desired values in inventory.
        </span>
        <Button
          variant="destructive"
          size="sm"
          disabled={driftCount === 0 || syncDrifted.isPending}
          onClick={() => setConfirmSync(true)}
        >
          {syncDrifted.isPending
            ? "Syncing..."
            : `Sync all drifted (${driftCount})`}
        </Button>
        {syncDrifted.isError && (
          <span className="text-xs text-destructive">
            {(syncDrifted.error as ApiError).message}
          </span>
        )}
      </div>

      {confirmSync && (
        <ConfirmDialog
          title="Sync drifted parameters"
          description={
            <>
              Push every <strong>drifted</strong> value from inventory to{" "}
              <code className="font-mono">{role}</code> (write + type-22 save).
            </>
          }
          confirmLabel="Sync all"
          confirmVariant="destructive"
          onCancel={() => setConfirmSync(false)}
          onConfirm={() => syncDrifted.mutate()}
        />
      )}
    </div>
  );
}
