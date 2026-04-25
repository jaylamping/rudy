import { spawn } from "node:child_process";

export type RunSshOptions = {
  /** Max wall time (ms). */
  timeoutMs: number;
  /** Max combined stdout+stderr bytes; process killed with SIGKILL if exceeded. */
  maxBytes: number;
};

export type RunSshResult = {
  code: number | null;
  signal: NodeJS.Signals | null;
  stdout: string;
  stderr: string;
  truncated: boolean;
  timedOut: boolean;
};

const DEFAULT_TIMEOUT_MS = 60_000;
const DEFAULT_MAX_BYTES = 2 * 1024 * 1024;

/**
 * Run a remote bash script on `target` (e.g. `user@host`) via `ssh target bash -s`.
 * Script is sent on stdin (no shell quoting footguns on local side).
 */
export function runRemoteBash(
  target: string,
  script: string,
  opts: Partial<RunSshOptions> = {},
): Promise<RunSshResult> {
  const timeoutMs = opts.timeoutMs ?? DEFAULT_TIMEOUT_MS;
  const maxBytes = opts.maxBytes ?? DEFAULT_MAX_BYTES;

  return new Promise((resolve, reject) => {
    const child = spawn("ssh", ["-o", "BatchMode=yes", "-o", "ConnectTimeout=15", target, "bash", "-s"], {
      stdio: ["pipe", "pipe", "pipe"],
      windowsHide: true,
    });

    let stdout = "";
    let stderr = "";
    let truncated = false;
    let timedOut = false;
    let byteCount = 0;

    const bump = (chunk: Buffer, which: "out" | "err") => {
      if (truncated) return;
      const s = chunk.toString("utf8");
      byteCount += Buffer.byteLength(s, "utf8");
      if (byteCount > maxBytes) {
        truncated = true;
        child.kill("SIGKILL");
        return;
      }
      if (which === "out") stdout += s;
      else stderr += s;
    };

    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);

    child.stdout?.on("data", (c: Buffer) => bump(c, "out"));
    child.stderr?.on("data", (c: Buffer) => bump(c, "err"));

    child.on("error", (err) => {
      clearTimeout(timer);
      reject(err);
    });

    child.on("close", (code, signal) => {
      clearTimeout(timer);
      resolve({
        code,
        signal,
        stdout,
        stderr,
        truncated,
        timedOut,
      });
    });

    child.stdin?.write(script, "utf8", () => {
      child.stdin?.end();
    });
  });
}

export function formatRunFailure(label: string, r: RunSshResult): string {
  const bits = [
    `${label}: exit=${r.code} signal=${r.signal ?? "none"}`,
    r.timedOut ? "timedOut=true" : "",
    r.truncated ? "truncated=true" : "",
  ].filter(Boolean);
  let body = bits.join(" ");
  if (r.stderr.trim()) body += `\n--- stderr ---\n${r.stderr}`;
  if (r.stdout.trim()) body += `\n--- stdout ---\n${r.stdout}`;
  return body;
}
