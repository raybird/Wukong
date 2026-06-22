# 協作鏈(Sequential Collaboration Chain)設計

**日期:** 2026-06-06
**狀態:** 已核可(roadmap 項目 E,聚焦「協作鏈」子能力)
**前置:** v1 orchestrator 單角色路由(`route`/`parse_role`)、cli `run_turn`。

## 目標

把 orchestrator 從「單一任務 → 單一角色」升級為「單一任務 → 有序多角色接力」。一個任務可由數個角色依序協作:前一棒的輸出累加成後一棒的 context。簡單任務退化為長度 1 的鏈,行為與成本與今日相同。

E 的另外兩個子能力(平行 fan-out、技能路由)**不在本 spec 範圍**,各自之後再開 spec。

## 設計原則

- **最小擴充、向後相容**:`route`(單角色)由 `plan_chain`(有序清單)取代;`Outcome`、`TurnOutput` 型別維持不變。
- **planner 自動決定**:每回合一次 LLM 規劃呼叫(取代既有 router),回傳有序角色清單。單角色 = 今日行為。
- **成本上限**:鏈長 cap 3。每棒一次 opencode execute。
- **底層 agent 只以 opencode 為準。**

## 架構總覽

```
任務 ──► plan_chain (1 LLM call) ──► [role₁, role₂, ...]  (cap 3)
         │
         ▼  依序執行,前步輸出累加
       role₁ execute ──► out₁
       role₂ execute (task + chain_context([out₁])) ──► out₂
       role₃ execute (task + chain_context([out₁,out₂])) ──► out₃
         │
         ▼
       最終輸出 = 最後一棒 (out₃)
```

兩個消費者:
1. **orchestrator demo bin**(`wukong-orchestrate`):直接呼叫 `orchestrate_chain`(無 persona、無記憶)。
2. **cli `run_turn`**:自行組 persona + 記憶 prompt,逐棒呼叫 `run_streaming`。

## orchestrator crate

新增於 `crates/wukong-orchestrator/src/lib.rs`(或新模組 `chain.rs`),保留既有 `route`/`parse_role`/`orchestrate`。

```rust
/// 一條協作鏈的完整結果。
#[derive(Debug, Clone)]
pub struct ChainOutcome {
    pub steps: Vec<Outcome>,   // Outcome { role, output } 已存在
}

impl ChainOutcome {
    /// 最後一棒的輸出;空鏈回空字串。
    pub fn final_output(&self) -> &str {
        self.steps.last().map(|o| o.output.as_str()).unwrap_or("")
    }
}

/// 規劃 prompt:要模型回傳有序、逗號分隔的角色清單。
pub fn planning_prompt(task: &str) -> String;

/// 從回應解析有序角色鏈:掃描五角色名最早出現位置 → 排序取前 3(天然去重);
/// 找不到任何角色 → vec![Role::Oracle]。
pub fn parse_chain(response: &str) -> Vec<Role>;

/// 一次 LLM call 規劃角色鏈。
pub async fn plan_chain(backend: &impl AiBackend, task: &str) -> Result<Vec<Role>, OrchestratorError>;

/// 把前序步驟渲染成 context 區塊;空 prior 回空字串。
/// 格式:"\n\n[前序協作]\n{role}: {output}\n{role}: {output}"
pub fn chain_context(prior: &[Outcome]) -> String;

/// demo bin 用:規劃後逐棒執行(execution_prompt + chain_context),回整鏈。
pub async fn orchestrate_chain(backend: &impl AiBackend, task: &str) -> Result<ChainOutcome, OrchestratorError>;
```

### `parse_chain` 規則

- 對 `Role::all()` 每個角色,找其名稱(`explorer`/`oracle`/`librarian`/`fixer`/`designer`,大小寫不敏感)在回應中的最早 byte 位置。
- 有出現的角色依位置升序排序,取前 3。
- 全無 → `vec![Role::Oracle]`。
- 天然去重(每角色只計最早一次)。

### `planning_prompt` 內容

要點:列出五角色與職責;要求「依執行順序回傳逗號分隔的角色名,簡單任務只回一個,最多三個,不要解釋」。沿用 `routing_prompt` 的語氣與角色描述來源(`Role::description`)。

### `orchestrate_chain` 流程

```rust
let roles = plan_chain(backend, task).await?;
let mut steps: Vec<Outcome> = Vec::new();
for role in roles {
    let prompt = format!("{}\n\n[任務]\n{}{}", role.card(), task, chain_context(&steps));
    let resp = backend.run(AgentRequest { prompt, continue_session: false }).await?;
    steps.push(Outcome { role, output: resp.text });
}
Ok(ChainOutcome { steps })
```

## cli `run_turn` 改寫

`crates/wukong-cli/src/lib.rs` 的 `run_turn`:

1. **recall** 一次(維持),供整鏈共用 `recall.data`。
2. `let roles = wukong_orchestrator::plan_chain(backend, input).await?;`(取代 `route`)。
3. 逐棒:
   ```rust
   let mut prior: Vec<Outcome> = Vec::new();
   let mut first = true;
   for role in roles {
       on_role(role);
       let augmented = format!("{input}{}", chain_context(&prior));
       let prompt = persona::build_prompt(role, &recall.data, &augmented);
       let resp = backend.run_streaming(
           AgentRequest { prompt, continue_session: first && cfg.continue_session },
           on_event,
       ).await?;
       prior.push(Outcome { role, output: resp.text });
       first = false;
   }
   ```
   - **session 接續**:只有第一棒用 `cfg.continue_session`(尊重 REPL/`-c`),後續棒一律 `false`(鏈內 context 由 prompt 傳遞)。
4. **remember**:`User: input` + `Assistant: <prior 最後一棒 output>`(維持今日記憶形狀)。
5. 回傳 `TurnOutput { role: 最後一棒角色, text: 最後一棒 output }`(型別不變)。

`persona::build_prompt` **不需修改**:把累加的前序協作併進 `input` 參數即可。

## 成本與使用者體驗

- **簡單任務**:planner 回單角色 → 1 plan + 1 execute,與今日 2 次呼叫同。
- **複雜任務**:最多 1 plan + 3 execute。每棒在 stderr 印自己的角色 header(`🐵 悟空·explorer` → `…·fixer` → `…·librarian`),使用者看得到接力。

## 測試策略

沿用既有 `MockBackend`(腳本回應 + 記錄 prompts)。

- `parse_chain`:
  - `"explorer, fixer, librarian"` → `[Explorer, Fixer, Librarian]`。
  - 順序依出現位置:`"先 fixer 再 explorer"` → `[Fixer, Explorer]`。
  - 超過 3 個 → 取前 3。
  - 重複角色只計一次。
  - 無角色字串 → `[Oracle]`。
- `chain_context`:空 → `""`;含兩步 → 含兩個 role 名與其 output。
- `orchestrate_chain`:腳本 `["explorer,fixer", "找到了", "修好了"]` → `steps.len()==2`、`final_output()=="修好了"`、第二棒 prompt 含 `"找到了"`。
- `run_turn`:
  - 單角色腳本 `["fixer", "done"]` → 仍通過(`out.role==Fixer`、`out.text=="done"`、記憶落 User+done)。
  - 多角色腳本 `["explorer,fixer", "f1", "f2"]` → `on_role` 被呼叫 2 次、`out.text=="f2"`、remember 的 Assistant 為 `f2`、第二棒 prompt 含 `f1`。

## 非目標(YAGNI)

- 不做平行 fan-out。
- 不做技能路由(角色已是粗粒度技能)。
- 不做鏈內動態重規劃(planner 一次定案)。
- 記憶只存最終輸出,不存每棒中間結果。**(2026-06-22 修訂放寬,見文末)**
- 不改 `TurnOutput` / `Outcome` 型別形狀。

## 修訂(2026-06-22):末棒空輸出回退

詳見 `2026-06-22-final-output-fallback-design.md`。本 spec 的兩條不變式調整如下:

- **「最終輸出 = 最後一棒」→「最後一棒非空輸出」**:當末棒為 executor、只用工具收尾未吐文字時,`run_turn` 回退取最近一棒非空輸出;全空才回 sentinel `(本回合未產生文字輸出)`。
- **「記憶只存最終輸出,不存每棒中間結果」放寬**:末棒空時,回退存最近非空的中間棒輸出(因為此時它就是使用者實際看到的答覆),以避免空字串污染記憶與對話歷史。

另於最後一棒常駐注入 `[輸出要求]`(`persona::final_answer_directive`),要求即使全程用工具也要文字總結,降低 executor 靜默收尾。
