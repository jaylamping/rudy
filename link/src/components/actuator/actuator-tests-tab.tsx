// Tests tab: invoke any of the bench routines (read / set_zero / smoke /
// jog / jog_overlimit) and stream `test_progress` frames into a live log.
//
// Subscribes to a per-run filter on the WT bridge so this page only sees
// the run it kicked off — the daemon runs at most one test per motor at a
// time anyway, but `run_id` makes the listener resilient if multiple
// operators trigger overlapping runs.

import { useMutation } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { api, ApiError } from "@/lib/api";
import { Button } from "@/components/ui/button";
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
import { Badge } from "@/components/ui/badge";
import { ConfirmDialog } from "@/components/params";
import { useTestProgress } from "@/lib/hooks/useTestProgress";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import type { TestName } from "@/lib/types/TestName";
import type { TestProgress } from "@/lib/types/TestProgress";

interface TestDef {
  name: TestName;
  label: string;
  description: string;
  destructive?: boolean;
  hasParams?: boolean;
}

const TESTS: TestDef[] = [
  {
    name: "read",
    label: "Read 0x70xx state",
    description:
      "Type-17 readback of run_mode + key firmware limits + observables. No motion.",
  },
  {
    name: "set_zero",
    label: "Set mechanical zero",
    description:
      "Issues type-6 to re-anchor the shaft's zero at its current position. Optionally type-22 (save) afterwards.",
    destructive: true,
    hasParams: true,
  },
  {
    name: "smoke",
    label: "Smoke (enable, observe ~1s)",
    description:
      "Enable in velocity mode at spd_ref=0; expect mechVel to stay below 0.1 rad/s. Stops on completion.",
    destructive: true,
  },
  {
    name: "jog",
    label: "Velocity jog ramp",
    description:
      "Trapezoidal ramp up/down to a small target velocity (capped 0.5 rad/s, 3 s).",
    destructive: true,
    hasParams: true,
  },
  {
    name: "jog_overlimit",
    label: "Over-limit clamp test",
    description:
      "Commands spd_ref=20 rad/s; expects firmware limit_spd to clamp the response into [2.5, 3.2] rad/s.",
    destructive: true,
  },
];

interface RunState {
  runId: string;
  test: TestName;
  startedAt: number;
}

export function ActuatorTestsTab({ motor }: { motor: MotorSummary }) {
  const [run, setRun] = useState<RunState | null>(null);
  const [confirm, setConfirm] = useState<TestDef | null>(null);
  const [params, setParams] = useState<{
    save: boolean;
    target_vel: number;
    duration: number;
  }>({
    save: false,
    target_vel: 0.2,
    duration: 2.0,
  });
  const [available, setAvailable] = useState(true);
  const lines = useTestProgress(run?.runId ?? null);

  const start = useMutation({
    mutationFn: async (test: TestName) => {
      const body: Parameters<typeof api.runTest>[2] = {};
      if (test === "set_zero") body.save = params.save;
      if (test === "jog") {
        body.target_vel = params.target_vel;
        body.duration = params.duration;
      }
      return api.runTest(motor.role, test, body);
    },
    onSuccess: ({ run_id }, test) => {
      setRun({ runId: run_id, test, startedAt: Date.now() });
      setConfirm(null);
    },
    onError: (e) => {
      if (e instanceof ApiError && e.status === 404) {
        setAvailable(false);
      }
    },
  });

  const verdict = lines.find((l) => l.level === "pass" || l.level === "fail");

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Bench routines</CardTitle>
          <CardDescription>
            Native rudydae implementations of the canonical RS03 bench
            routines. Each run is single-operator-locked, audit-logged, and
            broadcasts step-by-step progress over a reliable WebTransport
            stream.
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          {!available && (
            <p className="rounded-md border border-amber-500/40 bg-amber-500/10 p-3 text-xs text-amber-400">
              Tests endpoint is not yet deployed on this rudydae build.
            </p>
          )}
          {TESTS.map((t) => (
            <div
              key={t.name}
              className="flex flex-col gap-2 rounded-md border border-border/60 bg-background p-3 sm:flex-row sm:items-start sm:justify-between"
            >
              <div className="min-w-0 flex-1">
                <div className="flex items-center gap-2">
                  <span className="font-medium">{t.label}</span>
                  <code className="font-mono text-xs text-muted-foreground">
                    {t.name}
                  </code>
                  {t.destructive && (
                    <Badge variant="warning">moves the motor</Badge>
                  )}
                </div>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t.description}
                </p>
              </div>
              <Button
                variant={t.destructive ? "destructive" : "default"}
                disabled={!available || start.isPending || run !== null}
                onClick={() => setConfirm(t)}
              >
                Run
              </Button>
            </div>
          ))}
          {start.isError && (
            <p className="text-xs text-destructive">
              {(start.error as ApiError).message}
            </p>
          )}
        </CardContent>
      </Card>

      {run && (
        <Card>
          <CardHeader>
            <CardTitle className="text-base">
              {run.test} — run {run.runId.slice(0, 8)}
            </CardTitle>
            <CardDescription>
              Live progress stream. Lines arrive in order; the run ends when a
              pass / fail line appears.
            </CardDescription>
          </CardHeader>
          <CardContent className="space-y-2">
            <RunLog lines={lines} />
            {verdict && (
              <div className="flex items-center justify-between text-xs">
                <Badge
                  variant={verdict.level === "pass" ? "success" : "destructive"}
                >
                  {verdict.level === "pass" ? "PASS" : "FAIL"}
                </Badge>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setRun(null)}
                >
                  Close
                </Button>
              </div>
            )}
          </CardContent>
        </Card>
      )}

      {confirm && (
        <ConfirmDialog
          title={`Run ${confirm.label}`}
          description={
            <div className="space-y-3">
              <p>
                Run <code className="font-mono">{confirm.name}</code> on{" "}
                <code className="font-mono">{motor.role}</code>.{" "}
                {confirm.description}
              </p>
              {confirm.name === "set_zero" && (
                <Label className="flex items-center justify-between text-sm">
                  <span>Save to flash after re-anchoring</span>
                  <Switch
                    checked={params.save}
                    onCheckedChange={(b) =>
                      setParams((p) => ({ ...p, save: b }))
                    }
                  />
                </Label>
              )}
              {confirm.name === "jog" && (
                <div className="grid grid-cols-2 gap-3 text-sm">
                  <Label className="space-y-1">
                    <span>Target velocity (rad/s)</span>
                    <Input
                      type="number"
                      step={0.05}
                      min={-0.5}
                      max={0.5}
                      value={params.target_vel}
                      onChange={(e) =>
                        setParams((p) => ({
                          ...p,
                          target_vel: Number(e.target.value),
                        }))
                      }
                    />
                  </Label>
                  <Label className="space-y-1">
                    <span>Duration (s)</span>
                    <Input
                      type="number"
                      step={0.5}
                      min={1}
                      max={3}
                      value={params.duration}
                      onChange={(e) =>
                        setParams((p) => ({
                          ...p,
                          duration: Number(e.target.value),
                        }))
                      }
                    />
                  </Label>
                </div>
              )}
            </div>
          }
          confirmLabel="Run"
          confirmVariant={confirm.destructive ? "destructive" : "default"}
          onCancel={() => setConfirm(null)}
          onConfirm={() => start.mutate(confirm.name)}
        />
      )}
    </div>
  );
}

function RunLog({ lines }: { lines: TestProgress[] }) {
  const ref = useRef<HTMLDivElement | null>(null);
  useEffect(() => {
    ref.current?.scrollTo({ top: ref.current.scrollHeight });
  }, [lines.length]);
  return (
    <div
      ref={ref}
      className="max-h-72 overflow-y-auto rounded-md border border-border bg-background p-2 font-mono text-xs"
    >
      {lines.length === 0 && (
        <span className="text-muted-foreground">waiting for first frame...</span>
      )}
      {lines.map((l) => (
        <div
          key={`${l.run_id}-${l.seq}`}
          className={
            l.level === "fail"
              ? "text-destructive"
              : l.level === "pass"
              ? "text-emerald-400"
              : l.level === "warn"
              ? "text-amber-400"
              : "text-foreground"
          }
        >
          <span className="text-muted-foreground">[{l.step}]</span>{" "}
          {l.message}
        </div>
      ))}
    </div>
  );
}
