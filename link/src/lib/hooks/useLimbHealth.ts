import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/api";
import type { BootState } from "@/lib/types/BootState";
import type { MotorSummary } from "@/lib/types/MotorSummary";

export function effectiveLimbId(m: MotorSummary): string {
  return m.limb ?? m.role;
}

/** Matches `limb_health::quarantining_boot_state` in cortex. */
export function bootStateQuarantinesLimb(bs: BootState): boolean {
  switch (bs.kind) {
    case "out_of_band":
    case "offset_changed":
    case "home_failed":
      return true;
    default:
      return false;
  }
}

export type LimbHealthForMotion = {
  /** False when a *sibling* on the same limb is in a quarantining boot state. */
  healthy: boolean;
  /** Present motors on the shared limb (excluding `role`) that are quarantining. */
  quarantinedBy: { role: string; stateKind: string }[];
  limbId: string;
  /** Short explanation for tooltips on disabled controls. */
  blockReason: string | null;
};

/**
 * Client-side mirror of cortex's sibling-only limb quarantine so motion
 * buttons can disable early. Uses the shared `["motors"]` query cache.
 */
export function useLimbHealth(role: string): LimbHealthForMotion {
  const q = useQuery({
    queryKey: ["motors"],
    queryFn: () => api.listMotors(),
  });
  const motors = q.data;

  if (!motors?.length) {
    return {
      healthy: true,
      quarantinedBy: [],
      limbId: role,
      blockReason: null,
    };
  }

  const self = motors.find((m) => m.role === role);
  if (!self) {
    return {
      healthy: true,
      quarantinedBy: [],
      limbId: role,
      blockReason: null,
    };
  }

  const limbId = effectiveLimbId(self);
  const quarantinedBy: { role: string; stateKind: string }[] = [];

  for (const m of motors) {
    if (!m.present || m.role === role) continue;
    if (effectiveLimbId(m) !== limbId) continue;
    if (bootStateQuarantinesLimb(m.boot_state)) {
      quarantinedBy.push({
        role: m.role,
        stateKind: summarizeBootState(m.boot_state),
      });
    }
  }

  const healthy = quarantinedBy.length === 0;
  const blockReason = healthy
    ? null
    : `Limb ${limbId} quarantined — ${quarantinedBy.map((x) => `${x.role} (${x.stateKind})`).join(", ")}`;

  return { healthy, quarantinedBy, limbId, blockReason };
}

function summarizeBootState(bs: BootState): string {
  return bs.kind.replace(/_/g, " ");
}
