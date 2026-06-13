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
chown -R wukong:wukong /home/wukong/.config 2>/dev/null || true

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
