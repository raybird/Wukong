#!/usr/bin/env bash
set -euo pipefail

compose_file="docker-compose.yml"
entrypoint="scripts/docker-entrypoint.sh"
dockerfile="Dockerfile"
release_workflow=".github/workflows/release.yml"

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

require_count_in_file() {
    local pattern="$1"
    local expected="$2"
    local file="$3"
    local message="$4"
    local actual

    actual=$(grep -Fc -- "$pattern" "$file" || true)
    if [[ "$actual" != "$expected" ]]; then
        echo "FAIL: $message" >&2
        echo "expected $expected occurrences of '$pattern', found $actual" >&2
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
require_in_file "COPY crates/wukong-skills/assets/superpowers /usr/local/share/wukong/skills/superpowers" "$dockerfile" \
    "Docker image must package Superpowers skill assets"
require_in_file "/usr/local/share/wukong/skills/superpowers" "$dockerfile" \
    "Dockerfile must use the canonical image skill asset path"
require_in_file "dist/wukong-docker/crates/wukong-skills/assets" "$release_workflow" \
    "Docker release bundle must include the skill asset parent directory"
require_in_file "cp -R crates/wukong-skills/assets/superpowers dist/wukong-docker/crates/wukong-skills/assets/superpowers" "$release_workflow" \
    "Docker release bundle must include Superpowers skill assets in the Docker build context"
require_in_file 'IMAGE_SKILLS="/usr/local/share/wukong/skills/superpowers"' "$entrypoint" \
    "entrypoint must define image skill asset source"
require_in_file 'WORKSPACE_SKILLS="$WUKONG_WORKSPACE/.wukong/skills/superpowers"' "$entrypoint" \
    "entrypoint must define workspace skill asset destination"
require_in_file 'sync_wukong_skills()' "$entrypoint" \
    "entrypoint must provide a skill asset sync function"
require_in_file 'cmp -s "$IMAGE_SKILLS/SOURCE.md" "$WORKSPACE_SKILLS/SOURCE.md"' "$entrypoint" \
    "entrypoint must skip skill sync when SOURCE.md matches"
require_in_file 'cp -a "$IMAGE_SKILLS/." "$tmp_dir/"' "$entrypoint" \
    "entrypoint must copy image skill assets into a temporary directory"
require_in_file 'mv "$tmp_dir" "$WORKSPACE_SKILLS"' "$entrypoint" \
    "entrypoint must atomically install workspace skill assets"
require_in_file "curl -fsS http://localhost:4096/global/health || exit 1" "$compose_file" \
    "opencode server must expose a Compose healthcheck"
require_count_in_file "condition: service_healthy" 3 "$compose_file" \
    "web, telegram, and scheduler must wait for a healthy opencode server"

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
