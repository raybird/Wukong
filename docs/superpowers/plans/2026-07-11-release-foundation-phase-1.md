# Release Foundation Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one executable release gate, independently validate annotated RC/stable tags in CI, and establish deterministic manifest/checksum generation without changing Docker installation.

**Architecture:** `scripts/release.sh` performs every local preflight before creating an annotated tag, pushes only that tag, watches the matching workflow run, and verifies release assets. The workflow repeats tag validation so manual tag pushes cannot bypass the contract. Manifest/checksum helpers are tested now and consumed by Phase 2 once an image digest exists.

**Tech Stack:** Bash, Git, GitHub CLI, GitHub Actions, Cargo

---

## File Structure

- Create `scripts/release.sh`: authoritative maintainer release command.
- Create `scripts/test-release-command.sh`: temporary-repository and fake-`gh` black-box tests.
- Create `scripts/generate-release-manifest.sh`: deterministic schema-v1 JSON generator requiring a real image digest.
- Create `scripts/generate-sha256sums.sh`: deterministic aggregate checksum generator.
- Create `scripts/test-release-manifest.sh`: generator and checksum contract tests.
- Modify `.github/workflows/release.yml`: add validation and preserve current build/publish behavior.
- Modify `scripts/test-release-workflow.sh`: static workflow contract tests.
- Modify `.claude/skills/wukong-release/SKILL.md`: use the single command.
- Modify `CHANGELOG.md`: describe Phase 1 behavior and prepare the target version heading.

### Task 1: Tag Parser And CLI Contract

**Files:**
- Create: `scripts/release.sh`
- Create: `scripts/test-release-command.sh`

- [ ] **Step 1: Write failing parser tests**

Create a table-driven test that invokes `release.sh` with `--dry-run` in a fixture and asserts these argument-only failures occur before Git access:

```bash
assert_rejected ""
assert_rejected "0.18.0"
assert_rejected "v0.18"
assert_rejected "v0.18.0-rc.0"
assert_rejected "v0.18.0-beta.1"
assert_rejected "v0.18.0 --promote-from v0.18.0-rc.1 --promote-from v0.18.0-rc.2"
assert_rejected "v0.18.0-rc.1 --promote-from v0.18.0-rc.1"
assert_rejected "v0.18.0"
```

Also assert stable succeeds only with a matching source base:

```bash
assert_rejected "v0.18.0 --promote-from v0.19.0-rc.1"
```

- [ ] **Step 2: Run the test and verify failure**

Run: `bash scripts/test-release-command.sh`

Expected: FAIL because `scripts/release.sh` does not exist.

- [ ] **Step 3: Implement strict parsing**

Start `scripts/release.sh` with:

```bash
#!/usr/bin/env bash
set -euo pipefail

TAG_RE='^v[0-9]+\.[0-9]+\.[0-9]+(-rc\.[1-9][0-9]*)?$'
RC_RE='^v[0-9]+\.[0-9]+\.[0-9]+-rc\.[1-9][0-9]*$'
TAG=""
PROMOTE_FROM=""
DRY_RUN=false

die() { printf 'release: %s\n' "$*" >&2; exit 1; }

while (($#)); do
  case "$1" in
    --promote-from) [[ $# -ge 2 && -z "$PROMOTE_FROM" ]] || die "invalid --promote-from"; PROMOTE_FROM="$2"; shift 2 ;;
    --dry-run) $DRY_RUN && die "duplicate --dry-run"; DRY_RUN=true; shift ;;
    -*) die "unknown option: $1" ;;
    *) [[ -z "$TAG" ]] || die "only one product tag is allowed"; TAG="$1"; shift ;;
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
```

- [ ] **Step 4: Run syntax and parser tests**

Run:

```bash
bash -n scripts/release.sh
bash -n scripts/test-release-command.sh
bash scripts/test-release-command.sh
```

Expected: parser cases PASS; repository-dependent cases are not added yet.

- [ ] **Step 5: Commit**

```bash
git add scripts/release.sh scripts/test-release-command.sh
git commit -m "feat(release): validate release command arguments"
```

### Task 2: Repository Preflight

**Files:**
- Modify: `scripts/release.sh`
- Modify: `scripts/test-release-command.sh`

- [ ] **Step 1: Add temporary Git fixture tests**

Build each fixture with a bare remote and clean `main` branch:

```bash
git init --bare "$tmp/origin.git"
git init -b main "$tmp/work"
git -C "$tmp/work" config user.name "Release Test"
git -C "$tmp/work" config user.email "release-test@example.invalid"
git -C "$tmp/work" remote add origin "$tmp/origin.git"
git -C "$tmp/work" push -u origin main
```

Copy `Cargo.toml`, `Cargo.lock`, `CHANGELOG.md`, `.github/workflows/release.yml`, and required scripts into the fixture. Add cases for wrong repository anchors, detached HEAD, non-`main`/`release/*` branch, tracked/staged/untracked changes, missing upstream, ahead, behind, divergence, existing local tag, and remote-only tag.

- [ ] **Step 2: Verify tests fail before implementation**

Run: `bash scripts/test-release-command.sh`

Expected: FAIL on the first dirty/upstream scenario because preflight is absent.

- [ ] **Step 3: Implement repository checks in this order**

Add functions and call them before expensive checks:

```bash
repo_root() { git rev-parse --show-toplevel 2>/dev/null || die "not in a Git repository"; }
require_anchor() { [[ -e "$ROOT/$1" ]] || die "wrong repository: missing $1"; }
require_clean() { [[ -z "$(git status --porcelain=v1 --untracked-files=all)" ]] || die "worktree is not clean"; }
require_branch() {
  BRANCH="$(git symbolic-ref --quiet --short HEAD)" || die "detached HEAD is not releasable"
  [[ "$BRANCH" == main || "$BRANCH" == release/* ]] || die "release branch must be main or release/*"
}
require_synced_upstream() {
  UPSTREAM="$(git rev-parse --abbrev-ref --symbolic-full-name '@{upstream}')" || die "branch has no upstream"
  git fetch --prune origin
  git fetch --tags --prune origin
  read -r AHEAD BEHIND <<EOF
$(git rev-list --left-right --count HEAD..."$UPSTREAM")
EOF
  [[ "$AHEAD" == 0 && "$BEHIND" == 0 ]] || die "branch differs from upstream: ahead=$AHEAD behind=$BEHIND"
}
require_tag_absent() {
  ! git show-ref --verify --quiet "refs/tags/$TAG" || die "local tag already exists: $TAG"
  [[ -z "$(git ls-remote --tags origin "refs/tags/$TAG" "refs/tags/$TAG^{}")" ]] || die "remote tag already exists: $TAG"
}
```

Use repository anchors `Cargo.toml`, `Cargo.lock`, `.github/workflows/release.yml`, and `scripts/install.sh`.

- [ ] **Step 4: Run tests**

Run: `bash scripts/test-release-command.sh`

Expected: all repository preflight cases PASS.

- [ ] **Step 5: Commit**

```bash
git add scripts/release.sh scripts/test-release-command.sh
git commit -m "feat(release): enforce repository preflight"
```

### Task 3: Changelog, Promotion, Lockfile, And Environment Contracts

**Files:**
- Modify: `scripts/release.sh`
- Modify: `scripts/test-release-command.sh`

- [ ] **Step 1: Add failing semantic tests**

Cover missing base version heading, stable heading without ISO date, missing/lightweight/different-commit source RC, Cargo metadata failure, and changed env-facing files without all currently existing required companion files changed.

The env-sync fixture compares the candidate against its upstream and requires this set whenever any member changes:

```text
.env.example
docker-compose.yml
CHANGELOG.md
```

- [ ] **Step 2: Verify failures**

Run: `bash scripts/test-release-command.sh`

Expected: FAIL at the first missing changelog check.

- [ ] **Step 3: Implement semantic checks**

Use exact checks:

```bash
VERSION="${TAG#v}"
BASE_VERSION="${VERSION%%-rc.*}"
if [[ "$CHANNEL" == stable ]]; then
  grep -Eq "^## \[${BASE_VERSION//./\\.}\] - [0-9]{4}-[0-9]{2}-[0-9]{2}$" CHANGELOG.md || die "stable changelog entry needs a release date"
  [[ "$(git cat-file -t "$PROMOTE_FROM" 2>/dev/null)" == tag ]] || die "source RC must be annotated"
  [[ "$(git rev-parse "$PROMOTE_FROM^{commit}")" == "$(git rev-parse HEAD)" ]] || die "source RC must point to HEAD"
else
  grep -Eq "^## \[${BASE_VERSION//./\\.}\]( - [0-9]{4}-[0-9]{2}-[0-9]{2})?$" CHANGELOG.md || die "changelog is missing $BASE_VERSION"
fi
test -f Cargo.lock || die "Cargo.lock is required"
cargo metadata --locked --format-version 1 >/dev/null
```

Run env synchronization only when one of these three files differs from upstream; require all three paths in `git diff --name-only "$UPSTREAM"...HEAD`. Phase 2 adds release Compose and `docs/docker.md` to this set after the release deployment contract exists.

- [ ] **Step 4: Run tests**

Run: `bash scripts/test-release-command.sh`

Expected: all semantic checks PASS.

- [ ] **Step 5: Commit**

```bash
git add scripts/release.sh scripts/test-release-command.sh
git commit -m "feat(release): validate candidate metadata"
```

### Task 4: Ordered Quality Gate And Dry Run

**Files:**
- Modify: `scripts/release.sh`
- Modify: `scripts/test-release-command.sh`

- [ ] **Step 1: Add fake-command and no-tag tests**

Place fake `cargo` and `gh` executables first in `PATH`. Add `WUKONG_RELEASE_TEST_COMMANDS_FILE`, honored only when `WUKONG_RELEASE_TESTING=1`, to avoid recursive invocation of `test-release-command.sh`. For each command failure, assert no local or remote tag exists.

The production command list is exactly:

```text
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash scripts/test-release-workflow.sh
bash scripts/test-release-command.sh
bash scripts/test-installer-upgrade.sh
bash scripts/test-docker-runtime.sh
```

- [ ] **Step 2: Verify tests fail**

Run: `bash scripts/test-release-command.sh`

Expected: FAIL because checks and dry-run plan are absent.

- [ ] **Step 3: Implement the gate**

Add:

```bash
run_check() { printf 'release: running %s\n' "$*"; "$@" || die "failed: $*"; }
require_gh() { command -v gh >/dev/null || die "gh is required"; gh auth status >/dev/null || die "gh authentication failed"; }
```

Run all checks, re-run `require_clean`, then print channel, commit, annotation body, three targets, four components, and expected Phase 1 assets. When `DRY_RUN=true`, exit before `git tag`.

- [ ] **Step 4: Run tests**

Run: `bash scripts/test-release-command.sh`

Expected: each injected failure leaves no tag; dry-run invokes every check and leaves no tag.

- [ ] **Step 5: Commit**

```bash
git add scripts/release.sh scripts/test-release-command.sh
git commit -m "feat(release): run preflight quality gate"
```

### Task 5: Annotated Tag, Workflow Watch, And Asset Verification

**Files:**
- Modify: `scripts/release.sh`
- Modify: `scripts/test-release-command.sh`

- [ ] **Step 1: Add fake-`gh` success and failure tests**

Fake these commands: `gh auth status`, `gh run list --workflow Release --event push --json ...`, `gh run watch ID --exit-status`, and `gh release view TAG --json tagName,isPrerelease,url,assets`. Test delayed run discovery, wrong-tag run, workflow failure, missing asset, RC marked stable, stable marked prerelease, and full success.

- [ ] **Step 2: Verify tests fail**

Run: `bash scripts/test-release-command.sh`

Expected: FAIL because the command never tags or watches.

- [ ] **Step 3: Implement release execution**

Construct exact annotations:

```bash
if [[ "$CHANNEL" == rc ]]; then
  ANNOTATION="$TAG"
else
  ANNOTATION="${TAG}"$'\n'"promote-from: ${PROMOTE_FROM}"
fi
git tag -a "$TAG" -m "$ANNOTATION"
git push origin "refs/tags/$TAG"
```

Poll `gh run list` for a row whose `headBranch` equals `$TAG`, select the largest `databaseId`, watch it, then verify release channel and the current Phase 1 assets: twelve binary archives, three per-target checksum files, and the existing Docker bundle. On post-push failure, retain the public tag and print that source fixes require a new RC/patch.

- [ ] **Step 4: Run tests**

Run: `bash scripts/test-release-command.sh`

Expected: RC and stable tags are annotated, stable annotation contains one `promote-from`, and post-push failures do not delete tags.

- [ ] **Step 5: Commit**

```bash
git add scripts/release.sh scripts/test-release-command.sh
git commit -m "feat(release): publish and verify annotated tags"
```

### Task 6: Deterministic Manifest And Checksum Generators

**Files:**
- Create: `scripts/generate-release-manifest.sh`
- Create: `scripts/generate-sha256sums.sh`
- Create: `scripts/test-release-manifest.sh`

- [ ] **Step 1: Write failing generator tests**

Test required arguments, malformed tag/digest, RC with `promotedFrom`, stable without it, deterministic output, sorted targets, runtime input fields, checksum lexical order, exclusion of `SHA256SUMS`, and full file coverage.

- [ ] **Step 2: Verify tests fail**

Run: `bash scripts/test-release-manifest.sh`

Expected: FAIL because generator scripts do not exist.

- [ ] **Step 3: Implement manifest generation**

Accept explicit flags `--tag`, `--commit`, `--channel`, `--promoted-from`, `--image-reference`, `--image-digest`, `--platform`, `--runtime-inputs`, and `--output`. Use `python3 -c` to validate and write canonical JSON with `sort_keys=True`, separators `(',', ':')`, and a trailing newline. Emit:

```json
{"binaryTargets":["aarch64-apple-darwin","x86_64-unknown-linux-gnu","x86_64-unknown-linux-musl"],"channel":"rc","commit":"FULL_SHA","image":{"buildOriginTag":"v0.18.0-rc.1","digest":"sha256:HEX","platform":"linux/amd64","reference":"ghcr.io/raybird/wukong:v0.18.0-rc.1"},"productTag":"v0.18.0-rc.1","promotedFrom":null,"runtimeInputs":{},"schemaVersion":1}
```

Write to a same-directory temporary file and rename it.

- [ ] **Step 4: Implement checksum generation**

In `generate-sha256sums.sh`, enumerate regular files in the destination except `SHA256SUMS`, sort names with `LC_ALL=C`, hash with `sha256sum` or `shasum -a 256`, write release-root names, and atomically rename the output.

- [ ] **Step 5: Run tests**

Run:

```bash
bash -n scripts/generate-release-manifest.sh
bash -n scripts/generate-sha256sums.sh
bash scripts/test-release-manifest.sh
```

Expected: all generator and coverage cases PASS.

- [ ] **Step 6: Commit**

```bash
git add scripts/generate-release-manifest.sh scripts/generate-sha256sums.sh scripts/test-release-manifest.sh
git commit -m "feat(release): add manifest and checksum generators"
```

### Task 7: Independent Workflow Validation

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/test-release-workflow.sh`

- [ ] **Step 1: Expand static tests first**

Require `validate`, full-history checkout, exact tag regex, `git for-each-ref`, exactly one stable promotion metadata line, source RC commit equality, `cargo metadata --locked`, `needs: validate`, three targets, four binaries, one `softprops/action-gh-release@v2`, and unchanged RC prerelease/latest expressions.

- [ ] **Step 2: Verify static tests fail**

Run: `bash scripts/test-release-workflow.sh`

Expected: FAIL because `validate` is absent.

- [ ] **Step 3: Add the workflow validate job**

Checkout with `fetch-depth: 0`, validate `github.ref_name`, ensure `git cat-file -t` returns `tag`, read annotation using the approved `git for-each-ref` command, reject `promote-from` on RC, and for stable require one matching source RC on the same commit. Run locked Cargo metadata. Rename `build` to `build-binaries` and set `needs: validate`; leave Docker packaging and public assets otherwise unchanged in Phase 1. Phase 2 adds release Compose validation after that file exists.

- [ ] **Step 4: Run static and compatibility tests**

Run:

```bash
bash scripts/test-release-workflow.sh
bash scripts/test-installer-upgrade.sh
bash scripts/test-docker-runtime.sh
```

Expected: all pass; installer still expects the existing Docker build bundle.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml scripts/test-release-workflow.sh
git commit -m "feat(release): validate tags in workflow"
```

### Task 8: Maintainer Documentation And Phase Verification

**Files:**
- Modify: `.claude/skills/wukong-release/SKILL.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Update the release SOP**

Replace manual tag/push/watch instructions with:

```bash
./scripts/release.sh v0.18.0-rc.1 --dry-run
./scripts/release.sh v0.18.0-rc.1
./scripts/release.sh v0.18.0 --promote-from v0.18.0-rc.2
```

Keep release-note editing as a post-release editorial step and document that public tags are never reused.

- [ ] **Step 2: Add changelog entry**

Under `Unreleased`, document the single release gate, workflow annotation validation, and generator foundation. Add the `0.18.0` base heading used by RC preflight without assigning a stable date until stable promotion.

- [ ] **Step 3: Run complete Phase 1 verification**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash scripts/test-release-workflow.sh
bash scripts/test-release-command.sh
bash scripts/test-release-manifest.sh
bash scripts/test-installer-upgrade.sh
bash scripts/test-docker-runtime.sh
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 4: Run change-impact review**

Run `gitnexus_detect_changes({scope: "all", repo: "Wukong"})` and confirm only release flow processes are affected.

- [ ] **Step 5: Commit**

```bash
git add .claude/skills/wukong-release/SKILL.md CHANGELOG.md
git commit -m "docs: document single release command"
```
