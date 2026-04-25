# Pi Cortex MCP (`@rudy/pi-cortex-mcp`)

Local MCP server: **allowlisted** SSH to Rudy Pi (`jaylamping@rudy-pi` by default) for `cortex` ops — logs, systemd, health, CAN runbook, inventory, audit tail, **runtime settings** (`GET /api/settings` with SQLite fallback).

## Prereqs

- OpenSSH client (`ssh`) on PATH, key or agent auth to Pi (same as manual `ssh jaylamping@rudy-pi`).
- Tailscale / DNS so `rudy-pi` resolves if you use default host.
- One-time: `npm install && npm run build` in this directory (generates `dist/`).

## Cursor MCP

Merge into [`.cursor/mcp.json`](../../../.cursor/mcp.json) (or user MCP settings):

```json
"pi-cortex": {
  "command": "node",
  "args": ["${workspaceFolder}/tools/mcp/pi-cortex/dist/index.js"]
}
```

Optional env:

| Variable | Default | Purpose |
|----------|---------|---------|
| `PI_CORTEX_SSH` | — | Full target, e.g. `jaylamping@rudy-pi` (overrides user/host) |
| `PI_SSH_USER` | `jaylamping` | SSH user when `PI_CORTEX_SSH` unset |
| `PI_SSH_HOST` | `rudy-pi` | SSH host when `PI_CORTEX_SSH` unset |
| `PI_CORTEX_HTTP` | `http://127.0.0.1:8443` | Base URL **on the Pi** for curl (loopback from SSH session) |
| `PI_CORTEX_MCP_DRY_RUN` | — | Set to `1` to skip SSH (stub JSON only) |

After editing MCP config, refresh MCP in Cursor.

## Tools

| Tool | Effect |
|------|--------|
| `cortex_status` | `systemctl status`, `current.sha`, `/api/health` |
| `cortex_logs` | `journalctl -u cortex` (lines, optional `--since`) |
| `cortex_update_logs` | `journalctl -u cortex-update` |
| `cortex_restart` | `systemctl restart cortex` + health wait |
| `cortex_force_update` | `systemctl start cortex-update` + logs |
| `can_status` | `robot-can` + `ip` CAN details |
| `can_logs` | `robot-can` journal plus kernel CAN/SPI/MCP251x messages |
| `can_dump` | Bounded `candump -L` capture on `can0`/`can1`, optional `grep -E` filter |
| `can_sniff_cortex_restart` | Start bounded `candump`, restart `cortex`, return frames + journal tail |
| `can_send` | Send one validated `cansend` frame for explicit diagnostics |
| `can_bounce` | Runbook: stop cortex → restart `robot-can` → start cortex + health wait |
| `audit_tail` | `tail` `/var/lib/rudy/audit.jsonl` |
| `inventory_snapshot` | `head` live inventory YAML; optional `cortex.toml` |
| `settings_snapshot` | `curl /api/settings`; if down, read-only SQLite `settings_kv` via `[runtime].db_path` in TOML (Python `tomllib`) |

**No arbitrary shell** — only fixed remote scripts.

## Sudo

Mutating tools use `sudo systemctl …`. Same passwordless sudo expectations as your normal Pi workflow; if a tool hangs on password, fix sudoers on the Pi (narrow `NOPASSWD` for those units only).

## Dev

```bash
cd tools/mcp/pi-cortex
npm install
npm run build
npm test
```

## Settings / SQLite

Matches cortex: `GET /api/settings` returns merged registry + `SettingsGetResponse`. Fallback reads `settings_kv` from the DB file declared as `runtime.db_path` in `/etc/rudy/cortex.toml` (see `crates/cortex/src/settings/data.rs`).
