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
OPENCODE_CONFIG="/home/wukong/.config/opencode"
mkdir -p "$OPENCODE_CONFIG"

# ── Seed a default opencode config with a destructive-command guard ──
# Wukong always drives `opencode run` with stdin=null, so opencode can never
# receive an interactive permission answer (REPL, Web, Telegram, Scheduler all
# alike). We therefore run with `--dangerously-skip-permissions` (set in
# WUKONG_AGENT_CMD) to auto-approve prompts — but that flag still honours any
# explicit `deny`, so this config keeps a denylist that blocks catastrophic
# recursive deletes of absolute paths while still allowing deletes inside
# /workspace. Only written when missing; drop your own opencode.json into the
# `opencode-config` volume to override.
OPENCODE_CONFIG_FILE="$OPENCODE_CONFIG/opencode.json"
if [[ ! -f "$OPENCODE_CONFIG_FILE" ]]; then
    echo "[wukong] Seeding default opencode.json (destructive-rm guard)..."
    cat > "$OPENCODE_CONFIG_FILE" <<'OPENCODE_JSON'
{
  "$schema": "https://opencode.ai/config.json",
  "permission": {
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
fi

chown -R wukong:wukong /home/wukong/.config 2>/dev/null || true

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
    opencode)
        # Allow `docker compose run --rm wukong opencode ...` for auth/config setup.
        exec gosu wukong "$@"
        ;;
    *)
        # Default: run wukong CLI with all args
        exec gosu wukong wukong "$@"
        ;;
esac
