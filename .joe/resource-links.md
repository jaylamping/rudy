# Resource links

Educational links for Jetson bring-up, Rudy motion, CAN, ROS, and videos. Prefer official docs first, then videos for intuition.

Note: this list uses stable landing pages and search links where exact video titles may move.

## Jetson Orin Nano setup

- NVIDIA Jetson Orin Nano Developer Kit getting started: https://developer.nvidia.com/embedded/learn/get-started-jetson-orin-nano-devkit
- NVIDIA Jetson downloads: https://developer.nvidia.com/embedded/downloads
- NVIDIA Jetson docs landing: https://docs.nvidia.com/jetson/
- Jetson Linux Developer Guide archive R36.4.4: https://docs.nvidia.com/jetson/archives/r36.4.4/DeveloperGuide/
- Jetson Linux quick start R36.4.4: https://docs.nvidia.com/jetson/archives/r36.4.4/DeveloperGuide/IN/QuickStart.html
- Jetson Orin power/performance R36.4.4: https://docs.nvidia.com/jetson/archives/r36.4.4/DeveloperGuide/SD/PlatformPowerAndPerformance/JetsonOrinNanoSeriesJetsonOrinNxSeriesAndJetsonAgxOrinSeries.html
- NVIDIA Jetson AI Lab: https://www.jetson-ai-lab.com/
- Jetson AI Lab GenAI on Jetson: https://www.jetson-ai-lab.com/tutorials/genai-on-jetson-llms-vlms
- Jetson AI Lab models: https://www.jetson-ai-lab.com/models
- NVIDIA Jetson containers: https://catalog.ngc.nvidia.com/orgs/nvidia/containers/l4t-pytorch
- Jetson Containers project: https://github.com/dusty-nv/jetson-containers
- Tailscale Linux install: https://tailscale.com/kb/1031/install-linux

## Jetson videos and channels

- NVIDIA Developer channel, Jetson search: https://www.youtube.com/@NVIDIADeveloper/search?query=Jetson%20Orin%20Nano
- NVIDIA Jetson AI Lab search: https://www.youtube.com/results?search_query=NVIDIA+Jetson+AI+Lab+Orin+Nano
- JetsonHacks channel: https://www.youtube.com/@JetsonHacks
- JetsonHacks site: https://jetsonhacks.com/
- Practical first queries:
  - `Jetson Orin Nano first boot JetPack`
  - `Jetson Orin Nano Tailscale`
  - `Jetson Orin Nano local LLM`
  - `Jetson Orin Nano camera CSI`

## Rudy repo docs

- Repo architecture: `docs/architecture.md`
- Pi runbook: `docs/runbooks/pi5.md`
- Operator console runbook: `docs/runbooks/operator-console.md`
- Commissioning guide: `tools/robstride/commission.md`
- Motion primitive direction: `docs/decisions/0006-motion-primitives-as-unit-of-composition.md`
- Runtime authority boundary: `docs/decisions/0008-control-plane-and-runtime-fsm.md`
- Simulation ladder: `docs/decisions/0009-simulation-ladder-and-sim-to-sim.md`

## CAN and RobStride

- RobStride Motor Studio releases: https://github.com/RobStride/MotorStudio/releases
- RobStride product information releases: https://github.com/RobStride/Product_Information/releases
- Linux SocketCAN kernel docs: https://docs.kernel.org/networking/can.html
- Kvaser CAN protocol tutorial: https://www.kvaser.com/about-can/the-can-protocol/
- Rudy RS03 spec: `config/actuators/robstride_rs03.yaml`
- Rudy RS03 protocol ADR: `docs/decisions/0002-rs03-protocol-spec.md`

## Isaac Sim / Isaac Lab

Use a desktop/workstation GPU for Isaac Sim/Lab. The Jetson is the inference target, not the main simulator.

- Isaac Sim docs: https://docs.isaacsim.omniverse.nvidia.com/
- Isaac Sim GitHub: https://github.com/isaac-sim/IsaacSim
- Isaac Sim URDF importer docs: https://docs.isaacsim.omniverse.nvidia.com/latest/importer_exporter/import_urdf.html
- Isaac Sim ROS 2 bridge docs: https://docs.isaacsim.omniverse.nvidia.com/latest/ros2_tutorials/index.html
- Isaac Sim standalone Python examples: https://docs.isaacsim.omniverse.nvidia.com/latest/python_scripting/manual_standalone_python.html
- Isaac Lab docs: https://isaac-sim.github.io/IsaacLab/
- Isaac Lab GitHub: https://github.com/isaac-sim/IsaacLab
- Isaac Lab add-new-robot tutorial: https://isaac-sim.github.io/IsaacLab/main/source/tutorials/01_assets/add_new_robot.html
- Isaac Lab direct workflow tutorial index: https://isaac-sim.github.io/IsaacLab/main/source/tutorials/03_envs/index.html
- Isaac Lab RL training docs: https://isaac-sim.github.io/IsaacLab/main/source/overview/reinforcement-learning/index.html

## ROS, MoveIt, MuJoCo, and simulation

- ROS 2 Jazzy docs: https://docs.ros.org/en/jazzy/
- ROS 2 Jazzy installation: https://docs.ros.org/en/jazzy/Installation.html
- MoveIt 2 docs: https://moveit.picknik.ai/main/
- ros2_control docs: https://control.ros.org/
- MuJoCo docs: https://mujoco.readthedocs.io/
- MuJoCo modeling docs: https://mujoco.readthedocs.io/en/stable/modeling.html

## Simulation best-practice references

- Robotics Knowledgebase simulator choice: https://roboticsknowledgebase.com/wiki/robotics-project-guide/choose-a-sim/
- Best of Robot Simulators: https://github.com/knmcguire/best-of-robot-simulators
- Nine physics engines for RL: https://arxiv.org/html/2407.08590v1
- CI/CD for robotics case study: https://agrobofood.github.io/agrobofood-case-studies/case_studies/CICD-for-robotics.html
- Rudy simulation ladder ADR: `docs/decisions/0009-simulation-ladder-and-sim-to-sim.md`
- Rudy Isaac runbook: `docs/runbooks/isaac_lab.md`

## ROS and robotics videos

- Articulated Robotics channel: https://www.youtube.com/@ArticulatedRobotics
- MoveIt tutorials search: https://www.youtube.com/results?search_query=MoveIt+2+tutorial+ROS+2+Jazzy
- ros2_control tutorial search: https://www.youtube.com/results?search_query=ros2_control+hardware+interface+tutorial
- SocketCAN tutorial search: https://www.youtube.com/results?search_query=Linux+SocketCAN+tutorial
- Isaac Sim beginner tutorial search: https://www.youtube.com/results?search_query=Isaac+Sim+beginner+tutorial+robot+URDF
- Isaac Lab robot learning tutorial search: https://www.youtube.com/results?search_query=Isaac+Lab+robot+learning+tutorial
- NVIDIA Isaac Sim channel search: https://www.youtube.com/@NVIDIADeveloper/search?query=Isaac%20Sim

## Safety references

- Rudy robotics reference: `docs/robotics-best-practices-reference.md`
- URDF joint limits reference in repo export: `docs/research/firecrawl-exports-2026-04-15/urdf-joint-spec.md`
- OSHA robotics directive: https://www.osha.gov/enforcement/directives/std-01-12-002

## Good search prompts

- `Jetson Orin Nano headless setup Ubuntu`
- `Jetson Orin Nano JetPack 6 install`
- `Jetson Orin Nano run local LLM`
- `Tailscale Jetson Ubuntu setup`
- `RobStride RS03 CAN setup`
- `Linux SocketCAN candump cansend basics`
- `ROS 2 Jazzy MoveIt 2 robot arm tutorial`
- `ros2_control joint trajectory controller tutorial`
- `Isaac Sim URDF importer ROS2 bridge tutorial`
- `Isaac Lab add new robot tutorial`
- `Isaac Lab direct workflow reinforcement learning`
- `simulation to real robot best practices domain randomization`
