# simulation

Isaac Lab / Isaac Sim integration scaffold.

## Layout

- `configs/` — domain randomization, contact, and actuator dynamics YAML (version-controlled)
- `configs/scenarios/` — ADR-0009 joint-space replay scenarios shared by simulator adapters
- `simulation/schema.py` — simulator-neutral command/state/report dataclasses
- `simulation/compare.py` — sim-to-sim trace comparison and JSON report writer
- `simulation/envs/sim_env.py` — stub env API (no Isaac import at module import time)
- `launch/sim_stub.launch.xml` — placeholder launch (Isaac is typically host-managed)

## Console scripts

- `sim_train` — scaffold entrypoint (`simulation.scripts.train:main`)
- `sim2sim_compare` — build a machine-readable ADR-0009 report from Isaac/MuJoCo traces

Repo wrapper:

```bash
python3 scripts/sim2sim_compare.py \
  --scenario ros/src/simulation/configs/scenarios/joint_space_smoke.yaml \
  --isaac-trace /path/to/isaac_trace.json \
  --mujoco-trace /path/to/mujoco_trace.json \
  --out /tmp/sim_report.json
```
