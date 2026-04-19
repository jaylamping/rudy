// Overview tab: 2x2 metric grid plus a per-joint URDF highlight.
//
// Reads from the cached `["motors"]` summary. The WT bridge mounted at
// the router root keeps `motor.latest` fresh without any per-tab
// subscription work; uPlot picks up new samples via the `motor` prop.

import { Home, Maximize2 } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "@/lib/api";
import { HomingProgressBar } from "@/components/actuator/homing-progress";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { MotorChart, type MotorMetric } from "@/components/motor-chart";
import { Tooltip } from "@/components/ui/tooltip";
import { UrdfViewer } from "@/components/viz/urdf-viewer";
import { useLimbHealth } from "@/lib/hooks/useLimbHealth";
import type { MotorSummary } from "@/lib/types/MotorSummary";

const METRICS: { key: MotorMetric; title: string }[] = [
  { key: "pos", title: "Position" },
  { key: "vel", title: "Velocity" },
  { key: "torque", title: "Torque" },
  { key: "temp", title: "Temperature" },
];

export function ActuatorOverviewTab({ motor }: { motor: MotorSummary }) {
  // Map the motor.role to a single-joint state for the URDF preview. The
  // viewer already accepts unknown joints as a no-op (urdf-viewer.tsx
  // tries both `role` and `${role}_joint`).
  const jointStates = motor.latest
    ? { [motor.role]: motor.latest.mech_pos_rad }
    : {};

  return (
    <div className="space-y-4">
      <GoHomeBar motor={motor} />
      <div className="grid gap-4 lg:grid-cols-3">
        <div className="grid gap-4 sm:grid-cols-2 lg:col-span-2">
          {METRICS.map(({ key, title }) => (
            <Card key={key}>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm font-medium">{title}</CardTitle>
              </CardHeader>
              <CardContent>
                <MotorChart motor={motor} metric={key} height={140} />
              </CardContent>
            </Card>
          ))}
        </div>

        <Card className="lg:col-span-1">
          <CardHeader className="flex flex-row items-baseline justify-between space-y-0 pb-2">
            <CardTitle className="text-sm font-medium">Robot pose</CardTitle>
            <Link
              to="/viz"
              className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
            >
              <Maximize2 className="h-3 w-3" /> open
            </Link>
          </CardHeader>
          <CardContent className="relative h-[320px] p-0">
            <div className="absolute inset-0">
              <UrdfViewer
                jointStates={jointStates}
                showGrid
                background="transparent"
                className="h-full w-full"
              />
            </div>
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

// One-click "go to mechanical zero" for an already-Homed actuator. Reuses
// the slow-ramp homer endpoint with target_rad=0; the API allows re-homing
// from BootState::Homed (see crates/rudydae/src/api/home.rs). For non-Homed
// states the button is disabled and we point the operator at the Travel
// tab where the full Verify & Home ritual lives.
function GoHomeBar({ motor }: { motor: MotorSummary }) {
  const qc = useQueryClient();
  const limb = useLimbHealth(motor.role);
  const home = useMutation({
    mutationFn: () => api.homeMotor(motor.role, 0),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ["motors"] });
    },
  });
  const bs = motor.boot_state;
  const homed = bs.kind === "homed";
  const homeBlocked = !limb.healthy;
  const homeTip =
    homeBlocked && limb.blockReason ? limb.blockReason : "";
  const live =
    motor.latest != null
      ? ((motor.latest.mech_pos_rad * 180) / Math.PI).toFixed(2)
      : null;
  const autoHoming = bs.kind === "auto_homing";

  return (
    <Card>
      <CardContent className="flex flex-wrap items-center justify-between gap-3 py-3">
        {autoHoming && (
          <div className="basis-full rounded-md border border-sky-500/35 bg-sky-500/10 px-3 py-2 text-xs">
            <div className="font-medium text-sky-100">
              Boot orchestrator is homing this joint
            </div>
            <div className="mt-1.5 flex flex-wrap items-center gap-3 text-muted-foreground">
              <HomingProgressBar
                fromRad={bs.from_rad}
                targetRad={bs.target_rad}
                progressRad={bs.progress_rad}
              />
              <span className="font-mono text-[0.7rem] text-foreground/80">
                {(bs.progress_rad * (180 / Math.PI)).toFixed(1)}° → target{" "}
                {(bs.target_rad * (180 / Math.PI)).toFixed(1)}°
              </span>
            </div>
          </div>
        )}
        <div className="flex flex-col gap-0.5 text-sm">
          <span className="font-medium">Return to home</span>
          <span className="text-xs text-muted-foreground">
            {autoHoming
              ? "Wait for auto-homing to finish, or use Travel tab to retry manually if it failed."
              : homed
              ? "Slow-ramp to 0° using the verified home position."
              : "Run Verify & Home from the Travel tab first."}
            {live != null && (
              <>
                {" "}
                Live: <span className="font-mono">{live}°</span>
              </>
            )}
          </span>
        </div>
        <div className="flex items-center gap-2">
          {!homed && !autoHoming && (
            <Link
              to="/actuators/$role"
              params={{ role: motor.role }}
              search={{ tab: "travel" }}
              className="text-xs text-muted-foreground underline-offset-2 hover:underline"
            >
              go to Travel tab
            </Link>
          )}
          {homed && !autoHoming && homeTip ? (
            <Tooltip content={homeTip} className="max-w-xs whitespace-normal">
              <span className="inline-flex">
                <Button
                  variant="default"
                  size="sm"
                  disabled={home.isPending || homeBlocked}
                  onClick={() => home.mutate()}
                >
                  <Home className="mr-1.5 h-3.5 w-3.5" />
                  {home.isPending ? "Homing..." : "Go to 0°"}
                </Button>
              </span>
            </Tooltip>
          ) : (
            <Button
              variant="default"
              size="sm"
              disabled={
                !homed ||
                autoHoming ||
                home.isPending ||
                homeBlocked
              }
              onClick={() => home.mutate()}
            >
              <Home className="mr-1.5 h-3.5 w-3.5" />
              {home.isPending ? "Homing..." : "Go to 0°"}
            </Button>
          )}
        </div>
        {home.isError && (
          <p className="basis-full text-xs text-destructive">
            {(home.error as ApiError).message}
          </p>
        )}
        {home.isSuccess && (
          <p className="basis-full text-xs text-emerald-400">
            Homed at {((home.data.final_pos_rad * 180) / Math.PI).toFixed(2)}° in {home.data.ticks} ticks.
          </p>
        )}
      </CardContent>
    </Card>
  );
}
