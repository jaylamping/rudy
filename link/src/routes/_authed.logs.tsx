import { createFileRoute } from "@tanstack/react-router";
import { ComingSoon } from "@/components/coming-soon";

export const Route = createFileRoute("/_authed/logs")({
  component: () => (
    <ComingSoon
      title="Logs"
      phase="Phase 2"
      points={[
        "journald tail of rudyd.service as a reliable WebTransport stream.",
        "Kernel CAN errors (dmesg filtered to 'can' / 'socketcan').",
        "Audit log viewer (~/.rudyd/audit.jsonl), filterable by action.",
      ]}
    />
  ),
});
