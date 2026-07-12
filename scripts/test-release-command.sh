#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
release_script="$repo_root/scripts/release.sh"

grep -Fq 'for attempt in {1..24}' "$release_script" || { printf 'FAIL: workflow discovery must poll\n' >&2; exit 1; }

fail() {
  printf 'FAIL: %s\n' "$*" >&2
  exit 1
}

assert_rejected() {
  local description="$1"
  shift
  local output

  if output="$("$release_script" "$@" 2>&1)"; then
    fail "$description: expected failure"
  fi

  [[ "$output" == *"release:"* ]] || fail "$description: expected release error, got: $output"
}

assert_parsed() {
  local description="$1"
  local expected="$2"
  shift 2
  local output

  output="$("$release_script" "$@")" || fail "$description: expected success"
  [[ "$output" == *"$expected"* ]] || fail "$description: expected $expected, got: $output"
}

new_fixture() {
  FIXTURE_ROOT="$(mktemp -d)"
  FIXTURE_REMOTE="$FIXTURE_ROOT/origin.git"
  FIXTURE_WORK="$FIXTURE_ROOT/work"
  FIXTURE_BIN="$FIXTURE_ROOT/bin"

  git init --bare "$FIXTURE_REMOTE" >/dev/null
  git init -b main "$FIXTURE_WORK" >/dev/null
  git -C "$FIXTURE_WORK" config user.name "Release Test"
  git -C "$FIXTURE_WORK" config user.email "release-test@example.invalid"
  mkdir -p "$FIXTURE_WORK/.github/workflows" "$FIXTURE_WORK/scripts" "$FIXTURE_BIN"
  cp "$repo_root/Cargo.toml" "$FIXTURE_WORK/Cargo.toml"
  cp "$repo_root/Cargo.lock" "$FIXTURE_WORK/Cargo.lock"
  cp "$repo_root/CHANGELOG.md" "$FIXTURE_WORK/CHANGELOG.md"
  cp "$repo_root/.github/workflows/release.yml" "$FIXTURE_WORK/.github/workflows/release.yml"
  cp "$repo_root/docker-compose.release.yml" "$FIXTURE_WORK/docker-compose.release.yml"
  cp "$repo_root/scripts/install.sh" "$FIXTURE_WORK/scripts/install.sh"
  cp "$release_script" "$FIXTURE_WORK/scripts/release.sh"
  chmod +x "$FIXTURE_WORK/scripts/release.sh"
  cat > "$FIXTURE_BIN/cargo" <<'CARGO'
#!/usr/bin/env bash
set -euo pipefail
if [[ "$*" == "metadata --locked --format-version 1" ]]; then
  exit "${FAKE_CARGO_METADATA_EXIT:-0}"
fi
exit 0
CARGO
  chmod +x "$FIXTURE_BIN/cargo"
  cat > "$FIXTURE_BIN/gh" <<'GH'
#!/usr/bin/env bash
set -euo pipefail
case "$1 $2" in
  "auth status") exit "${FAKE_GH_AUTH_EXIT:-0}" ;;
  "run list") printf '%s\n' "${FAKE_GH_RUN_ID:-123}" ;;
  "run watch") exit "${FAKE_GH_WATCH_EXIT:-0}" ;;
  "release view")
    if [[ "$*" == *"assets"* ]]; then
      printf '%s\n' wukong-x86_64-unknown-linux-gnu.tar.gz wukong-telegram-x86_64-unknown-linux-gnu.tar.gz wukong-web-x86_64-unknown-linux-gnu.tar.gz wukong-schedulerd-x86_64-unknown-linux-gnu.tar.gz wukong-x86_64-unknown-linux-musl.tar.gz wukong-telegram-x86_64-unknown-linux-musl.tar.gz wukong-web-x86_64-unknown-linux-musl.tar.gz wukong-schedulerd-x86_64-unknown-linux-musl.tar.gz wukong-aarch64-apple-darwin.tar.gz wukong-telegram-aarch64-apple-darwin.tar.gz wukong-web-aarch64-apple-darwin.tar.gz wukong-schedulerd-aarch64-apple-darwin.tar.gz wukong-docker-v0.18.0-rc.1.tar.gz release-manifest.json SHA256SUMS
    else
      printf 'v0.18.0-rc.1\ttrue\n'
    fi
    ;;
  *) printf 'unexpected gh invocation: %s\n' "$*" >&2; exit 99 ;;
esac
GH
  chmod +x "$FIXTURE_BIN/gh"
  cat > "$FIXTURE_WORK/scripts/fake-release-check.sh" <<'CHECK'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$1" >> "$WUKONG_RELEASE_CHECK_LOG"
CHECK
  chmod +x "$FIXTURE_WORK/scripts/fake-release-check.sh"
  printf '%s\n' \
    'scripts/fake-release-check.sh first' \
    'scripts/fake-release-check.sh second' \
    > "$FIXTURE_WORK/release-checks.txt"
  printf '\n## [0.18.0] - 2026-07-12\n' >> "$FIXTURE_WORK/CHANGELOG.md"
  git -C "$FIXTURE_WORK" add .
  git -C "$FIXTURE_WORK" commit -m "fixture" >/dev/null
  git -C "$FIXTURE_WORK" remote add origin "$FIXTURE_REMOTE"
  git -C "$FIXTURE_WORK" push -u origin main >/dev/null
}

destroy_fixture() {
  rm -rf "${FIXTURE_ROOT:-}"
  FIXTURE_ROOT=""
}

assert_fixture_rejected() {
  local description="$1"
  local expected="$2"
  local output
  shift 2

  if output="$(cd "$FIXTURE_WORK" && PATH="$FIXTURE_BIN:$PATH" scripts/release.sh "$@" --dry-run 2>&1)"; then
    fail "$description: expected failure"
  fi
  [[ "$output" == *"$expected"* ]] || fail "$description: expected $expected, got: $output"
}

assert_fixture_parsed() {
  local output

  output="$(cd "$FIXTURE_WORK" && WUKONG_RELEASE_UNDER_TEST=1 PATH="$FIXTURE_BIN:$PATH" scripts/release.sh v0.18.0-rc.1 --dry-run)" || fail "valid RC: expected success"
  [[ "$output" == *"dry run for v0.18.0-rc.1 (rc)"* ]] || fail "valid RC: unexpected output: $output"
  output="$(cd "$FIXTURE_WORK" && WUKONG_RELEASE_UNDER_TEST=1 PATH="$FIXTURE_BIN:$PATH" scripts/release.sh v0.18.0 --dry-run)" || fail "valid stable: expected success"
  [[ "$output" == *"dry run for v0.18.0 (stable)"* ]] || fail "valid stable: unexpected output: $output"
}

assert_fixture_metadata_rejected() {
  local output

  if output="$(cd "$FIXTURE_WORK" && FAKE_CARGO_METADATA_EXIT=1 PATH="$FIXTURE_BIN:$PATH" scripts/release.sh v0.18.0-rc.1 --dry-run 2>&1)"; then
    fail "cargo metadata failure: expected failure"
  fi
  [[ "$output" == *"cargo metadata --locked failed"* ]] || fail "cargo metadata failure: unexpected output: $output"
}

assert_fixture_quality_gate() {
  local log="$FIXTURE_ROOT/release-check.log"
  local output

  output="$(cd "$FIXTURE_WORK" && \
    WUKONG_RELEASE_TESTING=1 \
    WUKONG_RELEASE_TEST_COMMANDS_FILE=release-checks.txt \
    WUKONG_RELEASE_CHECK_LOG="$log" \
    PATH="$FIXTURE_BIN:$PATH" \
    scripts/release.sh v0.18.0-rc.1 --dry-run)" || fail "quality gate: expected success"
  [[ "$output" == *"dry run"* ]] || fail "quality gate: expected dry-run plan"
  [[ "$(paste -sd, "$log")" == "first,second" ]] || fail "quality gate: checks did not run in order"
}

assert_fixture_release_compose_rejected() {
  local description="$1" expected="$2" mutation="$3" output
  eval "$mutation"
  git -C "$FIXTURE_WORK" add -A
  git -C "$FIXTURE_WORK" commit -m "mutate release compose" >/dev/null
  git -C "$FIXTURE_WORK" push origin main >/dev/null
  if output="$(cd "$FIXTURE_WORK" && WUKONG_RELEASE_UNDER_TEST=1 PATH="$FIXTURE_BIN:$PATH" scripts/release.sh v0.18.0-rc.1 --dry-run 2>&1)"; then
    fail "$description: expected failure"
  fi
  [[ "$output" == *"$expected"* ]] || fail "$description: expected $expected, got: $output"
}

assert_fixture_rc_tag_pushed() {
  local checks="$FIXTURE_WORK/release-checks.txt"

  cd "$FIXTURE_WORK"
  WUKONG_RELEASE_TESTING=1 \
    WUKONG_RELEASE_TEST_COMMANDS_FILE="$checks" \
    WUKONG_RELEASE_CHECK_LOG="$FIXTURE_ROOT/release-check.log" \
    PATH="$FIXTURE_BIN:$PATH" \
    scripts/release.sh v0.18.0-rc.1 >/dev/null || fail "RC release: expected success"
  [[ "$(git cat-file -t v0.18.0-rc.1)" == tag ]] || fail "RC release: tag is not annotated"
  [[ "$(git for-each-ref refs/tags/v0.18.0-rc.1 --format='%(contents)')" == "v0.18.0-rc.1" ]] || fail "RC release: annotation is incorrect"
  [[ -n "$(git ls-remote --tags origin refs/tags/v0.18.0-rc.1)" ]] || fail "RC release: tag was not pushed"
}

assert_fixture_workflow_failure_keeps_tag() {
  local output

  cd "$FIXTURE_WORK"
  if output="$(FAKE_GH_WATCH_EXIT=1 \
    WUKONG_RELEASE_TESTING=1 \
    WUKONG_RELEASE_TEST_COMMANDS_FILE=release-checks.txt \
    WUKONG_RELEASE_CHECK_LOG="$FIXTURE_ROOT/release-check.log" \
    PATH="$FIXTURE_BIN:$PATH" \
    scripts/release.sh v0.18.0-rc.1 2>&1)"; then
    fail "workflow failure: expected release failure"
  fi
  [[ -n "$(git ls-remote --tags origin refs/tags/v0.18.0-rc.1)" ]] || fail "workflow failure: pushed tag was removed"
}

assert_rejected "missing tag"
assert_rejected "missing v prefix" "0.18.0"
assert_rejected "incomplete stable tag" "v0.18"
assert_rejected "zero RC number" "v0.18.0-rc.0"
assert_rejected "unsupported prerelease" "v0.18.0-beta.1"
assert_rejected "removed promotion option" "v0.18.0" --promote-from "v0.18.0-rc.1"
assert_rejected "removed rehearsal option" "v0.18.0" --rehearsal-report report.json

trap destroy_fixture EXIT

new_fixture
assert_fixture_parsed

printf 'tracked change\n' >> "$FIXTURE_WORK/CHANGELOG.md"
assert_fixture_rejected "tracked change" "worktree is not clean" v0.18.0-rc.1
git -C "$FIXTURE_WORK" restore CHANGELOG.md

touch "$FIXTURE_WORK/untracked-file"
assert_fixture_rejected "untracked change" "worktree is not clean" v0.18.0-rc.1
rm "$FIXTURE_WORK/untracked-file"

git -C "$FIXTURE_WORK" checkout -b feature/release-test >/dev/null
assert_fixture_rejected "invalid branch" "release branch must be main or release/*" v0.18.0-rc.1
git -C "$FIXTURE_WORK" checkout main >/dev/null

git -C "$FIXTURE_WORK" tag v0.18.0-rc.1
assert_fixture_rejected "existing local tag" "local tag already exists" v0.18.0-rc.1
git -C "$FIXTURE_WORK" tag -d v0.18.0-rc.1 >/dev/null

git -C "$FIXTURE_WORK" tag v0.18.0-rc.1
git -C "$FIXTURE_WORK" push origin refs/tags/v0.18.0-rc.1 >/dev/null
git -C "$FIXTURE_WORK" tag -d v0.18.0-rc.1 >/dev/null
assert_fixture_rejected "existing remote tag" "remote tag already exists" v0.18.0-rc.1

destroy_fixture
new_fixture
git -C "$FIXTURE_WORK" branch --unset-upstream
assert_fixture_rejected "missing upstream" "branch has no upstream" v0.18.0-rc.1

destroy_fixture
new_fixture
git -C "$FIXTURE_WORK" checkout --detach >/dev/null
assert_fixture_rejected "detached HEAD" "detached HEAD is not releasable" v0.18.0-rc.1

destroy_fixture
new_fixture
rm "$FIXTURE_WORK/scripts/install.sh"
assert_fixture_rejected "wrong repository" "wrong repository: missing scripts/install.sh" v0.18.0-rc.1

destroy_fixture
new_fixture
printf 'ahead\n' > "$FIXTURE_WORK/ahead"
git -C "$FIXTURE_WORK" add ahead
git -C "$FIXTURE_WORK" commit -m "ahead" >/dev/null
assert_fixture_rejected "ahead of upstream" "branch differs from upstream: ahead=1 behind=0" v0.18.0-rc.1

destroy_fixture
new_fixture
REMOTE_WORK="$FIXTURE_ROOT/remote-work"
git clone "$FIXTURE_REMOTE" "$REMOTE_WORK" >/dev/null
git -C "$REMOTE_WORK" config user.name "Release Test"
git -C "$REMOTE_WORK" config user.email "release-test@example.invalid"
printf 'behind\n' > "$REMOTE_WORK/behind"
git -C "$REMOTE_WORK" add behind
git -C "$REMOTE_WORK" commit -m "behind" >/dev/null
git -C "$REMOTE_WORK" push origin main >/dev/null
assert_fixture_rejected "behind upstream" "branch differs from upstream: ahead=0 behind=1" v0.18.0-rc.1

destroy_fixture
new_fixture
printf 'ahead\n' > "$FIXTURE_WORK/ahead"
git -C "$FIXTURE_WORK" add ahead
git -C "$FIXTURE_WORK" commit -m "ahead" >/dev/null
REMOTE_WORK="$FIXTURE_ROOT/remote-work"
git clone "$FIXTURE_REMOTE" "$REMOTE_WORK" >/dev/null
git -C "$REMOTE_WORK" config user.name "Release Test"
git -C "$REMOTE_WORK" config user.email "release-test@example.invalid"
printf 'behind\n' > "$REMOTE_WORK/behind"
git -C "$REMOTE_WORK" add behind
git -C "$REMOTE_WORK" commit -m "behind" >/dev/null
git -C "$REMOTE_WORK" push origin main >/dev/null
assert_fixture_rejected "divergent upstream" "branch differs from upstream: ahead=1 behind=1" v0.18.0-rc.1

destroy_fixture
new_fixture
printf '# Changelog\n' > "$FIXTURE_WORK/CHANGELOG.md"
git -C "$FIXTURE_WORK" add CHANGELOG.md
git -C "$FIXTURE_WORK" commit -m "remove release heading" >/dev/null
git -C "$FIXTURE_WORK" push origin main >/dev/null
assert_fixture_rejected "missing changelog version" "changelog is missing 0.18.0" v0.18.0-rc.1

destroy_fixture
new_fixture
printf '# Changelog\n\n## [0.18.0]\n' > "$FIXTURE_WORK/CHANGELOG.md"
git -C "$FIXTURE_WORK" add CHANGELOG.md
git -C "$FIXTURE_WORK" commit -m "remove stable changelog date" >/dev/null
git -C "$FIXTURE_WORK" push origin main >/dev/null
assert_fixture_rejected "stable changelog date" "stable changelog entry needs a release date" v0.18.0

destroy_fixture
new_fixture
assert_fixture_metadata_rejected

destroy_fixture
new_fixture
assert_fixture_release_compose_rejected "missing release compose" "missing docker-compose.release.yml" 'rm "$FIXTURE_WORK/docker-compose.release.yml"'

destroy_fixture
new_fixture
assert_fixture_release_compose_rejected "release compose build" "must not contain build" 'printf "    build: .\n" >> "$FIXTURE_WORK/docker-compose.release.yml"'

destroy_fixture
new_fixture
assert_fixture_release_compose_rejected "release compose placeholder count" "must contain five image placeholders" "sed -i 's/__WUKONG_VERSION__/v0.18.0/g' \"\$FIXTURE_WORK/docker-compose.release.yml\""

destroy_fixture
new_fixture
assert_fixture_quality_gate

destroy_fixture
new_fixture
assert_fixture_rc_tag_pushed

destroy_fixture
new_fixture
assert_fixture_workflow_failure_keeps_tag

echo "release command parser checks passed"
