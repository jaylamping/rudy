#!/usr/bin/env python3
# Copyright 2026 Rudy contributors
# SPDX-License-Identifier: Apache-2.0

"""Repo-level wrapper for simulation.compare."""

from __future__ import annotations

import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
SIMULATION_SRC = REPO_ROOT / "ros" / "src" / "simulation"
sys.path.insert(0, str(SIMULATION_SRC))

from simulation.compare import main  # noqa: E402


if __name__ == "__main__":
    main()
