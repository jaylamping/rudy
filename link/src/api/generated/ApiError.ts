// Mirror of rudyd's `types::ApiError`.
// Source of truth: crates/rudyd/src/types.rs
export interface ApiError {
  error: string;
  detail: string | null;
}
