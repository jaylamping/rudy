# Research exports (offline copies)

This folder preserves material gathered for Rudy’s architecture and best-practices baseline so it is not lost if URLs move or paywalls appear.

**Curated index:** the up-to-date, organized bibliography lives in [`../robotics-best-practices-reference.md`](../robotics-best-practices-reference.md) (April 2026 refresh). Use this folder for raw captures; use that file for navigation and Rudy-specific notes.

## Contents

Subfolder **`firecrawl-exports-2026-04-15/`** contains:

| File | Description |
|------|-------------|
| `henki-ros2.md` | Henki Robotics blog: ROS 2 best practices summary (nodes, launch XML, params, logging, messages, executors, testing). |
| `henki-github.md` | GitHub landing page scrape: [henki_ros2_best_practices](https://github.com/henki-robotics/henki_ros2_best_practices) repo overview. |
| `ros2-dev-guide.md` | Official ROS 2 developer guide (Rolling): versioning, DCO, CI, documentation requirements. |
| `urdf-joint-spec.md` | ROS Wiki: URDF `<joint>` element (limits, dynamics, safety_controller, mimic). |
| `joint-limits-deep.md` | Leyaa deep dive: joint limits vs dynamics, ROS enforcement, Gazebo, pitfalls. |
| `programming-best-practices.md` | Intelligent Integrators: homing, modular programs, verification. |
| `robotics-knowledgebase-sims.md` | Robotics Knowledgebase: simulator selection, physics, URDF in sim, Gazebo / RL tools. |
| `moveit-overview.md` | PickNik MoveIt overview page. |
| `search-*.json` | Firecrawl search JSON (web results metadata; large files). |

## Usage

- Prefer the **canonical URLs** in [../robotics-best-practices-reference.md](../robotics-best-practices-reference.md) for citations and linking.
- Use these exports for **offline reading**, diffing, or audit trails. They may drift from the live site over time.
- Third-party content remains under original authors’ copyrights; redistribution here is for **project-internal reference** only.

## Regenerating

To refresh from the web (requires [Firecrawl](https://firecrawl.dev/) CLI auth), re-run searches/scrapes and copy new outputs into a dated subfolder following the same naming pattern.
