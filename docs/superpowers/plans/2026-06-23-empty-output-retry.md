# Empty Output Retry Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent first-install Docker configs and all-empty tool-only turns from producing avoidable `(本回合未產生文字輸出)` replies.

**Architecture:** Keep the existing `run_turn_observed` flow and alter only the all-empty answer-selection branch. Docker defaults are fixed in `.env.example`; runtime repair is implemented inside `crates/wukong-runtime/src/turn.rs` using the same final-step prompt context plus a text-only repair directive.

**Tech Stack:** Rust workspace, async trait-style backend tests, SQLite-backed memory tests, Docker Compose env files.

---

## File Map

- Modify `.env.example`: align first-install `WUKONG_AGENT_CMD` with compose default.
- Modify `crates/wukong-runtime/src/turn.rs`: add a repair prompt helper, preserve final-step context, retry once on all-empty output, and update tests.
- Modify or add a lightweight test in existing Rust tests for repair behavior; no new crate or module is needed.

## Task 1: Docker `.env.example` Default

**Files:**
- Modify: `.env.example:12-14`

- [ ] **Step 1: Write the failing check**

Run:

```bash
test "$(grep '^WUKONG_AGENT_CMD=' .env.example)" = 'WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions'
```

Expected before implementation: command exits non-zero because `.env.example` currently contains `WUKONG_AGENT_CMD=opencode run`.

- [ ] **Step 2: Update `.env.example`**

Change the AI Agent section to:

```env
# ── AI Agent 設定 ──
# 容器內 stdin 為空，opencode 無法互動確認權限；預設自動核准未被 deny 的工具請求。
WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions
```

- [ ] **Step 3: Verify the check passes**

Run:

```bash
test "$(grep '^WUKONG_AGENT_CMD=' .env.example)" = 'WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions'
```

Expected after implementation: command exits zero.

## Task 2: All-Empty Repair Tests

**Files:**
- Modify: `crates/wukong-runtime/src/turn.rs`

- [ ] **Step 1: Add a failing test for repair success**

In the existing `#[cfg(test)] mod tests` in `crates/wukong-runtime/src/turn.rs`, replace the current all-empty sentinel expectation with a new test that scripts an extra backend response:

```rust
#[tokio::test]
async fn run_turn_all_empty_repairs_with_text() {
    let mem = mem().await;
    let backend = MockBackend::new(vec!["fixer", "", "直接回答原問題"]);
    let out = run_turn(&mem, &backend, &test_cfg("project:T"), "你目前是什麼模型", &mut |_| {}, &mut |_| {})
        .await
        .unwrap();

    assert_eq!(out.text, "直接回答原問題");
    assert_eq!(backend.prompts.lock().unwrap().len(), 3);
    let repair_prompt = backend.prompts.lock().unwrap()[2].clone();
    assert!(repair_prompt.contains("[修復回覆]"));
    assert!(repair_prompt.contains("不要呼叫工具"));
    assert!(repair_prompt.contains("你目前是什麼模型"));

    let remembered = mem
        .recall(RecallQuery {
            query: "直接回答原問題".to_string(),
            top_k: 10,
            scope: Some("project:T".to_string()),
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();
    assert!(remembered.data.iter().any(|hit| hit.text.contains("Assistant: 直接回答原問題")));
}
```

- [ ] **Step 2: Add a failing test for repair still empty**

Add this test next to the repair-success test:

```rust
#[tokio::test]
async fn run_turn_all_empty_repair_empty_returns_sentinel() {
    let mem = mem().await;
    let backend = MockBackend::new(vec!["fixer", "", ""]);
    let out = run_turn(&mem, &backend, &test_cfg("project:T"), "go", &mut |_| {}, &mut |_| {})
        .await
        .unwrap();

    assert_eq!(out.text, "(本回合未產生文字輸出)");
    assert_eq!(backend.prompts.lock().unwrap().len(), 3);
}
```

- [ ] **Step 3: Run tests to verify failure**

Run:

```bash
cargo test -p wukong-runtime run_turn_all_empty -- --nocapture
```

Expected before implementation: repair-success test fails because no third backend call is made; sentinel behavior test may fail due prompt count.

## Task 3: Runtime Repair Implementation

**Files:**
- Modify: `crates/wukong-runtime/src/turn.rs:47-158`

- [ ] **Step 1: Preserve final step context**

Inside `run_turn_observed`, introduce storage for the final repair context before the loop:

```rust
let mut final_repair: Option<(Role, Option<String>, String)> = None;
```

Inside the loop, after `prompt` has final directives appended and before calling `backend.run_streaming`, store the final role, final session, and final prompt:

```rust
let session_id = if is_final { stored.clone() } else { None };
if is_final {
    final_repair = Some((role, session_id.clone(), prompt.clone()));
}
```

- [ ] **Step 2: Add repair prompt helper**

Below `run_turn_observed`, add:

```rust
fn append_empty_output_repair_directive(mut prompt: String) -> String {
    prompt.push_str("\n\n[修復回覆]\n");
    prompt.push_str(
        "上一輪沒有產生任何可回覆文字，可能是工具不可用、權限被拒，或只完成了工具呼叫。\n",
    );
    prompt.push_str(
        "這次不要呼叫工具，也不要嘗試讀取環境；請直接根據使用者原問題與已知上下文，用繁體中文給出可交付的文字回覆。",
    );
    prompt
}
```

- [ ] **Step 3: Replace all-empty sentinel selection with repair call**

Replace the current `let answer = if last.output...` block with:

```rust
let answer = if last.output.trim().is_empty() {
    if let Some(existing) = prior.iter().rev().find(|o| !o.output.trim().is_empty()).cloned() {
        existing
    } else if let Some((role, session_id, prompt)) = final_repair {
        let repair = backend
            .run_streaming(
                AgentRequest {
                    prompt: append_empty_output_repair_directive(prompt),
                    session_id: captured_session.clone().or(session_id),
                    thinking: cfg.thinking,
                    model: cfg.default_model.clone(),
                },
                on_event,
            )
            .await?;
        if !repair.text.trim().is_empty() {
            wukong_orchestrator::Outcome { role, output: repair.text }
        } else {
            wukong_orchestrator::Outcome {
                role,
                output: "(本回合未產生文字輸出)".to_string(),
            }
        }
    } else {
        wukong_orchestrator::Outcome {
            role: last.role,
            output: "(本回合未產生文字輸出)".to_string(),
        }
    }
} else {
    last
};
```

- [ ] **Step 4: Run focused runtime tests**

Run:

```bash
cargo test -p wukong-runtime run_turn_all_empty run_turn_falls_back_when_final_output_empty -- --nocapture
```

Expected after implementation: all focused tests pass.

## Task 4: Full Verification

**Files:**
- No additional code files unless verification reveals failures.

- [ ] **Step 1: Run runtime crate tests**

Run:

```bash
cargo test -p wukong-runtime
```

Expected: all `wukong-runtime` tests pass.

- [ ] **Step 2: Run gateway tests if stream parsing changes were avoided**

No gateway code should change. If it did, run:

```bash
cargo test -p wukong-gateway
```

Expected: all `wukong-gateway` tests pass.

- [ ] **Step 3: Run env default check**

Run:

```bash
test "$(grep '^WUKONG_AGENT_CMD=' .env.example)" = 'WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions'
```

Expected: exits zero.

- [ ] **Step 4: Run GitNexus changed-scope check before any commit**

Run GitNexus `detect_changes` for all uncommitted changes.

Expected: changed symbols are limited to `run_turn_observed`/helper tests plus `.env.example` and docs.

## Self-Review Notes

- Spec coverage: `.env.example` default, all-empty retry, no manual `SOUL.md` injection, sentinel last resort, and tests are all covered.
- Placeholder scan: no TBD/TODO placeholders remain.
- Type consistency: plan uses existing `Role`, `AgentRequest`, `RecallQuery`, `RecallMode`, `MockBackend`, and `run_turn` patterns already present in `turn.rs` tests.
