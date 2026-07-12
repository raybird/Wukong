# Release And Installation Evolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the approved release and installation evolution through four independently testable phases, beginning with an RC-only release foundation and ending with rehearsed Binary and Docker rollback.

**Architecture:** Git tags remain the sole product version. The release command validates and creates immutable tags; the workflow builds binaries, publishes or promotes one GHCR digest, creates verified release artifacts, and performs one serialized GitHub Release upload. The installer consumes those artifacts through staged transactions that replace only release-owned state and preserve all user-owned state.

**Tech Stack:** Bash, Git, GitHub CLI, GitHub Actions, Docker Buildx/Compose, GHCR, Rust/Cargo, systemd user services

---

## Plan Set

Execute these plans in order. Each phase has an explicit entry gate and must finish with a usable, testable deliverable.

1. `docs/superpowers/plans/2026-07-11-release-foundation-phase-1.md`
2. `docs/superpowers/plans/2026-07-11-reproducible-ghcr-phase-2.md`
3. `docs/superpowers/plans/2026-07-11-installer-migration-phase-3.md`
4. `docs/superpowers/plans/2026-07-11-rollback-rc-rehearsal-phase-4.md`

## Locked Cross-Phase Decisions

- Product tags match `vX.Y.Z` or `vX.Y.Z-rc.N`; Cargo workspace version `0.1.0` is never consulted.
- RC changelog preflight accepts an exact base-version heading such as `## [0.18.0]`; stable requires `## [0.18.0] - YYYY-MM-DD`.
- Phase 1 implements and tests manifest/checksum generators but does not publish an incomplete manifest with null image fields. Phase 2 publishes schema version 1 after the GHCR digest exists.
- `release/runtime-inputs.env` is the single checked-in source for immutable image inputs. Scripts parse an allowlisted `KEY=VALUE` format and never `source` it.
- Stable promotion reuses the RC image digest and RC binary assets. Stable does not rebuild either deliverable.
- Final public release assets are twelve target-suffixed binary archives, one minimal Docker bundle, `release-manifest.json`, and `SHA256SUMS`. Legacy per-target checksum files remain through Phase 2 so the old installer keeps working, then Phase 3 removes them only after the installer switches to `SHA256SUMS`.
- Release Compose uses the literal placeholder `__WUKONG_VERSION__` in five identical GHCR image references. Packaging replaces it with the validated product tag.
- Installer JSON is parsed and written with `python3`; Phase 3 adds `python3` as an explicit prerequisite. This avoids unsafe JSON parsing with grep and sed.
- Binary transaction staging and backups live under `~/.local/bin` and `~/.wukong`, respectively, so activation uses same-filesystem rename.
- Rollback rotates current and previous versions. A rollback from A to B leaves B current and A previous.
- A successful first rollback to a legacy Binary backup removes `install.json`, returning the installation to a recognized legacy state.
- Automatic rollback fails closed when release compatibility metadata is missing or declares an irreversible migration. There is no force bypass in this release.
- Stable promotion requires a machine-readable RC rehearsal report committed on the candidate commit and referenced in the stable tag annotation.

## Phase Gates

### Phase 1 Gate

Entry: current binary tag workflow passes.

Exit:

```bash
bash scripts/test-release-command.sh
bash scripts/test-release-workflow.sh
bash scripts/test-release-manifest.sh
./scripts/release.sh v0.18.0-rc.1 --dry-run
```

No Docker installer behavior changes in this phase.

### Phase 2 Gate

Entry: Phase 1 can safely create and watch an RC tag.

Exit:

```bash
bash scripts/test-release-workflow.sh
bash scripts/test-release-manifest.sh
bash scripts/test-release-image.sh
bash scripts/test-docker-runtime.sh
```

An RC image and its commit tag resolve to the manifest digest; simulated stable promotion resolves to the same digest without a build command.

### Phase 3 Gate

Entry: a tag-pinned GHCR image and verified minimal bundle exist.

Exit:

```bash
bash scripts/test-installer-upgrade.sh
bash scripts/test-docker-runtime.sh
```

Binary and Docker clean install, same-version rerun, upgrade, Scheduler opt-in, and ownership hash tests pass.

### Phase 4 Gate

Entry: both installer modes have staged activation and health checks.

Exit:

```bash
bash scripts/test-installer-upgrade.sh
bash scripts/test-rc-rehearsal.sh
./scripts/rehearse-rc.sh --from v0.17.1 --to v0.18.0-rc.1 --evidence /tmp/wukong-rehearsal.json
```

Previous stable to RC and back succeeds in both modes before stable promotion is permitted.

## Global Verification

Run after every phase:

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash scripts/test-release-workflow.sh
bash scripts/test-release-command.sh
bash scripts/test-installer-upgrade.sh
bash scripts/test-docker-runtime.sh
git diff --check
```

Run `gitnexus_detect_changes({scope: "all", repo: "Wukong"})` before every implementation commit. Stage only the files named by the active task; do not stage existing unrelated changes in `AGENTS.md`, `CLAUDE.md`, the approved design file, or `scratch/`.
