# Docker Runtime Skill Assets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Package Wukong's Superpowers skill files into Docker releases, seed them into `/workspace/.wukong/skills/superpowers/`, and point skill prompts at that stable runtime path.

**Architecture:** Keep `crates/wukong-skills/assets/superpowers/` as the canonical source. Docker build copies those assets into `/usr/local/share/wukong/skills/superpowers/`; the entrypoint mirrors them into the mounted workspace only when missing or stale; `persona::build_prompt_with_skill` points opencode to the workspace copy.

**Tech Stack:** Rust workspace tests with `cargo test`, Bash entrypoint logic, Dockerfile asset packaging, shell-based static runtime checks in `scripts/test-docker-runtime.sh`.

---

## File Structure

- Modify: `Dockerfile`
  - Responsibility: package canonical skill assets into the runtime image at `/usr/local/share/wukong/skills/superpowers/`.
- Modify: `scripts/docker-entrypoint.sh`
  - Responsibility: seed or refresh `/workspace/.wukong/skills/superpowers/` from image assets during startup.
- Modify: `scripts/test-docker-runtime.sh`
  - Responsibility: static checks that Dockerfile and entrypoint contain the expected asset packaging and runtime sync wiring.
- Modify: `crates/wukong-runtime/src/persona.rs`
  - Responsibility: update skill prompt path and tests from source-relative `crates/...` to `/workspace/.wukong/...`.
- Optional verification only: `crates/wukong-skills/src/catalog.rs`
  - Responsibility: existing catalog tests continue proving embedded skill content exists; no implementation change planned.

---

### Task 1: Add Static Docker Asset Packaging Checks

**Files:**
- Modify: `scripts/test-docker-runtime.sh`
- Later task modifies: `Dockerfile`

- [ ] **Step 1: Add failing Dockerfile checks**

Add these checks after the existing `entrypoint="scripts/docker-entrypoint.sh"` assignment:

```bash
dockerfile="Dockerfile"
```

Add these checks after the existing Agent Reach state checks and before the scheduler profile `awk` block:

```bash
require_in_file "COPY crates/wukong-skills/assets/superpowers /usr/local/share/wukong/skills/superpowers" "$dockerfile" \
    "Docker image must package Superpowers skill assets"
require_in_file "/usr/local/share/wukong/skills/superpowers" "$dockerfile" \
    "Dockerfile must use the canonical image skill asset path"
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
scripts/test-docker-runtime.sh
```

Expected: FAIL with a message containing:

```text
Docker image must package Superpowers skill assets
```

- [ ] **Step 3: Commit the failing test**

```bash
git add scripts/test-docker-runtime.sh
git commit -m "test: cover Docker skill asset packaging"
```

---

### Task 2: Package Skill Assets Into the Docker Image

**Files:**
- Modify: `Dockerfile`
- Test: `scripts/test-docker-runtime.sh`

- [ ] **Step 1: Update Dockerfile**

In `Dockerfile`, after the existing template copy lines:

```dockerfile
# Copy default workspace templates (SOUL.md, AGENTS.md)
RUN mkdir -p /usr/local/share/wukong
COPY workspace/SOUL.md workspace/AGENTS.md /usr/local/share/wukong/
```

add:

```dockerfile
# Copy runtime-readable Superpowers skill assets from the canonical source tree.
COPY crates/wukong-skills/assets/superpowers /usr/local/share/wukong/skills/superpowers
```

- [ ] **Step 2: Run static Docker runtime check**

Run:

```bash
scripts/test-docker-runtime.sh
```

Expected: PASS with:

```text
docker runtime persistence checks passed
```

- [ ] **Step 3: Inspect Dockerfile diff**

Run:

```bash
git diff -- Dockerfile scripts/test-docker-runtime.sh
```

Expected: diff only shows the new Dockerfile asset copy and the test checks from Task 1.

- [ ] **Step 4: Commit**

```bash
git add Dockerfile scripts/test-docker-runtime.sh
git commit -m "build: package Docker runtime skill assets"
```

---

### Task 3: Add Entrypoint Runtime Sync Checks

**Files:**
- Modify: `scripts/test-docker-runtime.sh`
- Later task modifies: `scripts/docker-entrypoint.sh`

- [ ] **Step 1: Add failing entrypoint checks**

Add these checks after the Dockerfile checks from Task 1 and before the scheduler profile `awk` block:

```bash
require_in_file 'IMAGE_SKILLS="/usr/local/share/wukong/skills/superpowers"' "$entrypoint" \
    "entrypoint must define image skill asset source"
require_in_file 'WORKSPACE_SKILLS="$WUKONG_WORKSPACE/.wukong/skills/superpowers"' "$entrypoint" \
    "entrypoint must define workspace skill asset destination"
require_in_file 'sync_wukong_skills()' "$entrypoint" \
    "entrypoint must provide a skill asset sync function"
require_in_file 'cmp -s "$IMAGE_SKILLS/SOURCE.md" "$WORKSPACE_SKILLS/SOURCE.md"' "$entrypoint" \
    "entrypoint must skip skill sync when SOURCE.md matches"
require_in_file 'cp -a "$IMAGE_SKILLS/." "$tmp_dir/"' "$entrypoint" \
    "entrypoint must copy image skill assets into a temporary directory"
require_in_file 'mv "$tmp_dir" "$WORKSPACE_SKILLS"' "$entrypoint" \
    "entrypoint must atomically install workspace skill assets"
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
scripts/test-docker-runtime.sh
```

Expected: FAIL with a message containing:

```text
entrypoint must define image skill asset source
```

- [ ] **Step 3: Commit the failing test**

```bash
git add scripts/test-docker-runtime.sh
git commit -m "test: cover Docker skill asset runtime sync"
```

---

### Task 4: Implement Entrypoint Workspace Skill Sync

**Files:**
- Modify: `scripts/docker-entrypoint.sh`
- Test: `scripts/test-docker-runtime.sh`

- [ ] **Step 1: Update entrypoint before template seeding**

In `scripts/docker-entrypoint.sh`, inside the `if [[ -n "${WUKONG_WORKSPACE:-}" ]]; then` block, after:

```bash
    mkdir -p "$WUKONG_WORKSPACE"
    chown wukong:wukong "$WUKONG_WORKSPACE" 2>/dev/null || true
```

insert:

```bash
    IMAGE_SKILLS="/usr/local/share/wukong/skills/superpowers"
    WORKSPACE_SKILLS="$WUKONG_WORKSPACE/.wukong/skills/superpowers"

    sync_wukong_skills() {
        if [[ ! -d "$IMAGE_SKILLS" ]]; then
            echo "[wukong] Warning: image skill assets missing at $IMAGE_SKILLS" >&2
            return 0
        fi

        if [[ -f "$IMAGE_SKILLS/SOURCE.md" && -f "$WORKSPACE_SKILLS/SOURCE.md" ]] && \
            cmp -s "$IMAGE_SKILLS/SOURCE.md" "$WORKSPACE_SKILLS/SOURCE.md"; then
            return 0
        fi

        local parent_dir tmp_dir old_dir
        parent_dir="$(dirname "$WORKSPACE_SKILLS")"
        tmp_dir="$parent_dir/.superpowers.tmp.$$"
        old_dir="$parent_dir/.superpowers.old.$$"

        if ! mkdir -p "$parent_dir"; then
            echo "[wukong] Warning: cannot create skill asset directory at $parent_dir" >&2
            return 0
        fi

        rm -rf "$tmp_dir" "$old_dir" 2>/dev/null || true
        if ! mkdir -p "$tmp_dir"; then
            echo "[wukong] Warning: cannot prepare temporary skill asset directory at $tmp_dir" >&2
            return 0
        fi

        if ! cp -a "$IMAGE_SKILLS/." "$tmp_dir/"; then
            echo "[wukong] Warning: failed to copy skill assets into $tmp_dir" >&2
            rm -rf "$tmp_dir" 2>/dev/null || true
            return 0
        fi

        if [[ -d "$WORKSPACE_SKILLS" ]]; then
            if ! mv "$WORKSPACE_SKILLS" "$old_dir"; then
                echo "[wukong] Warning: cannot replace existing skill assets at $WORKSPACE_SKILLS" >&2
                rm -rf "$tmp_dir" 2>/dev/null || true
                return 0
            fi
        fi

        if ! mv "$tmp_dir" "$WORKSPACE_SKILLS"; then
            echo "[wukong] Warning: failed to install skill assets at $WORKSPACE_SKILLS" >&2
            if [[ -d "$old_dir" ]]; then
                mv "$old_dir" "$WORKSPACE_SKILLS" 2>/dev/null || true
            fi
            rm -rf "$tmp_dir" 2>/dev/null || true
            return 0
        fi

        rm -rf "$old_dir" 2>/dev/null || true
        chown -R wukong:wukong "$WUKONG_WORKSPACE/.wukong" 2>/dev/null || true
        echo "[wukong] Workspace skill assets ready at $WORKSPACE_SKILLS"
    }

    sync_wukong_skills
```

- [ ] **Step 2: Run static Docker runtime check**

Run:

```bash
scripts/test-docker-runtime.sh
```

Expected: PASS with:

```text
docker runtime persistence checks passed
```

- [ ] **Step 3: Syntax-check the entrypoint**

Run:

```bash
bash -n scripts/docker-entrypoint.sh
```

Expected: no output and exit code 0.

- [ ] **Step 4: Commit**

```bash
git add scripts/docker-entrypoint.sh scripts/test-docker-runtime.sh
git commit -m "feat: seed Docker workspace skill assets"
```

---

### Task 5: Update Skill Prompt Path Tests

**Files:**
- Modify: `crates/wukong-runtime/src/persona.rs`
- Test: `cargo test -p wukong-runtime persona::tests::build_prompt_with_skill_includes_skill_block`

- [ ] **Step 1: Run GitNexus impact before editing `build_prompt_with_skill`**

Run GitNexus impact analysis for the symbol:

```text
target: build_prompt_with_skill
direction: upstream
file_path: crates/wukong-runtime/src/persona.rs
repo: Wukong
```

Expected: record the risk level, direct callers, and affected processes before editing. If risk is HIGH or CRITICAL, stop and ask the user before continuing.

- [ ] **Step 2: Update the failing assertion first**

In `crates/wukong-runtime/src/persona.rs`, change the assertion in `build_prompt_with_skill_includes_skill_block` from:

```rust
assert!(
    p.contains("crates/wukong-skills/assets/superpowers/test-driven-development/SKILL.md")
);
```

to:

```rust
assert!(
    p.contains("/workspace/.wukong/skills/superpowers/test-driven-development/SKILL.md")
);
assert!(!p.contains("crates/wukong-skills/assets/superpowers"));
```

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
cargo test -p wukong-runtime persona::tests::build_prompt_with_skill_includes_skill_block
```

Expected: FAIL because `build_prompt_with_skill` still emits `crates/wukong-skills/assets/superpowers/...`.

- [ ] **Step 4: Commit the failing test**

```bash
git add crates/wukong-runtime/src/persona.rs
git commit -m "test: require Docker workspace skill path"
```

---

### Task 6: Update Skill Prompt Runtime Path

**Files:**
- Modify: `crates/wukong-runtime/src/persona.rs`
- Test: `cargo test -p wukong-runtime persona::tests::build_prompt_with_skill_includes_skill_block`

- [ ] **Step 1: Update prompt text**

In `build_prompt_with_skill`, replace:

```rust
"\n\n[技能規範指引]\n你必須遵循 `{}` 的流程。請先閱讀並遵循專案中的技能規範文件：\n路徑：crates/wukong-skills/assets/superpowers/{}/SKILL.md",
skill.name, skill.name
```

with:

```rust
"\n\n[技能規範指引]\n你必須遵循 `{}` 的流程。請先閱讀並遵循 Docker runtime 內的技能規範文件：\n路徑：/workspace/.wukong/skills/superpowers/{}/SKILL.md",
skill.name, skill.name
```

- [ ] **Step 2: Run focused prompt test**

Run:

```bash
cargo test -p wukong-runtime persona::tests::build_prompt_with_skill_includes_skill_block
```

Expected: PASS.

- [ ] **Step 3: Run runtime crate tests**

Run:

```bash
cargo test -p wukong-runtime
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-runtime/src/persona.rs
git commit -m "feat: point skills at Docker workspace assets"
```

---

### Task 7: End-to-End Verification

**Files:**
- No source edits expected.

- [ ] **Step 1: Run Docker runtime static checks**

Run:

```bash
scripts/test-docker-runtime.sh
```

Expected: PASS with:

```text
docker runtime persistence checks passed
```

- [ ] **Step 2: Run relevant Rust tests**

Run:

```bash
cargo test -p wukong-skills -p wukong-runtime
```

Expected: PASS.

- [ ] **Step 3: Build Docker image if Docker is available**

Run:

```bash
docker build --build-arg VERSION=v0.16.14 -t wukong:skill-assets-test .
```

Expected: PASS. If Docker is unavailable or release assets cannot be fetched, record the exact failure and continue with static/Rust verification results.

- [ ] **Step 4: Verify image contains packaged skill assets if build passed**

Run:

```bash
docker run --rm --entrypoint test wukong:skill-assets-test -f /usr/local/share/wukong/skills/superpowers/brainstorming/SKILL.md
```

Expected: PASS with exit code 0.

- [ ] **Step 5: Verify workspace seeding if build passed**

Run:

```bash
docker run --rm -e WUKONG_WORKSPACE=/workspace --entrypoint /usr/local/bin/docker-entrypoint.sh wukong:skill-assets-test test -f /workspace/.wukong/skills/superpowers/brainstorming/SKILL.md
```

Expected: PASS with exit code 0.

- [ ] **Step 6: Run GitNexus change detection**

Run GitNexus detect changes:

```text
scope: all
repo: Wukong
```

Expected: changed symbols include `build_prompt_with_skill`; affected scope matches the prompt-generation path. If unexpected high-risk flows appear, inspect before finalizing.

- [ ] **Step 7: Review final git state**

Run:

```bash
git status --short --branch
git log --oneline -10
```

Expected: clean worktree after all task commits, with commits from this plan on `feature/docker-runtime-skill-assets`.

---

## Self-Review

- Spec coverage: Docker image asset packaging is covered by Tasks 1-2; workspace runtime sync is covered by Tasks 3-4; prompt path change is covered by Tasks 5-6; verification is covered by Task 7; sync script remains canonical and unchanged.
- Placeholder scan: no TBD/TODO placeholders remain; each code-changing step includes concrete snippets and commands.
- Type consistency: paths are consistent across Dockerfile, entrypoint, prompt, and tests: image path `/usr/local/share/wukong/skills/superpowers/`, workspace path `/workspace/.wukong/skills/superpowers/`.
