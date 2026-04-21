import { Link } from "@tanstack/react-router";
import { Settings2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { MotorChart } from "@/components/motor-chart";
import { radToDeg } from "@/lib/units";
import type { MotorSummary } from "@/lib/types/MotorSummary";

export function TelemetryGrid({ motors }: { motors: MotorSummary[] }) {
  return (
    <div className="grid gap-4 md:grid-cols-2">
      {motors.map((m) => (
        <MotorCard key={m.role} motor={m} />
      ))}
      {motors.length === 0 && (
        <div className="col-span-2 rounded-md border border-border bg-card p-6 text-sm text-muted-foreground">
          No motors in inventory.
        </div>
      )}
    </div>
  );
}

function MotorCard({ motor }: { motor: MotorSummary }) {
  const latest = motor.latest;
  return (
    <div className="rounded-lg border border-border bg-card p-4">
      <div className="mb-2 flex items-baseline justify-between">
        <div>
          <div className="font-medium">
            <Link
              to="/actuators/$role"
              params={{ role: motor.role }}
              className="hover:underline"
            >
              {motor.role}
            </Link>
          </div>
          <div className="text-xs text-muted-foreground">
            can_id 0x{motor.can_id.toString(16).padStart(2, "0").toUpperCase()}{" "}
            on {motor.can_bus}
          </div>
        </div>
        <div className="flex items-center gap-2">
          <Badge variant={motor.verified ? "success" : "warning"}>
            {motor.verified ? "verified" : "unverified"}
          </Badge>
          <Link
            to="/actuators/$role"
            params={{ role: motor.role }}
            aria-label={`Open ${motor.role} settings`}
            className="rounded-md border border-border p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
          >
            <Settings2 className="h-3.5 w-3.5" />
          </Link>
        </div>
      </div>
      <div className="mb-2">
        <MotorChart motor={motor} metric="pos" />
      </div>
      <dl className="grid grid-cols-4 gap-2 text-xs">
        <Stat
          label="pos"
          value={
            latest?.mech_pos_rad != null
              ? radToDeg(latest.mech_pos_rad)
              : undefined
          }
          unit="°"
        />
        <Stat
          label="vel"
          value={
            latest?.mech_vel_rad_s != null
              ? radToDeg(latest.mech_vel_rad_s)
              : undefined
          }
          unit="°/s"
        />
        <Stat label="vbus" value={latest?.vbus_v} unit="V" />
        <Stat label="temp" value={latest?.temp_c} unit="degC" />
      </dl>
    </div>
  );
}

function Stat({ label, value, unit }: { label: string; value: number | undefined; unit: string }) {
  return (
    <div className="rounded-md border border-border/60 bg-background px-2 py-1.5">
      <div className="text-muted-foreground">{label}</div>
      <div className="font-mono tabular-nums">
        {value === undefined ? "-" : value.toFixed(3)} {unit}
      </div>
    </div>
  );
}
