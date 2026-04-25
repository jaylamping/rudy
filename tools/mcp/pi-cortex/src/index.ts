import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { StdioServerTransport } from "@modelcontextprotocol/sdk/server/stdio.js";
import { z } from "zod";

import { cortexBaseUrl, dryRun, sshTarget } from "./config.js";
import {
  scriptAuditTail,
  scriptCanBounce,
  scriptCanStatus,
  scriptCortexForceUpdate,
  scriptCortexLogs,
  scriptCortexRestart,
  scriptCortexStatus,
  scriptCortexUpdateLogs,
  scriptInventorySnapshot,
  scriptSettingsSnapshot,
} from "./pi-actions.js";
import { formatRunFailure, runRemoteBash } from "./ssh.js";
import {
  healthMaxWaitSec,
  healthWaitMs,
  logLines,
  sanitizeSince,
} from "./validation.js";

const MAX_SCRIPT_TIMEOUT_MS = 180_000;
const MAX_OUTPUT_BYTES = 2 * 1024 * 1024;

async function runPiScript(script: string, timeoutMs: number): Promise<{ text: string; isError: boolean }> {
  if (dryRun()) {
    return {
      text: JSON.stringify(
        {
          dryRun: true,
          sshTarget: sshTarget(),
          scriptPreview: script.slice(0, 400),
        },
        null,
        2,
      ),
      isError: false,
    };
  }

  const boundedTimeout = Math.min(timeoutMs, MAX_SCRIPT_TIMEOUT_MS);
  const r = await runRemoteBash(sshTarget(), script, {
    timeoutMs: boundedTimeout,
    maxBytes: MAX_OUTPUT_BYTES,
  });

  const parts: string[] = [];
  if (r.stdout) parts.push(r.stdout);
  if (r.stderr) parts.push(`--- stderr ---\n${r.stderr}`);

  const text = parts.join("\n").trim() || "(no output)";
  const bad = r.timedOut || r.truncated || r.code !== 0;

  if (bad && !r.stdout && !r.stderr) {
    return { text: formatRunFailure("ssh", r), isError: true };
  }
  if (bad) {
    return { text: `${text}\n\n${formatRunFailure("ssh", r)}`, isError: true };
  }
  return { text, isError: false };
}

function textResult(text: string, isError: boolean) {
  return {
    content: [{ type: "text" as const, text }],
    ...(isError ? { isError: true as const } : {}),
  };
}

const linesSchema = z.object({
  lines: z.number().int().min(1).max(2000).optional(),
});

const logsSchema = linesSchema.extend({
  since: z.string().max(64).optional(),
});

const restartSchema = z.object({
  health_wait_ms: z.number().int().min(1000).max(120_000).optional(),
});

const inventorySchema = z.object({
  include_cortex_toml: z.boolean().optional(),
  max_bytes: z.number().int().min(1024).max(500_000).optional(),
});

export async function main(): Promise<void> {
  const base = cortexBaseUrl();
  const server = new McpServer({ name: "pi-cortex", version: "0.1.0" });

  server.registerTool(
    "cortex_status",
    {
      title: "Cortex status",
      description:
        "SSH to Pi: systemd status for cortex, /opt/rudy/current.sha, curl local /api/health.",
      inputSchema: z.object({}),
    },
    async () => {
      const { text, isError } = await runPiScript(scriptCortexStatus(base), 45_000);
      return textResult(text, isError);
    },
  );

  server.registerTool(
    "cortex_logs",
    {
      title: "Cortex journal",
      description: "SSH: journalctl -u cortex (bounded lines; optional --since).",
      inputSchema: logsSchema,
    },
    async (args: z.infer<typeof logsSchema>) => {
      const n = logLines(args.lines);
      const since = sanitizeSince(args.since);
      const { text, isError } = await runPiScript(scriptCortexLogs(n, since), 60_000);
      return textResult(text, isError);
    },
  );

  server.registerTool(
    "cortex_update_logs",
    {
      title: "Cortex-update journal",
      description: "SSH: journalctl -u cortex-update.",
      inputSchema: linesSchema,
    },
    async (args: z.infer<typeof linesSchema>) => {
      const n = logLines(args.lines);
      const { text, isError } = await runPiScript(scriptCortexUpdateLogs(n), 60_000);
      return textResult(text, isError);
    },
  );

  server.registerTool(
    "cortex_restart",
    {
      title: "Restart cortex",
      description: "SSH: sudo systemctl restart cortex; wait for /api/health.",
      inputSchema: restartSchema,
      annotations: { readOnlyHint: false, destructiveHint: true },
    },
    async (args: z.infer<typeof restartSchema>) => {
      const waitMs = healthWaitMs(args.health_wait_ms);
      const sec = healthMaxWaitSec(waitMs);
      const { text, isError } = await runPiScript(scriptCortexRestart(base, sec), waitMs + 30_000);
      return textResult(text, isError);
    },
  );

  server.registerTool(
    "cortex_force_update",
    {
      title: "Trigger cortex-update",
      description: "SSH: sudo systemctl start cortex-update; show recent updater logs + current.sha.",
      inputSchema: linesSchema,
      annotations: { readOnlyHint: false, destructiveHint: true },
    },
    async (args: z.infer<typeof linesSchema>) => {
      const n = logLines(args.lines);
      const { text, isError } = await runPiScript(scriptCortexForceUpdate(n), 90_000);
      return textResult(text, isError);
    },
  );

  server.registerTool(
    "can_status",
    {
      title: "CAN / robot-can status",
      description: "SSH: systemctl robot-can + ip link details for can0/can1.",
      inputSchema: z.object({}),
    },
    async () => {
      const { text, isError } = await runPiScript(scriptCanStatus(), 45_000);
      return textResult(text, isError);
    },
  );

  server.registerTool(
    "can_bounce",
    {
      title: "Bounce CAN (runbook)",
      description:
        "SSH: stop cortex, restart robot-can, start cortex, wait for health. Use when bus stuck per runbook.",
      inputSchema: restartSchema,
      annotations: { readOnlyHint: false, destructiveHint: true },
    },
    async (args: z.infer<typeof restartSchema>) => {
      const waitMs = healthWaitMs(args.health_wait_ms);
      const sec = healthMaxWaitSec(waitMs);
      const { text, isError } = await runPiScript(scriptCanBounce(base, sec), waitMs + 60_000);
      return textResult(text, isError);
    },
  );

  server.registerTool(
    "audit_tail",
    {
      title: "Audit log tail",
      description: "SSH: tail -n /var/lib/rudy/audit.jsonl",
      inputSchema: linesSchema,
    },
    async (args: z.infer<typeof linesSchema>) => {
      const n = logLines(args.lines);
      const { text, isError } = await runPiScript(scriptAuditTail(n), 45_000);
      return textResult(text, isError);
    },
  );

  server.registerTool(
    "inventory_snapshot",
    {
      title: "Inventory + optional cortex.toml",
      description: "SSH: head of /var/lib/rudy/inventory.yaml; optional /etc/rudy/cortex.toml.",
      inputSchema: inventorySchema,
    },
    async (args: z.infer<typeof inventorySchema>) => {
      const maxB = args.max_bytes ?? 120_000;
      const { text, isError } = await runPiScript(
        scriptInventorySnapshot(Boolean(args.include_cortex_toml), maxB),
        45_000,
      );
      return textResult(text, isError);
    },
  );

  server.registerTool(
    "settings_snapshot",
    {
      title: "Runtime settings",
      description:
        "SSH: GET /api/settings JSON first; if curl fails, read-only SQLite settings_kv from [runtime].db_path in /etc/rudy/cortex.toml (Python tomllib + sqlite3).",
      inputSchema: z.object({}),
    },
    async () => {
      const { text, isError } = await runPiScript(scriptSettingsSnapshot(base), 90_000);
      return textResult(text, isError);
    },
  );

  const transport = new StdioServerTransport();
  await server.connect(transport);
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
