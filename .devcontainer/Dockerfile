# Lapidary — Dev Container image (development, not production).
# Base is the official VS Code devcontainer Node image; Rust is added via a
# devcontainer feature (see devcontainer.json). We only add the native-build
# toolchain that `better-sqlite3` needs to compile, since the base image ships
# Node 22 + git + common dev utilities but not python3/make/g++.
FROM mcr.microsoft.com/devcontainers/javascript-node:22

RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 make g++ \
    && rm -rf /var/lib/apt/lists/*
