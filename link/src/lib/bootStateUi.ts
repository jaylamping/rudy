import type { BootState } from "@/lib/types/BootState";
import type { MotorSummary } from "@/lib/types/MotorSummary";

/** Lower = more urgent (dashboard sort: failures first). */
export function bootStateSortRank(bs: BootState): number {
  switch (bs.kind) {
    case "offset_changed":
      return 0;
    case "home_failed":
      return 1;
    case "unknown":
      return 2;
    case "out_of_band":
      return 3;
    case "auto_homing":
      return 4;
    case "in_band":
      return 5;
    case "homed":
      return 6;
    default:
      return 5;
  }
}

export function bootStateShortLabel(bs: BootState): string {
  switch (bs.kind) {
    case "homed":
      return "Homed";
    case "in_band":
      return "In band";
    case "unknown":
      return "Unknown";
    case "out_of_band":
      return "Out of band";
    case "auto_homing":
      return "Auto-homing";
    case "offset_changed":
      return "Offset changed";
    case "home_failed":
      return "Home failed";
    default:
      return "Unknown";
  }
}

/** Tailwind classes for the motor role label on the dashboard. */
export function bootStateRoleTextClass(bs: BootState): string {
  switch (bs.kind) {
    case "homed":
      return "text-emerald-400";
    case "auto_homing":
      return "text-sky-400";
    case "out_of_band":
    case "home_failed":
      return "text-amber-400";
    case "offset_changed":
      return "text-rose-400";
    case "in_band":
    case "unknown":
      return "text-muted-foreground";
    default:
      return "text-foreground";
  }
}

export function bootStateDotClass(bs: BootState): string {
  switch (bs.kind) {
    case "homed":
      return "bg-emerald-400";
    case "auto_homing":
      return "bg-sky-400";
    case "out_of_band":
    case "home_failed":
      return "bg-amber-400";
    case "offset_changed":
      return "bg-rose-500";
    case "in_band":
    case "unknown":
      return "bg-muted-foreground/60";
    default:
      return "bg-muted-foreground/40";
  }
}

export function isCriticalBootState(bs: BootState): boolean {
  return (
    bs.kind === "offset_changed" ||
    bs.kind === "home_failed" ||
    bs.kind === "out_of_band" ||
    bs.kind === "unknown"
  );
}

/** Suggested actuator tab for deep links from health summaries. */
export function suggestedActuatorTab(
  bs: BootState,
): "overview" | "travel" | "controls" {
  switch (bs.kind) {
    case "offset_changed":
      return "controls";
    case "home_failed":
    case "in_band":
    case "out_of_band":
      return "travel";
    default:
      return "overview";
  }
}

export function motorDisplayLabel(m: MotorSummary): string {
  return m.limb ? `${m.limb}.${m.role}` : m.role;
}
