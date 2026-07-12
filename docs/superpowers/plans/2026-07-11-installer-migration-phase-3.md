# Installer Migration Phase 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate Docker installation to verified pull-only releases and make Binary installation/upgrades idempotent, component-preserving, Scheduler-aware, and ownership-safe.

**Architecture:** Both modes download `SHA256SUMS` and `release-manifest.json`, validate archive allowlists, stage every release-owned file, and commit metadata only after successful activation. Docker replaces a five-path allowlist and verifies RepoDigest; Binary replaces only selected binaries/managed units and preserves configuration and workspace byte-for-byte.

**Tech Stack:** Bash, Python 3 JSON validation, Docker Compose, systemd user services, SHA256

---

## File Structure

- Modify `scripts/install.sh`: mode/action parsing, verification, transactions, metadata, Scheduler.
- Rewrite `scripts/test-installer-upgrade.sh`: temporary HOME, fake release server, Docker, and systemd integration harness.
- Modify `scripts/test-docker-runtime.sh`: bundle/ownership assertions.
- Modify `.github/workflows/release.yml` and `scripts/test-release-workflow.sh`: remove compatibility checksum assets after migration.
- Modify `README.md`, `docs/installation.md`, `docs/docker.md`, and `CHANGELOG.md`: installer behavior.

### Task 1: Integration Test Harness

**Files:**
- Rewrite: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Build fixture helpers**

Create temporary `HOME`, deployment, fake `PATH`, release asset directory, command log, and helpers for binary archives, Docker bundle, manifest, and checksums. Fake `curl` maps GitHub release URLs to fixture files. Fake Docker supports `compose version`, `compose pull`, `compose up`, `compose ps`, and `image inspect`. Fake systemd tracks enabled and active state.

- [ ] **Step 2: Preserve one legacy assertion as a failing migration test**

Assert Docker upgrade logs `compose pull` and `compose up -d --force-recreate`, and contains zero `build`, `down`, or `down -v` commands.

- [ ] **Step 3: Run and verify failure**

Run: `bash scripts/test-installer-upgrade.sh`

Expected: FAIL because the current installer builds locally.

- [ ] **Step 4: Commit harness**

```bash
git add scripts/test-installer-upgrade.sh
git commit -m "test(installer): add isolated upgrade harness"
```

### Task 2: Mode And Action Parsing

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add parsing tests**

Cover bare `--upgrade` as Docker, explicit Binary upgrade, `--with-schedulerd`, mutually exclusive upgrade/rollback, Docker rejection of Scheduler flag, and missing values before downloads.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh parsing`

Expected: Binary upgrade is rejected.

- [ ] **Step 3: Refactor parser**

Introduce `ACTION=install|upgrade`, `EXPLICIT_MODE`, `WITH_SCHEDULERD`, and pure `parse_args`, `resolve_mode_and_action`, `validate_args` functions. Preserve bare `--upgrade` compatibility. Keep rollback parsing reserved but reject it with a Phase 4 message.

- [ ] **Step 4: Run tests and commit**

```bash
bash -n scripts/install.sh
bash scripts/test-installer-upgrade.sh parsing
```

### Task 3: Release Metadata And Archive Verification

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add failure-preservation tests**

Test wrong checksum, missing checksum entry, manifest tag mismatch, malformed JSON, absolute path, `..`, symlink/hardlink, duplicate entry, unexpected binary entry, and unexpected Docker bundle entry. Snapshot live files and assert byte equality after each failure.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh verification`

Expected: current direct extraction violates the tests.

- [ ] **Step 3: Implement shared verification primitives**

Add a cleanup trap and functions `make_temp_dir`, `download_release_file`, `sha256_file`, `verify_sha256sums_entry`, `read_manifest_field`, `validate_manifest_version`, `safe_list_archive`, `validate_archive_entries`, and `extract_archive_to`. Require `python3` and use it for strict JSON key/type validation. Reject all archive links and entries outside exact mode-specific allowlists before extraction.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh verification
```

### Task 4: Pull-Only Docker Installation

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add Docker clean/rerun/upgrade tests**

Cover clean install, existing `.env`, same-version rerun, upgrade, pull failure, digest mismatch, arbitrary user file, Compose override, workspace, and no-volume-removal assertions.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh docker`

Expected: current bundle extraction/build path fails.

- [ ] **Step 3: Implement Docker ownership and transaction**

Define the release-owned allowlist:

```bash
DOCKER_RELEASE_OWNED=(docker-compose.yml .env.example LICENSE scripts/install.sh)
```

Download and verify bundle/manifest/checksums, stage it, assert exact tag and no `build:`, pull with a stable Compose project name, inspect `ghcr.io/raybird/wukong@sha256:...`, compare the normalized digest, replace only allowlisted files, initialize `.env` only when absent, run `compose up -d --force-recreate`, verify health, then atomically write `.wukong-release`.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh docker
bash scripts/test-docker-runtime.sh
```

### Task 5: Binary Metadata Schema

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add metadata tests**

Validate schema version, mode, target, product tags, deterministic component/service arrays, immutable `installedAt`, changed `updatedAt`, `0600` atomic writes, no secrets, and refusal of unknown schema.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh metadata`

Expected: metadata file is absent.

- [ ] **Step 3: Implement metadata**

Use `~/.wukong/install.json` with schema version 1 and fields from the approved design plus `previousBackupPath`. Parse/write through Python, write a same-directory temporary file, chmod `0600`, and rename only after successful activation. Discover legacy components by exact binary paths and actual enabled services through `systemctl --user is-enabled`.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh metadata
```

### Task 6: Non-Destructive Binary Clean Install

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add ownership tests**

Hash existing `config.env`, workspace templates, custom files, database, and Markdown mirror. Test missing-template initialization, existing config prompt suppression, Web loopback default, and independent Scheduler selection.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh binary-clean`

Expected: current installer truncates `config.env`.

- [ ] **Step 3: Split clean configuration from upgrades**

Create `initialize_config_if_missing`, `select_components_interactively`, and `initialize_workspace_templates_if_missing`. Existing config suppresses all settings prompts and remains byte-identical. Add a separate Scheduler yes/no prompt after Telegram/Web selection; `--with-schedulerd` selects it noninteractively.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh binary-clean
```

### Task 7: Component-Preserving Binary Upgrade

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add component matrix tests**

Test CLI-only, CLI+Web, CLI+Telegram+Web, existing Scheduler, Scheduler opt-in, same-version rerun, no prompts, no unselected downloads, and last-component checksum failure leaving every live binary unchanged.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh binary-upgrade`

Expected: current installer prompts and writes directly.

- [ ] **Step 3: Implement staged component transaction**

Always select `wukong`, select optional components only from valid metadata or existing binaries, append Scheduler only for explicit opt-in, download and verify every selected archive, extract all into a same-filesystem staging directory, verify executability, back up all affected binaries, rename replacements, and restore all backups if any activation step fails. Do not mutate metadata until completion.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh binary-upgrade
```

### Task 8: Scheduler And Managed systemd Units

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add service-state tests**

Cover Scheduler unit content, Linux enable/start on explicit install, macOS binary-only behavior, enabled Web restart, disabled Web preservation, existing Scheduler restart, customized unmanaged unit refusal, and metadata service state.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh systemd`

Expected: Scheduler unit is absent.

- [ ] **Step 3: Centralize managed units**

Render Telegram, Web, and Scheduler units with a `Managed by Wukong install.sh` marker. Scheduler executes `%h/.local/bin/wukong-schedulerd` with `%h/.wukong/config.env`. Capture enabled state before replacement; restart only previously enabled services. Newly explicit Scheduler is enabled and started. Refuse to overwrite materially customized units.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh systemd
```

### Task 9: Ownership Matrix And Documentation

**Files:**
- Modify: `scripts/test-installer-upgrade.sh`
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/test-release-workflow.sh`
- Modify: `README.md`
- Modify: `docs/installation.md`
- Modify: `docs/docker.md`
- Modify: `CHANGELOG.md`

- [ ] **Step 1: Add complete hash-preservation matrix**

For clean install, same-version rerun, and upgrade in both modes, hash every approved user-owned path before and after. Assert only allowlisted release files and metadata change.

After every installer test consumes global `SHA256SUMS`, add workflow tests that reject `checksums-${TARGET}.txt`, remove their generation/publication from the workflow, and retain aggregate checksum coverage for all remaining assets.

- [ ] **Step 2: Update documentation**

Document Binary upgrade/Scheduler commands, Docker GHCR pull/digest verification, metadata locations, exact ownership boundaries, no `down -v`, macOS Scheduler limitation, and the new Python 3 prerequisite. Remove release-installer guidance that says `docker compose build --no-cache`.

- [ ] **Step 3: Run complete Phase 3 verification**

```bash
bash -n scripts/install.sh
bash scripts/test-installer-upgrade.sh
bash scripts/test-release-command.sh
bash scripts/test-release-workflow.sh
bash scripts/test-release-manifest.sh
bash scripts/test-release-image.sh
bash scripts/test-docker-runtime.sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: all commands exit 0 and installer logs contain zero Docker builds.

- [ ] **Step 4: Run change-impact review and commit**

Run `gitnexus_detect_changes({scope: "all", repo: "Wukong"})`, then:

```bash
git add scripts/test-installer-upgrade.sh .github/workflows/release.yml scripts/test-release-workflow.sh README.md docs/installation.md docs/docker.md CHANGELOG.md
git commit -m "docs: describe transactional installer upgrades"
```
