# RC Prerelease Release Workflow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `v*-rc.*` tags create GitHub prereleases without becoming the latest stable release.

**Architecture:** Keep the existing tag-push release workflow and artifact packaging unchanged. Add release metadata controls directly to both `softprops/action-gh-release` upload steps so Linux and macOS jobs agree on prerelease/latest behavior.

**Tech Stack:** GitHub Actions YAML, `softprops/action-gh-release@v2`, shell-based static verification.

---

## File Structure

- Modify: `.github/workflows/release.yml` — add `prerelease` and `make_latest` inputs to both release upload steps.
- Create: `scripts/test-release-workflow.sh` — static test that verifies both upload steps contain the rc prerelease/latest expressions.
- Reference: `docs/superpowers/specs/2026-06-27-rc-prerelease-release-workflow-design.md` — approved behavior and scope.

## Tasks

### Task 1: Add Static Release Workflow Test

**Files:**
- Create: `scripts/test-release-workflow.sh`

- [ ] **Step 1: Write the failing test**

Create `scripts/test-release-workflow.sh` with this content:

```bash
#!/usr/bin/env bash
set -euo pipefail

workflow=".github/workflows/release.yml"

if [[ ! -f "$workflow" ]]; then
  echo "missing workflow: $workflow" >&2
  exit 1
fi

prerelease_count=$(grep -F -c "prerelease: \${{ contains(github.ref_name, '-rc.') }}" "$workflow" || true)
make_latest_count=$(grep -F -c "make_latest: \${{ contains(github.ref_name, '-rc.') && 'false' || 'true' }}" "$workflow" || true)

if [[ "$prerelease_count" != "2" ]]; then
  echo "expected prerelease expression in both release upload steps, found $prerelease_count" >&2
  exit 1
fi

if [[ "$make_latest_count" != "2" ]]; then
  echo "expected make_latest expression in both release upload steps, found $make_latest_count" >&2
  exit 1
fi
```

- [ ] **Step 2: Make the test executable**

Run:

```bash
chmod +x scripts/test-release-workflow.sh
```

Expected: command exits successfully.

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
scripts/test-release-workflow.sh
```

Expected: FAIL with:

```text
expected prerelease expression in both release upload steps, found 0
```

### Task 2: Add RC Prerelease Metadata To Release Uploads

**Files:**
- Modify: `.github/workflows/release.yml`
- Test: `scripts/test-release-workflow.sh`

- [ ] **Step 1: Update the Linux release upload step**

Change the first `Upload to release` step to include these inputs:

```yaml
      - name: Upload to release
        uses: softprops/action-gh-release@v2
        with:
          files: dist/*
          fail_on_unmatched_files: true
          prerelease: ${{ contains(github.ref_name, '-rc.') }}
          make_latest: ${{ contains(github.ref_name, '-rc.') && 'false' || 'true' }}
```

- [ ] **Step 2: Update the macOS release upload step**

Change the second `Upload to release` step to include these inputs:

```yaml
      - name: Upload to release
        uses: softprops/action-gh-release@v2
        with:
          files: dist/*
          fail_on_unmatched_files: true
          prerelease: ${{ contains(github.ref_name, '-rc.') }}
          make_latest: ${{ contains(github.ref_name, '-rc.') && 'false' || 'true' }}
```

- [ ] **Step 3: Run test to verify it passes**

Run:

```bash
scripts/test-release-workflow.sh
```

Expected: PASS with no output.

### Task 3: Verify Release Trigger Context And Commit

**Files:**
- Modify: `.github/workflows/release.yml`
- Create: `scripts/test-release-workflow.sh`
- Create: `docs/superpowers/specs/2026-06-27-rc-prerelease-release-workflow-design.md`
- Create: `docs/superpowers/plans/2026-06-27-rc-prerelease-release-workflow.md`

- [ ] **Step 1: Confirm the workflow still triggers only on version tags**

Run:

```bash
grep -F 'tags: ["v*"]' .github/workflows/release.yml
```

Expected output:

```text
    tags: ["v*"]
```

- [ ] **Step 2: Confirm git diff is scoped to release workflow automation**

Run:

```bash
git diff -- .github/workflows/release.yml scripts/test-release-workflow.sh docs/superpowers/specs/2026-06-27-rc-prerelease-release-workflow-design.md docs/superpowers/plans/2026-06-27-rc-prerelease-release-workflow.md
```

Expected: diff only includes the rc prerelease workflow spec, plan, static test, and release upload metadata.

- [ ] **Step 3: Commit the spec, plan, test, and workflow change**

Run:

```bash
git add .github/workflows/release.yml scripts/test-release-workflow.sh docs/superpowers/specs/2026-06-27-rc-prerelease-release-workflow-design.md docs/superpowers/plans/2026-06-27-rc-prerelease-release-workflow.md
git commit -m "ci: mark rc tags as prereleases"
```

Expected: commit succeeds with no AI attribution in the commit message.

### Task 4: Trigger And Inspect RC Release

**Files:**
- No code changes.

- [ ] **Step 1: Push the branch**

Run:

```bash
git push origin feature/docker-runtime-skill-assets
```

Expected: branch push succeeds.

- [ ] **Step 2: Create and push the rc tag**

Run:

```bash
git tag -a v0.16.15-rc.1 -m "v0.16.15-rc.1 - runtime skill assets"
git push origin v0.16.15-rc.1
```

Expected: tag push succeeds and starts the `Release` workflow.

- [ ] **Step 3: Watch the release workflow**

Run:

```bash
gh run list --workflow Release --limit 5
```

Pick the run for `v0.16.15-rc.1`, then run:

```bash
gh run watch <run-id>
```

Expected: all release jobs pass.

- [ ] **Step 4: Confirm release metadata**

Run:

```bash
gh release view v0.16.15-rc.1 --json tagName,isPrerelease,isLatest,url
```

Expected JSON contains:

```json
{
  "tagName": "v0.16.15-rc.1",
  "isPrerelease": true,
  "isLatest": false
}
```

- [ ] **Step 5: Confirm installer dry-run points at rc Docker asset**

Run:

```bash
bash scripts/install.sh --mode docker --version v0.16.15-rc.1 --dry-run
```

Expected output includes:

```text
https://github.com/raybird/Wukong/releases/download/v0.16.15-rc.1/wukong-docker-v0.16.15-rc.1.tar.gz
```

## Self-Review

- Spec coverage: Task 2 implements rc prerelease and latest behavior; Task 4 validates actual GitHub release metadata; build/package flow is untouched.
- Placeholder scan: No TBD/TODO/fill-in placeholders remain. The only variable is `<run-id>`, which is obtained in the preceding step.
- Type consistency: The same `contains(github.ref_name, '-rc.')` expression is used in the spec, test, and workflow steps.
