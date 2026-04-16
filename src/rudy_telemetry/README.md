# rudy_telemetry

Operational telemetry helpers for Rudy.

## Contents

- `config/analyzers.yaml` — starter `diagnostic_aggregator` configuration (extend per subsystem)
- `launch/diagnostics.launch.xml` — runs `diagnostic_aggregator`
- `launch/record_core.launch.xml` — `ros2 bag record` for core topics (`/joint_states`, `/tf`, `/diagnostics`, …)
