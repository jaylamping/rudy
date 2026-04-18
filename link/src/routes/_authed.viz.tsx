// Full-page 3D viewer. Drives joints from the same `useJointStates` hook
// the dashboard preview uses, so the two stay in sync. Controls overlay
// in the top-right corner; reset re-frames the bounded scene by remounting
// the viewer (cheap; URDF is already cached by the browser).

import { createFileRoute } from "@tanstack/react-router";
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { UrdfViewer } from "@/components/viz/urdf-viewer";
import {
  ViewerControls,
  type ViewerControlsState,
} from "@/components/viz/viewer-controls";
import { useJointStates } from "@/components/viz/use-joint-states";
import { api } from "@/lib/api";

export const Route = createFileRoute("/_authed/viz")({
  component: VizPage,
});

function VizPage() {
  const cfg = useQuery({ queryKey: ["config"], queryFn: () => api.config() });
  const { jointStates, staleness } = useJointStates();
  const [controls, setControls] = useState<ViewerControlsState>({
    showGrid: true,
    wireframe: false,
  });
  const [resetKey, setResetKey] = useState(0);

  const newest = staleness.newestMs;
  const ageS = newest != null ? (Date.now() - newest) / 1000 : null;

  return (
    <div className="flex h-[calc(100vh-3rem)] flex-col gap-4">
      <header className="flex flex-wrap items-baseline justify-between gap-2">
        <div>
          <h1 className="text-2xl font-semibold">Viz</h1>
          <p className="text-xs text-muted-foreground">
            URDF baked from{" "}
            <code className="font-mono">
              ros/src/description/urdf/robot.urdf.xacro
            </code>
            {cfg.data?.features.mock_can ? " · mock CAN driving joints" : ""}
          </p>
        </div>
        <div className="text-xs text-muted-foreground">
          last sample:{" "}
          <span className="font-mono tabular-nums">
            {ageS == null
              ? "no data"
              : ageS < 1
                ? `${Math.round(ageS * 1000)}ms`
                : `${ageS.toFixed(1)}s ago`}
          </span>
        </div>
      </header>

      <div className="relative flex-1 overflow-hidden rounded-lg border border-border bg-card">
        <UrdfViewer
          key={resetKey}
          jointStates={jointStates}
          showGrid={controls.showGrid}
          wireframe={controls.wireframe}
          background="transparent"
          className="h-full w-full"
        />
        <ViewerControls
          state={controls}
          onChange={setControls}
          onReset={() => setResetKey((k) => k + 1)}
          className="absolute right-3 top-3"
        />
      </div>
    </div>
  );
}
