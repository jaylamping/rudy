import { createFileRoute } from "@tanstack/react-router";
import { ComingSoon } from "@/components/coming-soon";

export const Route = createFileRoute("/_authed/viz")({
  component: () => (
    <ComingSoon
      title="Viz"
      phase="Phase 2"
      points={[
        "URDF 3D viewer (three-fiber + urdf-loader) driven by reconstructed joint_states.",
        "Ghost overlay of commanded vs actual pose.",
        "Phase 3: Isaac Lab policy ghost on top of the live robot.",
      ]}
    />
  ),
});
