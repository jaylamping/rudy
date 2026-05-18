# Resource links

Educational links for Jetson bring-up, Rudy motion, CAN, ROS, and videos. Prefer official docs first, then videos for intuition.

Note: this list uses stable landing pages and search links where exact video titles may move.

## Jetson Orin Nano setup

- NVIDIA Jetson Orin Nano Developer Kit getting started: https://developer.nvidia.com/embedded/learn/get-started-jetson-orin-nano-devkit
- NVIDIA Jetson downloads: https://developer.nvidia.com/embedded/downloads
- NVIDIA Jetson docs landing: https://docs.nvidia.com/jetson/
- NVIDIA Jetson AI Lab: https://www.jetson-ai-lab.com/
- NVIDIA Jetson containers: https://catalog.ngc.nvidia.com/orgs/nvidia/containers/l4t-pytorch
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

## ROS, MoveIt, and simulation

These are not required for the first single-joint wave, but they matter as the arm behaviors get richer.

- ROS 2 Jazzy docs: https://docs.ros.org/en/jazzy/
- ROS 2 Jazzy installation: https://docs.ros.org/en/jazzy/Installation.html
- MoveIt 2 docs: https://moveit.picknik.ai/main/
- ros2_control docs: https://control.ros.org/
- Isaac Lab docs: https://isaac-sim.github.io/IsaacLab/
- MuJoCo docs: https://mujoco.readthedocs.io/

## ROS and robotics videos

- Articulated Robotics channel: https://www.youtube.com/@ArticulatedRobotics
- MoveIt tutorials search: https://www.youtube.com/results?search_query=MoveIt+2+tutorial+ROS+2+Jazzy
- ros2_control tutorial search: https://www.youtube.com/results?search_query=ros2_control+hardware+interface+tutorial
- SocketCAN tutorial search: https://www.youtube.com/results?search_query=Linux+SocketCAN+tutorial

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
