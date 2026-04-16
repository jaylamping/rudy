# rudy_simulation

Isaac Lab / Isaac Sim integration scaffold.

## Layout

- `configs/` — domain randomization, contact, and actuator dynamics YAML (version-controlled)
- `rudy_simulation/envs/rudy_env.py` — stub env API (no Isaac import at module import time)
- `launch/sim_stub.launch.xml` — placeholder launch (Isaac is typically host-managed)

## Console scripts

- `rudy_train` — scaffold entrypoint (`rudy_simulation.scripts.train:main`)
