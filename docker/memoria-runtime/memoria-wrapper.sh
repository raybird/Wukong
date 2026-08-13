#!/usr/bin/env bash
# `memoria` as the agent sees it: the real CLI behind a concurrency gate.
#
# Memoria spawns one helper process per semantic recall and has no concurrency
# limit of its own (upstream issue-8). Each helper peaks at 450–624 MB and ~1.8
# cores while the container it runs in is capped well below the sum of a few of
# those, so two or three overlapping turns are enough to OOM the agent
# container. Until upstream grows a gate, this is where one goes.
#
# Only `recall --mode vector` reaches the helper — every other subcommand,
# including a default-mode recall, is a plain SQLite read (~74 MB / 0.08s) and
# passes straight through ungated.
set -euo pipefail

REAL="${MEMORIA_REAL_BIN:-/opt/memoria/bin/memoria-real}"
SLOTS="${MEMORIA_VECTOR_MAX_CONCURRENCY:-1}"
WAIT_SECS="${MEMORIA_VECTOR_QUEUE_WAIT_SECS:-120}"
LOCK_DIR="${MEMORIA_VECTOR_LOCK_DIR:-${TMPDIR:-/tmp}/memoria-vector-slots}"

# True only for `recall ... --mode vector` / `--mode=vector`.
wants_vector() {
    local seen_recall=0 arg prev=""
    for arg in "$@"; do
        [[ "$arg" == "recall" && $seen_recall -eq 0 ]] && seen_recall=1
        if [[ "$arg" == "--mode=vector" ]] || [[ "$prev" == "--mode" && "$arg" == "vector" ]]; then
            [[ $seen_recall -eq 1 ]] && return 0
        fi
        prev="$arg"
    done
    return 1
}

if ! wants_vector "$@"; then
    exec "$REAL" "$@"
fi

mkdir -p "$LOCK_DIR" 2>/dev/null || true
if [[ ! -w "$LOCK_DIR" ]]; then
    echo "[memoria] warning: $LOCK_DIR is not writable; running the semantic recall ungated" >&2
    exec "$REAL" "$@"
fi

# Grab any free slot without blocking; fall back to waiting on the first one.
for ((slot = 1; slot <= SLOTS; slot++)); do
    exec {fd}>"$LOCK_DIR/slot-$slot"
    if flock -n "$fd"; then
        exec "$REAL" "$@"
    fi
    exec {fd}>&-
done

exec {fd}>"$LOCK_DIR/slot-1"
if flock -w "$WAIT_SECS" "$fd"; then
    exec "$REAL" "$@"
fi

# Out of slots for longer than the queue is willing to wait. Point Memoria at a
# helper that does not exist so its own fail-open path takes over: the recall
# still returns, on the keyword floor, reporting route_mode=vector_unavailable.
# Say so on stderr — a silently downgraded recall is the failure mode that cost
# the most time to notice on the host.
echo "[memoria] semantic recall queued longer than ${WAIT_SECS}s (${SLOTS} slot(s) busy);" >&2
echo "[memoria] falling back to keyword recall for this query." >&2
exec env MEMORIA_VECTOR_RECALL_CMD=/nonexistent/memoria-vector-queue-full "$REAL" "$@"
