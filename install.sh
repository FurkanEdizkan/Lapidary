#!/usr/bin/env bash
# Lapidary in one command (A8): ./install.sh
#
# Writes deploy/.env the first time (a random database password, and on macOS or Windows your home
# folder as what the setup wizard may browse), builds and starts Lapidary with Podman or Docker, and
# prints the address and the setup code. Everything after that is in the browser. Run it again to
# rebuild and restart; an existing deploy/.env is never overwritten.
set -euo pipefail
cd "$(dirname "$0")"

say() { printf '%s\n' "$*"; }
die() { say "install.sh: $*" >&2; exit 1; }

if command -v podman > /dev/null 2>&1 && podman compose version > /dev/null 2>&1; then
  compose=(podman compose)
elif command -v docker > /dev/null 2>&1 && docker compose version > /dev/null 2>&1; then
  compose=(docker compose)
else
  die "neither Podman nor Docker with compose is installed. Install one (podman.io or docker.com), then run ./install.sh again."
fi

env=deploy/.env
if [ -f "$env" ]; then
  say "Keeping the settings in $env."
else
  password=$(LC_ALL=C tr -dc 'A-Za-z0-9' < /dev/urandom | head -c 32 || true)
  [ ${#password} -eq 32 ] || die "could not make a database password from /dev/urandom."
  sed "s/^POSTGRES_PASSWORD=$/POSTGRES_PASSWORD=$password/" deploy/.env.example > "$env"
  case "$(uname -s)" in
    # Docker Desktop: "/" is its own Linux VM, not this computer, so the wizard browses your home folder.
    Darwin | MINGW* | MSYS* | CYGWIN*)
      printf '\n# Set by install.sh: what the setup wizard may browse.\nLAPIDARY_BROWSE_ROOT=%s\n' "$HOME" >> "$env"
      ;;
  esac
  chmod 600 "$env"
  say "Wrote $env with a new database password."
fi

say "Building and starting Lapidary with ${compose[0]}; the first build takes a few minutes."
"${compose[@]}" --env-file "$env" -f deploy/compose.yaml up -d --build

logs() { "${compose[@]}" --env-file "$env" -f deploy/compose.yaml logs app 2> /dev/null | sed 's/\x1b\[[0-9;]*m//g'; }
url=''
for _ in $(seq 1 120); do
  url=$(logs | grep -o 'Lapidary is at http://[^ ]*' | tail -1 | sed 's/Lapidary is at //') || true
  [ -n "$url" ] && break
  sleep 1
done
[ -n "$url" ] || die "Lapidary did not report its address within two minutes. See: ${compose[*]} -f deploy/compose.yaml logs app"
code=$(logs | grep -o 'setup_code=[A-Z0-9-]*' | tail -1 | sed 's/setup_code=//') || true

say ""
say "Lapidary is running: $url"
if [ -n "$code" ]; then
  say "Open it and enter the setup code: $code"
else
  say "It is already set up: open it and sign in."
fi
case "$(uname -s)" in
  Darwin | MINGW* | MSYS* | CYGWIN*)
    say "Docker Desktop: turn on host networking (Settings > Resources > Network) if the address does not open."
    ;;
esac
