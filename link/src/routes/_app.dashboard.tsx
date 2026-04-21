import { createFileRoute } from "@tanstack/react-router";
import { Dashboard } from "@/pages/Dashboard";

// Layout route for the app shell. Any child route (telemetry, params, jog,
// viz, logs) renders inside `<AppShell />`. No auth guard: the console is
// only reachable over tailnet / localhost, so we trust the network.
export const Route = createFileRoute("/_app/dashboard")({
  component: Dashboard,
});
