import { createFileRoute, redirect } from "@tanstack/react-router";
import { AppShell } from "@/components/app-shell";
import { isAuthed } from "@/lib/hooks/useAuth";

// Layout route that guards the app shell. Any child route (telemetry, params,
// jog, viz, logs) renders inside `<AppShell />`.
export const Route = createFileRoute("/_authed")({
  beforeLoad: () => {
    if (!isAuthed()) {
      throw redirect({ to: "/login" });
    }
  },
  component: AppShell,
});
