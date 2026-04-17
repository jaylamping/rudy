import type { MotorFeedback } from "./MotorFeedback";

// Mirror of rudyd's `types::MotorSummary`.
// Source of truth: crates/rudyd/src/types.rs
export interface MotorSummary {
  role: string;
  can_bus: string;
  can_id: number;
  firmware_version: string | null;
  verified: boolean;
  latest: MotorFeedback | null;
}
