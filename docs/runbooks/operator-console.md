# Runbook: Operator console (`rudydae` + `link`)

Day-to-day operations. See [ADR-0004](../decisions/0004-operator-console.md)
for the architecture; see [deploy/pi5/tailscale-cert.md](../../deploy/pi5/tailscale-cert.md)
for cert provisioning.

## Start / stop / status

```bash
# On the Pi
sudo systemctl start  rudyd.service
sudo systemctl stop   rudyd.service
sudo systemctl status rudyd.service
sudo journalctl -u rudyd.service -f
```

Expected healthy startup log lines:

- `loaded config from /etc/rudy/rudyd.toml`
- `loaded inventory` with `motors=...`
- `rudydae: starting mock CAN core` (Phase 1) or real CAN core (Phase 1.5+)
- `rudydae http listener up (plaintext; TLS terminated upstream by ...)`
- `webtransport listener up` (when enabled)
- `rudydae is up`

## Reach the UI

From any Tailscale-connected machine, open the short MagicDNS name with no
port and no `.ts.net` suffix:

```
https://rudy-pi/
```

`tailscale serve` on the Pi terminates TLS at `:443` using an auto-renewing
Let's Encrypt cert and proxies decrypted requests to `rudyd` on
`127.0.0.1:8443`. WebTransport (telemetry firehose) lands directly on
`<host>:4433` because Tailscale Serve cannot proxy HTTP/3 — the cert for
that listener is provisioned manually (see
[deploy/pi5/tailscale-cert.md](../../deploy/pi5/tailscale-cert.md)).

No login screen — `rudydae` does not authenticate requests. Reachability is
gated entirely by Tailscale ACLs. If we ever need real auth back, see the
deleted `auth.rs` module in git history for the shared-bearer-token
starting point.

To inspect the proxy mapping on the Pi:

```bash
tailscale serve status   # what is being proxied where
ss -tlnp | grep 8443     # rudyd listens on 127.0.0.1 only
```

## Local UI development against the Pi

You can iterate on the React UI in `link/` without running `rudydae` locally
by pointing the Vite dev server at the Pi over Tailscale:

```bash
cd link
cp .env.example .env.local
# edit .env.local — set:
#   VITE_RUDYD_URL=https://rudy-pi/
npm run dev
```

`/api/*` requests from the dev server (http://localhost:5173) are proxied to
that URL. WebTransport is negotiated separately via `GET /api/config`, so the
telemetry firehose connects directly browser → `<host>:4433`. Both require
that you are on the tailnet.

If `VITE_RUDYD_URL` is unset the proxy falls back to `http://127.0.0.1:8443`,
which is the right choice when you _are_ running `rudydae` locally
(`cargo run -p rudydae` from `crates/`).

## Audit log

Every mutating action (parameter write, enable, stop, set_zero, save) writes a
JSONL entry to `config/rudyd.toml:paths.audit_log` (default on the Pi:
`/var/lib/rudy/audit.jsonl`).

```bash
# Tail live.
sudo tail -f /var/lib/rudy/audit.jsonl

# Last 10 parameter writes.
sudo jq -c 'select(.action | startswith("param_write"))' \
  /var/lib/rudy/audit.jsonl | tail -10

# Every denied action today.
sudo jq -c --arg d "$(date -Iseconds -u | cut -c1-10)" \
  'select(.result == "denied") | select(.timestamp | startswith($d))' \
  /var/lib/rudy/audit.jsonl
```

### Rotation

Ship `logrotate` (Phase 2 polish). For now, rotate by hand:

```bash
sudo systemctl stop rudyd.service
sudo mv /var/lib/rudy/audit.jsonl /var/lib/rudy/audit-$(date +%Y%m%d).jsonl
sudo gzip /var/lib/rudy/audit-*.jsonl
sudo systemctl start rudyd.service
```

## Common operations

### Commissioning an RS03 (Phase 1 target workflow)

Matches the runbook in [tools/robstride/commission.md](../../tools/robstride/commission.md).
The UI replaces Motor Studio for steps 4-7.

1. Motor power-cycled, bus quiet. Ensure it is listed in `inventory.yaml`
  with `verified: false`.
2. Sign in to `https://rudy.*.ts.net:8443/`.
3. Go to **Telemetry** to confirm the motor's `vbus`/`temp` readings look
  sensible (mock CAN will show synthetic data; real CAN shows real).
4. Go to **Params**, select the motor. In **Firmware limits (writable)**:
  - Set `limit_torque`, `limit_spd`, `limit_cur`, `canTimeout` to the
   values documented in `config/actuators/robstride_rs03.yaml:commissioning_defaults`.
  - Click **Write RAM**, type the confirm phrase, submit.
5. Repeat for every limit parameter. The UI range-checks every write against
  `hardware_range`.
6. Click **Save to flash** on any one limit (rudydae issues a single type-22
  save which flushes all pending RAM writes).
7. PSU-cycle the motor.
8. Back in **Params**, confirm the saved values persisted (Phase 1 UX
  shortcut: the UI's snapshot reloads on refresh; Phase 2 adds a "Read
   from motor" button).
9. Flip `inventory.yaml:verified: true` and commit. `rudydae` will now permit
  enable requests on this motor.

### Control lock

`rudydae` runs a lightweight single-operator lock to keep two browser tabs
from racing each other on the CAN bus. It is fully implicit: the first
mutating REST call from a fresh `X-Rudy-Session` claims the lock, and any
*other* concurrent session's mutator gets back 423 Locked with the holder's
session id in the `detail` field. There is no operator UI: a fresh tab just
works, and stale tabs find out they're stale by being refused.

Recovery from a stuck holder (rare; would only happen if a tab vanished
without the daemon restarting): restart `rudyd.service`. The lock is in
memory only; nothing on disk pins it.

The auto-acquire is recorded in `~/.rudyd/audit.jsonl` as
`control_lock_auto_acquire` and broadcast over WebTransport as a
`safety_event` `lock_changed` frame.

## Troubleshooting

### `cargo build -p rudydae` warns "using stub SPA"

The `link/dist/` directory is missing. Run `cd link && npm install && npm run build`
and rebuild rudydae. Without that, the SPA shows a stub page but the REST +
WebTransport surfaces still work (handy for backend-only testing).

### Browser cannot connect

- Confirm you are on the tailnet (`tailscale status` on your laptop).
- Confirm `tailscale serve` is up on the Pi (`tailscale serve status`).
  Expected: `https://<host>` -> `http://127.0.0.1:8443`.
- Confirm `rudyd` is listening on the loopback address (`ss -tlnp | grep 8443`
  on the Pi — should bind `127.0.0.1`, never `0.0.0.0`).
- Try `curl https://rudy-pi/api/config` from a tailnet device.

### Operator console returns 502 / "service unavailable"

That's `tailscale serve` reporting that `rudyd` on `127.0.0.1:8443` is not
answering. Either `rudyd` crashed (`journalctl -u rudyd -n 50`) or it's
listening on a different port. `bootstrap.sh` and `apply-release.sh` always
configure the proxy at `127.0.0.1:8443` — if you changed `[http] bind` in
`/etc/rudy/rudyd.toml` for some reason, change it back or update the
`tailscale serve` mapping to match.

### WebTransport not connecting but HTTPS works

- `GET /api/config` should return `webtransport.enabled = true` and a non-null
  `url`. If it does not, check `[webtransport] enabled = true` and that
  `cert_path` / `key_path` are set in `/etc/rudy/rudyd.toml`.
- The WT cert files must exist and be readable by the `rudy` user
  (`ls -la /var/lib/rudyd/tailscale/`).
- Chrome DevTools -> Network -> protocol column shows "webtransport". If it
  shows an error, the console tab usually has a more detailed message
  ("certificate verification failed", "connection refused", etc.).

### `rudydae` refuses to enable a motor

`{"error":"not_verified"}` means the inventory entry has `verified: false`.
Commission the motor first (see above) and flip the flag in
`config/actuators/inventory.yaml`. Override for benchtop commissioning is
`safety.require_verified = false` in `rudyd.toml` (do NOT keep this on the
Pi).