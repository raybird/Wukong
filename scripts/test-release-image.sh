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

  # The image must be opened and inspected before a public tag points at it.
  # Order matters: verifying after attach_immutable would mean a broken image is
  # already published by the time anyone notices.
  require_text 'bash scripts/test-release-image.sh smoke "$image@$image_digest"' "$workflow"
  local smoke_line attach_line
  smoke_line="$(grep -Fn 'test-release-image.sh smoke' "$workflow" | head -1 | cut -d: -f1)"
  attach_line="$(grep -Fn 'attach_immutable "$TAG"' "$workflow" | head -1 | cut -d: -f1)"
  [[ -n "$smoke_line" && -n "$attach_line" && "$smoke_line" -lt "$attach_line" ]] ||
    fail "the image smoke check must run before attach_immutable publishes the tag"
}

check_stable_tag_contract() {
  require_file "$workflow"
  require_text 'scripts/generate-sha256sums.sh dist' "$workflow"
  require_text 'regctl image copy "$image@$image_digest" "$image:latest"' "$workflow"
  ! grep -Fq 'attach_immutable latest' "$workflow" || fail "latest must remain a mutable stable pointer"
}

# Every COPY in Dockerfile.release must actually be reachable in the build context.
#
# The context is not the repository: release.yml assembles it file by file into
# release-context/. A COPY whose source was never copied in there does not fail the
# build — buildx just finds nothing and the file is silently absent from the image.
# v0.20.0 shipped exactly that: the entrypoint called opencode-idle-restart.sh, the
# image did not contain it, and the idle restart never ran. Nothing was red.
#
# This is a structural check, not a text match: it reads the real COPY sources and
# the real provisioning lines, so a newly added COPY cannot pass by accident.
check_context() {
  local sources source provisioned
  require_file "$dockerfile"
  require_file "$workflow"

  # All COPY sources = every arg between "COPY" and the destination, minus flags.
  sources="$(awk '/^COPY / { for (i = 2; i < NF; i++) if ($i !~ /^--/) print $i }' "$dockerfile")"
  [[ -n "$sources" ]] || fail "no COPY sources parsed from $dockerfile"

  while IFS= read -r source; do
    [[ -n "$source" ]] || continue
    if [[ "$source" == binaries/* ]]; then
      # Built by build-binaries and unpacked straight into the context.
      grep -Fq 'path: release-context/binaries' "$workflow" ||
        fail "$source is COPYed but release.yml never downloads artifacts into release-context/binaries"
      continue
    fi

    # Copied in explicitly. Both halves have to hold: the file must exist in the
    # repository, and release.yml must actually place it into the context.
    [[ -e "$root/$source" ]] ||
      fail "$dockerfile COPYs '$source' but it does not exist in the repository"
    provisioned="$(grep -F -- "$source" "$workflow" | grep -c 'release-context' || true)"
    [[ "$provisioned" -ge 1 ]] ||
      fail "$dockerfile COPYs '$source' but release.yml never copies it into release-context/ — it would vanish from the image with no build error"
  done <<<"$sources"
}

# Runtime files the entrypoint depends on. A missing one here is invisible at build
# time and usually silent at run time too, so assert against the real image.
RUNTIME_EXECUTABLES=(
  /usr/local/bin/docker-entrypoint.sh
  /usr/local/bin/opencode-idle-restart.sh
  /usr/local/bin/wukong
  /usr/local/bin/wukong-telegram
  /usr/local/bin/wukong-web
  /usr/local/bin/wukong-schedulerd
)

check_smoke() {
  local image="${1:?usage: test-release-image.sh smoke <image>}" path
  for binary in wukong wukong-telegram wukong-web wukong-schedulerd opencode agent-reach; do
    docker run --rm --entrypoint "$binary" "$image" --help >/dev/null
  done

  for path in "${RUNTIME_EXECUTABLES[@]}"; do
    docker run --rm --entrypoint test "$image" -x "$path" ||
      fail "$image is missing an executable runtime file: $path"
  done

  # The restart window is a local-time range; without tzdata TZ silently resolves to
  # UTC and the window quietly moves by the offset.
  docker run --rm --entrypoint test "$image" -f /usr/share/zoneinfo/Asia/Taipei ||
    fail "$image has no tzdata, so TZ cannot resolve and the restart window would drift"

  [[ "$(docker image inspect --format '{{.Config.User}}' "$image")" != "" ]] || true
}

case "${1:-all}" in
  pins) check_pins ;;
  static) check_static ;;
  context) check_context ;;
  registry) check_registry_contract ;;
  promotion) check_stable_tag_contract ;;
  smoke) shift; check_smoke "$@" ;;
  all) check_static; check_context; check_registry_contract; check_stable_tag_contract ;;
  *) fail "unknown check: $1" ;;
esac

echo "release image checks passed"
