#!/usr/bin/env bash
# Populate the semantic index from what is already in Memoria's SQLite store.
#
# Wiring MEMORIA_VECTOR_RECALL_CMD only enables the *read* side. Nothing embeds
# memories on write, so without this the vector table stays empty and every
# `recall --mode vector` returns the keyword floor while looking like it worked.
#
# The three steps come from Memoria's docs/OPERATIONS.md. `index build` has to
# run first: promoted git memories carry no memory_node, and the bridge payload
# derives its scope from memory_nodes, so skipping it silently indexes only the
# handful of memories written through `remember` (upstream issue-7).
set -euo pipefail

: "${MEMORIA_HOME:?MEMORIA_HOME must be set}"
PKG_DIR="${MEMORIA_PKG_DIR:-/opt/memoria/lib/node_modules/@raybird.chen/memoria}"
REAL="${MEMORIA_REAL_BIN:-/opt/memoria/bin/memoria-real}"
export LIBSQL_URL="${LIBSQL_URL:-file:$MEMORIA_HOME/.memory/vectors.db}"
export MEMORIA_EMBED_PROVIDER="${MEMORIA_EMBED_PROVIDER:-local}"

PAYLOAD_DIR="$(mktemp -d)"
trap 'rm -rf "$PAYLOAD_DIR"' EXIT

echo "[memoria-vector-sync] 1/3 index build"
"$REAL" index build

echo "[memoria-vector-sync] 2/3 bridge payload"
MEMORIA_MCP_PAYLOAD_MODE=full node \
    "$PKG_DIR/skills/memoria-memory-sync/scripts/build-mcp-bridge-payload.mjs" \
    --memoria-home "$MEMORIA_HOME" --out "$PAYLOAD_DIR"

shopt -s nullglob
payloads=("$PAYLOAD_DIR"/mcp-bridge-*.json)
shopt -u nullglob
if [[ ${#payloads[@]} -eq 0 ]]; then
    echo "[memoria-vector-sync] no bridge payload produced — nothing to embed" >&2
    exit 0
fi

echo "[memoria-vector-sync] 3/3 embed + upsert"
result="$(node "$PKG_DIR/skills/memoria-vector/vector-ingest.mjs" "${payloads[0]}")"
echo "$result"

# Verify rather than assume: an ingest that embeds nothing is the exact shape of
# the issue-7 failure, and it exits 0 either way.
embedded="$(printf '%s' "$result" | sed -n 's/.*"embedded":[[:space:]]*\([0-9]*\).*/\1/p')"
if [[ -z "$embedded" || "$embedded" -eq 0 ]]; then
    echo "[memoria-vector-sync] WARNING: embedded 0 entities — semantic recall will return" >&2
    echo "[memoria-vector-sync] the keyword floor. Check that this MEMORIA_HOME has memories." >&2
    exit 0
fi
echo "[memoria-vector-sync] done: $embedded entities embedded into $LIBSQL_URL"
