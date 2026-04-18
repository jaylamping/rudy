// Embedded shrink of the URDF viewer. Click-through opens the full /viz
// route. Drives joint angles from the same `useJointStates` hook that the
// full viewer uses.

import { Link } from "@tanstack/react-router";
import { Maximize2 } from "lucide-react";
import { UrdfViewer } from "@/components/viz/urdf-viewer";
import { useJointStates } from "@/components/viz/use-joint-states";
import { cn } from "@/lib/utils";
import { DashboardCard } from "./dashboard-card";

export function RobotPreviewCard({ className }: { className?: string }) {
  const { jointStates, staleness } = useJointStates();
  const newest = staleness.newestMs;
  const ageS = newest != null ? (Date.now() - newest) / 1000 : null;

  return (
    <DashboardCard
      title="Robot"
      className={className}
      hint={
        <Link
          to="/viz"
          className="flex items-center gap-1 text-muted-foreground hover:text-foreground"
        >
          <Maximize2 className="h-3 w-3" /> open
        </Link>
      }
      bodyClassName="relative"
    >
      <div className="absolute inset-0 mt-3">
        <UrdfViewer
          jointStates={jointStates}
          showGrid
          background="transparent"
          className="h-full w-full"
        />
      </div>
      <div className="pointer-events-none absolute bottom-2 right-2 rounded-sm bg-background/70 px-1.5 py-0.5 text-[10px] text-muted-foreground backdrop-blur">
        {ageS == null
          ? "no data"
          : ageS < 1
            ? `${Math.round(ageS * 1000)}ms`
            : `${ageS.toFixed(1)}s ago`}
      </div>
      {/* Force a min-height so absolute children fill something. */}
      <div className={cn("min-h-[260px]")} />
    </DashboardCard>
  );
}
