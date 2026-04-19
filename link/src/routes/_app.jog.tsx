import { createFileRoute } from "@tanstack/react-router";
import { ComingSoon } from "@/components/coming-soon";

export const Route = createFileRoute("/_authed/jog")({
  component: () => (
    <ComingSoon
      title="Jog"
      phase="Phase 2"
      points={[
        "Per-motor jog with dead-man switch (hold to move, release to stop).",
        "Enable / disable / set-zero buttons with typed confirmation.",
        "Live readback of mechPos above and below the commanded target.",
        "Respects the single-operator lock from AppState.",
      ]}
    />
  ),
});
