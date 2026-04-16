# murphy_simulation

Isaac Lab / Isaac Sim integration scaffold.

## Layout

- `configs/` — domain randomization, contact, and actuator dynamics YAML (version-controlled)
- `murphy_simulation/envs/murphy_env.py` — stub env API (no Isaac import at module import time)
- `launch/sim_stub.launch.xml` — placeholder launch (Isaac is typically host-managed)

## Console scripts

- `murphy_train` — scaffold entrypoint (`murphy_simulation.scripts.train:main`)
