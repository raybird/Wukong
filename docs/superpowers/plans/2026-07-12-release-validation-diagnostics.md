# Release Validation Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the GitHub Actions release validation failure attributable to a named check, publish a successful RC, and promote only after recorded rehearsal evidence validates it.

**Architecture:** Keep the existing release pipeline and its safety gates. Add deterministic, non-sensitive diagnostics around the tag validation boundary, then use a newly numbered RC tag to verify the exact GitHub-hosted execution path. A successful RC remains the sole source for the rehearsal report and stable promotion.

**Tech Stack:** Bash, GitHub Actions, GitHub CLI, Cargo workspace, GHCR, release shell scripts.

---

### Task 1: Lock Validation Diagnostics Into the Workflow Contract

**Files:**
- Modify: `scripts/test-release-workflow.sh:7-19`
- Test: `scripts/test-release-workflow.sh`

- [ ] **Step 1: Write the failing contract assertions**

Add these strings to the `for contract in` list in `scripts/test-release-workflow.sh`:

```bash
'validate: tag=$tag' 'validate: annotation=' 'validate: channel=rc'
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `bash scripts/test-release-workflow.sh`

Expected: `missing workflow contract: validate: tag=$tag`

- [ ] **Step 3: Commit the failing-test state only if it is needed for review**

Do not commit a deliberately broken main branch. Continue directly to Task 2 after observing the expected failure.

### Task 2: Emit Named Validation Boundaries

**Files:**
- Modify: `.github/workflows/release.yml:31-57`
- Test: `scripts/test-release-workflow.sh`

- [ ] **Step 1: Replace anonymous assertions with named RC validation diagnostics**

Replace the validation body with checks that retain the existing rules while printing safe state and channel selection:

```bash
set -euo pipefail
tag="${GITHUB_REF_NAME}"
rc_re='^v[0-9]+\.[0-9]+\.[0-9]+-rc\.[1-9][0-9]*$'
stable_re='^v[0-9]+\.[0-9]+\.[0-9]+$'
printf 'validate: tag=%q\n' "$tag"
[[ "$tag" =~ $rc_re || "$tag" =~ $stable_re ]] || { printf 'validate: invalid tag\n' >&2; exit 1; }
tag_type="$(git cat-file -t "refs/tags/$tag")"
printf 'validate: tag_type=%s\n' "$tag_type"
[[ "$tag_type" == tag ]] || { printf 'validate: tag is not annotated\n' >&2; exit 1; }
annotation="$(git for-each-ref "refs/tags/$tag" --format='%(contents)')"
printf 'validate: annotation=%q\n' "$annotation"
promote_from=""
if [[ "$tag" =~ $rc_re ]]; then
  ! grep -Eq '^promote-from:' <<<"$annotation" || { printf 'validate: RC tag contains promotion metadata\n' >&2; exit 1; }
  channel=rc
  printf 'validate: channel=rc\n'
else
  # Retain the existing stable-only promotion and rehearsal checks below.
  channel=stable
  printf 'validate: channel=stable\n'
fi
```

Keep the existing stable-branch checks unchanged within the `else` branch after its `channel=stable` assignment.

- [ ] **Step 2: Run the contract test to verify it passes**

Run: `bash scripts/test-release-workflow.sh`

Expected: `release workflow checks passed`

- [ ] **Step 3: Run the release test suite**

Run:

```bash
bash scripts/test-release-workflow.sh
bash scripts/test-release-command.sh
bash scripts/test-release-manifest.sh
```

Expected: all commands exit zero.

- [ ] **Step 4: Commit the diagnostic change**

```bash
git add .github/workflows/release.yml scripts/test-release-workflow.sh
git commit -m "fix(release): identify failed tag validation checks"
git push origin main
```

### Task 3: Validate the Hosted RC Pipeline

**Files:**
- No source changes
- Generated remote reference: `v0.18.0-rc.5`

- [ ] **Step 1: Confirm the release worktree is clean and synced**

Run: `git status --short --branch`

Expected: `## main...origin/main` only.

- [ ] **Step 2: Publish a fresh immutable RC tag**

Run: `./scripts/release.sh v0.18.0-rc.5`

Expected: all local quality gates pass and tag `v0.18.0-rc.5` is pushed. The command may return before a run appears because its watcher currently lacks polling.

- [ ] **Step 3: Discover and watch the exact run**

Run:

```bash
run_id="$(gh run list --workflow Release --branch v0.18.0-rc.5 --event push --json databaseId,headBranch --jq '.[] | select(.headBranch == "v0.18.0-rc.5") | .databaseId' | tail -n 1)"
gh run watch "$run_id" --exit-status
```

Expected: successful completion. If validation fails, use `gh run view "$run_id" --log-failed`; the named `validate:` output identifies the exact violated boundary. Do not create another tag until that evidence is assessed.

- [ ] **Step 4: Verify published release assets**

Run: `gh release view v0.18.0-rc.5 --json isPrerelease,assets`

Expected: `isPrerelease` is true and the release includes target archives, `wukong-docker-v0.18.0-rc.5.tar.gz`, `release-manifest.json`, and `SHA256SUMS`.

### Task 4: Rehearse and Promote Stable

**Files:**
- Create: `docs/release-rehearsals/v0.18.0-rc.5.json`
- Use: `scripts/rehearse-rc.sh`
- Use: `scripts/release.sh`

- [ ] **Step 1: Run the controlled RC rehearsal**

Run `scripts/rehearse-rc.sh v0.18.0-rc.5` with its required controlled Telegram and Scheduler credentials set in the shell environment.

Expected: generated report names RC tag, immutable image digest, rollback result, and successful controlled integration checks.

- [ ] **Step 2: Validate and commit rehearsal evidence**

Run: `scripts/validate-rehearsal-report.sh docs/release-rehearsals/v0.18.0-rc.5.json v0.18.0-rc.5 "$(git rev-parse HEAD)" "$(gh release download v0.18.0-rc.5 --pattern release-manifest.json --dir /tmp/wukong-rc5-manifest --clobber >/dev/null && python3 -c 'import json; print(json.load(open("/tmp/wukong-rc5-manifest/release-manifest.json"))["image"]["digest"])')"`

Expected: exit zero.

Commit:

```bash
git add docs/release-rehearsals/v0.18.0-rc.5.json
git commit -m "docs(release): record v0.18.0 rc rehearsal"
git push origin main
```

- [ ] **Step 3: Promote the exact RC commit**

Run:

```bash
./scripts/release.sh v0.18.0 --promote-from v0.18.0-rc.5 --rehearsal-report docs/release-rehearsals/v0.18.0-rc.5.json
```

Expected: workflow promotes the existing immutable GHCR digest and publishes stable release assets without rebuilding the product image.

- [ ] **Step 4: Verify stable outputs and change impact before final reporting**

Run:

```bash
gh release view v0.18.0 --json isPrerelease,assets
git status --short --branch
```

Run `gitnexus_detect_changes` before committing any additional source changes. Expected: only intended release workflow/test symbols and processes are affected.
