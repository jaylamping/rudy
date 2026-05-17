# Runbook: Isaac Lab / Isaac Sim (Rudy desktop)

## Preconditions

- **NVIDIA GPU** workstation with a supported Isaac Sim install (see NVIDIA docs for your distro).
- **Python**: Isaac Sim commonly pins **Python 3.11**; keep training envs aligned with Isaac Sim release notes.
- **ROS 2**: Jazzy sourced for ROS bridges / message generation used during sim bring-up.

## Repo layout

- **Configs**: `ros/src/simulation/configs/` (`domain_rand.yaml`, `contact.yaml`, `actuator_model.yaml`)
- **Scenarios**: `ros/src/simulation/configs/scenarios/` (ADR-0009 replay catalog)
- **Contract**: `ros/src/simulation/simulation/schema.py` (simulator-neutral command/state/report boundary)
- **Reports**: `scripts/sim2sim_compare.py` (Isaac-vs-MuJoCo JSON report writer)
- **Scaffold env**: `ros/src/simulation/simulation/envs/sim_env.py`
- **Training entrypoint**: `sim_train` console script (scaffold)

## Policy

- **URDF** in `description` remains the kinematic source of truth imported into sim.
- **Actuator spec** in `config/actuators/robstride_rs03.yaml` must inform sim actuator dynamics (see parity tests in `tests/`).

## Next wiring steps (intentionally not automated here)

1. Import Rudy USD/URDF into Isaac Lab task config.
2. Match observation/action spaces to **on-robot** sensors only (no privileged teacher features in deployment policy).
3. Make Isaac and MuJoCo adapters emit `SimState` JSON traces for `joint_space_smoke`.
4. Add deterministic regression tests that run in CI without a GPU (YAML + unit tests), and optional nightly GPU training jobs.