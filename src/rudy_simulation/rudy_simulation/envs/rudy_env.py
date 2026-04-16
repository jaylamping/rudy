# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Isaac Lab environment scaffold for Rudy.

This module intentionally avoids importing Isaac Lab at import time so `pytest` can run
on machines without Isaac Sim installed. Import Isaac Lab inside methods when wiring sim.
"""


class RudyEnvStub:
    """Placeholder env API; replace with Isaac Lab `DirectRLEnv` / task config."""

    def __init__(self, cfg_path: str | None = None) -> None:
        self.cfg_path = cfg_path

    def reset(self) -> dict:
        return {"obs": [], "privileged": []}

    def step(self, action):  # type: ignore[no-untyped-def]
        return {"obs": [], "reward": 0.0, "done": False}
