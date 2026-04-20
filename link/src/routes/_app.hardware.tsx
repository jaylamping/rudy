// Hardware overview: inventory (Assigned) and bus discovery (Unassigned).
// Global boot health stays in the shell header (`GlobalActuatorHealthBar`).

import { createFileRoute, Link } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";
import { api } from "@/lib/api";
import { HardwareSection } from "@/components/hardware/hardware-section";
import { OnboardingWizard } from "@/components/hardware/onboarding-wizard";
import { bootStateShortLabel } from "@/lib/bootStateUi";
import { Badge } from "@/components/ui/badge";
import { Button, buttonVariants } from "@/components/ui/button";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import { cn } from "@/lib/utils";
import type { Device } from "@/lib/types/Device";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import type { UnassignedDevice } from "@/lib/types/UnassignedDevice";

export const Route = createFileRoute("/_app/hardware")({
  component: HardwarePage,
});

function HardwarePage() {
  const qc = useQueryClient();
  const poll = useLiveInterval({ live: 30_000, fallback: 2_000 });
  const [onboardTarget, setOnboardTarget] = useState<UnassignedDevice | null>(null);
  const [onboardOpen, setOnboardOpen] = useState(false);

  const openOnboard = (d: UnassignedDevice) => {
    setOnboardTarget(d);
    setOnboardOpen(true);
  };

  const devicesQ = useQuery({
    queryKey: ["devices"],
    queryFn: () => api.listDevices(),
    refetchInterval: poll,
  });

  const motorsQ = useQuery({
    queryKey: ["motors"],
    queryFn: () => api.listMotors(),
    refetchInterval: poll,
  });

  const unassignedQ = useQuery({
    queryKey: ["hardware", "unassigned"],
    queryFn: () => api.listUnassignedHardware(),
    refetchInterval: poll,
  });

  const scanMut = useMutation({
    mutationFn: () => api.scanHardware({}),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: ["hardware", "unassigned"] });
    },
  });

  const motorByRole = useMemo(() => {
    const m = new Map<string, MotorSummary>();
    for (const row of motorsQ.data ?? []) {
      m.set(row.role, row);
    }
    return m;
  }, [motorsQ.data]);

  const grouped = useMemo(() => {
    const actuators: Device[] = [];
    const sensors: Device[] = [];
    const batteries: Device[] = [];
    for (const d of devicesQ.data ?? []) {
      if (d.kind === "actuator") actuators.push(d);
      else if (d.kind === "sensor") sensors.push(d);
      else batteries.push(d);
    }
    return { actuators, sensors, batteries };
  }, [devicesQ.data]);

  const unassigned = unassignedQ.data ?? [];
  const unassignedFirst = unassigned.length > 0;

  return (
    <div className="space-y-8">
      <OnboardingWizard
        open={onboardOpen}
        onOpenChange={(o) => {
          setOnboardOpen(o);
          if (!o) setOnboardTarget(null);
        }}
        device={onboardTarget}
      />
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Hardware</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Assigned inventory and devices seen on the bus that are not yet in{" "}
          <code className="text-xs">inventory.yaml</code>.
        </p>
      </div>

      {unassignedFirst ? (
        <UnassignedSection
          rows={unassigned}
          scanPending={scanMut.isPending}
          onScan={() => scanMut.mutate()}
          scanMessage={scanMut.data?.message}
          onOnboard={openOnboard}
        />
      ) : null}

      <HardwareSection
        title="Assigned"
        description="Everything in the current inventory file, grouped by kind."
      >
        {devicesQ.isPending ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            Loading devices…
          </div>
        ) : devicesQ.isError ? (
          <p className="text-sm text-destructive">Failed to load devices.</p>
        ) : (
          <div className="space-y-8">
            <AssignedActuatorsTable
              devices={grouped.actuators}
              motorByRole={motorByRole}
            />
            <AssignedPlaceholderTable title="Sensors" rows={grouped.sensors} kind="sensor" />
            <AssignedPlaceholderTable title="Batteries" rows={grouped.batteries} kind="battery" />
          </div>
        )}
      </HardwareSection>

      {!unassignedFirst ? (
        <UnassignedSection
          rows={unassigned}
          scanPending={scanMut.isPending}
          onScan={() => scanMut.mutate()}
          scanMessage={scanMut.data?.message}
          onOnboard={openOnboard}
        />
      ) : null}
    </div>
  );
}

function UnassignedSection({
  rows,
  scanPending,
  onScan,
  scanMessage,
  onOnboard,
}: {
  rows: UnassignedDevice[];
  scanPending: boolean;
  onScan: () => void;
  scanMessage?: string;
  onOnboard: (d: UnassignedDevice) => void;
}) {
  return (
    <HardwareSection
      title="Unassigned"
      description="CAN IDs seen on the bus or returned by Discover that are not in inventory yet."
    >
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <Button type="button" variant="secondary" size="sm" disabled={scanPending} onClick={onScan}>
          {scanPending ? (
            <Loader2 className="mr-2 h-4 w-4 animate-spin" />
          ) : (
            <RefreshCw className="mr-2 h-4 w-4" />
          )}
          Discover
        </Button>
        {scanMessage ? (
          <span className="text-xs text-muted-foreground">{scanMessage}</span>
        ) : null}
      </div>
      {rows.length === 0 ? (
        <p className="rounded-md border border-dashed border-border bg-muted/30 px-4 py-6 text-sm text-muted-foreground">
          No unassigned CAN IDs yet. Click Discover to run an active scan on the robot, or wait for
          the daemon to observe traffic (type-2 / type-17 frames). Then use Onboard to add a device to{" "}
          <code className="text-xs">inventory.yaml</code>.
        </p>
      ) : (
        <div className="overflow-x-auto rounded-md border border-border">
          <table className="w-full min-w-[640px] text-left text-sm">
            <thead className="border-b border-border bg-muted/30">
              <tr>
                <th className="px-3 py-2 font-medium">Bus</th>
                <th className="px-3 py-2 font-medium">CAN ID</th>
                <th className="px-3 py-2 font-medium">Source</th>
                <th className="px-3 py-2 font-medium">Family hint</th>
                <th className="px-3 py-2 font-medium">Last seen</th>
                <th className="px-3 py-2 font-medium"> </th>
              </tr>
            </thead>
            <tbody>
              {rows.map((r) => (
                <tr key={`${r.bus}-${r.can_id}`} className="border-b border-border/50">
                  <td className="px-3 py-2 font-mono text-xs">{r.bus}</td>
                  <td className="px-3 py-2 font-mono text-xs">0x{r.can_id.toString(16).padStart(2, "0")}</td>
                  <td className="px-3 py-2">{r.source}</td>
                  <td className="px-3 py-2 text-muted-foreground">{r.family_hint ?? "—"}</td>
                  <td className="px-3 py-2 text-muted-foreground">{r.last_seen_ms}</td>
                  <td className="px-3 py-2">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => onOnboard(r)}
                    >
                      Onboard
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </HardwareSection>
  );
}

function AssignedActuatorsTable({
  devices,
  motorByRole,
}: {
  devices: Device[];
  motorByRole: Map<string, MotorSummary>;
}) {
  const actuators = devices.filter((d): d is Device & { kind: "actuator" } => d.kind === "actuator");

  if (actuators.length === 0) {
    return (
      <div>
        <h3 className="mb-2 text-sm font-medium text-muted-foreground">Actuators</h3>
        <p className="text-sm text-muted-foreground">No actuators in inventory.</p>
      </div>
    );
  }

  const byLimb = new Map<string, typeof actuators>();
  const ungrouped: typeof actuators = [];
  for (const a of actuators) {
    const limb = a.limb;
    if (limb) {
      const list = byLimb.get(limb) ?? [];
      list.push(a);
      byLimb.set(limb, list);
    } else {
      ungrouped.push(a);
    }
  }
  const limbNames = [...byLimb.keys()].sort();

  return (
    <div className="space-y-6">
      <h3 className="text-sm font-medium text-muted-foreground">Actuators</h3>
      {limbNames.map((limb) => (
        <div key={limb}>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            {limb}
          </h4>
          <ActuatorTableRows actuators={byLimb.get(limb)!} motorByRole={motorByRole} />
        </div>
      ))}
      {ungrouped.length > 0 ? (
        <div>
          <h4 className="mb-2 text-xs font-semibold uppercase tracking-wide text-muted-foreground">
            Ungrouped
          </h4>
          <ActuatorTableRows actuators={ungrouped} motorByRole={motorByRole} />
        </div>
      ) : null}
    </div>
  );
}

function ActuatorTableRows({
  actuators,
  motorByRole,
}: {
  actuators: (Device & { kind: "actuator" })[];
  motorByRole: Map<string, MotorSummary>;
}) {
  return (
    <div className="overflow-x-auto rounded-md border border-border">
      <table className="w-full min-w-[720px] text-left text-sm">
        <thead className="border-b border-border bg-muted/30">
          <tr>
            <th className="px-3 py-2 font-medium">Role</th>
            <th className="px-3 py-2 font-medium">Bus</th>
            <th className="px-3 py-2 font-medium">ID</th>
            <th className="px-3 py-2 font-medium">Family</th>
            <th className="px-3 py-2 font-medium">Boot</th>
            <th className="px-3 py-2 font-medium"> </th>
          </tr>
        </thead>
        <tbody>
          {actuators.map((a) => {
            const motor = motorByRole.get(a.role);
            const boot = motor ? bootStateShortLabel(motor.boot_state) : "—";
            const fam =
              a.family.kind === "robstride"
                ? `RobStride ${a.family.model}`
                : a.family.kind;
            return (
              <tr key={a.role} className="border-b border-border/50">
                <td className="px-3 py-2 font-medium">{a.role}</td>
                <td className="px-3 py-2 font-mono text-xs">{a.can_bus}</td>
                <td className="px-3 py-2 font-mono text-xs">0x{a.can_id.toString(16).padStart(2, "0")}</td>
                <td className="px-3 py-2 text-muted-foreground">{fam}</td>
                <td className="px-3 py-2">
                  {motor ? (
                    <Badge variant="secondary" className="font-normal">
                      {boot}
                    </Badge>
                  ) : (
                    "—"
                  )}
                </td>
                <td className="px-3 py-2 text-right">
                  <Link
                    to="/actuators/$role"
                    params={{ role: a.role }}
                    className={cn(buttonVariants({ variant: "link", size: "sm" }), "h-auto px-0")}
                  >
                    Open
                  </Link>
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function AssignedPlaceholderTable({
  title,
  rows,
  kind,
}: {
  title: string;
  rows: Device[];
  kind: "sensor" | "battery";
}) {
  if (rows.length === 0) {
    return (
      <div>
        <h3 className="mb-2 text-sm font-medium text-muted-foreground">{title}</h3>
        <p className="text-sm text-muted-foreground">No {kind} entries in inventory.</p>
      </div>
    );
  }
  return (
    <div>
      <h3 className="mb-2 text-sm font-medium text-muted-foreground">{title}</h3>
      <div className="overflow-x-auto rounded-md border border-border">
        <table className="w-full min-w-[480px] text-left text-sm">
          <thead className="border-b border-border bg-muted/30">
            <tr>
              <th className="px-3 py-2 font-medium">Role</th>
              <th className="px-3 py-2 font-medium">Bus</th>
              <th className="px-3 py-2 font-medium">ID</th>
              <th className="px-3 py-2 font-medium">Note</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((d) => {
              if (d.kind !== "sensor" && d.kind !== "battery") return null;
              const role = d.role;
              const bus = d.can_bus;
              const id = d.can_id;
              return (
                <tr key={`${kind}-${role}`} className="border-b border-border/50">
                  <td className="px-3 py-2 font-mono text-xs">{role}</td>
                  <td className="px-3 py-2 font-mono text-xs">{bus}</td>
                  <td className="px-3 py-2 font-mono text-xs">0x{id.toString(16).padStart(2, "0")}</td>
                  <td className="px-3 py-2 text-muted-foreground">{placeholderNote(kind)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function placeholderNote(kind: "sensor" | "battery") {
  return kind === "sensor"
    ? "Sensor pipeline not wired yet — configuration UI coming soon."
    : "Battery management UI coming soon.";
}
