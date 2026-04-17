# simulation

Isaac Lab / Isaac Sim integration scaffold.

## Layout

- `configs/` — domain randomization, contact, and actuator dynamics YAML (version-controlled)
- `simulation/envs/sim_env.py` — stub env API (no Isaac import at module import time)
- `launch/sim_stub.launch.xml` — placeholder launch (Isaac is typically host-managed)

## Console scripts

- `sim_train` — scaffold entrypoint (`simulation.scripts.train:main`)
