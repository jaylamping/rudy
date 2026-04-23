# Raspberry Pi 5 deployment (Rudy)

Scripts in this directory target **Ubuntu LTS (aarch64)** on a Raspberry Pi 5 with the **Waveshare 2-CH CAN HAT** (MCP2515).

**Onboard today:** `cortex` + SocketCAN + systemd (`robot-can`, `cortex`, `cortex-update`, `cortex-watchdog`). **ROS 2 on the Pi is not installed** by these scripts; it will return when `driver_node` / `ros2_control` integration is implemented (desktop `ros/` workspace unchanged).

## How deployment works now

```
git push to main
    ↓
.github/workflows/release.yaml
    cross-builds cortex for aarch64
    builds the link/ SPA
    publishes a GitHub Release with a tarball + latest.json manifest
    ↓
    ├─ (push path) notify-pi job joins tailnet as tag:ci, runs
    │      `tailscale ssh rudy-pi -- sudo systemctl start cortex-update`
    │      → Pi pulls + applies the release within seconds of the build.
    │
    └─ (poll fallback) cortex-update.timer fires every 60s anyway,
           so a missed push (Pi offline, tailnet hiccup, fork/PR build
           with no secrets) still rolls forward on its own.
    ↓
cortex-update.sh checks latest.json, downloads tarball, verifies sha256
apply-release.sh installs into /opt/rudy, re-renders /etc/rudy/cortex.toml,
    re-asserts `tailscale serve`, restarts cortex
    ↓
new build live within seconds (push path) or ≤60s (poll fallback)
```

### CI push: required GitHub config

The `notify-pi` job in `release.yaml` runs only when these are set on the repo:


| Kind     | Name                     | Value                                                         |
| -------- | ------------------------ | ------------------------------------------------------------- |
| Variable | `TAILSCALE_PUSH_ENABLED` | `true` (gate; absent = skip the job, fall back to poll only)  |
| Variable | `PI_TAILNET_HOST`        | optional override, defaults to `rudy-pi`                      |
| Variable | `PI_SSH_USER`            | optional override, defaults to `jaylamping`                   |
| Secret   | `TS_OAUTH_CLIENT_ID`     | Tailscale OAuth client id (scope: `auth_keys`, tag: `tag:ci`) |
| Secret   | `TS_OAUTH_SECRET`        | matching OAuth client secret                                  |


Generate the OAuth client at [https://login.tailscale.com/admin/settings/oauth](https://login.tailscale.com/admin/settings/oauth) with **Devices → Auth Keys → Write** and tag `tag:ci`.

### CI push: required tailnet ACL

In the tailnet policy file (Tailscale admin → Access Controls), add:

```jsonc
{
  "tagOwners": {
    "tag:ci": ["autogroup:admin"],
    "tag:pi": ["autogroup:admin"],
  },
  "ssh": [
    {
      "action": "accept",
      "src":    ["tag:ci"],
      "dst":    ["tag:pi"],
      "users":  ["jaylamping", "root"],
    },
  ],
}
```

Then tag the Pi once: `sudo tailscale up --advertise-tags=tag:pi --ssh --hostname rudy-pi` (re-running `up` with extra flags is non-destructive). Confirm with `tailscale status --json | jq '.Self.Tags'`.

### CI push: passwordless sudo for the trigger

The job runs `sudo systemctl start cortex-update.service` as the SSH user, which must succeed without a TTY prompt. Add a sudoers drop-in once on the Pi:

```bash
sudo install -m 0440 /dev/stdin /etc/sudoers.d/rudy-ci-update <<'EOF'
jaylamping ALL=(root) NOPASSWD: /bin/systemctl start cortex-update.service, /bin/systemctl start cortex-update
EOF
sudo visudo -c
```

Scoped to that one unit start so it doesn't widen the SSH user's privileges.

If the push job ever fails (Pi offline, tailnet down, ACL drift, sudoers missing), it's marked `continue-on-error: true` — the release still publishes and the 60s poll picks it up. Watch `journalctl -u cortex-update -f` either way.

The Pi never compiles anything. After the one-time bootstrap, you can leave it alone — every `git push` to `main` rolls out automatically.

## How the operator console is reached

`cortex` does **not** terminate TLS itself for the REST/SPA surface. It binds
plaintext on `127.0.0.1:8443`, and `tailscale serve` fronts it with HTTPS
on `:443` of the Pi's tailnet IP using an auto-renewing Tailscale Let's
Encrypt cert. Operators browse:

```
https://rudy-pi/        # short MagicDNS name, no port, no .ts.net suffix
```

…from any device on the same tailnet. WebTransport (telemetry firehose)
keeps doing its own TLS on `:4433/udp` because `tailscale serve` does not
proxy HTTP/3. See `tailscale-cert.md` for both certs.

## One-time Pi bootstrap

On a fresh Pi (Ubuntu LTS aarch64, Waveshare 2-CH CAN HAT seated):

```bash
# 1. Tailscale (so the Pi has a stable hostname + Let's Encrypt HTTPS)
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up --ssh --hostname rudy-pi

# 2. Provision the WebTransport cert (one-time per host; auto-renew handled
#    later by a follow-up timer). Tailscale Serve handles the REST/SPA cert
#    automatically — no manual provisioning needed for the main UI.
TAILNAME=$(tailscale status --json | jq -r '.Self.DNSName' | sed 's/\.$//')
sudo install -d -o rudy -g rudy -m 0750 /var/lib/rudy/cortex/tailscale
sudo tailscale cert \
  --cert-file "/var/lib/rudy/cortex/tailscale/${TAILNAME}.crt" \
  --key-file  "/var/lib/rudy/cortex/tailscale/${TAILNAME}.key" \
  "${TAILNAME}"
sudo chown rudy:rudy "/var/lib/rudy/cortex/tailscale/${TAILNAME}".*
sudo chmod 0640 "/var/lib/rudy/cortex/tailscale/${TAILNAME}.key"

# 3. Bootstrap. Clone once, then this script is the only thing you ever run.
git clone https://github.com/jaylamping/rudy ~/rudy
sudo bash ~/rudy/deploy/pi5/bootstrap.sh
```

`bootstrap.sh` is idempotent. It will:

1. Install minimal runtime apt packages (no Rust, no Node).
2. Append the MCP2515 device-tree overlays to `/boot/firmware/config.txt` if missing (and ask you to reboot if it had to add them).
3. Install the `robot-can` service so `can0`/`can1` come up at boot.
4. Install `cortex-update.timer` and trigger the first update.
5. Install `cortex-watchdog.timer` so unhealthy daemon/UI states auto-restart.
6. Configure `tailscale serve` to front `cortex` on `https://<host>/`.

After it finishes, `journalctl -u cortex-update -f` will show the Pi pull the latest GitHub Release and start `cortex`. Open `https://rudy-pi/` from any device on the same tailnet.

## CAN I/O CPU pinning (Pi 5)

`bootstrap.sh` pins each CAN interface's **hard IRQ** to a non-zero CPU
core (one per iface, in the alphabetical order returned by
`ip -o link show type can`, leaving core 0 for the kernel + tokio +
axum / WebTransport). `cortex` then pins each per-bus worker thread to
the same core via the per-bus `cpu_pin` field in `[[can.buses]]` (or
the same auto-assignment rule, when `cpu_pin` is omitted).

The benefit: the kernel runs the SocketCAN softirq on whichever CPU
received the hard IRQ. Pinning the IRQ to the worker's core keeps the
kernel-side packet path and the user-space `recv()` loop resident in
the same L1/L2 cache, eliminating an inter-core hop on every received
frame. This is the highest-impact bus-determinism knob short of moving
to `SCHED_FIFO` (deferred — see `crates/cortex/src/can/bus_worker.rs`).

**Verify after bootstrap (or reboot):**

```bash
# Find the IRQ for the iface you care about.
grep can1 /proc/interrupts          # → e.g. 87:  1234567 ... spi0.0  can1
# Confirm it's pinned to the expected CPU (core 1 for the first bus).
cat /proc/irq/87/smp_affinity_list  # → 1

# Confirm the cortex worker is also on that core. The `Cpus_allowed_list`
# row of /proc/<pid>/status reflects current affinity.
pidof cortex | xargs -I{} grep -H Cpus_allowed_list /proc/{}/task/*/status \
  | grep rudy-can-can1
```

If the IRQ pin row shows `0-3` (i.e. unpinned), `bootstrap.sh` either
couldn't find the IRQ row in `/proc/interrupts` (uncommon, only happens
if the iface name moved between bootstrap and the IRQ scan) or the
filesystem isn't writable from the script's UID. Re-running `sudo bash deploy/pi5/bootstrap.sh` is the easiest fix.

## Day-to-day

```bash
# Check what version is running
ssh jaylamping@rudy-pi "cat /opt/rudy/current.sha"

# Watch the next deploy land (push from desktop, then on Pi)
journalctl -u cortex-update -f
journalctl -u cortex -f
journalctl -u cortex-watchdog.service -f
journalctl -t cortex-watchdog -f

# Force an immediate update check (instead of waiting up to 60s)
sudo systemctl start cortex-update

# Verify watchdog + health endpoint manually
systemctl status cortex-watchdog.timer --no-pager
curl -sS http://127.0.0.1:8443/api/health | jq
```

## Files


| File                      | Purpose                                                                                          |
| ------------------------- | ------------------------------------------------------------------------------------------------ |
| `bootstrap.sh`            | **One-time Pi setup** (apt, CAN, updater timer). Run once after flash.                           |
| `cortex-update.sh`        | Polls GitHub for new releases; downloads + verifies + applies.                                   |
| `apply-release.sh`        | Installs a staged tarball into `/opt/rudy` and restarts `cortex`.                                |
| `cortex-update.service`   | systemd unit for the updater (oneshot).                                                          |
| `cortex-update.timer`     | systemd timer; polls every 60s as a fallback for the CI push path.                               |
| `cortex-watchdog.sh`      | Health probe script; restarts `cortex` after repeated failures.                                  |
| `cortex-watchdog.service` | systemd oneshot unit that runs the watchdog probe.                                               |
| `cortex-watchdog.timer`   | systemd timer; runs watchdog probe every 15s.                                                    |
| `render-cortex-toml.sh`   | Renders `/etc/rudy/cortex.toml` from live system state.                                          |
| `cortex.service`          | systemd unit for the daemon itself.                                                              |
| `robot-can.service`       | systemd unit that brings `can0`/`can1` up at 1 Mbps.                                             |
| `can_setup.sh`            | Helper invoked by `robot-can.service`.                                                           |
| `install_can_overlays.sh` | Idempotent append of MCP2515 overlays to `/boot/firmware/config.txt`.                            |
| `config.txt.example`      | Reference SPI + MCP2515 overlay snippet.                                                         |
| `tailscale-cert.md`       | Tailscale HTTPS cert provisioning runbook.                                                       |
| `Dockerfile.pi5`          | Local cross-compilation image (CI uses a faster runner-native build).                            |
| `setup_pi5.sh`            | *Deprecated*; superseded by `bootstrap.sh`. Kept for reference.                                  |
| `install.sh`              | *Deprecated*; superseded by CI release + `apply-release.sh`. Kept for emergencies (build-on-Pi). |
| `deploy.sh`               | *Deprecated*; superseded by CI. Kept for offline iteration.                                      |


## Gotcha: `can0` / `can1` vs silkscreen labels

On the Waveshare 2-CH CAN HAT, the **silkscreen labels on the PCB** ("CAN0" / "CAN1" near the screw terminals) are *not* guaranteed to match the **Linux interface names** (`can0` / `can1`). The Linux names come from the order in which the two `mcp2515-canX` device tree overlays are registered, whereas the silkscreen labels come from the PCB designer's intent.

Empirically on **our** Pi 5 with the overlays in `config.txt.example` (`mcp2515-can0,interrupt=23` + `mcp2515-can1,interrupt=25`), the mapping is:


| silkscreen label | SPI CE | interrupt GPIO | **Linux iface** |
| ---------------- | ------ | -------------- | --------------- |
| `CAN0`           | CE0    | GPIO 23        | `**can1`**      |
| `CAN1`           | CE1    | GPIO 25        | `**can0**`      |


**Translation: if you plug your wires into the screw terminals labeled "CAN0" on the board, talk to `can1` in software.** This is counter-intuitive but matches what a `candump` sanity test confirmed 2026-04-17 (full RobStride RS03 parameter export received on `can1`, nothing on `can0`, with wires in the silkscreen-"CAN0" terminal).

If you move the wires to the other terminal, swap the iface name. If you re-order the overlays in `config.txt`, the Linux names swap too. There is no single "right" mapping — always verify with `candump` against a known-talking device on the bus.

Our convention going forward: **shoulder_actuator_a is on Linux `can1`** (silkscreen "CAN0").
`inventory.yaml` is the source of truth for which iface each motor is on.