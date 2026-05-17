FROM rust:bookworm

RUN apt-get update -qq \
    && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

RUN rustup component add rustfmt clippy
