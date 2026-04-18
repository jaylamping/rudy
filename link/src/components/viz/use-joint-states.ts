// Derive a `Record<jointName, radians>` from the cached motors query.
//
// Phase 1 maps motor.role 1:1 to URDF joint name. This matches today's
// inventory + xacro: e.g. inventory has `l_shoulder_pitch` and the URDF
// joint is `l_shoulder_pitch_joint`. We strip the `_joint` suffix on the
// URDF side when applying angles, see UrdfViewer.applyJointStates.
//
// When more sophisticated mapping is needed (mirrored joints, gear ratios,
// derived joints), extend the inventory/spec with a `joint_name` field
// rather than baking heuristics here.

import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";
import { api } from "@/lib/api";
import type { MotorSummary } from "@/lib/types/MotorSummary";

export type JointStateMap = Record<string, number>;

export interface UseJointStatesResult {
  jointStates: JointStateMap;
  isLoading: boolean;
  isMock: boolean;
  staleness: { newestMs: number | null; oldestMs: number | null };
}

export function useJointStates(): UseJointStatesResult {
  const motorsQ = useQuery({
    queryKey: ["motors"],
    queryFn: () => api.listMotors(),
    refetchInterval: 1_000,
  });

  return useMemo(() => {
    const motors: MotorSummary[] = motorsQ.data ?? [];
    const jointStates: JointStateMap = {};
    let newest: number | null = null;
    let oldest: number | null = null;

    for (const m of motors) {
      const fb = m.latest;
      if (!fb) continue;
      jointStates[m.role] = fb.mech_pos_rad;
      const t = Number(fb.t_ms);
      if (newest === null || t > newest) newest = t;
      if (oldest === null || t < oldest) oldest = t;
    }

    return {
      jointStates,
      isLoading: motorsQ.isPending,
      // Reusing the motors query's data; we don't currently know if CAN is
      // mocked from this hook alone. Callers that care should also read
      // /api/config or accept the false-default.
      isMock: false,
      staleness: { newestMs: newest, oldestMs: oldest },
    };
  }, [motorsQ.data, motorsQ.isPending]);
}
