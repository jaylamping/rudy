/** Remote bash fragments; executed via `ssh … bash -s` (stdin). Keep POSIX-only. */

export function scriptCortexStatus(baseUrl: string): string {
  return `set -euo pipefail
echo "=== /opt/rudy/current.sha ==="
cat /opt/rudy/current.sha 2>/dev/null || echo "<missing>"
echo "=== systemctl cortex ==="
systemctl status cortex --no-pager || true
echo "=== GET ${baseUrl}/api/health ==="
curl -fsS --connect-timeout 2 --max-time 8 "${baseUrl}/api/health" || echo "<health curl failed>"
`;
}

export function scriptCortexLogs(lines: number, since?: string): string {
  const sinceArg = since ? ` --since=${shQuote(since)}` : "";
  return `set -euo pipefail
journalctl -u cortex -n ${lines}${sinceArg} --no-pager || true
`;
}

export function scriptCortexUpdateLogs(lines: number): string {
  return `set -euo pipefail
journalctl -u cortex-update -n ${lines} --no-pager || true
`;
}

export function scriptCortexRestart(baseUrl: string, maxWaitSec: number): string {
  return `set -euo pipefail
sudo systemctl restart cortex.service
echo "restarted cortex.service; waiting for health (max ${maxWaitSec}s)"
for i in $(seq 1 ${maxWaitSec}); do
  if curl -fsS --connect-timeout 2 --max-time 5 "${baseUrl}/api/health" >/dev/null 2>&1; then
    echo health_ok iteration "$i"
    curl -fsS --connect-timeout 2 --max-time 5 "${baseUrl}/api/health" || true
    exit 0
  fi
  sleep 1
done
echo "health_timeout after ${maxWaitSec}s"
journalctl -u cortex -n 80 --no-pager || true
exit 1
`;
}

export function scriptCortexForceUpdate(lines: number): string {
  return `set -euo pipefail
sudo systemctl start cortex-update.service || sudo systemctl start cortex-update || true
sleep 2
echo "=== /opt/rudy/current.sha ==="
cat /opt/rudy/current.sha 2>/dev/null || echo "<missing>"
echo "=== cortex-update (last ${lines} lines) ==="
journalctl -u cortex-update -n ${lines} --no-pager || true
`;
}

export function scriptCanStatus(): string {
  return `set -euo pipefail
echo "=== robot-can ==="
systemctl status robot-can --no-pager || true
echo "=== can0 ==="
ip -details -statistics link show can0 || true
echo "=== can1 ==="
ip -details -statistics link show can1 || true
`;
}

export function scriptCanBounce(baseUrl: string, maxWaitSec: number): string {
  return `set -euo pipefail
echo "stopping cortex"
sudo systemctl stop cortex.service || true
sleep 2
echo "restarting robot-can"
sudo systemctl restart robot-can.service || true
sleep 2
ip -details link show can0 2>/dev/null | grep -E 'state|bitrate|restart-ms' || true
ip -details link show can1 2>/dev/null | grep -E 'state|bitrate|restart-ms' || true
echo "starting cortex"
sudo systemctl start cortex.service
for i in $(seq 1 ${maxWaitSec}); do
  if curl -fsS --connect-timeout 2 --max-time 5 "${baseUrl}/api/health" >/dev/null 2>&1; then
    echo health_ok iteration "$i"
    curl -fsS --connect-timeout 2 --max-time 5 "${baseUrl}/api/health" || true
    exit 0
  fi
  sleep 1
done
echo "health_timeout"
journalctl -u cortex -n 80 --no-pager || true
exit 1
`;
}

export function scriptAuditTail(lines: number): string {
  return `set -euo pipefail
if [[ ! -f /var/lib/rudy/audit.jsonl ]]; then echo "<missing /var/lib/rudy/audit.jsonl>"; exit 0; fi
tail -n ${lines} /var/lib/rudy/audit.jsonl
`;
}

export function scriptInventorySnapshot(includeToml: boolean, maxBytes: number): string {
  const tomlBlock = includeToml
    ? `echo "=== /etc/rudy/cortex.toml (truncated) ==="
head -c ${maxBytes} /etc/rudy/cortex.toml 2>/dev/null || echo "<missing or unreadable>"
`
    : "";
  return `set -euo pipefail
echo "=== /var/lib/rudy/inventory.yaml (truncated) ==="
head -c ${maxBytes} /var/lib/rudy/inventory.yaml 2>/dev/null || echo "<missing>"
${tomlBlock}`;
}

/** Authoritative runtime state from cortex HTTP APIs. Avoid seed/config files. */
export function scriptRuntimeSnapshot(baseUrl: string): string {
  const u = baseUrl.replace(/'/g, "");
  return `set -euo pipefail
BASE='${u}'
fetch_json() {
  name="$1"
  path="$2"
  echo "=== $name ($path) ==="
  curl -fsS --connect-timeout 3 --max-time 25 "$BASE$path" || echo "<curl failed: $path>"
  echo
}
fetch_json "settings" "/api/settings"
fetch_json "devices" "/api/devices"
fetch_json "motors" "/api/motors"
`;
}

/** Prefer GET /api/settings; fall back to read-only SQLite settings_kv via tomllib + sqlite3. */
export function scriptSettingsSnapshot(baseUrl: string): string {
  // shell-escape baseUrl for curl (minimal: disallow quotes)
  const u = baseUrl.replace(/'/g, "");
  return `set -euo pipefail
BASE='${u}'
ERRF=$(mktemp)
trap 'rm -f "$ERRF" /tmp/settings_api.json 2>/dev/null' EXIT
if curl -fsS --connect-timeout 3 --max-time 25 "$BASE/api/settings" -o /tmp/settings_api.json 2>"$ERRF"; then
  python3 - <<'PY'
import json
with open("/tmp/settings_api.json","r",encoding="utf-8") as f:
    body=json.load(f)
print(json.dumps({"source":"api","url":"/api/settings","body":body}))
PY
  exit 0
fi
export MCP_CURL_ERR_FILE="$ERRF"
python3 - <<'PY'
import json, os, sqlite3, sys
from pathlib import Path
curl_err = ""
p = os.environ.get("MCP_CURL_ERR_FILE")
if p and os.path.exists(p):
    curl_err = open(p, "r", encoding="utf-8", errors="replace").read()

def fail(msg, **extra):
    print(json.dumps({"source":"error","error":msg, "curl_stderr": curl_err, **extra}))
    sys.exit(0)

try:
    import tomllib
except ImportError:
    fail("tomllib missing (need Python 3.11+)")

cfg_path = Path("/etc/rudy/cortex.toml")
if not cfg_path.is_file():
    fail("missing /etc/rudy/cortex.toml")

try:
    data = tomllib.loads(cfg_path.read_text(encoding="utf-8"))
except Exception as e:
    fail("toml parse error", detail=str(e))

rt = data.get("runtime") or {}
enabled = bool(rt.get("enabled", False))
db_path = rt.get("db_path")
if not db_path:
    fail("runtime.db_path not set in cortex.toml", runtime_db_enabled=enabled)

db = str(db_path)
if not Path(db).is_file():
    fail("runtime db file missing", db_path=db, runtime_db_enabled=enabled)

rows = []
try:
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True, timeout=5)
    try:
        rows = con.execute("SELECT key, value_json FROM settings_kv ORDER BY key").fetchall()
    finally:
        con.close()
except Exception as e:
    fail("sqlite read failed", db_path=db, detail=str(e))

out = {
    "source": "sqlite",
    "runtime_db_enabled": enabled,
    "db_path": db,
    "note": "API unreachable; raw settings_kv rows",
    "settings_kv": [{"key": k, "value_json": v} for k, v in rows],
}
print(json.dumps(out))
PY
`;
}

function shQuote(s: string): string {
  // Safe single-quote for POSIX sh: wrap in '...' with '\'' for internal quotes
  return `'${s.replace(/'/g, `'\"'\"'`)}'`;
}
