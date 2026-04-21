// Devices overview, organised by *what* a device is rather than *where* it
// lives:
//   1. Actuators   — 2x2 grid of limb cards (arms + legs), plus a single
//                    "Trunk & head" card when neck/waist/spine actuators are
//                    present, plus an "Other actuators" card for anything
//                    with a non-canonical `limb` value.
//   2. Sensors     — flat table of every sensor in inventory (cameras,
//                    lidar, IMU, force, gyro). Carries an optional `limb`
//                    column so sensors mounted on a specific assembly can
//                    show where they are.
//   3. Peripherals — 2-column grid of sub-cards (Audio, Lights, Display,
//                    Cooling) populated from the `Peripheral` device kind.
//                    Collapses to a single placeholder card when inventory
//                    has no peripherals yet.
//   4. Power       — flat table of every battery. Card description notes
//                    that bus / rail monitoring will land here as the power
//                    pipeline comes online.
//   5. Unassigned  — CAN-bus discovery / onboarding (devices not yet in
//                    inventory).
//
// Global boot health stays in the shell header (`GlobalActuatorHealthBar`).

import { createFileRoute, Link } from "@tanstack/react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Loader2, RefreshCw, Trash2 } from "lucide-react";
import { useState } from "react";
import { ConfirmDialog } from "@/components/params";
import { queryKeys } from "@/api";
import { ApiError, api } from "@/lib/api";
import { DevicesSection } from "@/components/devices/devices-section";
import { OnboardingWizard } from "@/components/devices/onboarding-wizard";
import { bootStateShortLabel } from "@/lib/bootStateUi";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { Device } from "@/lib/types/Device";
import type { MotorSummary } from "@/lib/types/MotorSummary";
import type { UnassignedDevice } from "@/lib/types/UnassignedDevice";

export const Route = createFileRoute("/_app/devices")({
  component: DevicesPage,
});

// The 2x2 grid of "real" limbs that get their own card.
const LIMB_GRID = ["right_arm", "left_arm", "right_leg", "left_leg"] as const;
type LimbId = (typeof LIMB_GRID)[number];

// Limbs that share the single "Trunk & head" card.
const TRUNK_LIMBS = ["head", "torso"] as const;
type TrunkLimbId = (typeof TRUNK_LIMBS)[number];

const LIMB_LABELS: Record<LimbId, string> = {
  right_arm: "Right arm",
  left_arm: "Left arm",
  right_leg: "Right leg",
  left_leg: "Left leg",
};

const LIMB_DESCRIPTIONS: Record<LimbId, string> = {
  right_arm: "Shoulder through gripper joints for the right arm.",
  left_arm: "Shoulder through gripper joints for the left arm.",
  right_leg: "Hip through ankle joints for the right leg.",
  left_leg: "Hip through ankle joints for the left leg.",
};

function isLimbId(value: string | null | undefined): value is LimbId {
  return !!value && (LIMB_GRID as readonly string[]).includes(value);
}

function isTrunkLimbId(value: string | null | undefined): value is TrunkLimbId {
  return !!value && (TRUNK_LIMBS as readonly string[]).includes(value);
}

type ActuatorDevice = Device & { kind: "actuator" };
type SensorDevice = Device & { kind: "sensor" };
type BatteryDevice = Device & { kind: "battery" };
type PeripheralDevice = Device & { kind: "peripheral" };

// Top-level grouping used by the Peripherals section. Each peripheral
// `family.kind` falls into exactly one of these buckets so the UI can
// render labelled sub-tables (Audio = mics + speakers, etc.).
type PeripheralGroupId = "audio" | "lights" | "display" | "cooling";

const PERIPHERAL_GROUP_LABELS: Record<PeripheralGroupId, string> = {
  audio: "Audio",
  lights: "Lights",
  display: "Display",
  cooling: "Cooling",
};

const PERIPHERAL_GROUP_DESCRIPTIONS: Record<PeripheralGroupId, string> = {
  audio: "Microphones and speakers.",
  lights: "Status LEDs and indicator strips.",
  display: "Operator-facing screens.",
  cooling: "Fans and thermal management.",
};

function peripheralGroup(p: PeripheralDevice): PeripheralGroupId {
  switch (p.family.kind) {
    case "microphone":
    case "speaker":
      return "audio";
    case "led":
      return "lights";
    case "display":
      return "display";
    case "fan":
      return "cooling";
  }
}

function DevicesPage() {
  const qc = useQueryClient();
  const poll = useLiveInterval({ live: 30_000, fallback: 2_000 });
  const [onboardTarget, setOnboardTarget] = useState<UnassignedDevice | null>(
    null,
  );
  const [onboardOpen, setOnboardOpen] = useState(false);
  const [removeTarget, setRemoveTarget] = useState<{
    role: string;
    can_bus: string;
    can_id: number;
  } | null>(null);

  const openOnboard = (d: UnassignedDevice) => {
    setOnboardTarget(d);
    setOnboardOpen(true);
  };

  const devicesQ = useQuery({
    queryKey: queryKeys.devices.all(),
    queryFn: () => api.listDevices(),
    refetchInterval: poll,
  });

  const motorsQ = useQuery({
    queryKey: queryKeys.motors.all(),
    queryFn: () => api.listMotors(),
    refetchInterval: poll,
  });

  const unassignedQ = useQuery({
    queryKey: queryKeys.devices.unassigned(),
    queryFn: () => api.listUnassignedHardware(),
    refetchInterval: poll,
  });

  const scanMut = useMutation({
    mutationFn: () => api.scanHardware({}),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: queryKeys.devices.unassigned() });
    },
  });
  const removeMut = useMutation({
    mutationFn: (target: { role: string }) => api.removeDevice(target.role),
    onSuccess: () => {
      void qc.invalidateQueries({ queryKey: queryKeys.devices.all() });
      void qc.invalidateQueries({ queryKey: queryKeys.motors.all() });
      void qc.invalidateQueries({ queryKey: queryKeys.devices.unassigned() });
      setRemoveTarget(null);
    },
  });

  const motorByRole = new Map<string, MotorSummary>();
  for (const row of motorsQ.data ?? []) {
    motorByRole.set(row.role, row);
  }

  // Split inventory into the four top-level buckets (actuators, sensors,
  // peripherals, batteries), plus an "other" pile for actuators whose
  // `limb` field is set but doesn't match one of our six canonical limbs
  // (typo, future limb, etc.). Peripherals get further sub-grouped by
  // family inside the Peripherals section.
  const arms_legs = new Map<LimbId, ActuatorDevice[]>();
  for (const id of LIMB_GRID) arms_legs.set(id, []);
  const trunk: ActuatorDevice[] = [];
  const otherActuators: ActuatorDevice[] = [];
  const sensors: SensorDevice[] = [];
  const batteries: BatteryDevice[] = [];
  const peripheralsByGroup = new Map<PeripheralGroupId, PeripheralDevice[]>([
    ["audio", []],
    ["lights", []],
    ["display", []],
    ["cooling", []],
  ]);
  for (const d of devicesQ.data ?? []) {
    if (d.kind === "actuator") {
      if (isLimbId(d.limb)) {
        arms_legs.get(d.limb)!.push(d);
      } else if (isTrunkLimbId(d.limb)) {
        trunk.push(d);
      } else {
        otherActuators.push(d);
      }
    } else if (d.kind === "sensor") {
      sensors.push(d);
    } else if (d.kind === "battery") {
      batteries.push(d);
    } else {
      peripheralsByGroup.get(peripheralGroup(d))!.push(d);
    }
  }
  const split = {
    arms_legs,
    trunk,
    otherActuators,
    sensors,
    batteries,
    peripheralsByGroup,
  };

  const unassigned = unassignedQ.data ?? [];

  const onRequestRemove = (a: ActuatorDevice) => {
    removeMut.reset();
    setRemoveTarget({
      role: a.role,
      can_bus: a.can_bus,
      can_id: a.can_id,
    });
  };
  const removePendingRole = removeMut.isPending
    ? (removeTarget?.role ?? null)
    : null;

  return (
    <div className="space-y-12">
      <OnboardingWizard
        open={onboardOpen}
        onOpenChange={(o) => {
          setOnboardOpen(o);
          if (!o) setOnboardTarget(null);
        }}
        device={onboardTarget}
      />
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Devices</h1>
        <p className="mt-1 text-sm text-muted-foreground">
          Everything in <code className="text-xs">inventory.yaml</code>, grouped
          by actuators, sensors, and power. Newly discovered CAN nodes show up
          under
          <em> Unassigned</em> at the bottom.
        </p>
      </div>

      {devicesQ.isPending ? (
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading devices…
        </div>
      ) : devicesQ.isError ? (
        <p className="text-sm text-destructive">Failed to load devices.</p>
      ) : (
        <>
          <ActuatorsSection
            armsLegs={split.arms_legs}
            trunk={split.trunk}
            other={split.otherActuators}
            motorByRole={motorByRole}
            onRequestRemove={onRequestRemove}
            removePendingRole={removePendingRole}
          />
          <SensorsSection sensors={split.sensors} />
          <PeripheralsSection peripheralsByGroup={split.peripheralsByGroup} />
          <PowerSection batteries={split.batteries} />
        </>
      )}

      <UnassignedSection
        rows={unassigned}
        scanPending={scanMut.isPending}
        onScan={() => scanMut.mutate()}
        scanMessage={scanMut.data?.message}
        onOnboard={openOnboard}
      />

      {removeTarget ? (
        <ConfirmDialog
          title="Remove actuator from inventory?"
          description={
            <div className="space-y-2">
              <p>
                Remove <code className="font-mono">{removeTarget.role}</code> (
                {removeTarget.can_bus} / 0x
                {removeTarget.can_id.toString(16).padStart(2, "0")}) from{" "}
                <code className="font-mono">inventory.yaml</code>?
              </p>
              <p>
                This motor will disappear from the limb view and must be
                onboarded again before control.
              </p>
              {removeMut.isError ? (
                <p className="text-xs text-destructive">
                  {describeApiError(removeMut.error)}
                </p>
              ) : null}
            </div>
          }
          confirmLabel={removeMut.isPending ? "Removing..." : "Remove actuator"}
          confirmVariant="destructive"
          onCancel={() => {
            if (!removeMut.isPending) setRemoveTarget(null);
          }}
          onConfirm={() => {
            if (removeMut.isPending) return;
            removeMut.mutate(removeTarget);
          }}
        />
      ) : null}
    </div>
  );
}

// ---------------------------------------------------------------------------
// Actuators
// ---------------------------------------------------------------------------

function ActuatorsSection({
  armsLegs,
  trunk,
  other,
  motorByRole,
  onRequestRemove,
  removePendingRole,
}: {
  armsLegs: Map<LimbId, ActuatorDevice[]>;
  trunk: ActuatorDevice[];
  other: ActuatorDevice[];
  motorByRole: Map<string, MotorSummary>;
  onRequestRemove: (a: ActuatorDevice) => void;
  removePendingRole: string | null;
}) {
  const totalArms = LIMB_GRID.reduce(
    (acc, id) => acc + (armsLegs.get(id)?.length ?? 0),
    0,
  );
  const total = totalArms + trunk.length + other.length;

  return (
    <DevicesSection
      title="Actuators"
      description={`${total} ${total === 1 ? "actuator" : "actuators"} in inventory, grouped by limb.`}
    >
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        {LIMB_GRID.map((id) => (
          <LimbActuatorCard
            key={id}
            title={LIMB_LABELS[id]}
            description={LIMB_DESCRIPTIONS[id]}
            actuators={armsLegs.get(id) ?? []}
            motorByRole={motorByRole}
            onRequestRemove={onRequestRemove}
            removePendingRole={removePendingRole}
          />
        ))}
      </div>

      {trunk.length > 0 ? (
        <div className="mt-4">
          <TrunkActuatorCard
            actuators={trunk}
            motorByRole={motorByRole}
            onRequestRemove={onRequestRemove}
            removePendingRole={removePendingRole}
          />
        </div>
      ) : null}

      {other.length > 0 ? (
        <div className="mt-4">
          <Card>
            <CardHeader>
              <CardTitle className="text-base">Other actuators</CardTitle>
              <CardDescription>
                Actuators whose <code className="text-xs">limb</code> field
                doesn't match one of the six canonical limbs.
              </CardDescription>
            </CardHeader>
            <CardContent>
              <ActuatorTable
                actuators={other}
                motorByRole={motorByRole}
                onRequestRemove={onRequestRemove}
                removePendingRole={removePendingRole}
                showLimbColumn
              />
            </CardContent>
          </Card>
        </div>
      ) : null}
    </DevicesSection>
  );
}

function LimbActuatorCard({
  title,
  description,
  actuators,
  motorByRole,
  onRequestRemove,
  removePendingRole,
}: {
  title: string;
  description: string;
  actuators: ActuatorDevice[];
  motorByRole: Map<string, MotorSummary>;
  onRequestRemove: (a: ActuatorDevice) => void;
  removePendingRole: string | null;
}) {
  return (
    <Card className="flex flex-col">
      <CardHeader className="flex flex-row items-baseline justify-between gap-3 space-y-0">
        <div className="space-y-1">
          <CardTitle className="text-base">{title}</CardTitle>
          <CardDescription>{description}</CardDescription>
        </div>
        <Badge variant="secondary" className="font-normal">
          {actuators.length}
        </Badge>
      </CardHeader>
      <CardContent>
        {actuators.length === 0 ? (
          <p className="rounded-md border border-dashed border-border bg-muted/20 px-3 py-6 text-center text-sm text-muted-foreground">
            No actuators assigned to this limb yet.
          </p>
        ) : (
          <ActuatorTable
            actuators={actuators}
            motorByRole={motorByRole}
            onRequestRemove={onRequestRemove}
            removePendingRole={removePendingRole}
          />
        )}
      </CardContent>
    </Card>
  );
}

function TrunkActuatorCard({
  actuators,
  motorByRole,
  onRequestRemove,
  removePendingRole,
}: {
  actuators: ActuatorDevice[];
  motorByRole: Map<string, MotorSummary>;
  onRequestRemove: (a: ActuatorDevice) => void;
  removePendingRole: string | null;
}) {
  return (
    <Card>
      <CardHeader className="flex flex-row items-baseline justify-between gap-3 space-y-0">
        <div className="space-y-1">
          <CardTitle className="text-base">Trunk &amp; head</CardTitle>
          <CardDescription>
            Neck, waist, and spine actuators that aren't part of an arm or leg.
          </CardDescription>
        </div>
        <Badge variant="secondary" className="font-normal">
          {actuators.length}
        </Badge>
      </CardHeader>
      <CardContent>
        <ActuatorTable
          actuators={actuators}
          motorByRole={motorByRole}
          onRequestRemove={onRequestRemove}
          removePendingRole={removePendingRole}
          showLimbColumn
        />
      </CardContent>
    </Card>
  );
}

function ActuatorTable({
  actuators,
  motorByRole,
  onRequestRemove,
  removePendingRole,
  showLimbColumn = false,
}: {
  actuators: ActuatorDevice[];
  motorByRole: Map<string, MotorSummary>;
  onRequestRemove: (a: ActuatorDevice) => void;
  removePendingRole: string | null;
  showLimbColumn?: boolean;
}) {
  return (
    <div className="overflow-x-auto rounded-md border border-border">
      <table className="w-full min-w-[560px] text-left text-sm">
        <thead className="border-b border-border bg-muted/30">
          <tr>
            <th className="px-3 py-2 font-medium">Role</th>
            {showLimbColumn ? (
              <th className="px-3 py-2 font-medium">Limb</th>
            ) : null}
            <th className="px-3 py-2 font-medium">Bus</th>
            <th className="px-3 py-2 font-medium">ID</th>
            <th className="px-3 py-2 font-medium">Family</th>
            <th className="px-3 py-2 font-medium">Boot</th>
            <th className="px-3 py-2 text-right font-medium">Actions</th>
          </tr>
        </thead>
        <tbody>
          {actuators.map((a) => {
            const motor = motorByRole.get(a.role);
            const boot = motor ? bootStateShortLabel(motor.boot_state) : "—";
            const isEnabled = motor?.enabled ?? false;
            const fam =
              a.family.kind === "robstride"
                ? `RobStride ${a.family.model}`
                : a.family.kind;
            const removeBtn = (
              <Button
                variant="ghost"
                size="sm"
                className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                aria-label={`Remove ${a.role}`}
                onClick={() => onRequestRemove(a)}
                disabled={removePendingRole === a.role || isEnabled}
              >
                <Trash2 className="h-4 w-4" />
              </Button>
            );
            return (
              <tr
                key={a.role}
                className="border-b border-border/50 last:border-b-0"
              >
                <td className="px-3 py-2">
                  <Link
                    to="/actuators/$role"
                    params={{ role: a.role }}
                    className="font-medium hover:underline"
                  >
                    {a.role}
                  </Link>
                </td>
                {showLimbColumn ? (
                  <td className="px-3 py-2 text-muted-foreground">
                    {a.limb ?? "—"}
                  </td>
                ) : null}
                <td className="px-3 py-2 font-mono text-xs">{a.can_bus}</td>
                <td className="px-3 py-2 font-mono text-xs">
                  0x{a.can_id.toString(16).padStart(2, "0")}
                </td>
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
                  {isEnabled ? (
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <span className="inline-flex">{removeBtn}</span>
                      </TooltipTrigger>
                      <TooltipContent side="top">
                        Stop the motor first.
                      </TooltipContent>
                    </Tooltip>
                  ) : (
                    removeBtn
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

// ---------------------------------------------------------------------------
// Sensors
// ---------------------------------------------------------------------------

function SensorsSection({ sensors }: { sensors: SensorDevice[] }) {
  return (
    <DevicesSection
      title="Sensors"
      description={`${sensors.length} ${sensors.length === 1 ? "sensor" : "sensors"} in inventory.`}
    >
      <Card>
        <CardContent className="pt-4">
          {sensors.length === 0 ? (
            <p className="rounded-md border border-dashed border-border bg-muted/20 px-3 py-6 text-center text-sm text-muted-foreground">
              No sensors in inventory yet.
            </p>
          ) : (
            <div className="overflow-x-auto rounded-md border border-border">
              <table className="w-full min-w-[560px] text-left text-sm">
                <thead className="border-b border-border bg-muted/30">
                  <tr>
                    <th className="px-3 py-2 font-medium">Role</th>
                    <th className="px-3 py-2 font-medium">Limb</th>
                    <th className="px-3 py-2 font-medium">Family</th>
                    <th className="px-3 py-2 font-medium">Bus</th>
                    <th className="px-3 py-2 font-medium">ID</th>
                    <th className="px-3 py-2 font-medium">Note</th>
                  </tr>
                </thead>
                <tbody>
                  {sensors.map((s) => (
                    <tr
                      key={`sensor-${s.role}`}
                      className="border-b border-border/50 last:border-b-0"
                    >
                      <td className="px-3 py-2 font-mono text-xs">{s.role}</td>
                      <td className="px-3 py-2 text-muted-foreground">
                        {s.limb ?? "—"}
                      </td>
                      <td className="px-3 py-2 text-muted-foreground">
                        {`${s.family.kind} (${s.family.model})`}
                      </td>
                      <td className="px-3 py-2 font-mono text-xs">
                        {s.can_bus}
                      </td>
                      <td className="px-3 py-2 font-mono text-xs">
                        0x{s.can_id.toString(16).padStart(2, "0")}
                      </td>
                      <td className="px-3 py-2 text-muted-foreground">
                        Sensor pipeline not wired yet — configuration UI coming
                        soon.
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </CardContent>
      </Card>
    </DevicesSection>
  );
}

// ---------------------------------------------------------------------------
// Peripherals (audio, lights, display, cooling)
// ---------------------------------------------------------------------------

function PeripheralsSection({
  peripheralsByGroup,
}: {
  peripheralsByGroup: Map<PeripheralGroupId, PeripheralDevice[]>;
}) {
  const groups: PeripheralGroupId[] = ["audio", "lights", "display", "cooling"];
  const total = groups.reduce(
    (acc, g) => acc + (peripheralsByGroup.get(g)?.length ?? 0),
    0,
  );

  return (
    <DevicesSection
      title="Peripherals"
      description={`${total} ${total === 1 ? "peripheral" : "peripherals"} in inventory — audio, lights, display, and cooling hardware.`}
    >
      {total === 0 ? (
        <Card>
          <CardContent className="pt-4">
            <p className="rounded-md border border-dashed border-border bg-muted/20 px-3 py-6 text-center text-sm text-muted-foreground">
              No peripherals in inventory yet. Microphones, speakers, status
              LEDs, displays, and fans will appear here as they're added to{" "}
              <code className="text-xs">inventory.yaml</code>.
            </p>
          </CardContent>
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
          {groups.map((g) => {
            const rows = peripheralsByGroup.get(g) ?? [];
            if (rows.length === 0) return null;
            return <PeripheralGroupCard key={g} group={g} rows={rows} />;
          })}
        </div>
      )}
    </DevicesSection>
  );
}

function PeripheralGroupCard({
  group,
  rows,
}: {
  group: PeripheralGroupId;
  rows: PeripheralDevice[];
}) {
  return (
    <Card className="flex flex-col">
      <CardHeader className="flex flex-row items-baseline justify-between gap-3 space-y-0">
        <div className="space-y-1">
          <CardTitle className="text-base">
            {PERIPHERAL_GROUP_LABELS[group]}
          </CardTitle>
          <CardDescription>
            {PERIPHERAL_GROUP_DESCRIPTIONS[group]}
          </CardDescription>
        </div>
        <Badge variant="secondary" className="font-normal">
          {rows.length}
        </Badge>
      </CardHeader>
      <CardContent>
        <div className="overflow-x-auto rounded-md border border-border">
          <table className="w-full min-w-[480px] text-left text-sm">
            <thead className="border-b border-border bg-muted/30">
              <tr>
                <th className="px-3 py-2 font-medium">Role</th>
                <th className="px-3 py-2 font-medium">Family</th>
                <th className="px-3 py-2 font-medium">Limb</th>
                <th className="px-3 py-2 font-medium">Bus</th>
                <th className="px-3 py-2 font-medium">ID</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((p) => (
                <tr
                  key={`peripheral-${p.role}`}
                  className="border-b border-border/50 last:border-b-0"
                >
                  <td className="px-3 py-2 font-mono text-xs">{p.role}</td>
                  <td className="px-3 py-2 text-muted-foreground">
                    {`${p.family.kind} (${p.family.model})`}
                  </td>
                  <td className="px-3 py-2 text-muted-foreground">
                    {p.limb ?? "—"}
                  </td>
                  <td className="px-3 py-2 font-mono text-xs">{p.can_bus}</td>
                  <td className="px-3 py-2 font-mono text-xs">
                    0x{p.can_id.toString(16).padStart(2, "0")}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </CardContent>
    </Card>
  );
}

// ---------------------------------------------------------------------------
// Power
// ---------------------------------------------------------------------------

function PowerSection({ batteries }: { batteries: BatteryDevice[] }) {
  return (
    <DevicesSection
      title="Power"
      description={`${batteries.length} ${batteries.length === 1 ? "battery" : "batteries"} in inventory.`}
    >
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Batteries</CardTitle>
          <CardDescription>
            All packs in inventory. Bus and rail monitoring will land in this
            section as the power pipeline comes online.
          </CardDescription>
        </CardHeader>
        <CardContent>
          {batteries.length === 0 ? (
            <p className="rounded-md border border-dashed border-border bg-muted/20 px-3 py-6 text-center text-sm text-muted-foreground">
              No batteries in inventory yet.
            </p>
          ) : (
            <div className="overflow-x-auto rounded-md border border-border">
              <table className="w-full min-w-[480px] text-left text-sm">
                <thead className="border-b border-border bg-muted/30">
                  <tr>
                    <th className="px-3 py-2 font-medium">Role</th>
                    <th className="px-3 py-2 font-medium">Family</th>
                    <th className="px-3 py-2 font-medium">Bus</th>
                    <th className="px-3 py-2 font-medium">ID</th>
                    <th className="px-3 py-2 font-medium">Note</th>
                  </tr>
                </thead>
                <tbody>
                  {batteries.map((b) => (
                    <tr
                      key={`battery-${b.role}`}
                      className="border-b border-border/50 last:border-b-0"
                    >
                      <td className="px-3 py-2 font-mono text-xs">{b.role}</td>
                      <td className="px-3 py-2 text-muted-foreground">
                        {b.family.kind}
                      </td>
                      <td className="px-3 py-2 font-mono text-xs">
                        {b.can_bus}
                      </td>
                      <td className="px-3 py-2 font-mono text-xs">
                        0x{b.can_id.toString(16).padStart(2, "0")}
                      </td>
                      <td className="px-3 py-2 text-muted-foreground">
                        Battery management UI coming soon.
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </CardContent>
      </Card>
    </DevicesSection>
  );
}

// ---------------------------------------------------------------------------
// Unassigned (CAN bus discovery)
// ---------------------------------------------------------------------------

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
    <DevicesSection
      title="Unassigned"
      description="CAN IDs seen on the bus or returned by Discover that are not in inventory yet."
    >
      <div className="mb-4 flex flex-wrap items-center gap-2">
        <Button
          type="button"
          variant="secondary"
          size="sm"
          disabled={scanPending}
          onClick={onScan}
        >
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
          No unassigned CAN IDs yet. Click Discover to run an active scan on the
          robot, or wait for the daemon to observe traffic (type-2 / type-17
          frames). Then use Onboard to add a device to{" "}
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
                <tr
                  key={`${r.bus}-${r.can_id}`}
                  className="border-b border-border/50 last:border-b-0"
                >
                  <td className="px-3 py-2 font-mono text-xs">{r.bus}</td>
                  <td className="px-3 py-2 font-mono text-xs">
                    0x{r.can_id.toString(16).padStart(2, "0")}
                  </td>
                  <td className="px-3 py-2">{r.source}</td>
                  <td className="px-3 py-2 text-muted-foreground">
                    {r.family_hint ?? "—"}
                  </td>
                  <td className="px-3 py-2 text-muted-foreground">
                    {r.last_seen_ms}
                  </td>
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
    </DevicesSection>
  );
}

function describeApiError(error: unknown): string {
  if (error instanceof ApiError) {
    const detail =
      error.body && typeof error.body === "object" && "detail" in error.body
        ? String((error.body as { detail?: unknown }).detail ?? "")
        : "";
    return detail || error.message;
  }
  return "Failed to remove actuator.";
}
