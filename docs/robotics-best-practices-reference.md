# Robotics research and engineering reference (Rudy)

**Canonical bibliography for the repo.** Cursor agents: read this file (and `docs/research/`) for conceptual answers; cite URLs below.

**Last expanded:** April 2026.  
**How this refresh was gathered:** targeted web research plus the [Semantic Scholar Graph API](https://api.semanticscholar.org/api-docs/) (no key required; rate limits apply). When your Cursor session has **Tavily** and **Semantic Scholar** MCP enabled (`docs/mcp-research-stack.md`), prefer those tools for live searches so results stay current.

**Offline snapshots:** `docs/research/firecrawl-exports-2026-04-15/` (older page copies). Prefer canonical links here when they conflict.

---

## 1. Kinematics, dynamics, and whole-body control

### 1.1 Surveys and architecture (humanoids / WBC)

- **[A Survey of Behavior Foundation Models for humanoid whole-body control](https://arxiv.org/html/2506.20487v5)** (Yuan et al., arXiv HTML) — BFMs for WBC: pipelines, adaptation, limitations; links to [awesome-bfm-papers](https://github.com/yuanmingqi/awesome-bfm-papers).
- **[Whole-Body Control for Humanoid Robots: Architectures, Optimization Back-Ends, and Benchmarking](https://jmesopen.com/index.php/jmesopen/article/view/25)** — Review of WBC architectures, QP / optimization backends, benchmarking (frames 2015–2025 trends including RL integration).
- **[PhysMoDPO: Physically-plausible humanoid motion](https://arxiv.org/html/2603.13228v1)** — Recent learned motion generation with physics plausibility (useful when tying kinematics to sim).
- **NeurIPS 2025 (example):** [From Experts to a Generalist: general WBC for humanoids](https://neurips.cc/virtual/2025/poster/117371) — Learning-oriented WBC direction.

### 1.2 Recent highly cited WBC-style papers (Semantic Scholar snapshot, 2025)

Use these as entry points to related work (titles as indexed on Semantic Scholar):

| Paper (short) | Semantic Scholar |
|----------------|------------------|
| AMO: Adaptive Motion Optimization for Hyper-Dexterous Humanoid Whole-Body Control | https://www.semanticscholar.org/paper/974b2739bd0e746821f20752b2dcd563ca7946c9 |
| KungfuBot: Physics-Based Humanoid Whole-Body Control for Learning Highly-Dynamic Skills | https://www.semanticscholar.org/paper/5f0a671d8f2f61c2ba95879a60d2e563048c74bb |
| GMT: General Motion Tracking for Humanoid Whole-Body Control | https://www.semanticscholar.org/paper/d920e10667a298a90739b89aadaa4e67d9b00713 |
| SONIC: Supersizing Motion Tracking for Natural Humanoid Whole-Body Control | https://www.semanticscholar.org/paper/13b83398bbe64954d8e49e1102118870753c161f |
| LeVERB: Humanoid Whole-Body Control with Latent Vision-Language Instruction | https://www.semanticscholar.org/paper/2908c90439041ccd829a8666fa8073b6b85bde0e |

### 1.3 Software: rigid-body kinematics / dynamics stacks

- **[Pinocchio](https://github.com/stack-of-tasks/pinocchio)** — Fast RBD algorithms, derivatives; URDF / MJCF / SRDF; [docs](https://gepettoweb.laas.fr/doc/stack-of-tasks/pinocchio/devel/doxygen-html/), [HAL paper PDF](https://laas.hal.science/hal-01866228v1/document).
- **[Drake](https://drake.mit.edu/)** — Multibody dynamics, trajectory optimization, contact-rich modeling (complements MuJoCo/Isaac in Rudy’s planned stack).
- **[Orocos KDL](https://www.orocos.org/kdl.html)** — Classic chain kinematics; still referenced across ROS ecosystems ([ROS index](https://index.ros.org/p/orocos_kdl/)).

### 1.4 Task / motion planning (optimization angle)

- **[A Survey of Optimization-based Task and Motion Planning](https://arxiv.org/html/2404.02817v4)** — Classical → learning TAMP; constraints, dynamics interaction (relevant to joint limits + obstacles).
- **[Motion planning for manipulators in dynamic environments (2024)](https://onlinelibrary.wiley.com/doi/10.1155/2024/5969512)** — Review article (Wiley Journal of Sensors).
- **Teaching / intuition:** [MIT manipulation — trajectories / motion planning](http://manipulation.csail.mit.edu/trajectories.html) — joint limits and non-penetration as constraints in planning narrative.
- **Blog explainer:** [How do robot manipulators move?](https://roboticseabass.com/2024/06/30/how-do-robot-manipulators-move/) — joint limits, collisions, planners (accessible).

---

## 2. Joint limits, torque, safety, and `ros2_control`

### 2.1 URDF and “paper” robot description

- **[URDF `<joint>` spec (ROS Wiki)](http://wiki.ros.org/urdf/XML/joint)** — `limit`, `dynamics`, `safety_controller`, `mimic`.
- **[PR2 safety limits (historical but still the mental model for soft limits)](http://wiki.ros.org/pr2_controller_manager/safety_limits)** — k_velocity, k_position, soft band inside hard limits.
- **[Beyond URDF (arXiv)](https://arxiv.org/html/2512.23135v1)** — Next-generation description formats (when URDF hits limits).

### 2.2 ROS 2 control: where limits actually get enforced

- **[Joint limiting for ros2_control (control.ros.org, Jazzy)](https://control.ros.org/jazzy/doc/ros2_control/hardware_interface/doc/joint_limiting.html)** — Official behavior and configuration entry points.
- **[Joint limiting (Rolling hardware_interface)](https://docs.ros.org/en/rolling/p/hardware_interface/doc/joint_limiting.html)** — Same material, Rolling path.
- **[joint_limits namespace reference](https://control.ros.org/rolling/doc/api/namespacejoint__limits.html)** — Declare/get limits, acceleration bounds, etc.
- **[Controller manager user doc](https://control.ros.org/rolling/doc/ros2_control/controller_manager/doc/userdoc.html)** — URDF / `robot_description` interaction, parameters.
- **[How to use ros2_control joint_limits with MoveIt 2 (StackExchange)](https://robotics.stackexchange.com/questions/107983/how-to-use-ros2-controls-joint-limits)** — **MoveIt `joint_limits.yaml` vs `ros2_control`** are separate; changing one does not update the other — plan both.
- **[ros2_control demos](https://control.ros.org/rolling/doc/ros2_control_demos/doc/index.html)** — Includes joint limits / transmission examples.
- **Design discussion:** [JointLimitsInterface in components (GitHub #279)](https://github.com/ros-controls/ros2_control/issues/279) — integration history and pitfalls.

### 2.3 Pedagogy

- **[Joint limits and dynamics (Leyaa deep dive)](https://leyaa.ai/core-engineering/learn/ros/part-2/ros-joint-limits-and-dynamics/deep)** — position + velocity + effort; soft limits; sim vs real.

**Rudy practice:** keep limits in URDF as source of truth, mirror or override in YAML where `ros2_control` / MoveIt require it, and document the mapping in package READMEs.

---

## 3. Power distribution, batteries, and electromechanical architecture

### 3.1 Humanoid / legged context (vendor + trade press)

- **[Infineon: Humanoid robots](https://www.infineon.com/applications/robotics/humanoid-robots)** — Power stages, gate drivers, BMS ICs, CAN transceivers, MCUs (useful block-diagram thinking even if you do not use their silicon).
- **[STMicroelectronics: Humanoid Robot Reference Guide (PDF)](https://www.st.com/resource/en/brochure/humanoid-robot-reference-guide.pdf)** — Compact design, thermal, dynamic load framing for power distribution.
- **[Powering humanoid robots — battery technology survey article](https://www.the-innovation.org/article/doi/10.59717/ipj.energy-use.2026.100032)** — Batteries as bottleneck; architectures and BMS emphasis (open-access style venue — verify citation requirements for your own papers).

### 3.2 BMS and pack-level design (robots, not grid)

- **[Robot BMS challenges (AYAA / industry article)](https://www.ayaatech.com/news/top-challenges-in-robot-battery-management-and-how-a-robot-bms-solves-them/)** — SoC/SoH, safety, current profiling.

### 3.3 General distribution engineering (when scaling beyond hobby DC buses)

- **[Eaton: Basics of power system design](https://www.eaton.com/us/en-us/support/business-resources/consultants-engineers/consultant---engineer-resources-for-medium-voltage-power---eaton/power-distribution-design-guide.html)** — Protection, coordination, grounding discipline (analogy: branch protection and coordination for robot DC trees).

**Rudy practice:** separate **logic/compute power** from **high‑current actuator rails**, define **inrush / precharge** where bulk capacitance exists, document **fuse or electronic breaker** per branch, and keep **CAN / data** galvanic and routing plan alongside power (see §4).

---

## 4. Data distribution, fieldbuses, and “robot backbone”

### 4.1 CAN (Rudy’s near-term actuator bus)

- Still the default for many integrated actuators; pair with **clear ID allocation**, **bus loading analysis**, and **fault containment** (stubs, terminators, segmented harnesses).

### 4.2 EtherCAT and real-time Ethernet (when you outgrow CAN bandwidth)

- **[EtherCAT Technology Group](https://www.ethercat.org/en/tech_group.html)** — Specs and vendor ecosystem.
- **[What is EtherCAT (Dewesoft explainer)](https://dewesoft.com/blog/what-is-ethercat-protocol)** — deterministic frame processing mental model.
- **[Selecting the best network for robot control (Embedded)](https://www.embedded.com/selecting-the-best-network-for-robot-control/)** — CANopen / CAN FD vs Ethernet-class buses for motion.

### 4.3 TSN and converged IT/OT backbones

- **[TSN for industrial automation (intro)](https://maisvch.com/blog/tsn-time-sensitive-networking-industrial-automation/)** — IEEE 802.1AS / Qbv vocabulary.
- **[TSN as backbone for digital manufacturing (Control Engineering)](https://www.controleng.com/why-time-sensitive-networking-tsn-is-the-backbone-of-next-gen-digital-manufacturing/)** — Why determinism on Ethernet matters when cameras + control share wire.
- **[Real-time Ethernet for cobots — TSN vs EtherCAT vs PROFINET (2026 outlook)](https://promwad.com/news/real-time-ethernet-collaborative-robots-tsn-ethercat-profinet-2026)** — Trade-space article for picking a backbone.

### 4.4 Fieldbus design references (branching, spurs, power on the bus)

- **[Rockwell FOUNDATION Fieldbus design (PDF)](https://literature.rockwellautomation.com/idc/groups/literature/documents/rm/rsfbus-rm001_-en-p.pdf)** — Classic trunk/spur and power distribution discipline (analogous to robot harness design reviews).
- **[Fieldbus selection (GlobalSpec)](https://www.globalspec.com/learnmore/industrial_computers_embedded_systems/industrial_computing/fieldbus_products)** — Protocol comparison tables.

**Rudy practice:** document **data rate, worst-case latency, cable count, and failure modes** (open circuit, short, stuck dominant) alongside **power tree** in `docs/` or package README when hardware stabilizes.

---

## 5. ROS 2 software architecture and design patterns

- **[ROS 2 design articles (official)](https://design.ros2.org/)** — DDS, executors, QoS, and historical rationale.
- **[ROS on DDS (design article)](https://design.ros2.org/articles/ros_on_dds.html)** — Middleware mental model.
- **[Programming multiple robots with ROS 2 — design patterns](https://osrf.github.io/ros2multirobotbook/ros2_design_patterns.html)** — Topics, services, actions, parameters, callbacks.
- **[Design guide: common patterns in ROS 2](https://docs.ros.org/en/rolling/The-ROS2-Project/Contributing/Design-Guide.html)** — Composable nodes, large messages, patterns (Rolling path; pick your distro’s equivalent).
- **[ROS 2 Developer Guide](https://docs.ros.org/en/rolling/The-ROS2-Project/Contributing/Developer-Guide.html)** — SemVer, DCO, docs, REP 2004 quality categories.
- **[ROS 2 code style](https://docs.ros.org/en/rolling/The-ROS2-Project/Contributing/Code-Style-Language-Versions.html)** — C++ / Python norms.
- **Industrial survey PDF (ResearchGate landing):** [Current and emerging techniques in robotic software design: ROS 2, DDS, architectures](https://www.researchgate.net/publication/401165556_Current_and_Emerging_Techniques_in_Robotic_Software_Design_for_Industrial_Deployment_ROS_2_DDS_and_Software_Architectures) — sense–plan–act, pub/sub, microservice angles.
- **Real-time:** [A survey of real-time support and analysis in ROS 2 (arXiv PDF)](https://arxiv.org/pdf/2601.10722) — scheduling, executors, timing (read alongside your RTOS / PREEMPT_RT choices if any).

### 5.1 Henki-style project hygiene (still the default for Rudy)

- **[Henki ROS 2 Best Practices (GitHub)](https://github.com/henki-robotics/henki_ros2_best_practices)** — Single-responsibility nodes, XML launch, YAML params, logging discipline, messages vs services vs actions, testing split, `rosdep`.
- **[Henki blog](https://henkirobotics.com/ros-2-best-practices/)** — Narrative and agent examples.

---

## 6. Motion planning, MoveIt, safety culture

- **[MoveIt (PickNik)](https://picknik.ai/moveit/)** / **[moveit.ai](https://moveit.ai/)** — Planning, IK, collision, perception.
- **[MoveIt 2 + ros2_control joint limits discussion](https://robotics.stackexchange.com/questions/107983/how-to-use-ros2-controls-joint-limits)** — practical integration.
- **[MoveIt + external safety (GitHub discussion)](https://github.com/ros-planning/moveit/issues/1958)** — bridging safety PLCs / torque monitoring.
- **[OSHA robotics directive](http://www.osha.gov/enforcement/directives/std-01-12-002)** — compliance framing.
- **[Robot safety for (not) dummies](https://sixdegreesofrobotics.substack.com/p/robot-safety-for-not-dummies)** — force / pain thresholds, ISO/TS 15066 framing.
- **[Safety in robotic systems (Wiley)](https://onlinelibrary.wiley.com/doi/10.1002/rob.70022?af=R)** — lifecycle risk survey.

---

## 7. Simulation, CI, and “don’t fly blind” engineering

- **[Robotics Knowledgebase — choose a simulator](https://roboticsknowledgebase.com/wiki/robotics-project-guide/choose-a-sim/)** — Gazebo, Isaac Lab, MuJoCo, PyBullet trade space.
- **[best-of-robot-simulators](https://github.com/knmcguire/best-of-robot-simulators)** — Curated list.
- **[Nine physics engines for RL (arXiv)](https://arxiv.org/html/2407.08590v1)** — comparative study.
- **[Robot simulation software — 2026 perspective](https://www.blackcoffeerobotics.com/blog/which-robot-simulation-software-to-use)** — editorial snapshot.
- **CI:** [CI/CD for robotics (agROBOfood)](https://agrobofood.github.io/agrobofood-case-studies/case_studies/CICD-for-robotics.html), [cloud sim + GitHub Actions (PMC)](https://pmc.ncbi.nlm.nih.gov/articles/PMC11945058/), [basis (deterministic testing)](https://github.com/basis-robotics/basis), [ACT testing (arXiv)](https://arxiv.org/html/2604.11708v1), [r/robotics CI thread](https://www.reddit.com/r/robotics/comments/17k63yy/cicd_for_your_robot/).

---

## 8. Robot programming methodology (non-ROS)

- **[Robot programming best practices](https://intelligentintegrators.org/robot-programming-best-practices/)** — homing, modular structure, verification matrix.

---

## 9. Rudy stack alignment (intent)

| Layer | Direction |
|-------|-----------|
| Description | URDF / xacro in `rudy_description` |
| Bringup | XML launch + YAML params in `rudy_bringup` |
| Interfaces | `rudy_msgs` |
| Planning / control | MoveIt 2 + `ros2_control` (phased); keep URDF ↔ YAML limit parity documented |
| Sim-to-real | MuJoCo + Isaac Lab (per project plan) |
| Performance nodes | `ros2_rust` where appropriate |

---

## 10. How to use with Cursor

- **Humans:** bookmark this file; update Tier sections when you change architecture.
- **Agents:** `alwaysApply` rule in `.cursor/rules/robotics-reference.mdc` points here — read **this file** for depth; use **Tavily + Semantic Scholar MCP** when enabled (`docs/mcp-research-stack.md`) to refresh citations before major design decisions.
