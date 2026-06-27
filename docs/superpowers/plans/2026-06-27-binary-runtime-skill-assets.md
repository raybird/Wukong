# Binary Runtime Skill Assets Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Materialize embedded Superpowers skill files for binary-installed Wukong runs and point prompts at the resolved workspace-local skill path.

**Architecture:** Add a focused runtime skill-assets module that resolves the active workspace as `WUKONG_WORKSPACE` or `current_dir()`, writes embedded `wukong-skills` catalog content into `<workspace>/.wukong/skills/superpowers/`, and returns the absolute skill root. Turn execution ensures assets before planning/execution and passes the resolved root into prompt construction.

**Tech Stack:** Rust standard library filesystem APIs, existing `wukong-skills` catalog, `cargo test`, GitNexus impact analysis before symbol edits.

---

## File Structure

- Modify: `crates/wukong-skills/src/catalog.rs`
  - Responsibility: expose embedded `SOURCE.md` content for runtime materialization.
- Create: `crates/wukong-runtime/src/skill_assets.rs`
  - Responsibility: resolve workspace path, materialize skill files, and return absolute skill root.
- Modify: `crates/wukong-runtime/src/lib.rs`
  - Responsibility: export the new `skill_assets` module.
- Modify: `crates/wukong-runtime/src/persona.rs`
  - Responsibility: accept a resolved skill root path when building skill prompts and avoid Docker-specific wording.
- Modify: `crates/wukong-runtime/src/turn.rs`
  - Responsibility: ensure runtime skill assets before executing turns, pass the resolved skill root into prompt construction, and update tests.

---

### Task 1: Expose Embedded Source Attribution

**Files:**
- Modify: `crates/wukong-skills/src/catalog.rs`
- Modify: `crates/wukong-skills/src/lib.rs`
- Test: `cargo test -p wukong-skills source_content_is_embedded`

- [ ] **Step 1: Run GitNexus impact before editing `wukong-skills` catalog exports**

Run GitNexus impact analysis on the existing catalog entry point:

```text
target: all
direction: upstream
file_path: crates/wukong-skills/src/catalog.rs
repo: Wukong
```

Expected: record risk, direct callers, and affected processes. If HIGH or CRITICAL, stop and ask the user before editing.

- [ ] **Step 2: Write the failing test**

Add this test to `crates/wukong-skills/src/catalog.rs` inside `mod tests`:

```rust
#[test]
fn source_content_is_embedded() {
    let source = source_content();
    assert!(source.contains("Source Attribution"));
    assert!(source.contains("superpowers"));
}
```

In `crates/wukong-skills/src/lib.rs`, do not export anything yet.

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
cargo test -p wukong-skills source_content_is_embedded
```

Expected: FAIL with `cannot find function source_content`.

- [ ] **Step 4: Implement minimal source export**

Add this function near `find` in `crates/wukong-skills/src/catalog.rs`:

```rust
pub fn source_content() -> &'static str {
    include_str!("../assets/superpowers/SOURCE.md")
}
```

Update `crates/wukong-skills/src/lib.rs` export line to:

```rust
pub use catalog::{all, find, route_options, source_content, SkillId, SkillSpec};
```

- [ ] **Step 5: Run test to verify it passes**

Run:

```bash
cargo test -p wukong-skills source_content_is_embedded
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/wukong-skills/src/catalog.rs crates/wukong-skills/src/lib.rs
git commit -m "feat: expose embedded skill source attribution"
```

---

### Task 2: Add Runtime Skill Asset Materializer

**Files:**
- Create: `crates/wukong-runtime/src/skill_assets.rs`
- Modify: `crates/wukong-runtime/src/lib.rs`
- Test: `cargo test -p wukong-runtime skill_assets`

- [ ] **Step 1: Write the failing module declaration test**

Create `crates/wukong-runtime/src/skill_assets.rs` with only tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_dir_uses_current_dir_when_env_missing() {
        let temp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::remove_var("WUKONG_WORKSPACE");
        std::env::set_current_dir(temp.path()).unwrap();

        let resolved = resolve_skill_workspace().unwrap();

        std::env::set_current_dir(original).unwrap();
        assert_eq!(resolved, temp.path());
    }

    #[test]
    fn workspace_dir_uses_wukong_workspace_when_set() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("WUKONG_WORKSPACE", temp.path());

        let resolved = resolve_skill_workspace().unwrap();

        std::env::remove_var("WUKONG_WORKSPACE");
        assert_eq!(resolved, temp.path());
    }

    #[test]
    fn materialize_writes_skill_files_under_workspace() {
        let temp = tempfile::tempdir().unwrap();

        let root = materialize_runtime_skills(temp.path()).unwrap();

        assert_eq!(root, temp.path().join(".wukong/skills/superpowers"));
        assert!(root.join("brainstorming/SKILL.md").is_file());
        assert!(root.join("SOURCE.md").is_file());
    }
}
```

Add this line to `crates/wukong-runtime/src/lib.rs` so the tests compile against
missing functions:

```rust
pub mod skill_assets;
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p wukong-runtime skill_assets
```

Expected: FAIL with missing function errors for `resolve_skill_workspace` and
`materialize_runtime_skills`.

- [ ] **Step 3: Implement materializer**

Replace `crates/wukong-runtime/src/skill_assets.rs` with:

```rust
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn resolve_skill_workspace() -> io::Result<PathBuf> {
    if let Ok(value) = std::env::var("WUKONG_WORKSPACE") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return Ok(PathBuf::from(trimmed));
        }
    }
    std::env::current_dir()
}

pub fn materialize_default_runtime_skills() -> io::Result<PathBuf> {
    let workspace = resolve_skill_workspace()?;
    materialize_runtime_skills(&workspace)
}

pub fn materialize_runtime_skills(workspace: &Path) -> io::Result<PathBuf> {
    let root = workspace.join(".wukong/skills/superpowers");
    let source = wukong_skills::source_content();
    let source_path = root.join("SOURCE.md");

    if source_path
        .read_to_string()
        .map(|existing| existing == source)
        .unwrap_or(false)
    {
        return Ok(root);
    }

    let parent = root.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "skill asset root has no parent")
    })?;
    let tmp = parent.join(format!(".superpowers.tmp.{}", std::process::id()));
    let old = parent.join(format!(".superpowers.old.{}", std::process::id()));

    let _ = fs::remove_dir_all(&tmp);
    let _ = fs::remove_dir_all(&old);
    fs::create_dir_all(&tmp)?;

    for skill in wukong_skills::all() {
        let skill_dir = tmp.join(skill.name);
        fs::create_dir_all(&skill_dir)?;
        fs::write(skill_dir.join("SKILL.md"), skill.content)?;
    }
    fs::write(tmp.join("SOURCE.md"), source)?;

    if root.exists() {
        fs::rename(&root, &old)?;
    }
    if let Err(err) = fs::rename(&tmp, &root) {
        if old.exists() {
            let _ = fs::rename(&old, &root);
        }
        let _ = fs::remove_dir_all(&tmp);
        return Err(err);
    }
    let _ = fs::remove_dir_all(&old);

    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_dir_uses_current_dir_when_env_missing() {
        let temp = tempfile::tempdir().unwrap();
        let original = std::env::current_dir().unwrap();
        std::env::remove_var("WUKONG_WORKSPACE");
        std::env::set_current_dir(temp.path()).unwrap();

        let resolved = resolve_skill_workspace().unwrap();

        std::env::set_current_dir(original).unwrap();
        assert_eq!(resolved, temp.path());
    }

    #[test]
    fn workspace_dir_uses_wukong_workspace_when_set() {
        let temp = tempfile::tempdir().unwrap();
        std::env::set_var("WUKONG_WORKSPACE", temp.path());

        let resolved = resolve_skill_workspace().unwrap();

        std::env::remove_var("WUKONG_WORKSPACE");
        assert_eq!(resolved, temp.path());
    }

    #[test]
    fn materialize_writes_skill_files_under_workspace() {
        let temp = tempfile::tempdir().unwrap();

        let root = materialize_runtime_skills(temp.path()).unwrap();

        assert_eq!(root, temp.path().join(".wukong/skills/superpowers"));
        assert!(root.join("brainstorming/SKILL.md").is_file());
        assert!(root.join("SOURCE.md").is_file());
    }
}
```

Keep the `pub mod skill_assets;` line added during the RED step.

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cargo test -p wukong-runtime skill_assets
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-runtime/src/skill_assets.rs crates/wukong-runtime/src/lib.rs
git commit -m "feat: materialize binary runtime skill assets"
```

---

### Task 3: Make Prompt Builder Use Resolved Skill Root

**Files:**
- Modify: `crates/wukong-runtime/src/persona.rs`
- Modify: `crates/wukong-runtime/src/turn.rs`
- Test: `cargo test -p wukong-runtime persona::tests::build_prompt_with_skill_includes_skill_block` and `cargo test -p wukong-runtime turn::tests::run_turn_injects_planned_skill_into_execute_prompt`

- [ ] **Step 1: Run GitNexus impact before editing `build_prompt_with_skill` and `run_turn_traced`**

Run GitNexus impact analysis for both symbols:

```text
target: build_prompt_with_skill
direction: upstream
file_path: crates/wukong-runtime/src/persona.rs
repo: Wukong
```

```text
target: run_turn_traced
direction: upstream
file_path: crates/wukong-runtime/src/turn.rs
repo: Wukong
```

Expected: record risk, direct callers, and affected processes. If either is HIGH or CRITICAL, stop and ask the user before editing.

- [ ] **Step 2: Write failing test expectations**

Update `build_prompt_with_skill_includes_skill_block` in `crates/wukong-runtime/src/persona.rs` to call:

```rust
let p = build_prompt_with_skill(
    Role::Fixer,
    Some(skill),
    "/tmp/project/.wukong/skills/superpowers".as_ref(),
    &[],
    "fix the bug",
);
```

and assert:

```rust
assert!(
    p.contains("/tmp/project/.wukong/skills/superpowers/test-driven-development/SKILL.md")
);
assert!(!p.contains("/workspace/.wukong/skills/superpowers"));
assert!(!p.contains("Docker runtime"));
```

Update `build_prompt_with_skill_omits_skill_block_when_absent` to pass the same skill root argument.

Update `run_turn_injects_planned_skill_into_execute_prompt` in `crates/wukong-runtime/src/turn.rs` to assert the prompt contains `.wukong/skills/superpowers/test-driven-development/SKILL.md` and does not contain `Docker runtime`.

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
cargo test -p wukong-runtime persona::tests::build_prompt_with_skill_includes_skill_block
cargo test -p wukong-runtime turn::tests::run_turn_injects_planned_skill_into_execute_prompt
```

Expected: FAIL because `build_prompt_with_skill` does not yet accept a skill root argument.

- [ ] **Step 4: Implement prompt root parameter and turn materialization**

In `crates/wukong-runtime/src/persona.rs`, change the signature to:

```rust
pub fn build_prompt_with_skill(
    role: Role,
    skill: Option<&SkillSpec>,
    skill_root: &std::path::Path,
    hits: &[RecallHit],
    input: &str,
) -> String {
```

Inside the skill block, build the file path with:

```rust
let skill_path = skill_root.join(skill.name).join("SKILL.md");
```

and use this format string:

```rust
"\n\n[技能規範指引]\n你必須遵循 `{}` 的流程。請先閱讀並遵循 runtime 已準備好的技能規範文件：\n路徑：{}",
skill.name,
skill_path.display()
```

In `crates/wukong-runtime/src/turn.rs`, before the loop in `run_turn_traced`, add:

```rust
let skill_root = crate::skill_assets::materialize_default_runtime_skills().map_err(|err| {
    WukongError::Backend(wukong_gateway::GatewayError::AgentFailed {
        code: None,
        stderr: format!("failed to prepare runtime skill assets: {err}"),
    })
})?;
```

and update the prompt call to:

```rust
let mut prompt = persona::build_prompt_with_skill(role, skill, &skill_root, &recall.data, &augmented);
```

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cargo test -p wukong-runtime persona::tests::build_prompt_with_skill_includes_skill_block
cargo test -p wukong-runtime turn::tests::run_turn_injects_planned_skill_into_execute_prompt
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/wukong-runtime/src/persona.rs crates/wukong-runtime/src/turn.rs
git commit -m "feat: resolve runtime skill prompt paths"
```

---

### Task 4: Verify Binary and Docker Runtime Skill Assets

**Files:**
- No source edits expected.

- [ ] **Step 1: Run focused Rust tests**

Run:

```bash
cargo test -p wukong-skills -p wukong-runtime
```

Expected: PASS.

- [ ] **Step 2: Run Docker static check**

Run:

```bash
scripts/test-docker-runtime.sh
```

Expected: PASS with `docker runtime persistence checks passed`.

- [ ] **Step 3: Run full workspace tests**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 4: Run GitNexus change detection**

Run GitNexus detect changes:

```text
scope: all
repo: Wukong
```

Expected: changed symbols include the skill asset materializer and prompt/turn symbols. Inspect unexpected high-risk flows before finalizing.

- [ ] **Step 5: Push branch**

```bash
git status --short --branch
git push
```

Expected: clean worktree and branch pushed to `origin/feature/docker-runtime-skill-assets`.

---

## Self-Review

- Spec coverage: workspace resolution, binary materialization, prompt path resolution, Docker compatibility, and verification are all covered.
- Placeholder scan: no TBD/TODO placeholders remain; each code-changing step includes concrete snippets and commands.
- Type consistency: `materialize_runtime_skills` returns a `PathBuf` skill root, and `build_prompt_with_skill` accepts `&Path` for that root throughout the plan.
