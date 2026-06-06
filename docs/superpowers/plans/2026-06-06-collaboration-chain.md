# 協作鏈(Sequential Collaboration Chain)實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 讓 orchestrator 從「單角色路由」升級為「有序多角色接力鏈」,planner 取代 router 並自動決定鏈,前步輸出累加餵給後步;單角色任務退化為長度 1 的鏈(行為與成本同今日)。

**Architecture:** orchestrator 新增 `planning_prompt`/`parse_chain`/`plan_chain`(對應既有 `routing_prompt`/`parse_role`/`route`)與 `chain_context`/`ChainOutcome`/`orchestrate_chain`;cli `run_turn` 改用 `plan_chain` 逐棒執行,persona prompt 把累加前序協作併進 input。鏈長 cap 3。

**Tech Stack:** Rust 2021、tokio、既有 `AiBackend` trait、clap。

**慣例提醒:** cargo 不在 PATH,指令前綴 `. "$HOME/.cargo/env" &&`;測試+commit 串接時 `set -o pipefail`。**git commit 訊息只寫功能描述,絕不含任何 AI 署名。** `cargo test` 的 TESTNAME 過濾一次只吃一個。

---

## 檔案結構

- `crates/wukong-orchestrator/src/router.rs`(改):新增 `planning_prompt`、`parse_chain`、`plan_chain`(緊鄰既有 routing 對應物)。
- `crates/wukong-orchestrator/src/lib.rs`(改):新增 `ChainOutcome`、`chain_context`、`orchestrate_chain`;更新 `pub use`。
- `crates/wukong-orchestrator/src/main.rs`(改):demo bin 改用 `orchestrate_chain` 印整鏈。
- `crates/wukong-cli/src/lib.rs`(改):`run_turn` 改為協作鏈執行。
- READMEs(改):orchestrator + cli + root。

---

## Task 1: `planning_prompt` 與 `parse_chain`

**Files:**
- Modify: `crates/wukong-orchestrator/src/router.rs`

- [ ] **Step 1: 寫失敗測試**

在 `crates/wukong-orchestrator/src/router.rs` 的 `mod tests` 內新增:

```rust
    #[test]
    fn planning_prompt_lists_roles_and_task() {
        let p = planning_prompt("build and document a parser");
        for role in Role::all() {
            assert!(p.contains(role.name()), "missing role {}", role.name());
        }
        assert!(p.contains("build and document a parser"));
    }

    #[test]
    fn parse_chain_reads_ordered_roles() {
        assert_eq!(
            parse_chain("explorer, fixer, librarian"),
            vec![Role::Explorer, Role::Fixer, Role::Librarian]
        );
    }

    #[test]
    fn parse_chain_orders_by_appearance() {
        // Order follows position in the text, not Role::all() order.
        assert_eq!(parse_chain("先 fixer 再 explorer"), vec![Role::Fixer, Role::Explorer]);
    }

    #[test]
    fn parse_chain_caps_at_three() {
        let c = parse_chain("explorer oracle librarian fixer designer");
        assert_eq!(c.len(), 3);
        assert_eq!(c, vec![Role::Explorer, Role::Oracle, Role::Librarian]);
    }

    #[test]
    fn parse_chain_dedups_repeats() {
        assert_eq!(parse_chain("fixer then fixer again"), vec![Role::Fixer]);
    }

    #[test]
    fn parse_chain_falls_back_to_oracle() {
        assert_eq!(parse_chain("no role mentioned here"), vec![Role::Oracle]);
    }
```

注意:`Role` 的具體變體名(`Explorer`/`Oracle`/`Librarian`/`Fixer`/`Designer`)見 `crates/wukong-orchestrator/src/role.rs`;測試開頭已有 `use super::*;` 帶入 `Role`。

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-orchestrator parse_chain`
Expected: 編譯失敗(`planning_prompt`、`parse_chain` 未定義)。

- [ ] **Step 3: 實作兩個純函式**

在 `crates/wukong-orchestrator/src/router.rs` 的 `parse_role` 之後加:

```rust
/// Build the planning prompt: list roles and ask for an ordered, comma-
/// separated chain (one role for simple tasks, at most three).
pub fn planning_prompt(task: &str) -> String {
    let mut s = String::from(
        "You are a planner. Decide which roles should collaborate on the task, \
         in execution order.\nRoles:\n",
    );
    for role in Role::all() {
        s.push_str(&format!("- {}: {}\n", role.name(), role.description()));
    }
    s.push_str(
        "\nReply with ONLY a comma-separated list of role names in execution order \
         (lowercase). Use a single role for simple tasks; at most three. No explanation.\n\n[Task]\n",
    );
    s.push_str(task);
    s
}

/// Parse an ordered role chain from the reply. Each role is matched by the
/// earliest position its name appears (case-insensitive); roles are ordered by
/// that position, deduped, and capped at three. Empty match falls back to a
/// single Oracle.
pub fn parse_chain(response: &str) -> Vec<Role> {
    let lower = response.to_lowercase();
    let mut found: Vec<(usize, Role)> = Role::all()
        .into_iter()
        .filter_map(|role| lower.find(role.name()).map(|pos| (pos, role)))
        .collect();
    found.sort_by_key(|(pos, _)| *pos);
    let chain: Vec<Role> = found.into_iter().map(|(_, r)| r).take(3).collect();
    if chain.is_empty() {
        vec![Role::Oracle]
    } else {
        chain
    }
}
```

- [ ] **Step 4: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-orchestrator`
Expected: 全綠(新增 6 測試通過,既有 router 測試不受影響)。

- [ ] **Step 5: commit**

```bash
set -o pipefail
git add crates/wukong-orchestrator/src/router.rs
git commit -m "feat(orchestrator): planning prompt and ordered chain parser"
```

---

## Task 2: `ChainOutcome` 與 `chain_context`

**Files:**
- Modify: `crates/wukong-orchestrator/src/lib.rs`

- [ ] **Step 1: 寫失敗測試**

在 `crates/wukong-orchestrator/src/lib.rs` 的 `mod tests` 內新增:

```rust
    #[test]
    fn chain_context_empty_for_no_prior() {
        assert_eq!(chain_context(&[]), "");
    }

    #[test]
    fn chain_context_includes_roles_and_outputs() {
        let prior = vec![
            Outcome { role: Role::Explorer, output: "找到了問題".to_string() },
            Outcome { role: Role::Fixer, output: "已修正".to_string() },
        ];
        let c = chain_context(&prior);
        assert!(c.contains("前序協作"));
        assert!(c.contains("explorer"));
        assert!(c.contains("找到了問題"));
        assert!(c.contains("fixer"));
        assert!(c.contains("已修正"));
    }

    #[test]
    fn chain_outcome_final_is_last_step() {
        let co = ChainOutcome {
            steps: vec![
                Outcome { role: Role::Explorer, output: "a".to_string() },
                Outcome { role: Role::Fixer, output: "b".to_string() },
            ],
        };
        assert_eq!(co.final_output(), "b");
    }

    #[test]
    fn chain_outcome_final_empty_for_no_steps() {
        let co = ChainOutcome { steps: vec![] };
        assert_eq!(co.final_output(), "");
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-orchestrator chain_context`
Expected: 編譯失敗(`chain_context`、`ChainOutcome` 未定義)。

- [ ] **Step 3: 實作**

在 `crates/wukong-orchestrator/src/lib.rs` 的 `Outcome` 定義之後加:

```rust
/// The full result of a collaboration chain: every step in order.
#[derive(Debug, Clone)]
pub struct ChainOutcome {
    pub steps: Vec<Outcome>,
}

impl ChainOutcome {
    /// The last step's output; empty string for an empty chain.
    pub fn final_output(&self) -> &str {
        self.steps.last().map(|o| o.output.as_str()).unwrap_or("")
    }
}

/// Render prior chain steps as a context block to prepend onto the next step's
/// task. Empty slice yields an empty string (so the first step is unchanged).
pub fn chain_context(prior: &[Outcome]) -> String {
    if prior.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\n[前序協作]\n");
    for o in prior {
        s.push_str(&format!("{}: {}\n", o.role.name(), o.output));
    }
    s
}
```

- [ ] **Step 4: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-orchestrator`
Expected: 全綠。

- [ ] **Step 5: commit**

```bash
set -o pipefail
git add crates/wukong-orchestrator/src/lib.rs
git commit -m "feat(orchestrator): ChainOutcome and chain context rendering"
```

---

## Task 3: `plan_chain` 與 `orchestrate_chain`

**Files:**
- Modify: `crates/wukong-orchestrator/src/router.rs`
- Modify: `crates/wukong-orchestrator/src/lib.rs`

- [ ] **Step 1: 寫 `plan_chain` 失敗測試**

在 `crates/wukong-orchestrator/src/router.rs` 的 `mod tests` 內新增。先確認該測試模組是否有可用的 MockBackend;router.rs 既有測試只測純函式,沒有 MockBackend。為避免重複,這個 backend 互動測試放到 lib.rs(已有 MockBackend)。**本步改為在 `lib.rs` 的 `mod tests` 新增**:

```rust
    #[tokio::test]
    async fn plan_chain_parses_backend_reply() {
        let backend = MockBackend::new(&["explorer, fixer"]);
        let chain = plan_chain(&backend, "build a thing").await.unwrap();
        assert_eq!(chain, vec![Role::Explorer, Role::Fixer]);
    }

    #[tokio::test]
    async fn orchestrate_chain_runs_each_role_in_order() {
        // [0] planner reply, [1] explorer output, [2] fixer output.
        let backend = MockBackend::new(&["explorer, fixer", "找到了", "修好了"]);
        let co = orchestrate_chain(&backend, "fix it").await.unwrap();
        assert_eq!(co.steps.len(), 2);
        assert_eq!(co.steps[0].role, Role::Explorer);
        assert_eq!(co.steps[1].role, Role::Fixer);
        assert_eq!(co.final_output(), "修好了");

        // The second step's prompt carries the first step's output.
        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 3); // plan + 2 executes
        assert!(prompts[2].contains("找到了"));
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-orchestrator orchestrate_chain_runs`
Expected: 編譯失敗(`plan_chain`、`orchestrate_chain` 未定義)。

- [ ] **Step 3: 實作 `plan_chain`(router.rs)**

在 `crates/wukong-orchestrator/src/router.rs` 的 `route` 之後加:

```rust
/// Phase 1 (chain): ask the backend for an ordered role chain.
pub async fn plan_chain(backend: &impl AiBackend, task: &str) -> Result<Vec<Role>, OrchestratorError> {
    let resp = backend
        .run(AgentRequest {
            prompt: planning_prompt(task),
            continue_session: false,
        })
        .await?;
    Ok(parse_chain(&resp.text))
}
```

- [ ] **Step 4: 實作 `orchestrate_chain`(lib.rs)**

在 `crates/wukong-orchestrator/src/lib.rs` 的 `orchestrate` 之後加:

```rust
/// Plan a role chain, then run each role in order, accumulating prior outputs
/// into each step's prompt. Makes 1 planning call + one call per role.
pub async fn orchestrate_chain(
    backend: &impl AiBackend,
    task: &str,
) -> Result<ChainOutcome, OrchestratorError> {
    let roles = router::plan_chain(backend, task).await?;
    let mut steps: Vec<Outcome> = Vec::new();
    for role in roles {
        let prompt = format!("{}\n\n[任務]\n{}{}", role.card(), task, chain_context(&steps));
        let resp = backend
            .run(AgentRequest { prompt, continue_session: false })
            .await?;
        steps.push(Outcome { role, output: resp.text });
    }
    Ok(ChainOutcome { steps })
}
```

注意:lib.rs 目前以 `use router::{parse_role, route, routing_prompt};`(經由 `pub use`)引用。`plan_chain` 以 `router::plan_chain` 完整路徑呼叫即可,免動 import;或在 Task 4 一併加進 `pub use`。

- [ ] **Step 5: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-orchestrator`
Expected: 全綠。

- [ ] **Step 6: commit**

```bash
set -o pipefail
git add crates/wukong-orchestrator/src/router.rs crates/wukong-orchestrator/src/lib.rs
git commit -m "feat(orchestrator): plan_chain and orchestrate_chain"
```

---

## Task 4: 匯出與 demo bin

**Files:**
- Modify: `crates/wukong-orchestrator/src/lib.rs`
- Modify: `crates/wukong-orchestrator/src/main.rs`

- [ ] **Step 1: 更新 `pub use`**

在 `crates/wukong-orchestrator/src/lib.rs` 把:

```rust
pub use router::{parse_role, route, routing_prompt};
```

改為:

```rust
pub use router::{parse_chain, parse_role, plan_chain, planning_prompt, route, routing_prompt};
```

`ChainOutcome`、`chain_context`、`orchestrate_chain` 已是 lib.rs 內 `pub`,自動對外可見。

- [ ] **Step 2: demo bin 改用 `orchestrate_chain`**

在 `crates/wukong-orchestrator/src/main.rs`:

把 `use wukong_orchestrator::orchestrate;` 改為:

```rust
use wukong_orchestrator::orchestrate_chain;
```

把 `match orchestrate(...)` 區塊改為:

```rust
    match orchestrate_chain(&backend, &cli.task.join(" ")).await {
        Ok(chain) => {
            for step in &chain.steps {
                eprintln!("[role: {}]", step.role.name());
            }
            println!("{}", chain.final_output());
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
```

- [ ] **Step 3: 編譯並跑測試**

Run: `. "$HOME/.cargo/env" && cargo build -p wukong-orchestrator && cargo test -p wukong-orchestrator`
Expected: 編譯成功、測試全綠。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-orchestrator/src/lib.rs crates/wukong-orchestrator/src/main.rs
git commit -m "feat(orchestrator): export chain API and run chain in demo bin"
```

---

## Task 5: cli `run_turn` 改為協作鏈

**Files:**
- Modify: `crates/wukong-cli/src/lib.rs`

- [ ] **Step 1: 寫多角色失敗測試**

在 `crates/wukong-cli/src/lib.rs` 的 `mod tests` 內新增(既有單角色測試 `run_turn_routes_executes_and_persists`、`execution_prompt_carries_persona_and_role` 保留不動,作為向後相容驗證):

```rust
    #[tokio::test]
    async fn run_turn_runs_multi_role_chain() {
        let mem = open_memory().await;
        // [0] planner -> explorer,fixer ; [1] explorer output ; [2] fixer output
        let backend = MockBackend::new(&["explorer, fixer", "f1", "f2"]);
        let mut roles_seen: Vec<Role> = Vec::new();
        let out = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "build and fix",
            &mut |_| {},
            &mut |r| roles_seen.push(r),
        )
        .await
        .unwrap();

        // on_role fired once per step, in order.
        assert_eq!(roles_seen, vec![Role::Explorer, Role::Fixer]);
        // Final output is the last step.
        assert_eq!(out.text, "f2");
        assert_eq!(out.role, Role::Fixer);

        // Second execute prompt carries the first step's output.
        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 3); // plan + explorer + fixer
        assert!(prompts[2].contains("f1"));

        // Memory stored the user input and the FINAL assistant output only.
        drop(prompts);
        let r = mem
            .recall(RecallQuery {
                query: "build and fix".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("Assistant: f2")));
        assert!(!r.data.iter().any(|h| h.text.contains("Assistant: f1")));
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-cli run_turn_runs_multi_role_chain`
Expected: FAIL(目前 `run_turn` 只跑單角色,`roles_seen` 只會有一個、`prompts.len()` 為 2)。

- [ ] **Step 3: 改寫 `run_turn` 主體**

把 `crates/wukong-cli/src/lib.rs` 中 `run_turn` 的「步驟 2~5」(從 `// 2. Route the task to a role.` 到函式結尾 `Ok(TurnOutput { ... })`)替換為:

```rust
    // 2. Plan an ordered role chain (replaces single-role routing).
    let roles = wukong_orchestrator::plan_chain(backend, input).await?;

    // 3. Run each role in order, accumulating prior outputs into the prompt.
    let mut prior: Vec<wukong_orchestrator::Outcome> = Vec::new();
    let mut first = true;
    for role in roles {
        on_role(role);
        let augmented = format!("{input}{}", wukong_orchestrator::chain_context(&prior));
        let prompt = persona::build_prompt(role, &recall.data, &augmented);
        let resp = backend
            .run_streaming(
                AgentRequest {
                    prompt,
                    // Only the first step honors the caller's session continuation;
                    // intra-chain context is passed via the prompt, not the session.
                    continue_session: first && cfg.continue_session,
                },
                on_event,
            )
            .await?;
        prior.push(wukong_orchestrator::Outcome { role, output: resp.text });
        first = false;
    }

    // 4. Final output = last step. Fall back safely if the chain was empty.
    let last = prior
        .last()
        .cloned()
        .unwrap_or(wukong_orchestrator::Outcome { role: Role::Oracle, output: String::new() });

    // 5. Persist the turn: user input + the final assistant output only.
    memory
        .remember(RememberInput {
            scope: cfg.scope.clone(),
            session_id: None,
            items: vec![
                MemoryItem {
                    kind: MemoryKind::Event,
                    text: format!("User: {input}"),
                    importance: None,
                },
                MemoryItem {
                    kind: MemoryKind::Event,
                    text: format!("Assistant: {}", last.output),
                    importance: None,
                },
            ],
        })
        .await?;

    Ok(TurnOutput {
        role: last.role,
        text: last.output,
    })
```

注意:`Outcome` 需 `Clone`(orchestrator 已 `#[derive(Debug, Clone)]`)。`Role` 已在檔頭 `use wukong_orchestrator::Role;`。

- [ ] **Step 4: 跑全 cli 測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-cli`
Expected: 全綠 —— 新的多角色測試通過,且既有單角色測試 `run_turn_routes_executes_and_persists`(腳本 `["fixer","done"]` → 鏈 `[Fixer]`、`prompts.len()==2`、`out.text=="done"`)與 `execution_prompt_carries_persona_and_role` 仍通過。

- [ ] **Step 5: commit**

```bash
set -o pipefail
git add crates/wukong-cli/src/lib.rs
git commit -m "feat(cli): run turns as a sequential collaboration chain"
```

---

## Task 6: clippy 與文件

**Files:**
- Modify: `README.md`
- Modify: `crates/wukong-orchestrator/README.md`
- Modify: `crates/wukong-cli/README.md`

- [ ] **Step 1: clippy 全綠**

Run: `. "$HOME/.cargo/env" && cargo clippy --all-targets -- -D warnings`
Expected:零警告。若有,逐一修正後再跑。

- [ ] **Step 2: 真實 opencode 煙霧測試(手動)**

```bash
. "$HOME/.cargo/env"
TMP=$(mktemp -d)
export WUKONG_MEMORY_DB="sqlite://$TMP/m.db"
# 簡單任務:應為單角色(plan 回一個)
cargo run -q -p wukong-cli -- --scope project:Demo "什麼是 BM25?"
echo "----- 複雜任務(可能成鏈)-----"
cargo run -q -p wukong-cli -- --scope project:Demo "探查這個 repo 的記憶層架構，找出可改進處，並寫一段文件說明"
```
Expected:簡單任務 stderr 只見一個 `🐵 悟空·<role>`;複雜任務可能見多個角色 header 依序出現,最後 stdout 為最後一棒輸出。觀察 planner 是否回多角色、接力 context 是否生效。

- [ ] **Step 3: 更新文件**

- `crates/wukong-orchestrator/README.md`:新增「協作鏈」段,說明 `plan_chain`/`orchestrate_chain`/`ChainOutcome`/`chain_context`、cap 3、fallback Oracle。
- `crates/wukong-cli/README.md`:在「進入點/活動渲染」附近說明每回合改走協作鏈(planner 自動決定,簡單任務單角色;複雜任務最多 3 棒,每棒印角色 header)。
- `README.md`:Roadmap 把「平行多角色調度…」拆解,標記協作鏈已完成(`✅`),並在資料流/簡介適當處點出多角色接力。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add README.md crates/wukong-orchestrator/README.md crates/wukong-cli/README.md
git commit -m "docs: document sequential collaboration chain"
```

---

## 完成後

依 `superpowers:finishing-a-development-branch`:跑全測試 → 呈現 4 選項。合併後比照 v0.2/v0.3/v0.4 慣例詢問是否開 **v0.5.0 release**。

## 自我複查紀錄

- **Spec 覆蓋:** `planning_prompt`/`parse_chain`(T1)、`ChainOutcome`/`chain_context`(T2)、`plan_chain`/`orchestrate_chain`(T3)、匯出+demo bin(T4)、`run_turn` 改寫含 session 接續與只存最終輸出(T5)、文件(T6)。spec 每節皆有對應 task。
- **型別一致:** `Outcome { role, output }`(既有,T2/T3/T5 使用)、`ChainOutcome { steps }` + `final_output()`(T2 定義,T3/T5 使用)、`chain_context(&[Outcome]) -> String`(T2 定義,T3/T5 使用)、`plan_chain`/`parse_chain`/`planning_prompt`(T1/T3)。`TurnOutput { role, text }` 不變。
- **向後相容:** 既有單角色 run_turn 測試(`["fixer","done"]`)在新鏈邏輯下仍成立(鏈長 1、prompts==2)。
- **前向引用:** T3 plan_chain 測試放在 lib.rs(有 MockBackend);T3 orchestrate_chain 以 `router::plan_chain` 完整路徑呼叫,T4 再加進 `pub use`。
