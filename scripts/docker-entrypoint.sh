#!/usr/bin/env bash
set -euo pipefail

# ── Dynamic UID/GID alignment ──
# Run with -e USER_ID=$(id -u) -e GROUP_ID=$(id -g) to match host user.
#
# This ensures mounted volumes have the correct ownership on both host
# and container sides, preventing permission issues.
# ───────────────────────────────

# Default to current container user if not overridden
USER_ID=${USER_ID:-$(id -u wukong)}
GROUP_ID=${GROUP_ID:-$(id -g wukong)}

if [[ "$USER_ID" != "$(id -u wukong)" || "$GROUP_ID" != "$(id -g wukong)" ]]; then
    echo "[wukong] Aligning container user to host UID=${USER_ID} GID=${GROUP_ID}"

    # Update group ID
    groupmod -o -g "$GROUP_ID" wukong 2>/dev/null || true

    # Update user ID
    usermod -o -u "$USER_ID" wukong 2>/dev/null || true

    # Fix home directory ownership
    chown -R wukong:wukong /home/wukong 2>/dev/null || true
fi

# Ensure workspace directory exists and is writable
if [[ -n "${WUKONG_WORKSPACE:-}" ]]; then
    mkdir -p "$WUKONG_WORKSPACE"
    chown wukong:wukong "$WUKONG_WORKSPACE" 2>/dev/null || true

    # 2026-11-26（#45）：Superpowers 技能與 agent 定義由 wukong 的入口啟動時寫到
    # $WUKONG_WORKSPACE/.opencode/（V2 讀得到的位置），這裡不再從映像複製技能。

    # ── Auto-initialize workspace templates if missing ──
    if [[ ! -f "$WUKONG_WORKSPACE/SOUL.md" && -f "/usr/local/share/wukong/SOUL.md" ]]; then
        echo "[wukong] Workspace SOUL.md is missing. Initializing from template..."
        cp "/usr/local/share/wukong/SOUL.md" "$WUKONG_WORKSPACE/SOUL.md"
        chown wukong:wukong "$WUKONG_WORKSPACE/SOUL.md" 2>/dev/null || true
    fi
    if [[ ! -f "$WUKONG_WORKSPACE/AGENTS.md" && -f "/usr/local/share/wukong/AGENTS.md" ]]; then
        echo "[wukong] Workspace AGENTS.md is missing. Initializing from template..."
        cp "/usr/local/share/wukong/AGENTS.md" "$WUKONG_WORKSPACE/AGENTS.md"
        chown wukong:wukong "$WUKONG_WORKSPACE/AGENTS.md" 2>/dev/null || true
    fi
fi

# Ensure opencode config dir exists (backed by Docker volume)
# NOTE: this must NOT be named OPENCODE_CONFIG — opencode reads that variable as
# a path to a config FILE, and it is exported below to point at the user layer.
OPENCODE_CONFIG_DIR="/home/wukong/.config/opencode"
mkdir -p "$OPENCODE_CONFIG_DIR"

# ── Layered opencode config: shipped baseline + untouched user overrides ──
# opencode merges its config sources deeply, later sources winning per key, in
# this order: ~/.config/opencode/opencode.json (global) → $OPENCODE_CONFIG →
# project config → OPENCODE_CONFIG_CONTENT. We use the bottom two layers:
#
#   opencode.json - Wukong's baseline. REWRITTEN ON EVERY START so that new
#                   defaults ship with an image upgrade. Never hand-edit it.
#   user.json     - your overrides, pointed at by OPENCODE_CONFIG. Created empty
#                   once and never touched again; keys here beat the baseline.
#
# Seeding only when the file was missing (the pre-0.18.8 behaviour) meant no
# existing deployment ever received a new default: the v0.18.7 CPU guardrails
# and the external_directory rule below both failed to reach running hosts.
#
# ── The baseline: a destructive-command guard ──
# It is written in OpenCode V2's native format (issue-0044). V2 still reads the V1
# keys (`permission`, `snapshot`) by converting them, but drops what it cannot
# convert without a word: V1's compaction.prune and compaction.tail_turns simply
# vanished. Writing native keys means what is below is what V2 runs with;
# scripts/test-opencode-baseline.sh checks that V2 reads it back unchanged.
#
# Two backends reach opencode, and only one of them takes a CLI flag:
#   * CLI backend (`opencode run`) — Wukong always starts it with stdin=null, so
#     it can never answer an interactive prompt. WUKONG_AGENT_CMD therefore
#     carries `--dangerously-skip-permissions`.
#   * Server and local backends (`opencode serve`) — no equivalent flag, so THIS
#     FILE is the only permission control Web, Telegram and Scheduler have.
# Both still honour an explicit `deny`, so the shell rules below block recursive
# deletes of absolute paths while still allowing deletes inside /workspace.
# `permissions` is an ordered list and the last matching rule wins: keep the
# /workspace allow after the denies. To change any of it, put your own keys in
# user.json — do not edit opencode.json, it is overwritten on restart. Rules in
# user.json come after these, so they win; the V1 `permission` form still works
# there too.
#
#   external_directory - /tmp is allowed explicitly so a scheduled job touching
#                 it never stops on a prompt nobody answers until the stream
#                 deadline kills the turn (see
#                 docs/2026-08-06-docker-runtime-handover.md). V2 2.3.2 does not
#                 ask by default, but these rules keep that true if a default or a
#                 broader user rule changes. Grant further paths one at a time.
#
# The baseline also carries CPU guardrails for long conversations. opencode 1.18
# was measured at ~430x write amplification on a single session (every streaming
# update persisted as a full snapshot of the part, 38 MB written for a 91 KB
# final part). These keys cut the base cost:
#   snapshots   - opencode keeps a bare git repo per project and commits the
#                 whole workspace on every edit; on a bind-mounted /workspace
#                 that is expensive per tool call, and the store grows without
#                 bound (>1 GB for a single project is normal). Wukong's
#                 workspace is the user's own git repo, so this is redundant.
#                 Disabling it also disables opencode's own revert/undo.
#   tool_output - tool results are the bulk of part payloads; capping them caps
#                 what gets re-serialised on every subsequent update.
#   compaction  - lets opencode trim its own context. Since 2026-11-26 (#45)
#                 this is the only compaction: Wukong no longer compacts on
#                 turn count (WUKONG_SESSION_COMPACT_EVERY_TURNS is ignored).
#                 `background` is off: V2 would otherwise pre-write a summary as a
#                 session nears the limit, spending tokens on summaries that may
#                 never be used, which V1 never did. V1's prune and tail_turns
#                 have no V2 counterpart.
#   watcher     - keeps the in-container file watcher off build output.
OPENCODE_CONFIG_FILE="$OPENCODE_CONFIG_DIR/opencode.json"
OPENCODE_USER_CONFIG_FILE="$OPENCODE_CONFIG_DIR/user.json"
OPENCODE_BASELINE_MARKER="$OPENCODE_CONFIG_DIR/.wukong-baseline"

# One-time migration off the old seed-if-missing layout. A config that predates
# the marker may carry hand-written rules, and those must not vanish just
# because the baseline is now managed: keep a backup and promote it to the user
# layer, where it still wins over the baseline.
if [[ -f "$OPENCODE_CONFIG_FILE" && ! -f "$OPENCODE_BASELINE_MARKER" ]]; then
    cp -a "$OPENCODE_CONFIG_FILE" "$OPENCODE_CONFIG_FILE.pre-baseline.bak"
    if [[ ! -f "$OPENCODE_USER_CONFIG_FILE" ]]; then
        cp -a "$OPENCODE_CONFIG_FILE" "$OPENCODE_USER_CONFIG_FILE"
        echo "[wukong] Existing opencode.json moved to user.json (backup: opencode.json.pre-baseline.bak)."
        echo "[wukong] Delete user.json to run on Wukong defaults alone."
    fi
fi

echo "[wukong] Writing opencode.json baseline (destructive-rm guard + /tmp access + CPU guardrails)..."
cat > "$OPENCODE_CONFIG_FILE" <<'OPENCODE_JSON'
{
  "$schema": "https://opencode.ai/config.json",
  "snapshots": false,
  "compaction": {
    "auto": true,
    "background": false
  },
  "tool_output": {
    "max_lines": 500,
    "max_bytes": 65536
  },
  "watcher": {
    "ignore": [
      "**/node_modules/**",
      "**/target/**",
      "**/.git/**",
      "**/dist/**",
      "**/build/**"
    ]
  },
  "permissions": [
    { "action": "external_directory", "resource": "/tmp", "effect": "allow" },
    { "action": "external_directory", "resource": "/tmp/*", "effect": "allow" },
    { "action": "external_directory", "resource": "/tmp/**", "effect": "allow" },
    { "action": "shell", "resource": "*", "effect": "allow" },
    { "action": "shell", "resource": "*rm -rf /*", "effect": "deny" },
    { "action": "shell", "resource": "*rm -fr /*", "effect": "deny" },
    { "action": "shell", "resource": "*rm -Rf /*", "effect": "deny" },
    { "action": "shell", "resource": "*rm -rF /*", "effect": "deny" },
    { "action": "shell", "resource": "*rm -r -f /*", "effect": "deny" },
    { "action": "shell", "resource": "*rm -f -r /*", "effect": "deny" },
    { "action": "shell", "resource": "*rm -r /*", "effect": "deny" },
    { "action": "shell", "resource": "*rm -R /*", "effect": "deny" },
    { "action": "shell", "resource": "*rm --recursive*", "effect": "deny" },
    { "action": "shell", "resource": "*rm --force* /*", "effect": "deny" },
    { "action": "shell", "resource": "*rm -rf ~*", "effect": "deny" },
    { "action": "shell", "resource": "*rm -rf $HOME*", "effect": "deny" },
    { "action": "shell", "resource": "*sudo rm *", "effect": "deny" },
    { "action": "shell", "resource": "*rm * /workspace/*", "effect": "allow" }
  ]
}
OPENCODE_JSON
printf 'wukong-managed baseline; edits belong in user.json\n' > "$OPENCODE_BASELINE_MARKER"

# The user layer is created once and never rewritten.
if [[ ! -f "$OPENCODE_USER_CONFIG_FILE" ]]; then
    printf '{\n  "$schema": "https://opencode.ai/config.json"\n}\n' > "$OPENCODE_USER_CONFIG_FILE"
fi
# opencode reads this as the higher-precedence config layer, so user keys win.
export OPENCODE_CONFIG="$OPENCODE_USER_CONFIG_FILE"

chown -R wukong:wukong /home/wukong/.config 2>/dev/null || true

# Ensure Agent Reach state volume is writable by the runtime user.
AGENT_REACH_STATE="/home/wukong/.agent-reach"
mkdir -p "$AGENT_REACH_STATE"
chown -R wukong:wukong "$AGENT_REACH_STATE" 2>/dev/null || true

# Ensure opencode session/runtime dirs exist and are writable before gosu.
OPENCODE_STATE="/home/wukong/.local/share/opencode"
OPENCODE_RUNTIME="/home/wukong/.local/state"
mkdir -p "$OPENCODE_STATE" "$OPENCODE_RUNTIME"
chown -R wukong:wukong /home/wukong/.local 2>/dev/null || true

# Ensure persistent data volume is writable by the runtime user.
mkdir -p /data
chown -R wukong:wukong /data 2>/dev/null || true

# ── Dispatch ──
# If first arg is a known wukong binary name, run it directly.
# Otherwise default to 'wukong' (CLI/REPL mode).
# ───────────────────────────────

case "${1:-}" in
    wukong|wukong-telegram|wukong-web|wukong-schedulerd)
        # Run as wukong user, preserving environment
        exec gosu wukong "$@"
        ;;
    opencode|agent-reach|gh|python3|pipx)
        # `opencode serve` is the only long-lived one of these, and it is the one
        # that accumulates: unlike `opencode run`, which exits after every turn and
        # so resets its heap, caches and DB handles for free, the server keeps every
        # turn's residue forever. Start the idle-restart supervisor alongside it to
        # put that reset back. It is a sibling process, not a wrapper, so opencode
        # stays PID 1 and its own signal handling is untouched.
        if [[ "${1:-}" == "opencode" && "${2:-}" == "serve" ]]; then
            # Deleting sessions only returns pages to SQLite's freelist; the file
            # shrinks only on VACUUM. Do it here, before the server opens the
            # database — the idle restart brings this moment around about daily.
            # It skips itself unless enough of the file is reclaimable and the
            # disk can hold the rewrite. A failure is logged and never blocks the
            # server from starting (docs/issues/issue-0003).
            #
            # Ask the binary whether it knows the subcommand first. A `wukong` that
            # predates it does not reject `opencode vacuum` — it takes the words as
            # a prompt and runs an agent turn with them. This script and the binary
            # normally ship together, but a local `docker build` pairs this script
            # with whatever release the Dockerfile's VERSION arg downloads.
            #
            # SQLITE_TMPDIR: VACUUM writes a full temporary copy, by default under
            # /var/tmp — the container's overlay root filesystem, which the disk
            # check (made against the database's own filesystem) knows nothing
            # about. Keep the copy next to the database so the check covers it.
            if wukong opencode --help 2>/dev/null | grep -q 'vacuum'; then
                opencode_db="${WUKONG_OPENCODE_DB:-$OPENCODE_STATE/opencode.db}"
                WUKONG_OPENCODE_DB="$opencode_db" \
                    SQLITE_TMPDIR="$(dirname "$opencode_db")" \
                    gosu wukong wukong opencode vacuum \
                    || echo "[wukong] WARNING: opencode.db vacuum failed; starting the server anyway." >&2
            fi
            # Say so out loud when the script is missing. v0.20.0 shipped an image
            # without it (the release build uses Dockerfile.release with a curated
            # context that had not been updated), and because this is launched in
            # the background the failure was invisible: opencode started fine and
            # the restart simply never happened.
            if [[ -x /usr/local/bin/opencode-idle-restart.sh ]]; then
                gosu wukong /usr/local/bin/opencode-idle-restart.sh &
            else
                echo "[wukong] WARNING: opencode-idle-restart.sh is missing from this image;" >&2
                echo "[wukong] the periodic idle restart is DISABLED. This is a packaging bug." >&2
            fi
        fi
        # Allow `docker compose run --rm wukong <tool> ...` for runtime setup.
        exec gosu wukong "$@"
        ;;
    *)
        # Default: run wukong CLI with all args
        exec gosu wukong wukong "$@"
        ;;
esac
