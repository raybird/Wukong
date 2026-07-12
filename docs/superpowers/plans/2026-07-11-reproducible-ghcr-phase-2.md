# Reproducible GHCR Publication Phase 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish an immutable `linux/amd64` GHCR image from workflow-built musl binaries, promote stable tags by digest without rebuilding, and ship a verified pull-only release bundle.

**Architecture:** A release-only Dockerfile consumes a narrow generated build context and immutable runtime inputs. RC workflows push by digest, then attach protected product and commit tags; stable workflows verify the source RC manifest and attach the stable tag to the same digest. A package job creates all public artifacts before generating one global checksum file.

**Tech Stack:** GitHub Actions, Docker Buildx, GHCR, Bash, OCI manifests, Docker Compose

---

## File Structure

- Create `Dockerfile.release`: production image from CI binaries.
- Create `docker-compose.release.yml`: five-service pull-only template.
- Create `release/runtime-inputs.env`: immutable base/runtime inputs.
- Create `release/package.json` and `release/package-lock.json`: locked Opencode install.
- Create `scripts/test-release-image.sh`: release image and registry operation tests.
- Modify `.github/workflows/release.yml`: five-job pipeline.
- Modify `scripts/test-release-workflow.sh`: image/promotion/package contracts.
- Modify `scripts/test-release-manifest.sh`: full runtime input schema.
- Modify `scripts/test-docker-runtime.sh`: development/release Compose separation.
- Modify `scripts/release.sh` and `scripts/test-release-command.sh`: mandatory release Compose preflight.
- Modify `docker-compose.yml`, `Dockerfile`, and `.env.example` only where parity comments/defaults require it.

### Task 1: Freeze Runtime Inputs

**Files:**
- Create: `release/runtime-inputs.env`
- Create: `release/package.json`
- Create: `release/package-lock.json`
- Modify: `scripts/test-release-image.sh`

- [ ] **Step 1: Write failing pin validation tests**

Require an immutable base reference, Debian snapshot timestamp, exact Opencode version/integrity, Agent Reach full commit SHA/archive checksum, and reject `latest`, `main`, empty values, or malformed digests.

- [ ] **Step 2: Run and verify failure**

Run: `bash scripts/test-release-image.sh`

Expected: FAIL because pin files are absent.

- [ ] **Step 3: Add reviewed pin files**

Resolve and freeze values rather than typing examples into the committed file:

```bash
BASE_IMAGE_DIGEST="$(docker buildx imagetools inspect debian:bookworm-slim --format '{{json .Manifest.Digest}}' | tr -d '"')"
OPENCODE_VERSION="$(npm view opencode-ai version)"
AGENT_REACH_REF="$(git ls-remote https://github.com/Panniantong/agent-reach.git HEAD | cut -f1)"
curl -fsSL "https://github.com/Panniantong/agent-reach/archive/${AGENT_REACH_REF}.tar.gz" -o /tmp/agent-reach.tar.gz
AGENT_REACH_ARCHIVE_SHA256="$(sha256sum /tmp/agent-reach.tar.gz | cut -d' ' -f1)"
npm install --package-lock-only --prefix release "opencode-ai@${OPENCODE_VERSION}"
OPENCODE_INTEGRITY="$(node -p "require('./release/package-lock.json').packages['node_modules/opencode-ai'].integrity")"
DEBIAN_SNAPSHOT="$(date -u +%Y%m%dT000000Z)"
printf '%s\n' \
  "BASE_IMAGE=debian:bookworm-slim@${BASE_IMAGE_DIGEST}" \
  "DEBIAN_SNAPSHOT=${DEBIAN_SNAPSHOT}" \
  "OPENCODE_VERSION=${OPENCODE_VERSION}" \
  "OPENCODE_INTEGRITY=${OPENCODE_INTEGRITY}" \
  "AGENT_REACH_REF=${AGENT_REACH_REF}" \
  "AGENT_REACH_ARCHIVE_SHA256=${AGENT_REACH_ARCHIVE_SHA256}" \
  > release/runtime-inputs.env
```

Verify the selected Debian snapshot serves every required package before committing. The committed file contains only resolved immutable values; tests reject angle brackets, floating refs, and missing integrity fields.

- [ ] **Step 4: Run pin tests**

Run: `bash scripts/test-release-image.sh pins`

Expected: pin contract PASS and no floating input found.

- [ ] **Step 5: Commit**

```bash
git add release/runtime-inputs.env release/package.json release/package-lock.json scripts/test-release-image.sh
git commit -m "build: pin release image inputs"
```

### Task 2: Release Compose Contract

**Files:**
- Create: `docker-compose.release.yml`
- Modify: `scripts/test-docker-runtime.sh`
- Modify: `docker-compose.yml`
- Modify: `scripts/release.sh`
- Modify: `scripts/test-release-command.sh`

- [ ] **Step 1: Add failing Compose parity tests**

Assert development Compose contains `build:`, release Compose contains none, all five release services use `ghcr.io/raybird/wukong:__WUKONG_VERSION__`, no release service uses `latest`, and volumes, commands, OpenCode healthcheck, service dependencies, Scheduler presence, and loopback Web default match development Compose. Add release-command cases for a missing file, a `build:` key, and wrong placeholder count.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-docker-runtime.sh`

Expected: FAIL because release Compose is absent.

- [ ] **Step 3: Add release Compose**

Copy the five service definitions and named volumes from development Compose, remove every `build` mapping, and set each `image` to the exact placeholder. Add comments identifying root Compose as development-only and release Compose as packaging input. Make local and workflow preflight require the file, reject `build:`, require five placeholders, and include `.env.example`, both Compose files, `docs/docker.md`, and `CHANGELOG.md` in the synchronized env-change set.

- [ ] **Step 4: Validate both files**

```bash
docker compose -f docker-compose.yml config -q
docker compose -f docker-compose.release.yml config -q
bash scripts/test-docker-runtime.sh
bash scripts/test-release-command.sh
```

Expected: both Compose files parse and parity tests PASS.

- [ ] **Step 5: Commit**

```bash
git add docker-compose.yml docker-compose.release.yml scripts/test-docker-runtime.sh scripts/release.sh scripts/test-release-command.sh
git commit -m "feat(docker): add pull-only release compose"
```

### Task 3: Release Image Definition

**Files:**
- Create: `Dockerfile.release`
- Modify: `scripts/test-release-image.sh`

- [ ] **Step 1: Add failing static and smoke tests**

Require `COPY binaries/wukong*`, prohibit GitHub Release binary downloads, validate OCI labels, exact base digest, snapshot package source, `npm ci`, Agent Reach checksum, all four executable `--help` commands, exact Opencode version, canonical workspace/skill assets, and a non-root dispatched process.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-release-image.sh static`

Expected: FAIL because `Dockerfile.release` is absent.

- [ ] **Step 3: Implement `Dockerfile.release`**

Use build arguments for immutable inputs and labels, copy only staged binaries/runtime files, install Debian packages from the fixed snapshot, run `npm ci --omit=dev` from locked release package files, download Agent Reach by immutable ref with SHA256 verification, create the `wukong` user, copy the existing entrypoint, and preserve UID/GID remapping followed by `gosu` privilege drop.

The label block is:

```dockerfile
LABEL org.opencontainers.image.source="https://github.com/raybird/Wukong" \
      org.opencontainers.image.revision="$REVISION" \
      org.opencontainers.image.version="$BUILD_ORIGIN_TAG" \
      org.opencontainers.image.created="$CREATED"
```

- [ ] **Step 4: Build a narrow local context and smoke test**

Stage four test binaries plus entrypoint, workspace templates, skills, and release lockfiles under `/tmp/wukong-release-context`, then run:

```bash
docker buildx build --platform linux/amd64 --file Dockerfile.release --load --tag wukong:release-test /tmp/wukong-release-context
bash scripts/test-release-image.sh smoke wukong:release-test
```

Expected: all runtime assertions PASS.

- [ ] **Step 5: Commit**

```bash
git add Dockerfile.release scripts/test-release-image.sh
git commit -m "feat(docker): build release image from ci binaries"
```

### Task 4: Refactor Workflow Job Graph

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/test-release-workflow.sh`

- [ ] **Step 1: Add failing structural tests**

Require `contents: write`, `packages: write`, jobs `validate`, `build-binaries`, `publish-image`, `package-release`, `publish`, exact `needs` edges, `cargo build --release --locked`, and one release upload.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-release-workflow.sh`

Expected: FAIL because image/package jobs are absent.

- [ ] **Step 3: Create the five-job graph**

Keep Phase 1 validation. Have `build-binaries` upload target-suffixed tarballs plus a private musl-binaries artifact. Prevent bare cross-target binaries from entering public artifacts. Add per-tag concurrency with `cancel-in-progress: false`.

- [ ] **Step 4: Run workflow tests**

Run: `bash scripts/test-release-workflow.sh`

Expected: graph and binary matrix tests PASS; image behavior tests remain pending.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml scripts/test-release-workflow.sh
git commit -m "ci(release): split release pipeline jobs"
```

### Task 5: Immutable RC Image Publication

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/test-release-workflow.sh`
- Modify: `scripts/test-release-image.sh`

- [ ] **Step 1: Add fake-registry tests**

Model missing tags, same-digest rerun, conflicting RC tag, conflicting commit tag, delayed registry visibility, and successful product/commit tagging. Assert protected tags never change on conflict.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-release-image.sh registry`

Expected: FAIL because registry operations are absent.

- [ ] **Step 3: Implement RC publication**

For RC only, download musl binaries, stage the narrow context, log in to GHCR, build/push by digest, capture `sha256:...`, inspect `ghcr.io/raybird/wukong:$TAG` and `:sha-$COMMIT`, fail if either resolves differently, attach absent tags with a pinned manifest-preserving registry client, and verify both resolve to the captured digest. Smoke-test `repository@digest`.

- [ ] **Step 4: Run tests**

```bash
bash scripts/test-release-workflow.sh
bash scripts/test-release-image.sh registry
```

Expected: immutable rerun and conflict cases PASS.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml scripts/test-release-workflow.sh scripts/test-release-image.sh
git commit -m "feat(release): publish immutable rc images"
```

### Task 6: Stable Digest Promotion

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/test-release-workflow.sh`
- Modify: `scripts/test-release-image.sh`

- [ ] **Step 1: Add promotion tests**

Test missing/malformed RC manifest, checksum failure, source commit mismatch, wrong platform/repository, moved RC tag, absent stable tag, same-digest idempotent stable tag, and conflicting stable tag. Assert no build command executes on stable.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-release-image.sh promotion`

Expected: FAIL because stable path does not promote.

- [ ] **Step 3: Implement stable path**

Download the source RC `release-manifest.json` and `SHA256SUMS`, verify the manifest checksum, validate source tag/commit/channel/platform, independently resolve the RC and commit GHCR tags, attach the stable product tag to that exact digest, and verify equality. Reuse source RC binary release assets rather than rebuilding.

- [ ] **Step 4: Run tests**

Run:

```bash
bash scripts/test-release-image.sh promotion
bash scripts/test-release-workflow.sh
```

Expected: promotion is digest-preserving and build-free.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml scripts/test-release-workflow.sh scripts/test-release-image.sh
git commit -m "feat(release): promote stable image digest"
```

### Task 7: Manifest, Minimal Bundle, And Global Checksums

**Files:**
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/generate-release-manifest.sh`
- Modify: `scripts/test-release-manifest.sh`
- Modify: `scripts/test-release-workflow.sh`

- [ ] **Step 1: Add full packaging tests**

Require manifest runtime inputs, image build-origin tag, stable `promotedFrom`, twelve binary archives, three compatibility per-target checksum files, minimal bundle entries, no Dockerfile/source/templates, manifest generation before checksum generation, and complete `SHA256SUMS` coverage excluding itself.

- [ ] **Step 2: Verify failure**

Run: `bash scripts/test-release-manifest.sh && bash scripts/test-release-workflow.sh`

Expected: FAIL because packaging still uses the legacy bundle.

- [ ] **Step 3: Implement package job**

Download binary assets and image outputs, replace `__WUKONG_VERSION__` in release Compose, assert no placeholder or `build:` remains, create this bundle:

```text
wukong-docker/docker-compose.yml
wukong-docker/.env.example
wukong-docker/LICENSE
wukong-docker/scripts/install.sh
wukong-docker/release-manifest.json
```

Generate the canonical manifest, archive the bundle, retain three per-target checksum files for the pre-Phase-3 installer, then generate `SHA256SUMS`. Upload one exact workflow artifact to the serialized `publish` job.

- [ ] **Step 4: Run packaging tests**

```bash
bash scripts/test-release-manifest.sh
bash scripts/test-release-workflow.sh
bash scripts/test-docker-runtime.sh
```

Expected: all package and ownership assertions PASS.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/release.yml scripts/generate-release-manifest.sh scripts/test-release-manifest.sh scripts/test-release-workflow.sh
git commit -m "feat(release): package verified release artifacts"
```

### Task 8: Phase 2 Verification

**Files:**
- Modify: `CHANGELOG.md`
- Modify: `.claude/skills/wukong-release/SKILL.md`

- [ ] **Step 1: Document GHCR and bundle behavior**

Describe RC image publication, stable digest promotion, immutable conflicts, minimal bundle, and traceability. Do not yet document installer pull-only behavior as active until Phase 3.

- [ ] **Step 2: Run full checks**

```bash
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked
bash scripts/test-release-command.sh
bash scripts/test-release-workflow.sh
bash scripts/test-release-manifest.sh
bash scripts/test-release-image.sh
bash scripts/test-installer-upgrade.sh
bash scripts/test-docker-runtime.sh
git diff --check
```

Expected: all commands exit 0.

- [ ] **Step 3: Run change-impact review**

Run `gitnexus_detect_changes({scope: "all", repo: "Wukong"})`; review release and Docker runtime processes.

- [ ] **Step 4: Commit**

```bash
git add CHANGELOG.md .claude/skills/wukong-release/SKILL.md
git commit -m "docs: describe reproducible ghcr releases"
```
