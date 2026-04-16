# Rudy development container: ROS 2 Jazzy (Noble) + Rust + cross tools + CAN utils.
# x86_64 host. Isaac Sim / Isaac Lab are installed on the host GPU stack, not inside this image.
FROM osrf/ros:jazzy-desktop-full-noble

ENV DEBIAN_FRONTEND=noninteractive
ENV LANG=C.UTF-8

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    git \
    curl \
    ca-certificates \
    can-utils \
    iproute2 \
    pkg-config \
    python3-pip \
    python3-venv \
    gcc-aarch64-linux-gnu \
    g++-aarch64-linux-gnu \
    qemu-user-static \
    binfmt-support \
    ros-jazzy-rmw-cyclonedds-cpp \
    ros-jazzy-diagnostic-aggregator \
    ros-jazzy-ros2-control \
    ros-jazzy-ros2-controllers \
    ros-jazzy-hardware-interface \
    ros-jazzy-pluginlib \
    && rm -rf /var/lib/apt/lists/*

# Rust (stable) + aarch64 target for Pi 5 cross-builds
ENV RUSTUP_HOME=/opt/rustup
ENV CARGO_HOME=/opt/cargo
ENV PATH="${CARGO_HOME}/bin:${PATH}"
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && rustup target add aarch64-unknown-linux-gnu \
    && rustup component add rustfmt clippy

# colcon extensions for mixed workspaces (optional future ros2_rust)
RUN pip3 install --no-cache-dir --break-system-packages \
    colcon-common-extensions \
    colcon-cargo \
    colcon-ros-cargo

WORKDIR /workspace
