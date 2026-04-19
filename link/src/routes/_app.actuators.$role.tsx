// Per-actuator detail page.
//
// Sticky header with role / can_id / bus / verified+present pills + last
// telemetry, then six tabs (Overview, Travel, Firmware, Controls, Tests,
// Inventory). Reads from the shared `["motors"]` cache so the WT bridge's
// per-frame updates flow in without any new subscription.

import { createFileRoute, Link, notFound } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { ArrowLeft } from "lucide-react";
import { useMemo } from "react";
import { api } from "@/lib/api";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import { ActuatorOverviewTab } from "@/components/actuator/actuator-overview-tab";
import { ActuatorFirmwareTab } from "@/components/actuator/actuator-firmware-tab";
import { ActuatorControlsTab } from "@/components/actuator/actuator-controls-tab";
import { ActuatorTravelTab } from "@/components/actuator/actuator-travel-tab";
import { ActuatorTestsTab } from "@/components/actuator/actuator-tests-tab";
import { ActuatorInventoryTab } from "@/components/actuator/actuator-inventory-tab";

const STALE_MS = 3_000;
const HOT_DEGC = 65;

const TABS = [
  { value: "overview", label: "Overview" },
  { value: "travel", label: "Travel limits" },
  { value: "firmware", label: "Firmware" },
  { value: "controls", label: "Controls" },
  { value: "tests", label: "Tests" },
  { value: "inventory", label: "Inventory" },
] as const;
type TabValue = (typeof TABS)[number]["value"];

export const Route = createFileRoute("/_app/actuators/$role")({
  // Pre-fetch the motors list before the route renders so we can fail fast
  // on an unknown role with a clean 404 instead of "Loading..." -> empty.
  loader: async ({ context, params }) => {
    const motors = await context.queryClient.ensureQueryData({
      queryKey: ["motors"],
      queryFn: () => api.listMotors(),
    });
    if (!motors.find((m) => m.role === params.role)) {
      throw notFound();
    }
  },
  validateSearch: (s: Record<string, unknown>): { tab?: TabValue } => {
    const t = typeof s.tab === "string" ? s.tab : undefined;
    return {
      tab: TABS.find((tab) => tab.value === t)?.value,
    };
  },
  notFoundComponent: NotFoundActuator,
  component: ActuatorDetailPage,
});

function NotFoundActuator() {
  return (
    <div className="space-y-3">
      <Link
        to="/"
        className="inline-flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
      >
        <ArrowLeft className="h-3 w-3" /> back to overview
      </Link>
      <div className="rounded-lg border border-border bg-card p-6 text-sm text-muted-foreground">
        Unknown actuator role.
      </div>
    </div>
  );
}

function ActuatorDetailPage() {
  const { role } = Route.useParams();
  const { tab } = Route.useSearch();
  const navigate = Route.useNavigate();

  const motorsQ = useQuery({
    queryKey: ["motors"],
    queryFn: () => api.listMotors(),
    refetchInterval: useLiveInterval({ live: 30_000, fallback: 2_000 }),
  });

  const motor = useMemo(
    () => motorsQ.data?.find((m) => m.role === role),
    [motorsQ.data, role],
  );

  // The loader has already proven the role exists; this branch only hits if
  // the role disappears between mount and re-render (e.g. inventory.yaml
  // edited live). Keep the guard so the tab renderers can assume `motor`.
  if (!motor) {
    if (motorsQ.isPending) {
      return <div className="text-muted-foreground">Loading...</div>;
    }
    return <NotFoundActuator />;
  }

  const activeTab: TabValue = tab ?? "overview";

  return (
    <div className="space-y-4">
      <div>
        <Link
          to="/telemetry"
          className="inline-flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
        >
          <ArrowLeft className="h-3 w-3" /> all actuators
        </Link>
      </div>

      <ActuatorHeader motor={motor} />

      <Tabs
        value={activeTab}
        onValueChange={(v) => navigate({ search: { tab: v as TabValue } })}
      >
        <TabsList className="flex w-full justify-start overflow-x-auto">
          {TABS.map((t) => (
            <TabsTrigger key={t.value} value={t.value}>
              {t.label}
            </TabsTrigger>
          ))}
        </TabsList>

        <TabsContent value="overview">
          <ActuatorOverviewTab motor={motor} />
        </TabsContent>
        <TabsContent value="travel">
          <ActuatorTravelTab motor={motor} />
        </TabsContent>
        <TabsContent value="firmware">
          <ActuatorFirmwareTab role={motor.role} />
        </TabsContent>
        <TabsContent value="controls">
          <ActuatorControlsTab motor={motor} />
        </TabsContent>
        <TabsContent value="tests">
          <ActuatorTestsTab motor={motor} />
        </TabsContent>
        <TabsContent value="inventory">
          <ActuatorInventoryTab motor={motor} />
        </TabsContent>
      </Tabs>
    </div>
  );
}

function ActuatorHeader({ motor }: { motor: MotorSummary }) {
  const fb = motor.latest;
  const ageS = fb ? (Date.now() - Number(fb.t_ms)) / 1000 : null;
  const stale = ageS != null && ageS * 1000 > STALE_MS;
  const hot = fb ? fb.temp_c >= HOT_DEGC : false;

  return (
    <header className="rounded-lg border border-border bg-card p-4">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <div className="flex flex-wrap items-baseline gap-3">
          <h1 className="text-2xl font-semibold">{motor.role}</h1>
          <span className="text-sm text-muted-foreground">
            can_id 0x{motor.can_id.toString(16).padStart(2, "0").toUpperCase()}{" "}
            on {motor.can_bus}
          </span>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Badge variant={motor.verified ? "success" : "warning"}>
            {motor.verified ? "verified" : "unverified"}
          </Badge>
          {motor.firmware_version && (
            <Badge variant="outline">fw {motor.firmware_version}</Badge>
          )}
          <Badge variant={fb ? (stale ? "warning" : "success") : "outline"}>
            {fb ? (stale ? `stale ${ageS?.toFixed(1)}s` : "live") : "no data"}
          </Badge>
          {!motor.travel_limits && (
            <Link
              to="/actuators/$role"
              params={{ role: motor.role }}
              search={{ tab: "travel" }}
              title="No software travel band has been configured for this actuator."
            >
              <Badge variant="warning">needs travel limits</Badge>
            </Link>
          )}
          <BootStateBadge motor={motor} />
        </div>
      </div>

      <div className="mt-3 grid grid-cols-2 gap-2 text-xs sm:grid-cols-4">
        <Stat label="position" value={fb?.mech_pos_rad} unit="rad" />
        <Stat label="velocity" value={fb?.mech_vel_rad_s} unit="rad/s" />
        <Stat label="vbus" value={fb?.vbus_v} unit="V" />
        <Stat
          label="temp"
          value={fb?.temp_c}
          unit="degC"
          tone={hot ? "warn" : undefined}
        />
      </div>
    </header>
  );
}

// Per-power-cycle gate badge. Drives off the discriminated `boot_state`
// union from MotorSummary; renders a colored pill plus, for OutOfBand,
// a tooltip with the offending position.
function BootStateBadge({ motor }: { motor: MotorSummary }) {
  const bs = motor.boot_state;
  const RAD_TO_DEG = 180 / Math.PI;
  if (bs.kind === "homed") {
    return <Badge variant="success">homed</Badge>;
  }
  if (bs.kind === "in_band") {
    return (
      <Link
        to="/actuators/$role"
        params={{ role: motor.role }}
        search={{ tab: "travel" }}
        title="In band but not yet homed; click to run Verify & Home."
      >
        <Badge variant="warning">needs verify &amp; home</Badge>
      </Link>
    );
  }
  if (bs.kind === "out_of_band") {
    const pos = (bs.mech_pos_rad * RAD_TO_DEG).toFixed(1);
    const lo = (bs.min_rad * RAD_TO_DEG).toFixed(1);
    const hi = (bs.max_rad * RAD_TO_DEG).toFixed(1);
    return (
      <Badge
        variant="destructive"
        title={`At ${pos}° outside [${lo}°, ${hi}°]; manual recovery required`}
      >
        out of band: {pos}°
      </Badge>
    );
  }
  if (bs.kind === "auto_recovering") {
    const from = (bs.from_rad * RAD_TO_DEG).toFixed(1);
    const target = (bs.target_rad * RAD_TO_DEG).toFixed(1);
    return (
      <Badge variant="warning" className="animate-pulse">
        auto-recovering {from}° → {target}°
      </Badge>
    );
  }
  return <Badge variant="outline">no telemetry</Badge>;
}

function Stat({
  label,
  value,
  unit,
  tone,
}: {
  label: string;
  value: number | undefined | null;
  unit: string;
  tone?: "warn";
}) {
  return (
    <div className="rounded-md border border-border/60 bg-background px-2 py-1.5">
      <div className="text-muted-foreground">{label}</div>
      <div
        className={
          "font-mono tabular-nums" +
          (tone === "warn" ? " text-amber-400" : "")
        }
      >
        {value == null ? "-" : value.toFixed(3)} {unit}
      </div>
    </div>
  );
}
