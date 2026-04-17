# Runbook: Operator console (`rudyd` + `link`)

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

- `rudyd: loaded config from /etc/rudy/rudyd.toml`
- `rudyd: loaded inventory (motors=...)`
- `rudyd: starting mock CAN core` (Phase 1) or `rudyd: starting SocketCAN core` (Phase 1.5+)
- `rudyd: https listener up`
- `rudyd: webtransport listener up`
- `rudyd is up`

## Reach the UI

From any Tailscale-connected machine:

```
https://rudy.your-tailnet.ts.net:8443/
```

You will be prompted for the operator token.

## Token rotation

The shared operator token lives at the path referenced by
`config/rudyd.toml:auth.token_file`. On the Pi this is typically
`/etc/rudy/rudyd.token` (chmod `0600`, owned by `rudy:rudy`).

```bash
# 1. Generate a new token.
sudo -u rudy openssl rand -hex 32 | sudo tee /etc/rudy/rudyd.token.new >/dev/null
sudo chmod 0600 /etc/rudy/rudyd.token.new
sudo chown rudy:rudy /etc/rudy/rudyd.token.new

# 2. Atomically replace the old file.
sudo mv /etc/rudy/rudyd.token.new /etc/rudy/rudyd.token

# 3. Bounce rudyd so it re-reads the file. (Phase 2: SIGHUP-triggered reload.)
sudo systemctl restart rudyd.service

# 4. Copy the new token to your password manager.
sudo cat /etc/rudy/rudyd.token
```

Open sessions with the previous token will 401 on their next REST call and
bounce to the login screen.

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
6. Click **Save to flash** on any one limit (rudyd issues a single type-22
   save which flushes all pending RAM writes).
7. PSU-cycle the motor.
8. Back in **Params**, confirm the saved values persisted (Phase 1 UX
   shortcut: the UI's snapshot reloads on refresh; Phase 2 adds a "Read
   from motor" button).
9. Flip `inventory.yaml:verified: true` and commit. `rudyd` will now permit
   enable requests on this motor.

### Control lock

Phase 2 surfaces a visible control-lock indicator in the sidebar. In Phase 1,
`rudyd` tracks the lock internally (see `state::AppState::control_lock`) but
does not yet enforce it at the REST layer.

## Troubleshooting

### `cargo build -p rudyd` warns "using stub SPA"

The `link/dist/` directory is missing. Run `cd link && npm install && npm run build`
and rebuild rudyd. Without that, the SPA shows a stub page but the REST +
WebTransport surfaces still work (handy for backend-only testing).

### Browser cannot connect

- Confirm you are on the tailnet (`tailscale status` on your laptop).
- Confirm the Pi listens only on the Tailscale-local address (`ss -tlnp` on
  the Pi).
- Try `curl -k https://rudy.*.ts.net:8443/api/config -H "Authorization: Bearer $(sudo cat /etc/rudy/rudyd.token)"`.

### WebTransport not connecting but HTTPS works

- `GET /api/config` should return `webtransport.enabled = true` and a non-null
  `url`. If it does not, check `[webtransport] enabled = true` in
  `/etc/rudy/rudyd.toml`.
- The cert used for WT must match the cert used for HTTPS (Tailscale cert
  covers both).
- Chrome DevTools -> Network -> protocol column shows "webtransport". If it
  shows an error, the console tab usually has a more detailed message
  ("certificate verification failed", "connection refused", etc.).

### `rudyd` refuses to enable a motor

`{"error":"not_verified"}` means the inventory entry has `verified: false`.
Commission the motor first (see above) and flip the flag in
`config/actuators/inventory.yaml`. Override for benchtop commissioning is
`safety.require_verified = false` in `rudyd.toml` (do NOT keep this on the
Pi).
