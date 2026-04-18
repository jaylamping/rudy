# Raspberry Pi 5 deployment (Rudy)

Scripts in this directory target **Ubuntu LTS (aarch64)** on a Raspberry Pi 5 with the **Waveshare 2-CH CAN HAT** (MCP2515).

**Onboard today:** `rudydae` + SocketCAN + systemd (`robot-can`, `rudyd`, `rudy-update`). **ROS 2 on the Pi is not installed** by these scripts; it will return when `driver_node` / `ros2_control` integration is implemented (desktop `ros/` workspace unchanged).

## How deployment works now

```
git push to main
    ↓
.github/workflows/release.yaml
    cross-builds rudydae for aarch64
    builds the link/ SPA
    publishes a GitHub Release with a tarball + latest.json manifest
    ↓
Pi: rudy-update.timer fires every 60s
    rudy-update.sh checks latest.json, downloads tarball, verifies sha256
    apply-release.sh installs into /opt/rudy and restarts rudyd
    ↓
new build live in ~60–90s, no SSH required
```

The Pi never compiles anything. After the one-time bootstrap, you can leave it alone — every `git push` to `main` rolls out automatically.

## One-time Pi bootstrap

On a fresh Pi (Ubuntu LTS aarch64, Waveshare 2-CH CAN HAT seated):

```bash
# 1. Tailscale (so the Pi has a stable hostname + Let's Encrypt HTTPS)
curl -fsSL https://tailscale.com/install.sh | sh
sudo tailscale up --ssh --hostname rudy-pi

# 2. Provision a Tailscale cert (one-time per host; auto-renew handled later)
TAILNAME=$(tailscale status --json | jq -r '.Self.DNSName' | sed 's/\.$//')
sudo install -d -o rudy -g rudy -m 0750 /var/lib/rudyd/tailscale  # rudy user is created in step 3 below if not present; safe to skip and re-run
sudo tailscale cert \
  --cert-file "/var/lib/rudyd/tailscale/${TAILNAME}.crt" \
  --key-file  "/var/lib/rudyd/tailscale/${TAILNAME}.key" \
  "${TAILNAME}"
sudo chown rudy:rudy "/var/lib/rudyd/tailscale/${TAILNAME}".*
sudo chmod 0640 "/var/lib/rudyd/tailscale/${TAILNAME}.key"

# 3. Bootstrap. Clone once, then this script is the only thing you ever run.
git clone https://github.com/jaylamping/rudy ~/rudy
sudo bash ~/rudy/deploy/pi5/bootstrap.sh
```

`bootstrap.sh` is idempotent. It will:

1. Install minimal runtime apt packages (no Rust, no Node).
2. Append the MCP2515 device-tree overlays to `/boot/firmware/config.txt` if missing (and ask you to reboot if it had to add them).
3. Install the `robot-can` service so `can0`/`can1` come up at boot.
4. Install `rudy-update.timer` and trigger the first update.

After it finishes, `journalctl -u rudy-update -f` will show the Pi pull the latest GitHub Release and start `rudyd`.

## Day-to-day

```bash
# Check what version is running
ssh jaylamping@rudy-pi "cat /opt/rudy/current.sha"

# Watch the next deploy land (push from desktop, then on Pi)
journalctl -u rudy-update -f
journalctl -u rudyd -f

# Force an immediate update check (instead of waiting up to 60s)
sudo systemctl start rudy-update
```

## Files

| File                       | Purpose                                                              |
| -------------------------- | -------------------------------------------------------------------- |
| `bootstrap.sh`             | **One-time Pi setup** (apt, CAN, updater timer). Run once after flash. |
| `rudy-update.sh`           | Polls GitHub for new releases; downloads + verifies + applies.       |
| `apply-release.sh`         | Installs a staged tarball into `/opt/rudy` and restarts `rudyd`.     |
| `rudy-update.service`      | systemd unit for the updater (oneshot).                              |
| `rudy-update.timer`        | systemd timer; polls every 60s.                                      |
| `render-rudyd-toml.sh`     | Renders `/etc/rudy/rudyd.toml` from live system state.               |
| `rudyd.service`            | systemd unit for the daemon itself.                                  |
| `robot-can.service`        | systemd unit that brings `can0`/`can1` up at 1 Mbps.                 |
| `can_setup.sh`             | Helper invoked by `robot-can.service`.                               |
| `install_can_overlays.sh`  | Idempotent append of MCP2515 overlays to `/boot/firmware/config.txt`. |
| `config.txt.example`       | Reference SPI + MCP2515 overlay snippet.                             |
| `tailscale-cert.md`        | Tailscale HTTPS cert provisioning runbook.                           |
| `Dockerfile.pi5`           | Local cross-compilation image (CI uses a faster runner-native build).|
| `setup_pi5.sh`             | _Deprecated_; superseded by `bootstrap.sh`. Kept for reference.      |
| `install.sh`               | _Deprecated_; superseded by CI release + `apply-release.sh`. Kept for emergencies (build-on-Pi). |
| `deploy.sh`                | _Deprecated_; superseded by CI. Kept for offline iteration.          |

## Gotcha: `can0` / `can1` vs silkscreen labels

On the Waveshare 2-CH CAN HAT, the **silkscreen labels on the PCB** ("CAN0" / "CAN1" near the screw terminals) are *not* guaranteed to match the **Linux interface names** (`can0` / `can1`). The Linux names come from the order in which the two `mcp2515-canX` device tree overlays are registered, whereas the silkscreen labels come from the PCB designer's intent.

Empirically on **our** Pi 5 with the overlays in `config.txt.example` (`mcp2515-can0,interrupt=23` + `mcp2515-can1,interrupt=25`), the mapping is:

| silkscreen label | SPI CE | interrupt GPIO | **Linux iface** |
| ---------------- | ------ | -------------- | --------------- |
| `CAN0`           | CE0    | GPIO 23        | **`can1`**      |
| `CAN1`           | CE1    | GPIO 25        | **`can0`**      |

**Translation: if you plug your wires into the screw terminals labeled "CAN0" on the board, talk to `can1` in software.** This is counter-intuitive but matches what a `candump` sanity test confirmed 2026-04-17 (full RobStride RS03 parameter export received on `can1`, nothing on `can0`, with wires in the silkscreen-"CAN0" terminal).

If you move the wires to the other terminal, swap the iface name. If you re-order the overlays in `config.txt`, the Linux names swap too. There is no single "right" mapping — always verify with `candump` against a known-talking device on the bus.

Our convention going forward: **shoulder_actuator_a is on Linux `can1`** (silkscreen "CAN0").
`inventory.yaml` is the source of truth for which iface each motor is on.
