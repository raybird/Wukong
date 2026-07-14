#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
runtime_inputs="$root/release/runtime-inputs.env"
dockerfile="$root/Dockerfile.release"
workflow="$root/.github/workflows/release.yml"

fail() {
  printf 'release image: %s\n' "$*" >&2
  exit 1
}

require_file() {
  [[ -f "$1" ]] || fail "missing $1"
}

require_text() {
  grep -Fq -- "$1" "$2" || fail "missing contract '$1' in $2"
}

check_pins() {
  require_file "$runtime_inputs"
  # shellcheck disable=SC1090
  source "$runtime_inputs"
  [[ "${BASE_IMAGE:-}" =~ ^debian:bookworm-slim@sha256:[0-9a-f]{64}$ ]] || fail "BASE_IMAGE must use a sha256 digest"
  [[ "${DEBIAN_SNAPSHOT:-}" =~ ^[0-9]{8}T[0-9]{6}Z$ ]] || fail "DEBIAN_SNAPSHOT must be immutable"
  [[ "${OPENCODE_VERSION:-}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "OPENCODE_VERSION must be exact"
  [[ "${OPENCODE_INTEGRITY:-}" =~ ^sha512- ]] || fail "OPENCODE_INTEGRITY must be an npm integrity value"
  [[ "${AGENT_REACH_REF:-}" =~ ^[0-9a-f]{40}$ ]] || fail "AGENT_REACH_REF must be a full commit SHA"
  [[ "${AGENT_REACH_ARCHIVE_SHA256:-}" =~ ^[0-9a-f]{64}$ ]] || fail "AGENT_REACH_ARCHIVE_SHA256 must be a sha256 digest"
  grep -Eqi '(^|=)(latest|main|<|>)' "$runtime_inputs" && fail "runtime inputs contain a floating value"
  grep -Fq "\"opencode-ai\": \"$OPENCODE_VERSION\"" "$root/release/package.json" || fail "package.json does not pin OpenCode"
  grep -Fq "\"integrity\": \"$OPENCODE_INTEGRITY\"" "$root/release/package-lock.json" || fail "package lock integrity does not match runtime inputs"
}

check_static() {
  local debian_line security_line
  check_pins
  require_file "$dockerfile"
  require_text 'COPY binaries/wukong' "$dockerfile"
  require_text 'DEBIAN_SNAPSHOT' "$dockerfile"
  require_text 'snapshot.debian.org' "$dockerfile"
  require_text 'npm ci --omit=dev' "$dockerfile"
  require_text 'AGENT_REACH_ARCHIVE_SHA256' "$dockerfile"
  require_text 'org.opencontainers.image.source="https://github.com/raybird/Wukong"' "$dockerfile"
  require_text 'COPY scripts/docker-entrypoint.sh' "$dockerfile"
  require_text 'gosu wukong' "$dockerfile"
  debian_line="$(grep -Fn 's|http://deb.debian.org/debian|' "$dockerfile" | cut -d: -f1)"
  security_line="$(grep -Fn 's|http://deb.debian.org/debian-security|' "$dockerfile" | cut -d: -f1)"
  [[ "$security_line" -lt "$debian_line" ]] || fail "security snapshot replacement must precede its Debian URL prefix"
  ! grep -Eqi 'github\.com/.*/releases/download' "$dockerfile" || fail "release image must not download GitHub release binaries"
  ! grep -Fq -- '--help >/dev/null' "$dockerfile" || fail "release image build must not run smoke tests"
}

check_registry_contract() {
  require_file "$workflow"
  require_text 'regctl image digest' "$workflow"
  require_text 'regctl image copy' "$workflow"
  require_text 'regctl image copy "$image@$expected" "$image:$tag"' "$workflow"
  require_text 'musl-binaries' "$workflow"
}

check_stable_tag_contract() {
  require_file "$workflow"
  require_text 'scripts/generate-sha256sums.sh dist' "$workflow"
  require_text 'regctl image copy "$image@$image_digest" "$image:latest"' "$workflow"
  ! grep -Fq 'attach_immutable latest' "$workflow" || fail "latest must remain a mutable stable pointer"
}

check_smoke() {
  local image="${1:?usage: test-release-image.sh smoke <image>}"
  for binary in wukong wukong-telegram wukong-web wukong-schedulerd opencode agent-reach; do
    docker run --rm --entrypoint "$binary" "$image" --help >/dev/null
  done
  [[ "$(docker image inspect --format '{{.Config.User}}' "$image")" != "" ]] || true
}

case "${1:-all}" in
  pins) check_pins ;;
  static) check_static ;;
  registry) check_registry_contract ;;
  promotion) check_stable_tag_contract ;;
  smoke) shift; check_smoke "$@" ;;
  all) check_static; check_registry_contract; check_stable_tag_contract ;;
  *) fail "unknown check: $1" ;;
esac

echo "release image checks passed"
