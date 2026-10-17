#!/bin/sh
# Lapidary runs as the person who installed it (A7, owner 2026-10-17): it reaches the folders they can,
# and what it writes is theirs, with no permission command to run first.
#
# The container starts as root only to choose that user, then drops to them before the server starts:
#   1. LAPIDARY_UID / LAPIDARY_GID, when deploy/.env sets them;
#   2. else the owner of deploy/, mounted read-only at /etc/lapidary/owner by deploy/compose.yaml;
#   3. else 10001, the image's own user.
# Never root on a rootful engine. Under rootless Podman container root *is* the person, so it stays.
set -eu

server=/usr/local/bin/lapidary-server
# Started as somebody already (`--user`, or an old compose file): nothing to choose.
[ "$(id -u)" = 0 ] || exec "$server" "$@"

uid=${LAPIDARY_UID:-}
gid=${LAPIDARY_GID:-}
if [ -z "$uid" ] && [ -d /etc/lapidary/owner ]; then
  uid=$(stat -c %u /etc/lapidary/owner)
  gid=$(stat -c %g /etc/lapidary/owner)
fi
uid=${uid:-10001}
gid=${gid:-$uid}

# Root inside a user namespace maps to the person; root with the identity map is the machine's root.
identity=no
while read -r inside outside _; do
  if [ "$inside" = 0 ] && [ "$outside" = 0 ]; then identity=yes; fi
done < /proc/self/uid_map
if [ "$uid" = 0 ] && [ "$identity" = yes ]; then
  echo "lapidary: deploy/ belongs to root, and Lapidary does not run as root; running as 10001. Set LAPIDARY_UID in deploy/.env to run as you." >&2
  uid=10001
  gid=10001
fi

# Lapidary's own folders follow the user. The models mount is given over only while it is still 10001's,
# which only an earlier Lapidary made it.
for dir in /var/lib/lapidary /var/lib/lapidary-uploads; do
  if [ -d "$dir" ] && [ "$(stat -c %u "$dir")" != "$uid" ]; then chown -R "$uid:$gid" "$dir"; fi
done
if [ -d /models ] && [ "$(stat -c %u /models)" = 10001 ] && [ "$uid" != 10001 ]; then
  chown -R "$uid:$gid" /models
fi

echo "lapidary: running as uid $uid, gid $gid" >&2
exec setpriv --reuid="$uid" --regid="$gid" --clear-groups --inh-caps=-all --bounding-set=-all --no-new-privs -- "$server" "$@"
