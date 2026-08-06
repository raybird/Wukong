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

    IMAGE_SKILLS="/usr/local/share/wukong/skills/superpowers"
    WORKSPACE_SKILLS="$WUKONG_WORKSPACE/.wukong/skills/superpowers"

    sync_wukong_skills() {
        if [[ ! -d "$IMAGE_SKILLS" ]]; then
            echo "[wukong] Warning: image skill assets missing at $IMAGE_SKILLS" >&2
            return 0
        fi

        if [[ -f "$IMAGE_SKILLS/SOURCE.md" && -f "$WORKSPACE_SKILLS/SOURCE.md" ]] && \
            cmp -s "$IMAGE_SKILLS/SOURCE.md" "$WORKSPACE_SKILLS/SOURCE.md"; then
            return 0
        fi

        local parent_dir tmp_dir old_dir
        parent_dir="$(dirname "$WORKSPACE_SKILLS")"
        tmp_dir="$parent_dir/.superpowers.tmp.$$"
        old_dir="$parent_dir/.superpowers.old.$$"

        if ! mkdir -p "$parent_dir"; then
            echo "[wukong] Warning: cannot create skill asset directory at $parent_dir" >&2
            return 0
        fi

        rm -rf "$tmp_dir" "$old_dir" 2>/dev/null || true
        if ! mkdir -p "$tmp_dir"; then
            echo "[wukong] Warning: cannot prepare temporary skill asset directory at $tmp_dir" >&2
            return 0
        fi

        if ! cp -a "$IMAGE_SKILLS/." "$tmp_dir/"; then
            echo "[wukong] Warning: failed to copy skill assets into $tmp_dir" >&2
            rm -rf "$tmp_dir" 2>/dev/null || true
            return 0
        fi

        if [[ -d "$WORKSPACE_SKILLS" ]]; then
            if ! mv "$WORKSPACE_SKILLS" "$old_dir"; then
                echo "[wukong] Warning: cannot replace existing skill assets at $WORKSPACE_SKILLS" >&2
                rm -rf "$tmp_dir" 2>/dev/null || true
                return 0
            fi
        fi

        if ! mv "$tmp_dir" "$WORKSPACE_SKILLS"; then
            echo "[wukong] Warning: failed to install skill assets at $WORKSPACE_SKILLS" >&2
            if [[ -d "$old_dir" ]]; then
                mv "$old_dir" "$WORKSPACE_SKILLS" 2>/dev/null || true
            fi
            rm -rf "$tmp_dir" 2>/dev/null || true
            return 0
        fi

        rm -rf "$old_dir" 2>/dev/null || true
        chown -R wukong:wukong "$WUKONG_WORKSPACE/.wukong" 2>/dev/null || true
        echo "[wukong] Workspace skill assets ready at $WORKSPACE_SKILLS"
    }

    sync_wukong_skills

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
# Two backends reach opencode, and only one of them takes a CLI flag:
#   * CLI backend (`opencode run`) — Wukong always starts it with stdin=null, so
#     it can never answer an interactive prompt. WUKONG_AGENT_CMD therefore
#     carries `--dangerously-skip-permissions`.
#   * Server backend (`opencode serve`) — the compose default, selected by
#     WUKONG_AGENT_SERVER_URL. `serve` has no equivalent flag, so THIS FILE is
#     the only permission control Web, Telegram and Scheduler have.
# `--dangerously-skip-permissions` still honours an explicit `deny`, so the bash
# denylist below blocks catastrophic recursive deletes of absolute paths while
# still allowing deletes inside /workspace. To change any of it, put your own
# keys in user.json — do not edit opencode.json, it is overwritten on restart.
# Keep user rules specific and self-contained: opencode resolves permissions by
# "last matching rule wins", and merging two rule objects gives no guarantee
# about where a user key lands in that order.
#
#   external_directory - defaults to `ask`, which is what stalls unattended work:
#                 a scheduled job touching /tmp stops on a prompt nobody answers
#                 until the stream deadline kills the turn (see
#                 docs/2026-08-06-docker-runtime-handover.md). /tmp is allowed
#                 explicitly in three key forms because opencode matches the
#                 requested path against these globs and reports the request as
#                 `/tmp/*`. Grant further paths one at a time — do NOT relax this
#                 to a blanket "allow", it applies to every path-based tool.
#
# The seed also carries CPU guardrails for long conversations. opencode 1.18
# persists every streaming update as a FULL snapshot of the part into its
# durable event log (`opencode.db`), so one long answer costs O(text x updates)
# in JSON serialisation plus SQLite writes — measured at ~430x write
# amplification on a single session (38 MB written for a 91 KB final part).
# These keys cut the base cost of that amplification:
#   snapshot    - opencode keeps a bare git repo per project and commits the
#                 whole workspace on every edit; on a bind-mounted /workspace
#                 that is expensive per tool call, and the store grows without
#                 bound (>1 GB for a single project is normal). Wukong's
#                 workspace is the user's own git repo, so this is redundant.
#                 Disabling it also disables opencode's own revert/undo.
#   tool_output - tool results are the bulk of part payloads; capping them caps
#                 what gets re-serialised on every subsequent update.
#   compaction  - lets opencode trim its own context. Complements (does not
#                 replace) Wukong's WUKONG_SESSION_COMPACT_EVERY_TURNS, which
#                 compacts on turn count rather than on context pressure.
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
  "snapshot": false,
  "compaction": {
    "auto": true,
    "prune": true,
    "tail_turns": 8
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
  "permission": {
    "external_directory": {
      "/tmp": "allow",
      "/tmp/*": "allow",
      "/tmp/**": "allow"
    },
    "bash": {
      "*": "allow",
      "*rm -rf /*": "deny",
      "*rm -fr /*": "deny",
      "*rm -Rf /*": "deny",
      "*rm -rF /*": "deny",
      "*rm -r -f /*": "deny",
      "*rm -f -r /*": "deny",
      "*rm -r /*": "deny",
      "*rm -R /*": "deny",
      "*rm --recursive*": "deny",
      "*rm --force* /*": "deny",
      "*rm -rf ~*": "deny",
      "*rm -rf $HOME*": "deny",
      "*sudo rm *": "deny",
      "*rm * /workspace/*": "allow"
    }
  }
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
        # Allow `docker compose run --rm wukong <tool> ...` for runtime setup.
        exec gosu wukong "$@"
        ;;
    *)
        # Default: run wukong CLI with all args
        exec gosu wukong wukong "$@"
        ;;
esac
