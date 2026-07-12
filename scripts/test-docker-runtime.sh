#!/usr/bin/env bash
set -euo pipefail

release_compose="docker-compose.release.yml"
if [[ ! -f "$release_compose" ]]; then
  echo "missing release compose: $release_compose" >&2
  exit 1
fi

if grep -Eq '^[[:space:]]*build:' "$release_compose"; then
  echo "release compose must not contain build directives" >&2
  exit 1
fi

image_count=$(grep -Fc 'ghcr.io/raybird/wukong:__WUKONG_VERSION__' "$release_compose" || true)
if [[ "$image_count" != 5 ]]; then
  echo "release compose must pin all five services to the product placeholder" >&2
  exit 1
fi

if grep -Fq ':latest' "$release_compose"; then
  echo "release compose must not use latest" >&2
  exit 1
fi

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
require_in_file "# Development-only Compose" "$compose_file" \
    "development Compose must identify its local-build role"
require_in_file "# Release deployment template" "$release_compose" \
    "release Compose must identify its pull-only role"
for service in wukong opencode-server wukong-telegram wukong-web wukong-schedulerd; do
    require_in_file "  $service:" "$release_compose" "release Compose must include $service"
done
require_in_file 'WUKONG_WEB_BIND:-127.0.0.1' "$release_compose" \
    "release Compose must retain the loopback web default"
require_in_file 'curl -fsS http://localhost:4096/global/health || exit 1' "$release_compose" \
    "release Compose must retain the OpenCode healthcheck"
require_count_in_file "condition: service_healthy" 3 "$release_compose" \
    "release services must retain server dependencies"
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
require_in_file 'DOCKER_RELEASE_OWNED=(docker-compose.yml .env.example LICENSE scripts/install.sh)' scripts/install.sh \
    "installer must replace only Docker release-owned files"
if grep -Eq 'docker compose (build|down)' scripts/install.sh; then
    echo "FAIL: release installer must pull and recreate without local builds or volume removal" >&2
    exit 1
fi

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
