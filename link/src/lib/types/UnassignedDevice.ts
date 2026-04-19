/** Wire shape for `GET /api/hardware/unassigned` (matches `rudydae::api::hardware::UnassignedDevice`). */
export type UnassignedDevice = {
  bus: string;
  can_id: number;
  source: string;
  first_seen_ms: number;
  last_seen_ms: number;
  family_hint?: string | null;
  identification_payload?: unknown;
};
