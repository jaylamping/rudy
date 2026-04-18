// Overview tab: 2x2 metric grid plus a per-joint URDF highlight.
//
// Reads from the cached `["motors"]` summary. The WT bridge mounted at
// the router root keeps `motor.latest` fresh without any per-tab
// subscription work; uPlot picks up new samples via the `motor` prop.

import { Maximize2 } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { MotorChart, type MotorMetric } from "@/components/motor-chart";
import { UrdfViewer } from "@/components/viz/urdf-viewer";
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
  );
}
