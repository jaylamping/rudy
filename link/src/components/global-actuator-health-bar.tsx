import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { queryKeys } from "@/api";
import { api } from "@/lib/api";
import {
  bootStateShortLabel,
  isCriticalBootState,
  motorDisplayLabel,
  suggestedActuatorTab,
} from "@/lib/bootStateUi";
import { useLiveInterval } from "@/lib/hooks/useLiveInterval";
import { cn } from "@/lib/utils";
import type { MotorSummary } from "@/lib/types/MotorSummary";

const MAX_LINKS = 5;

/**
 * Sticky header summary of actuator boot health. Uses the same `queryKeys.motors.all()`
 * query as the dashboard (no extra polling pattern).
 */
export function GlobalActuatorHealthBar() {
  const q = useQuery({
    queryKey: queryKeys.motors.all(),
    queryFn: () => api.listMotors(),
    refetchInterval: useLiveInterval({ live: 30_000, fallback: 2_000 }),
  });

  const motors = q.data;
  if (q.isPending || !motors?.length) {
    return (
      <div className="min-w-0 text-xs text-muted-foreground">Actuators…</div>
    );
  }

  const critical = motors.filter((m) => isCriticalBootState(m.boot_state));
  const notHomed = motors.filter((m) => m.boot_state.kind !== "homed");
  const homing = motors.filter((m) => m.boot_state.kind === "auto_homing");

  if (critical.length > 0) {
    return (
      <div className="min-w-0 text-xs">
        <span className="font-medium text-amber-400">
          {critical.length} issue{critical.length === 1 ? "" : "s"}:{" "}
        </span>
        <span className="text-muted-foreground">
          {critical.slice(0, MAX_LINKS).map((m, i) => (
            <span key={m.role}>
              {i > 0 ? ", " : ""}
              <ActuatorHealthLink motor={m} />
            </span>
          ))}
          {critical.length > MAX_LINKS && (
            <span className="text-muted-foreground">
              {" "}
              +{critical.length - MAX_LINKS} more
            </span>
          )}
        </span>
      </div>
    );
  }

  if (homing.length > 0) {
    return (
      <div className="min-w-0 text-xs text-sky-300/90">
        <span className="font-medium">Auto-homing: </span>
        {homing.slice(0, MAX_LINKS).map((m, i) => (
          <span key={m.role}>
            {i > 0 ? ", " : ""}
            <ActuatorHealthLink motor={m} />
          </span>
        ))}
        {homing.length > MAX_LINKS && (
          <span className="text-muted-foreground">
            {" "}
            +{homing.length - MAX_LINKS} more
          </span>
        )}
      </div>
    );
  }

  if (notHomed.length > 0) {
    return (
      <div className="min-w-0 text-xs text-muted-foreground">
        <span className="text-foreground/80">
          {notHomed.length} not homed
        </span>
        {" · "}
        <Link to="/" className="underline-offset-2 hover:text-foreground hover:underline">
          dashboard
        </Link>
      </div>
    );
  }

  return (
    <div className={cn("min-w-0 text-xs font-medium text-emerald-400/90")}>
      ✓ All {motors.length} actuator{motors.length === 1 ? "" : "s"} homed
    </div>
  );
}

function ActuatorHealthLink({ motor }: { motor: MotorSummary }) {
  const tab = suggestedActuatorTab(motor.boot_state);
  const label = motorDisplayLabel(motor);
  const kind = bootStateShortLabel(motor.boot_state);
  return (
    <Link
      to="/actuators/$role"
      params={{ role: motor.role }}
      search={{ tab }}
      className="text-foreground underline-offset-2 hover:text-sky-200 hover:underline"
    >
      {label} ({kind})
    </Link>
  );
}
