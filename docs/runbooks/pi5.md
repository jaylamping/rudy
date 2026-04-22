# Runbook: Raspberry Pi 5 (Rudy onboard)

## Preconditions

- **OS**: Ubuntu LTS for Raspberry Pi (aarch64). 24.04 LTS is the documented baseline; newer releases work for **`cortex` + SocketCAN** (no ROS packages on the Pi for now).
- **Time sync**: `chrony` installed by `deploy/pi5/setup_pi5.sh` (important for logs and any future distributed stack).

**ROS 2 on the Pi:** deferred. The desktop [`ros/`](../../ros/) workspace and CI still use Jazzy; onboard ROS/`ros2_control` will return when the Pi `driver_node` path is implemented. Do not add `packages.ros.org` apt sources on the Pi unless you are intentionally matching a supported Ubuntu series for Jazzy (currently **noble**).

## CAN HAT (Waveshare 2-CH, MCP2515)

1. Confirm SPI + MCP2515 overlays match your board revision (oscillator + IRQ GPIOs).
2. On the Pi, either run `sudo bash deploy/pi5/install_can_overlays.sh` or merge the lines from [`deploy/pi5/config.txt.example`](../../deploy/pi5/config.txt.example) into `/boot/firmware/config.txt`.
3. Reboot, then verify interfaces:

```bash
ip link show can0
ip link show can1
```

4. Install and enable the systemd unit:

```bash
sudo bash deploy/pi5/setup_pi5.sh
sudo systemctl start robot-can
```

Or run the setup script once, then bring up manually:

```bash
sudo ./deploy/pi5/can_setup.sh 1000000
```

## Install / deploy `cortex`

**Rename note:** Older installs used `rudyd.service`, `/etc/rudy/rudyd.toml`, and `/var/lib/rudyd/tailscale/`. The daemon is now **`cortex`** (`cortex.service`, `/etc/rudy/cortex.toml`, `/var/lib/rudy/cortex/tailscale/`). `deploy/pi5/apply-release.sh` and `deploy/pi5/install.sh` run a one-shot migration from the old paths on upgrade.

The Pi pulls prebuilt aarch64 releases from GitHub Actions on a 60-second
timer. You do not build on the Pi any more.

**One-time bootstrap on a fresh Pi (after Tailscale + cert):**

```bash
git clone https://github.com/jaylamping/rudy ~/rudy
sudo bash ~/rudy/deploy/pi5/bootstrap.sh
```

That installs `cortex-update.timer`, which polls the latest GitHub Release.
On every push to `main`, [`.github/workflows/release.yaml`](../../.github/workflows/release.yaml)
cross-builds `cortex` for `aarch64-unknown-linux-gnu`, bundles the SPA,
and publishes the tarball + `latest.json` manifest. Within ~60s of a green
build, the Pi downloads it (sha256-verified) and restarts `cortex`.

**Day-to-day:**

```bash
# Force an immediate update check
sudo systemctl start cortex-update

# Watch deploys land
journalctl -u cortex-update -f
journalctl -u cortex -f

# What commit is running?
cat /opt/rudy/current.sha
```

**Emergency build-on-Pi path (offline, no GHA):**

```bash
sudo bash deploy/pi5/install.sh   # legacy; compiles locally
```

## CAN debugging checklist

- Termination enabled only at bus ends.
- `candump can0` shows traffic when actuators are powered and addressed correctly.
- `ip -details -statistics link show can0` for error counters / bus-off recovery.

## Troubleshooting: a device won't show up under "Unassigned"

The flow that populates `GET /api/hardware/unassigned`:

```
device on bus → cortex passive RX or active scan → state.seen_can_ids → filter out anything in inventory → unassigned list
```

There is **no separate "removed devices" file or blacklist** to clean. `seen_can_ids` is in-memory only and entries TTL after 5 minutes. If a device isn't appearing, it's one of:

1. **An inventory row still claims that `(can_bus, can_id)`.** `present: false` does **not** free the slot — the row must be deleted (or its `can_id` changed). Use `DELETE /api/devices/:role` (preferred — also clears `seen_can_ids`) or edit `/var/lib/rudy/inventory.yaml` and `systemctl restart cortex` (cortex does **not** watch the file). Note: the live file is `/var/lib/rudy/inventory.yaml`, **not** `/opt/rudy/config/actuators/inventory.yaml` (which is just the seed).
2. **The bus isn't carrying frames cleanly.** See the next section.

## Troubleshooting: bus health (ERROR-PASSIVE / BUS-OFF)

A CAN controller transitions through three error states based on its TX/RX error counters:

| State | TEC threshold | Behaviour |
|-------|---------------|-----------|
| `ERROR-ACTIVE` | TEC < 128 | Healthy. Active error flags. |
| `ERROR-PASSIVE` | TEC ≥ 128 | Still TX/RX, but only passive error flags + 8-bit-time delay between transmissions. **Recovers automatically** once ACKs resume. |
| `BUS-OFF` | TEC ≥ 256 | Controller silent. With `restart-ms 100` (set by `can_setup.sh`) the kernel re-arms automatically; otherwise needs a manual `ip link` cycle. |

Check current state:

```bash
ip -details -statistics link show can0
# Look at: `can state ERROR-ACTIVE | ERROR-PASSIVE | BUS-OFF`
# RX/TX counters: zero RX with non-zero TX errors usually means "no node is ACKing"
```

The `error-warn` / `error-pass` / `bus-off` columns are **cumulative since interface bring-up**, not current state — don't be alarmed by non-zero history if `state` is `ERROR-ACTIVE`.

### When `ERROR-PASSIVE` won't go away

ERROR-PASSIVE only clears once another node successfully ACKs a frame. If nothing ever does, the controller stays passive forever. Likely causes:

- **Bad termination.** A CAN bus needs **exactly two 120Ω terminators**, one at each physical end. With everything powered off, measure CAN_H ↔ CAN_L:
  - `~60Ω` — two terminators (correct)
  - `~120Ω` — only one terminator (will work but marginal at 1 Mbit)
  - `open` / `40Ω` — zero / too many terminators
- **No live nodes on the bus.** `RX 0 packets` since boot means no node has ever transmitted. Either nothing is wired to that bus, the device(s) are unpowered, or the device's CAN_ID/bitrate is wrong.
- **RobStride termination footgun.** RS03 has two pass-through CAN connectors and only **one** has the onboard 240Ω terminator engaged (parallel with the bus's other 120Ω → ~80Ω combined, the standard impedance match). When you remove the bus-end RS03, you lose the terminator at that end. See `can_status` (param `0x3041`) on each motor to see which connector is currently terminated, and re-plan the chain so the bus-end motor is on its terminated connector.

### Recovering a degraded bus

```bash
# Stop cortex so it isn't holding the AF_CAN sockets
sudo systemctl stop cortex

# Bounce both interfaces (this re-applies bitrate, restart-ms, txqueuelen, etc.)
sudo systemctl restart robot-can

# Verify state and settings
ip -details link show can0 | grep -E 'state|restart-ms|bitrate'
ip -details link show can1 | grep -E 'state|restart-ms|bitrate'
# Expect: state ERROR-ACTIVE, restart-ms 100, bitrate 1000000

sudo systemctl start cortex
```

⚠️ Bouncing CAN interfaces while cortex is running will break its open sockets and the bus workers won't auto-reconnect. Always stop cortex first.

### After-an-accident checklist

When a motor was physically removed (impact, swap, RMA) and the new one isn't being discovered:

1. **Inventory clean?** `grep -n "can_id: 9\|can_id: 0x9\|can_id: 0x09" /var/lib/rudy/inventory.yaml` (substitute the right ID). If anything matches, remove via `DELETE /api/devices/:role` or hand-edit + restart cortex.
2. **Bus electrically healthy?** `ip -details -statistics link show can0` — must be `ERROR-ACTIVE` and have non-zero RX traffic.
3. **Termination physically correct?** Measure 60Ω across the bus with everything off; ensure the bus-end motor sits on its terminated connector.
4. **Motor's CAN_ID is what you expect?** An impact or accidental factory-reset can wipe CAN_ID back to default (typically `0x7F`). Run a wide scan to find it:
   ```bash
   curl -sS -X POST http://localhost:8443/api/hardware/scan \
     -H 'content-type: application/json' \
     -d '{"id_min": 1, "id_max": 127, "timeout_ms": 1000}' | jq '.message, .discovered'
   ```
   If it appears at an unexpected ID, set CAN_ID back to your intended value via Motor Studio (or a CAN-side tool) and **save to flash** before onboarding.
5. **Still nothing?** With cortex stopped, `candump can0` while you power-cycle the motor — RobStrides emit a frame on boot. No frame at all = motor / wiring / bitrate / power problem, not a cortex problem.
