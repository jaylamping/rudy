# Joe notes

Personal Rudy bring-up folder: practical steps, safety reminders, and learning links for getting from a Jetson Orin Nano box to a slow right-arm wave.

## Start here

1. Read `box-to-wave-roadmap.md`.
2. Keep `wave-day-checklist.md` open during hardware testing.
3. Use `isaac-jetson-learning-path.md` for simulation, Jetson, and model-runtime study.
4. Use `resource-links.md` when you need setup docs or videos.

## Non-negotiable boundary

The inference board never controls motors directly.

```text
Jetson model or app
  -> intent proposal
  -> human or policy approval
  -> cortex primitive / motion API
  -> cortex safety gates + audit
  -> SocketCAN
  -> RobStride actuators
```

No LLM process opens CAN, publishes raw joint commands, streams trajectories, or bypasses `cortex`.

## Current repo anchors

- Architecture: `docs/architecture.md`
- Pi bring-up: `docs/runbooks/pi5.md`
- Operator console: `docs/runbooks/operator-console.md`
- Commissioning: `tools/robstride/commission.md`
- Actuator inventory: `config/actuators/inventory.yaml`
- RS03 hardware spec: `config/actuators/robstride_rs03.yaml`
- Motion API: `crates/cortex/src/api/motion/run.rs`
- Motion controller: `crates/cortex/src/motion/controller.rs`
- Isaac / sim runbook: `docs/runbooks/isaac_lab.md`

## First wave success definition

Rudy passes the first wave demo when one verified right-arm joint oscillates slowly inside a narrow travel band for 5-10 seconds, stops on command, logs cleanly, and shows no CAN, current, thermal, or firmware fault regression.
