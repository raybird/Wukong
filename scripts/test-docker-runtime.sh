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

echo "docker runtime persistence checks passed"
