import type { ParamValue } from "./ParamValue";

// Mirror of rudyd's `types::ParamSnapshot`.
// Source of truth: crates/rudyd/src/types.rs
export interface ParamSnapshot {
  role: string;
  values: Record<string, ParamValue>;
}
