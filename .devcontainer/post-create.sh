#!/usr/bin/env bash
# Runs once when the dev container is created (see devcontainer.json).
# Populates the container-local node_modules volumes and builds the optional
# rust-mesh sidecar. Safe to re-run.
set -euo pipefail

cd "$(dirname "$0")/.."

# The node_modules named volumes (see docker-compose.yml) come up owned by root,
# but lifecycle commands run as the unprivileged 'node' user — so npm install
# would hit EACCES. Hand each mountpoint to the current user once. The base
# image grants the dev user passwordless sudo.
if command -v sudo >/dev/null 2>&1; then
  for d in node_modules server/node_modules web/node_modules; do
    if [ -d "$d" ] && [ ! -w "$d" ]; then
      echo "==> Taking ownership of $d (named-volume mountpoint)"
      sudo chown "$(id -u):$(id -g)" "$d"
    fi
  done
fi

# Reproducible install straight from the committed lockfile. npm ci never
# rewrites package-lock.json (so the bind-mounted lockfile stays clean) and is
# faster on a cold cache. Falls back to npm install if the lockfile is missing
# or out of sync with package.json.
echo "==> Installing workspace dependencies (npm ci)"
npm ci || { echo "!! npm ci failed (lockfile out of sync?) — falling back to npm install"; npm install; }

if [ ! -f .env ]; then
  echo "==> Seeding .env from .env.example"
  cp .env.example .env
fi

# Optional Rust mesh sidecar. Non-fatal: the app generates LOD/thumbnails
# client-side when the binary is absent. The Rust feature may install cargo
# outside the non-login shell's PATH, so look for it explicitly.
if ! command -v cargo >/dev/null 2>&1; then
  [ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
  [ -d /usr/local/cargo/bin ] && export PATH="/usr/local/cargo/bin:$PATH"
fi

echo "==> Building rust-mesh sidecar (optional)"
if command -v cargo >/dev/null 2>&1; then
  npm run build:mesh || echo "!! rust-mesh build failed — continuing without the sidecar"
else
  echo "!! cargo not found — skipping rust-mesh (the Rust feature may still be installing)"
fi

echo "==> Dev container ready. Start everything with: npm run dev"
