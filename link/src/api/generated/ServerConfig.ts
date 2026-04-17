// Mirror of rudyd's `types::ServerConfig`.
// Source of truth: crates/rudyd/src/types.rs
export interface ServerConfig {
  version: string;
  actuator_model: string;
  webtransport: WebTransportAdvert;
  features: ServerFeatures;
}

export interface WebTransportAdvert {
  enabled: boolean;
  url: string | null;
}

export interface ServerFeatures {
  mock_can: boolean;
  require_verified: boolean;
}
