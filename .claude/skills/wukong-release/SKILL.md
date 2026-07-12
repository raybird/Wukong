---
name: wukong-release
description: Use when preparing, promoting, tagging, or publishing Wukong releases, including RC-to-stable promotion, GitHub release notes, gh release operations, release workflow checks, and installer/version verification.
---

# Wukong Release

## Overview

Release Wukong with `scripts/release.sh`: it validates the candidate, creates the annotated tag, watches the GitHub Actions workflow, and verifies GitHub Release assets before optional release-note editing.

Core principle: verify first, tag the exact commit, preserve release-note style, and never mix unrelated dirty work into the release.

## Preconditions

- Work from repository root.
- Do not change or revert unrelated dirty files.
- Do not publish if tests or release workflow checks are failing.
- Do not add `Co-Authored-By`, AI attribution, or generated-by lines to commits, tags, or release notes.
- If making any commit before release, run `gitnexus_detect_changes({scope: "all", repo: "Wukong"})` before committing.

## Inspect State

Run:

```bash
git fetch --tags --prune
git status --short --branch
git log --oneline --decorate --max-count=12
git tag --list 'v*' --sort=-version:refname | head -20
gh release list --limit 10
```

Confirm:

- `main` points at the intended release commit.
- The latest RC tag points at the commit to promote.
- Dirty files are understood and unrelated to release tagging.
- If `Dockerfile` is dirty, inspect whether it is only a local default `ARG VERSION` bump. Do not include that bump in a hotfix release unless changing the source Dockerfile default is the release's explicit purpose; release Docker bundles are generated from the tag by CI.
- Existing stable version and next stable version are clear.

## Verify Candidate

Use the checks relevant to the release. For normal runtime/Docker releases, run:

```bash
scripts/test-docker-runtime.sh
scripts/test-release-workflow.sh
scripts/test-release-manifest.sh
scripts/test-release-image.sh
cargo test -p wukong-skills -p wukong-runtime
cargo test
```

If the release touched web assets, also run the relevant `node --check` commands and `cargo test -p wukong-web`.

Stop on failure. Report exact failing command and output summary.

### Breaking env-var sync check

If this release **adds or changes any environment variable that affects startup or default behavior** (e.g. a new fail-closed guard, a changed default bind/port, a required secret), confirm all user-facing touchpoints are in sync before promoting:

- `.env.example` — the variable is documented with its default and failure mode.
- `docker-compose.yml` — the service `environment`/`ports` reflect the new default.
- `docs/docker.md` — the env-var table and quick start match the new behavior.
- `CHANGELOG.md` **and** the GitHub Release notes — include a `⚠️ 升級注意（Breaking）` section with a copy-pasteable fix for affected users.

Case study of skipping this: v0.17.0 added the `WUKONG_WEB_ALLOW_INSECURE` fail-closed guard without syncing the compose default or release notes, so upgraded Docker deployments crash-looped with no visible cause. See `docs/superpowers/plans/2026-07-08-web-allow-insecure-upgrade-fix.md`.

## Promote RC To Stable

Use the release command after the candidate commit is clean and synchronized:

```bash
./scripts/release.sh vX.Y.Z-rc.N --dry-run
./scripts/release.sh vX.Y.Z-rc.N
./scripts/release.sh vX.Y.Z --promote-from vX.Y.Z-rc.N
```

The stable command requires the source RC and stable tag to resolve to the same commit. It creates `promote-from:` annotation metadata. CI verifies the RC manifest/checksum and promotes the existing GHCR digest to the stable tag; it never rebuilds the stable image. Never manually create, move, or reuse public release tags.

## Watch Release Workflow

`scripts/release.sh` waits for the tag-filtered workflow run and verifies the prerelease channel plus all expected assets, including `release-manifest.json` and `SHA256SUMS`. RC publication builds a pinned image from musl artifacts and protects product/commit tags from digest conflicts. The bundle contains only the pull-only Compose deployment inputs; Phase 3 has not yet made the installer pull-only.

## Write Release Notes

Use `gh` to reference previous stable release style:

```bash
gh release view <previous-stable-tag> --json tagName,name,isPrerelease,body,url
gh release view <latest-rc-tag> --json tagName,name,isPrerelease,body,url
git log --oneline <previous-stable-tag>..vX.Y.Z
```

Follow Wukong style:

- Title format: `🐵 vX.Y.Z — 短題：主題 × 主題`
- Body headings: `## 新增`, `## 修正`, `## 文件`, `## 驗證`
- Keep bullets concise and user-facing.
- Mention concrete commands under `## 驗證`.
- Do not include AI attribution.
- Prefer Traditional Chinese, Taiwan wording.

Write notes to a temporary file outside source control, for example `/tmp/opencode/wukong-vX.Y.Z-release.md`, then run:

```bash
gh release edit vX.Y.Z --title "🐵 vX.Y.Z — 短題：主題 × 主題" --notes-file /tmp/opencode/wukong-vX.Y.Z-release.md
```

## Post-Release Checks

Run:

```bash
gh release view vX.Y.Z --json tagName,name,isPrerelease,body,url
bash scripts/install.sh --mode docker --version vX.Y.Z --dry-run
bash scripts/install.sh --mode binary --version vX.Y.Z --dry-run
git status --short --branch
```

Confirm:

- The release is not a prerelease.
- The title and body match prior stable releases.
- Installer dry-runs point at `vX.Y.Z` assets.
- Worktree only contains expected pre-existing dirty files.

## Common Mistakes

- Tagging before pushing `main`: push `main` first so repository history and release tag are easy to inspect.
- Editing release notes from memory: always compare with at least one previous stable release via `gh release view`.
- Publishing from the wrong commit: verify `git log --decorate` shows the intended RC or stable commit.
- Mixing release doc updates with unrelated dirty files: commit only intended files, or avoid committing entirely before tagging.
- Committing a stale source `Dockerfile` `ARG VERSION` bump: CI writes the tagged version into the release Docker bundle, so a local source Dockerfile bump can accidentally ship the wrong default in a hotfix.
- Trusting RC release bodies: RC releases may have empty bodies; use stable release style as the source of truth.
