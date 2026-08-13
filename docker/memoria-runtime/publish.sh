#!/usr/bin/env bash
# Publish the baked Memoria runtime into the shared volume mounted at /out, then
# exit. Agent containers mount that same volume read-only, so this runs once per
# `docker compose up` and is a no-op when the volume already holds this version.
set -euo pipefail

SRC=/opt/memoria
DEST=${MEMORIA_PUBLISH_DEST:-/out}
VERSION="$(cat "$SRC/VERSION")"

# Named volumes are created root-owned, and the agent runs as an unprivileged
# user, so MEMORIA_HOME has to be handed over before the first `memoria init`.
# Doing it here rather than in Wukong's entrypoint keeps this overlay purely
# additive: it works against an already-published Wukong image instead of
# requiring one rebuilt with a matching entrypoint.
DATA_DEST=${MEMORIA_DATA_DEST:-/data-init}
if [[ -d "$DATA_DEST" ]]; then
    owner="${MEMORIA_DATA_UID:-1000}:${MEMORIA_DATA_GID:-1000}"
    mkdir -p "$DATA_DEST/.memory"
    # Not recursive: the store grows without bound and re-walking it on every
    # start would cost startup time proportional to its size. Only the two
    # directories Memoria creates files in need to change hands.
    chown "$owner" "$DATA_DEST" "$DATA_DEST/.memory"
    echo "[memoria-runtime] $DATA_DEST owned by $owner"
fi

if [[ ! -d "$DEST" ]]; then
  echo "[memoria-runtime] $DEST is not mounted — nothing to publish" >&2
  exit 1
fi

if [[ -f "$DEST/VERSION" ]] && [[ "$(cat "$DEST/VERSION")" == "$VERSION" ]]; then
  echo "[memoria-runtime] $VERSION already published to $DEST"
  exit 0
fi

echo "[memoria-runtime] publishing $VERSION to $DEST (this copies ~1 GB on first run)"
# --delete so a downgrade or a version with fewer files cannot leave the
# previous release's files mixed into the volume.
rsync -a --delete "$SRC/" "$DEST/"

# The consumer runs as the host user (USER_ID/GROUP_ID in compose), and mounts
# this read-only, so read+traverse for everyone is both necessary and enough.
chmod -R a+rX "$DEST"

echo "[memoria-runtime] published $VERSION"
