// Inventory tab: surfaces the per-motor commissioning record (every field
// in `inventory.yaml` for this motor, including the free-form `extra` map
// the typed loader passes through).
//
// Also hosts the "mark verified" toggle that flips the `verified` flag in
// `inventory.yaml` and audits the change.

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { api, ApiError } from "@/lib/api";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { ConfirmDialog } from "@/components/params";
import type { MotorSummary } from "@/lib/types/MotorSummary";

export function ActuatorInventoryTab({ motor }: { motor: MotorSummary }) {
  const qc = useQueryClient();
  const detail = useQuery({
    queryKey: ["inventory", motor.role],
    queryFn: () => api.getInventory(motor.role),
    retry: false,
  });
  const supported = !(
    detail.isError && (detail.error as ApiError | undefined)?.status === 404
  );

  const [confirm, setConfirm] = useState(false);
  const [note, setNote] = useState("");
  const verified = motor.verified;

  const setVerified = useMutation({
    mutationFn: () => api.setVerified(motor.role, { verified: !verified, note }),
    onSuccess: () => {
      setConfirm(false);
      setNote("");
      qc.invalidateQueries({ queryKey: ["motors"] });
      qc.invalidateQueries({ queryKey: ["inventory", motor.role] });
    },
  });

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader className="flex flex-row items-baseline justify-between space-y-0">
          <div className="space-y-1">
            <CardTitle className="text-base">Verified status</CardTitle>
            <CardDescription>
              Verified motors are eligible for enable / jog / tests. Flip to
              unverified to lock down a motor under maintenance.
            </CardDescription>
          </div>
          <Switch
            checked={verified}
            disabled={!supported || setVerified.isPending}
            onCheckedChange={() => setConfirm(true)}
          />
        </CardHeader>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">Commissioning record</CardTitle>
          <CardDescription>
            Read-only view of <code className="font-mono">inventory.yaml</code>{" "}
            for this motor. Includes commissioning notes, baseline parameter
            dumps, and per-field timestamps.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {!supported && (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-400">
              Inventory passthrough endpoint is not yet deployed on this
              rudydae build.
            </p>
          )}
          {detail.isPending && supported && (
            <div className="text-xs text-muted-foreground">Loading...</div>
          )}
          {detail.data && (
            <pre className="overflow-x-auto rounded-md border border-border bg-background p-3 text-xs">
              {JSON.stringify(detail.data, null, 2)}
            </pre>
          )}
        </CardContent>
      </Card>

      {confirm && (
        <ConfirmDialog
          title={verified ? "Mark unverified" : "Mark verified"}
          description={
            <div className="space-y-3">
              <p>
                Set <code className="font-mono">{motor.role}</code> to{" "}
                <code className="font-mono">
                  verified = {String(!verified)}
                </code>
                . The change is written to{" "}
                <code className="font-mono">inventory.yaml</code> and audit-logged.
                {!verified &&
                  " Verified motors become eligible for enable / jog / tests."}
                {verified &&
                  " Unverified motors cannot be enabled or jogged until re-verified."}
              </p>
              <Label className="space-y-1 text-sm">
                <span>Operator note (optional)</span>
                <Input
                  value={note}
                  onChange={(e) => setNote(e.target.value)}
                  placeholder="e.g. recommissioned 2026-04-18"
                />
              </Label>
            </div>
          }
          phrase={`verify ${motor.role}`}
          confirmLabel={verified ? "Unverify" : "Verify"}
          confirmVariant={verified ? "destructive" : "default"}
          onCancel={() => {
            setConfirm(false);
            setNote("");
          }}
          onConfirm={() => setVerified.mutate()}
        />
      )}

      {setVerified.isError && (
        <p className="text-xs text-destructive">
          {(setVerified.error as ApiError).message}
        </p>
      )}
    </div>
  );
}
