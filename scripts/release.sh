#!/usr/bin/env bash
set -euo pipefail

TAG_RE='^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[1-9][0-9]*)?$'
RC_RE='^v[0-9]+\.[0-9]+\.[0-9]+-rc\.[1-9][0-9]*$'
TAG=""
PROMOTE_FROM=""
REHEARSAL_REPORT=""
DRY_RUN=false

die() {
  printf 'release: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'USAGE'
Usage: scripts/release.sh <vX.Y.Z|vX.Y.Z-rc.N> [--promote-from vX.Y.Z-rc.N] [--rehearsal-report path] [--dry-run]
USAGE
}

require_repository() {
  ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || die "not in a Git repository"
  for path in Cargo.toml Cargo.lock .github/workflows/release.yml scripts/install.sh; do
    [[ -e "$ROOT/$path" ]] || die "wrong repository: missing $path"
  done
  cd "$ROOT"
}

require_branch() {
  BRANCH="$(git symbolic-ref --quiet --short HEAD)" || die "detached HEAD is not releasable"
  [[ "$BRANCH" == main || "$BRANCH" == release/* ]] || die "release branch must be main or release/*"
}

require_clean_worktree() {
  [[ -z "$(git status --porcelain=v1 --untracked-files=all)" ]] || die "worktree is not clean"
}

require_synced_upstream() {
  UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}')" || die "branch has no upstream"
  git fetch --prune --no-tags origin

  local ahead behind
  read -r ahead behind < <(git rev-list --left-right --count HEAD..."$UPSTREAM")
  [[ "$ahead" == 0 && "$behind" == 0 ]] || die "branch differs from upstream: ahead=$ahead behind=$behind"
}

require_tag_absent() {
  ! git show-ref --verify --quiet "refs/tags/$TAG" || die "local tag already exists: $TAG"
  [[ -z "$(git ls-remote --tags origin "refs/tags/$TAG" "refs/tags/$TAG^{}")" ]] || die "remote tag already exists: $TAG"
}

require_changelog() {
  local version="${TAG#v}"
  local base_version="${version%%-rc.*}"
  local escaped_version="${base_version//./\\.}"

  if [[ "$CHANNEL" == stable ]]; then
    grep -Eq "^## \[${escaped_version}\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$" CHANGELOG.md || die "stable changelog entry needs a release date"
  else
    grep -Eq "^## \[${escaped_version}\]( - [0-9]{4}-[0-9]{2}-[0-9]{2})?$" CHANGELOG.md || die "changelog is missing $base_version"
  fi
}

require_promotion_source() {
  [[ "$CHANNEL" == stable ]] || return 0
  git rev-parse --verify --quiet "refs/tags/$PROMOTE_FROM" >/dev/null || die "source RC tag does not exist: $PROMOTE_FROM"
  [[ "$(git cat-file -t "$PROMOTE_FROM")" == tag ]] || die "source RC must be annotated"
  [[ "$(git rev-parse "$PROMOTE_FROM^{commit}")" == "$(git rev-parse HEAD)" ]] || die "source RC must point to HEAD"
}

require_rehearsal_report() {
  [[ "$CHANNEL" == stable ]] || return 0
  [[ -n "$REHEARSAL_REPORT" ]] || die "stable releases require --rehearsal-report"
  [[ "$REHEARSAL_REPORT" != /* && "$REHEARSAL_REPORT" != *".."* ]] || die "rehearsal report must be a repository-relative path"
  [[ -f "$REHEARSAL_REPORT" ]] || die "rehearsal report is missing: $REHEARSAL_REPORT"
  [[ -z "$(git status --porcelain=v1 -- "$REHEARSAL_REPORT")" ]] || die "rehearsal report must be committed at HEAD"
  local source_digest
  source_digest="$(git show "$PROMOTE_FROM:release-manifest.json" 2>/dev/null | python3 -c 'import json,sys; print(json.load(sys.stdin)["image"]["digest"])')" || die "source RC manifest is unavailable"
  scripts/validate-rehearsal-report.sh "$REHEARSAL_REPORT" "$PROMOTE_FROM" "$(git rev-parse HEAD)" "$source_digest" || die "rehearsal report does not satisfy stable promotion gate"
}

require_lockfile() {
  [[ -f Cargo.lock ]] || die "Cargo.lock is required"
  cargo metadata --locked --format-version 1 >/dev/null || die "cargo metadata --locked failed"
}

require_release_compose() {
  local compose="docker-compose.release.yml" placeholders
  [[ -f "$compose" ]] || die "missing docker-compose.release.yml"
  ! grep -Eq '^[[:space:]]*build:' "$compose" || die "release compose must not contain build"
  placeholders="$(grep -Fc 'ghcr.io/raybird/wukong:__WUKONG_VERSION__' "$compose" || true)"
  [[ "$placeholders" == 5 ]] || die "release compose must contain five image placeholders"
}

require_gh() {
  command -v gh >/dev/null 2>&1 || die "gh is required"
  gh auth status >/dev/null || die "gh authentication failed"
}

run_check() {
  printf 'release: running %s\n' "$*"
  "$@" || die "failed: $*"
}

run_required_checks() {
  if [[ "${WUKONG_RELEASE_UNDER_TEST:-0}" == 1 ]]; then
    return 0
  fi

  if [[ "${WUKONG_RELEASE_TESTING:-0}" == 1 ]]; then
    [[ -n "${WUKONG_RELEASE_TEST_COMMANDS_FILE:-}" ]] || die "test command file is required"
    while read -r command argument; do
      [[ -n "$command" ]] || continue
      run_check "$command" "$argument"
    done < "$WUKONG_RELEASE_TEST_COMMANDS_FILE"
    return 0
  fi

  run_check cargo fmt --all -- --check
  run_check cargo clippy --all-targets --locked -- -D warnings
  run_check cargo test --workspace --locked
  run_check bash scripts/test-release-workflow.sh
  run_check bash scripts/test-release-manifest.sh
  run_check bash scripts/test-release-image.sh
  WUKONG_RELEASE_UNDER_TEST=1 run_check bash scripts/test-release-command.sh
  run_check bash scripts/test-installer-upgrade.sh
  run_check bash scripts/test-docker-runtime.sh
}

print_release_plan() {
  printf 'release: dry run for %s (%s)\n' "$TAG" "$CHANNEL"
  printf 'release: commit %s\n' "$(git rev-parse HEAD)"
  printf 'release: targets x86_64-unknown-linux-gnu x86_64-unknown-linux-musl aarch64-apple-darwin\n'
  printf 'release: components wukong wukong-telegram wukong-web wukong-schedulerd\n'
}

create_and_push_tag() {
  local annotation="$TAG"
  if [[ "$CHANNEL" == stable ]]; then
    annotation+=$'\n'
    annotation+="promote-from: $PROMOTE_FROM"
    annotation+=$'\n'
    annotation+="rehearsal-report: $REHEARSAL_REPORT"
  fi

  git tag -a "$TAG" -m "$annotation"
  git push origin "refs/tags/$TAG"
}

watch_release_workflow() {
  local run_id
  run_id="$(gh run list --workflow Release --branch "$TAG" --event push --json databaseId,headBranch --jq ".[] | select(.headBranch == \"$TAG\") | .databaseId" | tail -n 1)"
  [[ -n "$run_id" ]] || die "release workflow did not appear for $TAG"
  gh run watch "$run_id" --exit-status || die "release workflow failed; public tag $TAG remains and must not be reused"
}

verify_release_assets() {
  local release_info expected_prerelease asset
  release_info="$(gh release view "$TAG" --json tagName,isPrerelease --jq '[.tagName, .isPrerelease] | @tsv')" || die "GitHub Release is unavailable for $TAG"
  [[ "$release_info" == $TAG$'\t'* ]] || die "GitHub Release tag does not match $TAG"
  if [[ "$CHANNEL" == rc ]]; then expected_prerelease=true; else expected_prerelease=false; fi
  [[ "$release_info" == *$'\t'"$expected_prerelease" ]] || die "GitHub Release prerelease channel is incorrect"

  local assets
  assets="$(gh release view "$TAG" --json assets --jq '.assets[].name')" || die "GitHub Release assets are unavailable"
  for asset in \
    wukong-x86_64-unknown-linux-gnu.tar.gz \
    wukong-telegram-x86_64-unknown-linux-gnu.tar.gz \
    wukong-web-x86_64-unknown-linux-gnu.tar.gz \
    wukong-schedulerd-x86_64-unknown-linux-gnu.tar.gz \
    wukong-x86_64-unknown-linux-musl.tar.gz \
    wukong-telegram-x86_64-unknown-linux-musl.tar.gz \
    wukong-web-x86_64-unknown-linux-musl.tar.gz \
    wukong-schedulerd-x86_64-unknown-linux-musl.tar.gz \
    wukong-aarch64-apple-darwin.tar.gz \
    wukong-telegram-aarch64-apple-darwin.tar.gz \
    wukong-web-aarch64-apple-darwin.tar.gz \
    wukong-schedulerd-aarch64-apple-darwin.tar.gz \
    checksums-x86_64-unknown-linux-gnu.txt \
    checksums-x86_64-unknown-linux-musl.txt \
    checksums-aarch64-apple-darwin.txt \
    "wukong-docker-$TAG.tar.gz" \
    release-manifest.json \
    SHA256SUMS; do
    grep -Fxq "$asset" <<<"$assets" || die "GitHub Release is missing asset: $asset"
  done
}

while (($#)); do
  case "$1" in
    --promote-from)
      [[ $# -ge 2 && -z "$PROMOTE_FROM" ]] || die "invalid --promote-from"
      PROMOTE_FROM="$2"
      shift 2
      ;;
    --rehearsal-report)
      [[ $# -ge 2 && -z "$REHEARSAL_REPORT" ]] || die "invalid --rehearsal-report"
      REHEARSAL_REPORT="$2"
      shift 2
      ;;
    --dry-run)
      $DRY_RUN && die "duplicate --dry-run"
      DRY_RUN=true
      shift
      ;;
    --help)
      usage
      exit 0
      ;;
    -*)
      die "unknown option: $1"
      ;;
    *)
      [[ -z "$TAG" ]] || die "only one product tag is allowed"
      TAG="$1"
      shift
      ;;
  esac
done

[[ "$TAG" =~ $TAG_RE ]] || die "tag must be vX.Y.Z or vX.Y.Z-rc.N"

if [[ "$TAG" =~ $RC_RE ]]; then
  [[ -z "$PROMOTE_FROM" ]] || die "RC releases cannot use --promote-from"
  CHANNEL=rc
else
  [[ "$PROMOTE_FROM" =~ $RC_RE ]] || die "stable releases require --promote-from vX.Y.Z-rc.N"
  [[ "${PROMOTE_FROM%-rc.*}" == "$TAG" ]] || die "stable and RC base versions differ"
  CHANNEL=stable
fi

require_repository
require_branch
require_clean_worktree
require_synced_upstream
require_tag_absent
require_changelog
require_promotion_source
require_rehearsal_report
require_lockfile
require_release_compose
require_gh
run_required_checks
require_clean_worktree

if $DRY_RUN; then
  print_release_plan
  exit 0
fi

create_and_push_tag
watch_release_workflow
verify_release_assets

printf 'release: preflight passed for %s release %s\n' "$CHANNEL" "$TAG"
