# Open-source robotics coding: best practices and resources

Curated reference for Murphy (ROS 2, sim-to-real, manipulation). Use this document plus `research/firecrawl-exports-2026-04-15/` for offline copies.

---

## Tier 1 — Foundations (read first)

### ROS 2 architecture and project patterns

- **[Henki ROS 2 Best Practices (GitHub)](https://github.com/henki-robotics/henki_ros2_best_practices)** — Single-document guidelines: single-responsibility nodes; separate ROS I/O from core logic; **prefer XML launch**; parameters in YAML, not hard-coded; logging levels and throttling; proper message types (avoid deprecated `std_msgs` primitives); **services** for fast work, **actions** for long/cancellable work; Google C++ / PEP 8; C++ for high-bandwidth paths; composable nodes; use `rosdep`; document topics/services/actions/params; **unit-test core logic**, integration-test ROS I/O.
- **[Henki blog: ROS 2 Best Practices](https://henkirobotics.com/ros-2-best-practices/)** — Narrative + before/after agent examples.
- **[ROS 2 Developer Guide (official)](https://docs.ros.org/en/rolling/The-ROS2-Project/Contributing/Developer-Guide.html)** — SemVer, DCO, REP 2004 quality tiers, deprecation, public API, README contents.
- **[ROS 2 Code style](https://docs.ros.org/en/rolling/The-ROS2-Project/Contributing/Code-Style-Language-Versions.html)** — C++ and Python conventions referenced by Henki.

### URDF, joint limits, dynamics

- **[URDF `<joint>` (ROS Wiki)](http://wiki.ros.org/urdf/XML/joint)** — `limit` (lower, upper, effort, velocity), `dynamics`, `safety_controller` (soft limits, k_velocity, k_position), `mimic`, joint types.
- **[Joint limits and dynamics (Leyaa)](https://leyaa.ai/core-engineering/learn/ros/part-2/ros-joint-limits-and-dynamics/deep)** — Position, velocity, and effort limits; controllers do not automatically enforce limits unless configured; soft limits; sim vs hardware.

### Robot programming methodology (non-ROS-specific)

- **[Robot programming best practices](https://intelligentintegrators.org/robot-programming-best-practices/)** — Homing first (position-aware, stepped), modular main + subprograms with gated conditions, rigorous verification / failure modes.

---

## Tier 2 — Motion planning, safety, control

### Motion planning (ROS)

- **[MoveIt (PickNik)](https://picknik.ai/moveit/)** / **[moveit.ai](https://moveit.ai/)** — Manipulation stack for ROS: planning, IK, collision, perception.
- **[MoveIt tutorials (Kinetic-era index; use ROS 2 distro docs for current)](http://docs.ros.org/en/kinetic/api/moveit_tutorials/html/index.html)** — Historical index; prefer current distro MoveIt 2 tutorials.
- **[Configure MoveIt 2 for simulated arm (Automatic Addison, Jazzy)](https://automaticaddison.com/configure-moveit-2-for-a-simulated-robot-arm-ros-2-jazzy/)** — Practical setup walkthrough.
- **[MoveIt safety / external safety discussion (GitHub)](https://github.com/ros-planning/moveit/issues/1958)** — Integrating external safety with MoveIt.

### Safety standards and reading

- **[OSHA robotics safety directive](http://www.osha.gov/enforcement/directives/std-01-12-002)** — Regulatory framing for industrial robotics.
- **[Robot safety for (not) dummies (Substack)](https://sixdegreesofrobotics.substack.com/p/robot-safety-for-not-dummies)** — Force thresholds, human contact, ISO/TS 15066 context.
- **[Safety considerations in robotic systems (Wiley)](https://onlinelibrary.wiley.com/doi/10.1002/rob.70022?af=R)** — Survey paper on risk across lifecycle.

---

## Tier 3 — Simulation and testing

### Choosing simulators

- **[Robotics Knowledgebase: Choose a simulator](https://roboticsknowledgebase.com/wiki/robotics-project-guide/choose-a-sim/)** — Gazebo vs AirSim vs CoppeliaSim vs Unity; RL side MuJoCo vs PyBullet vs Isaac Lab; URDF in sim; integration questions.
- **[best-of-robot-simulators (GitHub)](https://github.com/knmcguire/best-of-robot-simulators)** — Curated list of simulator projects.

### CI/CD and automated testing

- **[CI/CD for robotics (agROBOfood case study)](https://agrobofood.github.io/agrobofood-case-studies/case_studies/CICD-for-robotics.html)** — GitLab CI patterns for robotics software.
- **[Cloud simulation + CI (PMC article)](https://pmc.ncbi.nlm.nih.gov/articles/PMC11945058/)** — GitHub Actions, AWS RoboMaker-style pipelines.
- **[basis-robotics/basis (GitHub)](https://github.com/basis-robotics/basis)** — Framework emphasizing deterministic testing.
- **[ACT: Automated CPS testing (arXiv)](https://arxiv.org/html/2604.11708v1)** — Continuous testing for open-source robotic platforms.
- **[r/robotics: CI/CD for your robot (Reddit)](https://www.reddit.com/r/robotics/comments/17k63yy/cicd_for_your_robot/)** — Practitioner discussion.

### Simulator / engine comparisons (search hits worth bookmarking)

- **[Nine physics engines for RL (arXiv)](https://arxiv.org/html/2407.08590v1)** — Comparative review.
- **[Robot simulation software — 2026 perspective (Black Coffee Robotics)](https://www.blackcoffeerobotics.com/blog/which-robot-simulation-software-to-use)** — Landscape article.

---

## Tier 4 — Advanced topics

### Robot description beyond plain URDF

- **[Beyond URDF (arXiv)](https://arxiv.org/html/2512.23135v1)** — Research on richer robot description.
- **[URDF to Xacro workflow (RealMan)](https://develop.realman-robotics.com/en/symbiosis/demo/URDFmodelassembly/)** — Parameterized models.

### Frameworks (sim, dynamics, learning)

- **[Drake](https://drake.mit.edu/)** — Dynamics and optimization.
- **[MuJoCo (GitHub)](https://github.com/deepmind/mujoco)** — Contact-rich sim, RL-friendly.
- **[Isaac Lab (GitHub)](https://github.com/isaac-sim/IsaacLab)** — GPU-parallel training on Isaac Sim.

### Production-oriented ROS 2

- **[ROS 2 modular architecture (Softeq)](https://www.softeq.com/blog/ros2-modular-architecture-scalable-robotics-business)** — Scaling teams/products.
- **[Multiple ROS 2 tasks orchestration (Robotics StackExchange)](https://robotics.stackexchange.com/questions/117654/best-practices-for-managing-and-orchestrating-multiple-ros-2-tasks-efficiently)** — SLAM + nav + mapping coordination.

---

## Murphy stack alignment (intent)

| Layer | Direction |
|-------|-----------|
| Description | URDF / xacro in `murphy_description` |
| Bringup | XML launch + YAML params in `murphy_bringup` |
| Interfaces | `murphy_msgs` (custom types as needed) |
| Sim-to-real | MuJoCo + Isaac Lab + MoveIt 2 (phased) |
| Performance nodes | `ros2_rust` where appropriate |

---

## How to use this with Cursor

The same link list is in [`.cursor/rules/robotics-reference.mdc`](../.cursor/rules/robotics-reference.mdc) with `alwaysApply: true` so agents consult it for conceptual questions. This `docs/` copy is the **human-readable canonical** version to version in Git and share in PRs.
