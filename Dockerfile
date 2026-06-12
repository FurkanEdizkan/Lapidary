# Manifold Print Library — single-image build for Docker and Podman.
# Multi-stage: (1) optional Rust mesh sidecar, (2) Node build, (3) slim runtime.

# ---------- Stage 1: Rust mesh sidecar (optional but included) ----------
FROM rust:1-slim AS rust
WORKDIR /build
COPY rust-mesh/ ./rust-mesh/
RUN cargo build --release --manifest-path rust-mesh/Cargo.toml \
    && cp rust-mesh/target/release/rust-mesh /usr/local/bin/rust-mesh

# ---------- Stage 2: Node build (web SPA + server TS) ----------
FROM node:22-slim AS build
# better-sqlite3 is a native module — it needs a toolchain to compile.
RUN apt-get update && apt-get install -y --no-install-recommends python3 make g++ \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY package.json package-lock.json* ./
COPY server/package.json ./server/
COPY web/package.json ./web/
RUN npm install
COPY . .
RUN npm run build \
    && npm prune --omit=dev

# ---------- Stage 3: Runtime ----------
FROM node:22-slim AS runtime
ENV NODE_ENV=production \
    DATA_DIR=/data \
    PORT=5174 \
    MESH_SIDECAR_BIN=rust-mesh
WORKDIR /app
COPY --from=rust /usr/local/bin/rust-mesh /usr/local/bin/rust-mesh
COPY --from=build /app/node_modules ./node_modules
COPY --from=build /app/server/dist ./server/dist
COPY --from=build /app/server/node_modules ./server/node_modules
COPY --from=build /app/web/dist ./web/dist
COPY --from=build /app/package.json ./package.json

RUN mkdir -p /data && chown -R node:node /data
USER node
VOLUME /data
EXPOSE 5174
CMD ["node", "server/dist/index.js"]
