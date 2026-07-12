# Rollback And RC Rehearsal Phase 4 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add verified previous-version and legacy rollback, automatic transaction recovery, migration compatibility guards, and a stable-promotion gate backed by real RC rehearsal evidence.

**Architecture:** Rollback reuses Phase 3 download, verification, staging, backup, activation, health, and metadata primitives rather than introducing a parallel path. Release manifests declare data compatibility; the installer fails closed before mutation when rollback safety is unknown. A rehearsal command records machine-readable evidence consumed by local and workflow stable validation.

**Tech Stack:** Bash, Python 3 JSON validation, Docker Compose, systemd, GitHub Actions

---

## File Structure

- Modify `scripts/install.sh`: rollback, recovery, compatibility guard.
- Modify `scripts/test-installer-upgrade.sh`: fault injection and rollback matrix.
- Modify `scripts/generate-release-manifest.sh`: data compatibility schema.
- Create `release/data-compatibility.json`: reviewed release compatibility input.
- Create `scripts/rehearse-rc.sh`: real RC rehearsal orchestrator.
- Create `scripts/test-rc-rehearsal.sh`: evidence schema/gate tests.
- Create `docs/release-rehearsal.md`: operator procedure.
- Modify `scripts/release.sh`, workflow/tests, docs, changelog, and release skill.

### Task 1: Finalize Rollback Metadata Invariants

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add metadata invariant tests**

Cover `previousVersion`, optional `previousBackupPath`, mutual exclusion, known tags/digests, prior component/service backup manifest, unknown schema, absent rollback source, atomic write failure, and current/previous rotation.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh rollback-metadata`

Expected: rollback metadata is not yet validated.

- [ ] **Step 3: Implement strict validation**

Extend Binary metadata with `previousBackupPath`; store prior component/service state in each transaction backup manifest. Extend Docker metadata with current/previous image reference and digest. Bare rollback accepts exactly one verified source. Successful A-to-B rollback writes B current and A previous; failed transactions leave old metadata byte-identical.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh rollback-metadata
```

### Task 2: Legacy Binary Backup And First Rollback

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`
- Modify: `docs/installation.md`

- [ ] **Step 1: Add legacy tests**

Test CLI-only/full legacy sets, hashes/modes, managed units, enabled states, backup copy failure, replacement failure, local backup rollback without network, tampering, traversal/symlinks, corrupt modern metadata refusal, and next known upgrade clearing the pointer.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh legacy-rollback`

Expected: no local backup is created.

- [ ] **Step 3: Implement verified legacy backup**

Create `~/.wukong/backups/legacy-20260711T130000Z/` in tests and use the same `legacy-YYYYMMDDTHHMMSSZ` format at runtime through a temporary sibling directory. Store recognized binaries and installer-managed units plus a manifest containing relative path, mode, SHA256, and service state. Never include config/data/workspace. Verify copied hashes before rename. On first bare rollback, validate the backup allowlist and hashes, restore atomically, restore services, and remove `install.json` only after success.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh legacy-rollback
```

### Task 3: Known-Version Binary Rollback And Fault Recovery

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add rollback and fault-injection tests**

Cover bare previous version, explicit older version, current-version verified no-op, Scheduler flag rejection, download/checksum/archive failures, every binary/unit rename boundary, daemon reload, enable/restart/readiness, metadata write, and interrupt signal. Assert original hashes/service states/metadata after each failure.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh binary-rollback`

Expected: `--rollback` is rejected.

- [ ] **Step 3: Reuse the Binary transaction**

Resolve target from metadata or explicit version, verify all target assets, stage them, capture current binaries/units/service state in a transaction backup, stop affected active services, activate by rename, restore desired prior service state, run readiness, and commit rotated metadata. Install signal/error traps only after activation begins; recovery restores old files, removes newly introduced files, restores services, and retains backup paths if recovery fails.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh binary-rollback
```

### Task 4: Docker Rollback And Health Recovery

**Files:**
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`
- Modify: `scripts/test-docker-runtime.sh`

- [ ] **Step 1: Add Docker rollback tests**

Cover cached previous image, verified pull of uncached previous image, checksum/archive/digest failure before replacement, target health timeout, successful old-version recreation, recovery failure reporting, metadata rotation, and preservation of `.env`, overrides, workspace, and volumes.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-installer-upgrade.sh docker-rollback`

Expected: Docker rollback is absent.

- [ ] **Step 3: Implement Docker recovery**

Verify the target release, pull and verify its digest, back up current release-owned files, activate staged files, recreate, and wait with a bounded health loop. Require OpenCode and Web healthy; require Telegram/Scheduler running and not restarting. On failure, restore old files, recreate the old product-tagged image, verify old health, and preserve old metadata byte-for-byte.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-installer-upgrade.sh docker-rollback
bash scripts/test-docker-runtime.sh
```

### Task 5: Migration Compatibility Guard

**Files:**
- Create: `release/data-compatibility.json`
- Modify: `scripts/generate-release-manifest.sh`
- Modify: `scripts/test-release-manifest.sh`
- Modify: `scripts/install.sh`
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add compatibility tests**

Test compatible target, explicitly irreversible migration, missing/malformed metadata, affected state, instructions URL, stable equality with source RC, and refusal before any mutation.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-release-manifest.sh && bash scripts/test-installer-upgrade.sh compatibility`

Expected: manifest lacks `dataCompatibility`.

- [ ] **Step 3: Add reviewed compatibility input**

Use this exact schema:

```json
{"affectedState":[],"backupRequired":false,"instructionsUrl":null,"irreversibleMigration":false,"rollbackSafeTo":"v0.17.1","schemaVersion":1}
```

Embed it in release manifests. Before rollback, verify current and target manifests and require the current release to declare the target safe. Missing or irreversible declarations fail closed and print affected state and recovery URL. Do not add a force bypass.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-release-manifest.sh
bash scripts/test-installer-upgrade.sh compatibility
```

### Task 6: Complete Transaction Matrix

**Files:**
- Modify: `scripts/test-installer-upgrade.sh`

- [ ] **Step 1: Add aggregate matrix**

Require Binary clean/install/rerun/upgrade/rollback/legacy/fault cases on Linux and macOS mode simulation, plus Docker clean/rerun/upgrade/rollback/digest/health/recovery cases. Snapshot user-owned state for every row.

- [ ] **Step 2: Run the matrix**

Run: `bash scripts/test-installer-upgrade.sh`

Expected: all fake-environment cases PASS without real GitHub, Docker daemon, or systemd.

- [ ] **Step 3: Commit**

```bash
git add scripts/test-installer-upgrade.sh
```

### Task 7: Executable RC Rehearsal And Evidence

**Files:**
- Create: `scripts/rehearse-rc.sh`
- Create: `scripts/test-rc-rehearsal.sh`
- Create: `docs/release-rehearsal.md`

- [ ] **Step 1: Write failing evidence-schema tests**

Require from/to tags, commit, workflow/release URLs, manifest hash, image digest, timestamps, environment, Binary and Docker matrix rows, state hashes, rollback duration, compatibility result, and no required `FAIL` or `SKIP`.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-rc-rehearsal.sh`

Expected: rehearsal script is absent.

- [ ] **Step 3: Implement rehearsal orchestration**

Accept explicit `--from`, `--to`, `--binary-home`, `--docker-dir`, and `--evidence`. Run clean RC install, previous-stable upgrade, same-version rerun, Scheduler opt-in, rollback, Docker clean/upgrade/rollback, Web health, controlled Telegram response, controlled Scheduler job, credential availability, and state preservation. Record hashes and status only, never secret values. Write canonical JSON atomically.

- [ ] **Step 4: Document operator procedure**

Explain prerequisites, isolated environments, controlled Telegram/Scheduler checks, SQLite-consistent snapshots, failure handling, and evidence retention.

- [ ] **Step 5: Run tests and commit**

```bash
bash -n scripts/rehearse-rc.sh
bash scripts/test-rc-rehearsal.sh
```

### Task 8: Stable Promotion Rehearsal Gate

**Files:**
- Modify: `scripts/release.sh`
- Modify: `scripts/test-release-command.sh`
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/test-release-workflow.sh`

- [ ] **Step 1: Add gate tests**

Test missing report, wrong RC/commit/digest, missing Binary row, missing Docker row, skipped rollback, failed compatibility, and valid evidence. Require workflow-side validation so manual stable tags cannot bypass the gate.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-release-command.sh && bash scripts/test-release-workflow.sh`

Expected: stable promotion does not require evidence.

- [ ] **Step 3: Implement evidence binding**

Add `--rehearsal-report path`. For stable, validate canonical report before tagging and include exactly one annotation line:

```text
rehearsal-report: docs/release-rehearsals/v0.18.0-rc.2.json
```

Require the report be committed at HEAD. Workflow reads the line, validates the committed report independently, and confirms RC tag, commit, image digest, required rows, and compatibility result before promotion.

- [ ] **Step 4: Run tests and commit**

```bash
bash scripts/test-release-command.sh
bash scripts/test-release-workflow.sh
```

### Task 9: Documentation, Real RC Gate, And Final Verification

**Files:**
- Modify: `README.md`
- Modify: `docs/installation.md`
- Modify: `docs/docker.md`
- Modify: `CHANGELOG.md`
- Modify: `.claude/skills/wukong-release/SKILL.md`

- [ ] **Step 1: Document rollback contracts**

Add exact Binary/Docker rollback commands, metadata rotation, legacy backup behavior, user-owned exclusions, compatibility refusal, recovery instructions, and stable rehearsal requirement. Add migration backup/restore notes when `irreversibleMigration` is true.

- [ ] **Step 2: Run automated verification**

```bash
bash -n scripts/install.sh
bash -n scripts/release.sh
bash -n scripts/rehearse-rc.sh
bash scripts/test-installer-upgrade.sh
bash scripts/test-release-command.sh
bash scripts/test-release-workflow.sh
bash scripts/test-release-manifest.sh
bash scripts/test-release-image.sh
bash scripts/test-docker-runtime.sh
bash scripts/test-rc-rehearsal.sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

Expected: all commands exit 0.

- [ ] **Step 3: Rehearse the actual RC**

```bash
./scripts/rehearse-rc.sh \
  --from v0.17.1 \
  --to v0.18.0-rc.1 \
  --binary-home /tmp/wukong-v0.18.0-rc.1-binary \
  --docker-dir /tmp/wukong-v0.18.0-rc.1-docker \
  --evidence docs/release-rehearsals/v0.18.0-rc.1.json
bash scripts/test-rc-rehearsal.sh docs/release-rehearsals/v0.18.0-rc.1.json
```

Expected: previous stable to RC and rollback PASS in both modes; cached Docker rollback completes within ten minutes.

- [ ] **Step 4: Run change-impact review**

Run `gitnexus_detect_changes({scope: "all", repo: "Wukong"})` and review installer, release, Docker, Scheduler, and service activation flows.

- [ ] **Step 5: Commit documentation and evidence**

```bash
git add README.md docs/installation.md docs/docker.md CHANGELOG.md .claude/skills/wukong-release/SKILL.md docs/release-rehearsals/v0.18.0-rc.1.json
git commit -m "docs: record rc upgrade rollback rehearsal"
```
