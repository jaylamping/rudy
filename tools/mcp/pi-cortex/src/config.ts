import process from "node:process";

/** SSH target, e.g. `jaylamping@rudy-pi`. */
export function sshTarget(): string {
  const full = process.env.PI_CORTEX_SSH?.trim();
  if (full) return full;
  const user = process.env.PI_SSH_USER?.trim() || "jaylamping";
  const host = process.env.PI_SSH_HOST?.trim() || "rudy-pi";
  return `${user}@${host}`;
}

export const defaultCortexBaseUrl = "http://127.0.0.1:8443";

export function cortexBaseUrl(): string {
  return process.env.PI_CORTEX_HTTP?.trim() || defaultCortexBaseUrl;
}

/** Dry-run: no SSH; tools return stub or throw with clear message. */
export function dryRun(): boolean {
  return process.env.PI_CORTEX_MCP_DRY_RUN === "1";
}
