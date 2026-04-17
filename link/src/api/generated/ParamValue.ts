// Mirror of rudyd's `types::ParamValue`.
// Source of truth: crates/rudyd/src/types.rs
export interface ParamValue {
  name: string;
  index: number;
  type: string;
  units: string | null;
  value: unknown;
  hardware_range: [number, number] | null;
}
