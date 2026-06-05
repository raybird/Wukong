# wukong-orchestrator v1 設計

> 子專案 3／4 ──「七十二變・分身」：角色調度引擎
> 日期：2026-06-05
> 狀態：已核可，待轉實作計畫

## 背景與定位

「孫悟空」四柱中的第三柱。柱 1 `wukong-memory`、柱 2 `wukong-gateway` 已完成並合併 `main`。本柱是「七十二變」── 把一項任務交給最合適的專家角色處理。

| 子專案 | 神話對應 | 概念來源 | 狀態 |
|--------|---------|---------|------|
| 1. `wukong-memory` | 鬥戰勝佛／本我 | Memoria | ✅ 完成 |
| 2. `wukong-gateway` | 齊天大聖／肉身 | TeleNexus | ✅ 完成 |
| **3. `wukong-orchestrator`** | 七十二變／分身 | tao-of-coding | ← 本文件 |
| 4. `wukong`（金箍棒） | 修成正果 | 三者融合 | 待做 |

## 目標與非目標

### v1 目標（最小「七十二變」核心）
兩段式 LLM 自動路由：

> 第 1 次 agent 呼叫判斷該用哪個角色 → 第 2 次以該角色卡執行任務 → 回傳（角色, 輸出）

- 五角色（取自 tao-of-coding）：Explorer / Oracle / Librarian / Fixer / Designer，角色卡以精簡常數內嵌
- 路由：LLM 選角色 + 關鍵字解析，解析失敗 fallback `Oracle`
- 透過 `wukong-gateway` 的 `AiBackend` trait 執行（單向依賴 gateway，不改動 gateway，無循環相依）
- 對外形態：函式庫 crate + 薄 demo bin `wukong-orchestrate`

### v1 非目標（延後 v2+）
- 記憶 recall/remember（留給 gateway / 柱 4 金箍棒整合）
- 平行多角色調度、角色協作鏈（主責+協作）
- tao 的技能路由（brainstorming/TDD… 對應技能）
- 每角色不同模型分配
- 接入統一 `wukong` CLI（柱 4 負責）

## 技術棧

Rust + tokio + clap（derive）+ thiserror。`wukong-orchestrator` 以 path 依賴 `wukong-gateway`（取 `AiBackend` / `AgentRequest` / `AgentResponse` / `AgentCliBackend` / `GatewayError`）。

## 架構：crate 佈局

新增第四個 crate（lib + bin `wukong-orchestrate`），加入既有 workspace：

```
crates/wukong-orchestrator/
├── Cargo.toml          # lib + [[bin]] name="wukong-orchestrate"
└── src/
    ├── lib.rs          # 模組接線 + 重新匯出 + orchestrate()
    ├── role.rs         # Role enum + 五角色卡（內嵌精簡）
    ├── router.rs       # routing_prompt + parse_role + route()
    ├── error.rs        # OrchestratorError
    └── main.rs         # 薄 demo bin
```

職責單一：`role` 是純資料、`router` 管「選角色」、`lib::orchestrate()` 串起兩段、`error` 收斂錯誤。所有邏輯吃 `AiBackend` trait，可用 mock backend 測試。

依賴方向：`wukong-orchestrator → wukong-gateway`（單向）。gateway 不反向依賴 orchestrator，無循環。

## Role 與角色卡

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Explorer,
    Oracle,
    Librarian,
    Fixer,
    Designer,
}

impl Role {
    /// 五角色固定陣列。
    pub fn all() -> [Role; 5];
    /// 小寫 id："explorer"/"oracle"/"librarian"/"fixer"/"designer"。
    pub fn name(&self) -> &'static str;
    /// 一行職掌描述，供路由 prompt 列出。
    pub fn description(&self) -> &'static str;
    /// 角色系統 prompt（精簡，內嵌常數）。
    pub fn card(&self) -> &'static str;
}
```

角色卡內嵌精簡版（取自 tao-of-coding 角色目錄）：
- **Explorer**：結構洞察，快速掃描專案結構、理解檔案關聯與依賴。
- **Oracle**：架構專家，擅長重構、決策分析與技術取捨。
- **Librarian**：文件專家，負責撰寫文件、翻譯與註解。
- **Fixer**：實作專家，程式碼修正、單元測試補全、語法修正，高效交付可運作的程式。
- **Designer**：設計專家，負責 UI/UX 與前端體驗。

## Router（選角色）

```rust
/// 列出五角色 + description，要求 LLM「只回一個角色名」。
pub fn routing_prompt(task: &str) -> String;

/// 大小寫無關掃描回應；依 Role::all() 順序回傳第一個其 name 出現在
/// 回應中的 Role；都沒中 → fallback Role::Oracle。
pub fn parse_role(response: &str) -> Role;

/// 第 1 段：呼叫 backend 取得路由結果。
pub async fn route(backend: &impl AiBackend, task: &str) -> Result<Role, OrchestratorError>;
```

`route` 以 `backend.run(AgentRequest { prompt: routing_prompt(task), continue_session: false })` 取得回應，再 `parse_role(&resp.text)`。fallback 預設 `Oracle`（通用分析角色）。

`parse_role` 以 `Role::all()` 的順序掃描 `response.to_lowercase()`，第一個 `contains(role.name())` 命中者勝（五角色名互不為子字串，無歧義）。

## Orchestrate（兩段式）

```rust
pub struct Outcome {
    pub role: Role,
    pub output: String,
}

/// 組執行 prompt：role.card() + "\n\n[任務]\n" + task。
pub fn execution_prompt(role: Role, task: &str) -> String;

pub async fn orchestrate(
    backend: &impl AiBackend,
    task: &str,
) -> Result<Outcome, OrchestratorError> {
    let role = route(backend, task).await?;             // 第 1 次呼叫：選角色
    let prompt = execution_prompt(role, task);
    let resp = backend
        .run(AgentRequest { prompt, continue_session: false })
        .await?;                                         // 第 2 次：以角色執行
    Ok(Outcome { role, output: resp.text })
}
```

兩段都 `continue_session: false`、不碰記憶。

## 錯誤處理

`OrchestratorError`（thiserror）：
- `Backend(#[from] wukong_gateway::GatewayError)`

## demo bin（`wukong-orchestrate`）

```
wukong-orchestrate [--agent-cmd "opencode run"] <task>...
```

- 任務為位置參數（trailing），以空白接回
- `--agent-cmd` ／ `WUKONG_AGENT_CMD` ／ 預設 `["opencode","run"]`，空白切分成 argv
- 組 `AgentCliBackend { command, continue_args: vec![] }`
- 呼叫 `orchestrate`，把選到的角色印到 stderr（`[role: fixer]`），輸出印到 stdout
- 錯誤 → stderr + `exit(1)`

## 測試（TDD）

- `role.rs`：五角色 `name()` 互異、`card()` 與 `description()` 非空
- `router.rs`：
  - `routing_prompt` 含五角色名與任務文字
  - `parse_role`：`"fixer"`→Fixer、`"FIXER"`→Fixer、`"I'd pick oracle"`→Oracle、`"garbage"`→Oracle（fallback）
- `lib.rs`（mock backend，腳本化兩段回應）：
  - mock 第 1 次回 `"fixer"`、第 2 次回 `"done"`
  - 斷言 `Outcome.role == Fixer`、`output == "done"`
  - 斷言第 2 次呼叫收到的 prompt 含 Fixer 角色卡片段

Mock backend：測試內定義實作 `AiBackend` 的型別，用 `Mutex<VecDeque<String>>` 依序吐回應、`Mutex<Vec<String>>` 記錄收到的 prompts。

## 驗收標準

1. `cargo test` 全綠（單元 + mock 整合）
2. `cargo clippy --all-targets -- -D warnings` 乾淨
3. `wukong-orchestrate --agent-cmd "printf fixer" "fix the bug"` 印出 `[role: fixer]` 並跑完兩段（用 `printf fixer` 當假 agent，路由穩定回 Fixer）
4. `parse_role` 解析失敗時 fallback `Oracle`
5. orchestrate 對 backend 進行剛好兩次呼叫（路由 + 執行）
