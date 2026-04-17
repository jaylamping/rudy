# tests

Integration tests for Rudy.

## `launch_testing`

- `test/test_robot_state_headless.launch.py` — includes `bringup` headless launch (robot state + joint states, no RViz)

## Notes

GPU-backed sim tests intentionally live outside default CI; keep them behind optional markers/jobs when added.
