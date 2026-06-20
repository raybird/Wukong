#!/usr/bin/env bash
set -euo pipefail

compose_file="docker-compose.yml"
entrypoint="scripts/docker-entrypoint.sh"

require_in_file() {
    local pattern="$1"
    local file="$2"
    local message="$3"

    if ! grep -Fq -- "$pattern" "$file"; then
        echo "FAIL: $message" >&2
        echo "missing pattern: $pattern" >&2
        exit 1
    fi
}

require_in_file "opencode-state:/home/wukong/.local/share/opencode" "$compose_file" \
    "docker compose must persist opencode session state"
require_in_file "opencode-state:" "$compose_file" \
    "docker compose must declare the opencode state volume"
require_in_file "OPENCODE_STATE=\"/home/wukong/.local/share/opencode\"" "$entrypoint" \
    "entrypoint must name opencode state directory"
require_in_file "OPENCODE_RUNTIME=\"/home/wukong/.local/state\"" "$entrypoint" \
    "entrypoint must name opencode runtime state directory"
require_in_file 'mkdir -p "$OPENCODE_STATE" "$OPENCODE_RUNTIME"' "$entrypoint" \
    "entrypoint must create opencode runtime directories before gosu"
require_in_file "chown -R wukong:wukong /home/wukong/.local" "$entrypoint" \
    "entrypoint must chown Docker-created .local directories"
require_in_file "agent-reach-state:/home/wukong/.agent-reach" "$compose_file" \
    "docker compose must persist Agent Reach state"
require_in_file "AGENT_REACH_STATE=\"/home/wukong/.agent-reach\"" "$entrypoint" \
    "entrypoint must name Agent Reach state directory"
require_in_file 'mkdir -p "$AGENT_REACH_STATE"' "$entrypoint" \
    "entrypoint must create Agent Reach state directory before gosu"
require_in_file 'chown -R wukong:wukong "$AGENT_REACH_STATE"' "$entrypoint" \
    "entrypoint must chown Docker-created Agent Reach state volume"

if awk '
    /^  wukong-schedulerd:/ { in_scheduler = 1; next }
    /^  [a-zA-Z0-9_-]+:/ { in_scheduler = 0 }
    in_scheduler && /profiles:/ { found = 1 }
    END { exit found ? 0 : 1 }
' "$compose_file"; then
    echo "FAIL: wukong-schedulerd must start by default, not only through a compose profile" >&2
    exit 1
fi

echo "docker runtime persistence checks passed"
