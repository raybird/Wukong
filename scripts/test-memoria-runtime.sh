#!/usr/bin/env bash
# End-to-end check for the optional Memoria runtime overlay.
#
# The pairing this guards: the runtime image bundles better-sqlite3 and
# onnxruntime as ABI-specific native modules, and they are loaded by the Node
# inside the *Wukong* image, not the one they were built against. Nothing at
# build time notices a mismatch — it surfaces as NODE_MODULE_VERSION at the
# moment the agent shells out, which is exactly where nobody is looking.
#
# So this does not compare version strings. It publishes the runtime into a real
# volume, mounts it into the real Wukong image, and runs the commands the agent
# actually runs, semantic recall included.
#
# Usage: scripts/test-memoria-runtime.sh [runtime-image] [wukong-image]
set -euo pipefail

RUNTIME_IMAGE="${1:-wukong-memoria-runtime:1.25.0-test}"
WUKONG_IMAGE="${2:-ghcr.io/raybird/wukong:v0.20.1}"
VOL_RUNTIME="wukong-memoria-runtime-test-$$"
VOL_DATA="wukong-memoria-data-test-$$"
# A pristine, root-owned volume for the unprivileged run; the one above has
# already been written to as root and would mask an ownership bug.
VOL_DATA_NONROOT="wukong-memoria-data-nonroot-$$"
FAILED=0

cleanup() { docker volume rm -f "$VOL_RUNTIME" "$VOL_DATA" "$VOL_DATA_NONROOT" >/dev/null 2>&1 || true; }
trap cleanup EXIT

pass() { echo "✓ $1"; }
fail() { echo "✗ $1" >&2; FAILED=1; }

echo "runtime: $RUNTIME_IMAGE"
echo "wukong:  $WUKONG_IMAGE"
echo

docker volume create "$VOL_RUNTIME" >/dev/null
docker volume create "$VOL_DATA" >/dev/null
docker volume create "$VOL_DATA_NONROOT" >/dev/null

# ── 1. Publish into the volume ──
if docker run --rm -e MEMORIA_DATA_UID=1000 -e MEMORIA_DATA_GID=1000 \
        -v "$VOL_RUNTIME:/out" -v "$VOL_DATA_NONROOT:/data-init" \
        "$RUNTIME_IMAGE" >/dev/null 2>&1; then
    pass "runtime publishes into the shared volume"
else
    fail "runtime failed to publish"
    exit 1
fi

# Publishing twice must be a cheap no-op, not a second 1 GB copy.
if docker run --rm -v "$VOL_RUNTIME:/out" "$RUNTIME_IMAGE" 2>&1 | grep -q "already published"; then
    pass "re-publishing the same version is a no-op"
else
    fail "re-publish did not short-circuit on the version marker"
fi

# ── 2. Everything the agent needs, run inside the Wukong image ──
# Same mounts and env the compose overlay sets, so a break here is a real break.
run_in_wukong() {
    docker run --rm \
        -v "$VOL_RUNTIME:/opt/memoria:ro" \
        -v "$VOL_DATA:/memoria" \
        -e MEMORIA_HOME=/memoria \
        -e LIBSQL_URL=file:/memoria/.memory/vectors.db \
        -e MEMORIA_EMBED_PROVIDER=local \
        -e MEMORIA_VECTOR_RECALL_CMD=/opt/memoria/lib/node_modules/@raybird.chen/memoria/skills/memoria-vector/vector-recall.mjs \
        -e PATH=/opt/memoria/bin:/usr/local/bin:/usr/bin:/bin \
        --entrypoint bash "$WUKONG_IMAGE" -c "$1"
}

# better-sqlite3 under the Wukong image's Node — the ABI check, made concrete.
if out="$(run_in_wukong 'memoria --version' 2>&1)"; then
    pass "memoria CLI runs under the Wukong image's Node (v$out)"
else
    fail "memoria CLI failed to start — likely a Node ABI mismatch:"
    printf '%s\n' "$out" | sed 's/^/    /' >&2
    exit 1
fi

# The three commands the host workflow uses. `brief` is why the CLI has to be
# here at all — it is the one with no HTTP endpoint, so a sidecar could not
# serve it.
for cmd in "memoria init" "memoria remember '容器內語意召回驗證：悟空的記憶層'" "memoria brief"; do
    if run_in_wukong "$cmd" >/dev/null 2>&1; then
        pass "${cmd%% *} ${cmd#* } ok"
    else
        fail "failed: $cmd"
    fi
done

# ── 3. Semantic recall, for real ──
# Ingest first: wiring the helper only enables the read side, and an unpopulated
# vector table degrades to the keyword floor while looking healthy.
if out="$(run_in_wukong 'memoria init >/dev/null 2>&1; memoria remember "容器內語意召回驗證：悟空的記憶層" >/dev/null 2>&1; memoria-vector-sync' 2>&1)"; then
    if grep -q "entities embedded" <<<"$out"; then
        pass "vector-sync embedded memories into libSQL"
    else
        fail "vector-sync embedded nothing:"
        printf '%s\n' "$out" | tail -5 | sed 's/^/    /' >&2
    fi
else
    fail "vector-sync failed:"
    printf '%s\n' "$out" | tail -10 | sed 's/^/    /' >&2
fi

# route_mode is the only honest signal here: the call returns ok:true whether or
# not the helper ran. Memoria reports `vector` when the semantic route stood
# alone and `hybrid_vector` when it fused with keyword hits — both mean the
# helper answered. `vector_unavailable` / `vector_timeout` / a bare `keyword` are
# the degraded paths, and they are what a missing helper or model looks like.
out="$(run_in_wukong 'memoria init >/dev/null 2>&1; memoria remember "容器內語意召回驗證：悟空的記憶層" >/dev/null 2>&1; memoria-vector-sync >/dev/null 2>&1; memoria recall "記憶層" --mode vector --json' 2>&1 || true)"
route="$(sed -n 's/.*"route_mode":"\([a-z_]*\)".*/\1/p' <<<"$out")"
case "$route" in
    vector | hybrid_vector)
        pass "semantic recall engaged (route_mode=$route)" ;;
    vector_unavailable | vector_timeout)
        fail "semantic recall degraded ($route) — the helper or its model is missing" ;;
    *)
        fail "unexpected recall route (${route:-none}):"
        printf '%s\n' "$out" | head -3 | sed 's/^/    /' >&2 ;;
esac

# ── 4. The concurrency gate (Memoria issue-8) ──
if run_in_wukong 'grep -q "MEMORIA_VECTOR_MAX_CONCURRENCY" /opt/memoria/bin/memoria' >/dev/null 2>&1; then
    pass "concurrency gate is present in the published wrapper"
else
    fail "the published memoria is not the gated wrapper"
fi

# The gate has to actually hold a second caller off, not merely exist. Two
# concurrent vector recalls with one slot and no willingness to queue: exactly
# one must be turned away onto the keyword floor, and say so.
gate_out="$(run_in_wukong '
    memoria init >/dev/null 2>&1
    memoria remember "並行閘門驗證" >/dev/null 2>&1
    memoria-vector-sync >/dev/null 2>&1
    export MEMORIA_VECTOR_MAX_CONCURRENCY=1 MEMORIA_VECTOR_QUEUE_WAIT_SECS=0
    memoria recall "閘門" --mode vector --json >/tmp/a.out 2>/tmp/a.err &
    memoria recall "閘門" --mode vector --json >/tmp/b.out 2>/tmp/b.err &
    wait
    cat /tmp/a.err /tmp/b.err
' 2>&1 || true)"
if [[ "$(grep -c "falling back to keyword recall" <<<"$gate_out")" -eq 1 ]]; then
    pass "gate admits one semantic recall and sheds the concurrent one"
else
    fail "gate did not shed exactly one of two concurrent semantic recalls:"
    printf '%s\n' "$gate_out" | tail -5 | sed 's/^/    /' >&2
fi

# A default-mode recall must not pay the gate: it never spawns a helper.
if out="$(run_in_wukong 'memoria init >/dev/null 2>&1; MEMORIA_VECTOR_LOCK_DIR=/nonexistent-and-unwritable memoria recall "x" --json' 2>&1)"; then
    if grep -q "running the semantic recall ungated" <<<"$out"; then
        fail "default-mode recall went through the vector gate"
    else
        pass "default-mode recall bypasses the gate"
    fi
else
    fail "default-mode recall failed"
fi

# ── 5. The path that actually ships ──
# Everything above runs as root with a bypassed entrypoint, which hides the two
# things most likely to break in production: the agent runs unprivileged, and
# the data volume arrives root-owned. Go through the real entrypoint (it gosu's
# down to the runtime user) and drive memoria from there. python3 is used only
# because the entrypoint's dispatch will exec it — a bare shell command would be
# rerouted to the wukong binary.
nonroot_out="$(docker run --rm \
    -e USER_ID=1000 -e GROUP_ID=1000 \
    -e MEMORIA_HOME=/memoria \
    -e LIBSQL_URL=file:/memoria/.memory/vectors.db \
    -e MEMORIA_EMBED_PROVIDER=local \
    -e MEMORIA_VECTOR_RECALL_CMD=/opt/memoria/lib/node_modules/@raybird.chen/memoria/skills/memoria-vector/vector-recall.mjs \
    -e PATH=/opt/memoria/bin:/home/wukong/.local/bin:/usr/local/bin:/usr/local/sbin:/usr/sbin:/usr/bin:/sbin:/bin \
    -v "$VOL_RUNTIME:/opt/memoria:ro" \
    -v "$VOL_DATA_NONROOT:/memoria" \
    "$WUKONG_IMAGE" python3 -c "
import subprocess, re
def run(c):
    r = subprocess.run(['bash','-c',c], capture_output=True, text=True)
    return r.returncode, (r.stdout + r.stderr).strip()
print('user=' + run('id -un')[1])
print('init=%d remember=%d brief=%d' % (
    run('memoria init')[0],
    run('memoria remember \"entrypoint path check\"')[0],
    run('memoria brief')[0]))
run('memoria-vector-sync')
rc, out = run('memoria recall \"check\" --mode vector --json')
m = re.search(r'\"route_mode\":\"([a-z_]+)\"', out)
print('route=' + (m.group(1) if m else 'none'))
" 2>&1 || true)"

if grep -q "user=wukong" <<<"$nonroot_out"; then
    pass "entrypoint drops to the unprivileged runtime user"
else
    fail "did not run as the unprivileged user:"
    printf '%s\n' "$nonroot_out" | tail -5 | sed 's/^/    /' >&2
fi
if grep -q "init=0 remember=0 brief=0" <<<"$nonroot_out"; then
    pass "memoria writes to a root-created data volume as the runtime user"
else
    fail "memoria could not write as the runtime user (MEMORIA_HOME ownership):"
    printf '%s\n' "$nonroot_out" | grep -E "init=|user=" | sed 's/^/    /' >&2
fi
if grep -qE "route=(vector|hybrid_vector)" <<<"$nonroot_out"; then
    pass "semantic recall works unprivileged against the read-only runtime"
else
    fail "semantic recall failed on the unprivileged path: $(grep -o 'route=[a-z_]*' <<<"$nonroot_out")"
fi

echo
if [[ $FAILED -eq 0 ]]; then
    echo "all checks passed"
else
    echo "some checks failed" >&2
fi
exit $FAILED
