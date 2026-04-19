// Overview / dashboard. Composes widgets from `@/components/dashboard`.
//
// Layout philosophy: the grid is 12 columns on lg+. Each widget owns its
// col-span via the `className` prop, so adding a card is a single import +
// one line below. Heights default to fit-content so cards self-size; the
// 3D preview opts into a fixed min-height to render a usable viewport.

import { createFileRoute } from "@tanstack/react-router";
import {
  ActuatorStatusCard,
  CacheStatusCard,
  ConnectionCard,
  DashboardGrid,
  RemindersCard,
  RobotPreviewCard,
  SystemHealthCard,
} from "@/components/dashboard";

export const Route = createFileRoute("/_app/")({
  component: OverviewPage,
});

function OverviewPage() {
  return (
    <div className="space-y-4">
      <header className="flex items-baseline justify-between">
        <h1 className="text-2xl font-semibold">Overview</h1>
        <div className="text-xs text-muted-foreground">
          live state · refresh every 1-15s
        </div>
      </header>

      <DashboardGrid>
        <SystemHealthCard className="lg:col-span-7" />
        <ConnectionCard className="lg:col-span-5" />
        <ActuatorStatusCard className="lg:col-span-7" />
        <RemindersCard className="lg:col-span-5" />
        <RobotPreviewCard className="lg:col-span-8" />
        <CacheStatusCard className="lg:col-span-4" />
      </DashboardGrid>
    </div>
  );
}
