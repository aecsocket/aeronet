FROM mcr.microsoft.com/devcontainers/rust:1-1-bullseye

# https://github.com/bevyengine/bevy/blob/main/docs/linux_dependencies.md
RUN apt-get update && apt-get install -y \
    pkg-config \
    libx11-dev \
    libasound2-dev \
    libudev-dev \
    libxkbcommon-dev \
    libxkbcommon0 \
    libxkbcommon-x11-0 \
    libxcursor1 \
    libx11-xcb1 \
    libxi6 \
    libwayland-dev \
    libwayland-client0 \
    # TODO
    vulkan-tools \
    mesa-utils

RUN cargo install cargo-shear
