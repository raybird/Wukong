# wukong (金箍棒) v1 設計

> 子專案 4／4 ──「修成正果」：統一 CLI，把三柱合為一個產品
> 日期：2026-06-05
> 狀態：已核可，待轉實作計畫

## 背景與定位

「孫悟空」四柱的最後一柱（金箍棒）。前三柱已完成並合併 `main`。本柱把三者綁成單一 `wukong` CLI：一個有人格、有記憶、會按任務變出專家角色的助手。

| 子專案 | 神話對應 | 狀態 |
|--------|---------|------|
| 1. `wukong-memory` | 鬥戰勝佛／本我 | ✅ 完成 |
| 2. `wukong-gateway` | 齊天大聖／肉身 | ✅ 完成 |
| 3. `wukong-orchestrator` | 七十二變／分身 | ✅ 完成 |
| **4. `wukong`（金箍棒）** | 修成正果 | ← 本文件 |

## 目標與非目標

### v1 目標
統一 CLI 的「全合體」：每回合 recall → 路由選角色 → 以「人格 + 角色 + 記憶」執行 → remember。

- 新建頂層 crate `wukong-cli`（lib + bin `wukong`），依賴三柱（單向、無循環）
- 孫悟空人格：系統 prompt 前置 + 輕量點綴頭（`🐵 悟空·{role}`）
- 角色路由預設開（每回合先 `route` 再執行）
- 記憶 scope 依工作目錄分專案（沿用 gateway config）
- 最大重用：沿用 gateway 的 `cli::Cli` 與 `config::GatewayConfig`

### v1 非目標（延後 v2+）
- Telegram bot、Web Console、排程、可觀測性（TeleNexus 完整願景）
- 互動 REPL、串流輸出
- 可設定/多重人格、每角色不同模型
- 路由可開關旗標（v1 固定全合體）

## 技術棧

Rust + tokio + clap（derive）+ thiserror。`wukong-cli` 以 path 依賴 `wukong-memory` / `wukong-gateway` / `wukong-orchestrator`。

## 架構：crate 佈局與撞名解法

新建頂層 crate `wukong-cli`（lib + bin `wukong`）。**移除 gateway 既有的 `wukong` bin**（gateway 改為純 lib，其 `cli`/`config`/`backend`/`prompt` 模組仍公開供重用），避免 workspace 內兩個 `wukong` binary 撞名。

```
crates/wukong-cli/
├── Cargo.toml          # [lib] wukong_cli + [[bin]] name="wukong"
└── src/
    ├── lib.rs          # run_turn（合體管線）+ WukongError + TurnOutput + 重新匯出
    ├── persona.rs      # WUKONG_PERSONA 常數 + build_prompt()
    └── main.rs         # 薄殼：解析 → 開記憶 → 跑 run_turn → 印出

# 同時修改：
crates/wukong-gateway/Cargo.toml   # 移除 [[bin]] 區段
crates/wukong-gateway/src/main.rs  # 刪除
```

依賴方向：`wukong-cli → { wukong-memory, wukong-gateway, wukong-orchestrator }`（單向，無循環）。gateway 的 `pipeline::run_turn`（簡單記憶+backend 回合）保留為 lib API（已有測試），v1 不再有 bin 使用它。

最大重用：沿用 gateway 的 `cli::Cli`（旗標：prompt、`-c`/`--continue`、`--scope`、`--db`、`--agent-cmd`）與 `config::GatewayConfig::resolve`，不另寫 CLI/Config。

## 人格層（persona.rs）

```rust
pub const WUKONG_PERSONA: &str =
    "你是孫悟空（齊天大聖、鬥戰勝佛），一位全知全能的助手。\
     以略帶豪氣、機敏的口吻回應，但內容務必專業、精準、可執行。";

/// 組合執行 prompt：人格 + 角色卡 +（記憶 context + 使用者輸入）。
pub fn build_prompt(role: Role, hits: &[RecallHit], input: &str) -> String {
    let body = wukong_gateway::prompt::compose_prompt(hits, input); // 重用 gateway 記憶組裝
    format!("{WUKONG_PERSONA}\n\n{}\n\n{body}", role.card())
}
```

`wukong_gateway::prompt::compose_prompt(hits, input)` 在有命中時產 `[相關記憶]…[使用者輸入]…`、無命中時回純 `input`。前置人格與角色卡。

## 合體管線（lib.rs `run_turn`）

```rust
#[derive(Debug, Clone)]
pub struct TurnOutput {
    pub role: wukong_orchestrator::Role,
    pub text: String,
}

pub async fn run_turn(
    memory: &wukong_memory::Memory,
    backend: &impl wukong_gateway::backend::AiBackend,
    cfg: &wukong_gateway::config::GatewayConfig,
    input: &str,
) -> Result<TurnOutput, WukongError> {
    // 1. recall（scope 依 cwd 分專案，來自 gateway config）
    let recall = memory
        .recall(wukong_memory::RecallQuery {
            query: input.to_string(),
            top_k: cfg.recall_top_k,
            scope: Some(cfg.scope.clone()),
            mode: wukong_memory::RecallMode::Hybrid,
        })
        .await?;

    // 2. 路由選角色（orchestrator phase 1）
    let role = wukong_orchestrator::route(backend, input).await?;

    // 3. 組「人格 + 角色 + 記憶」prompt
    let prompt = persona::build_prompt(role, &recall.data, input);

    // 4. 執行
    let resp = backend
        .run(wukong_gateway::backend::AgentRequest {
            prompt,
            continue_session: cfg.continue_session,
        })
        .await?;

    // 5. remember 落盤（User + Assistant 兩筆 Event）
    memory
        .remember(wukong_memory::RememberInput {
            scope: cfg.scope.clone(),
            session_id: None,
            items: vec![
                wukong_memory::MemoryItem {
                    kind: wukong_memory::MemoryKind::Event,
                    text: format!("User: {input}"),
                    importance: None,
                },
                wukong_memory::MemoryItem {
                    kind: wukong_memory::MemoryKind::Event,
                    text: format!("Assistant: {}", resp.text),
                    importance: None,
                },
            ],
        })
        .await?;

    Ok(TurnOutput { role, text: resp.text })
}
```

合體三柱：記憶 recall/remember + orchestrator 路由 + gateway backend + 人格。每回合兩次 backend 呼叫（路由 + 執行）。

## 錯誤處理

`WukongError`（thiserror）：
- `Memory(#[from] wukong_memory::MemoryError)`
- `Orchestrator(#[from] wukong_orchestrator::OrchestratorError)`
- `Backend(#[from] wukong_gateway::GatewayError)`

（`route` 回 `OrchestratorError`、`backend.run` 回 `GatewayError`、記憶回 `MemoryError`，三者皆有 `From`。）

## main（薄殼）

```
wukong [OPTIONS] <PROMPT>...   # 旗標同 gateway::cli::Cli
```

流程：`Cli::parse()` → `GatewayConfig::resolve(&cli)` → `Memory::open(&cfg.db_url)` → `AgentCliBackend { command: cfg.agent_command, continue_args: cfg.continue_args }` → `run_turn(&memory, &backend, &cfg, &cli.prompt_text())`：
- 成功 → stderr 印點綴頭 `🐵 悟空·{role.name()}`、stdout 印答覆
- 失敗 → stderr `error: {e}` + `std::process::exit(1)`
- 開記憶失敗 → stderr + `exit(1)`

## 測試（TDD）

- `persona.rs`：
  - `build_prompt(Fixer, &[], "x")` 含 `WUKONG_PERSONA` 片段、`你是 Fixer`、`x`
  - 有記憶命中時含 `[相關記憶]`
- `lib.rs`（mock backend 腳本化 + temp memory）：
  - mock：route 回 `"fixer"`、exec 回 `"done"`；記錄收到的 prompts
  - `run_turn` → `TurnOutput.role == Fixer`、`text == "done"`
  - 記憶已寫入：之後 recall 找得到含 `User:` 的記憶
  - 第 2 次（exec）prompt 同時含 `WUKONG_PERSONA` 片段與 `你是 Fixer`
- gateway：移除 bin 後 `cargo test -p wukong-gateway` 仍全綠

Mock backend：測試內實作 gateway `AiBackend`，用 `Mutex<VecDeque<String>>` 依序吐回應、`Mutex<Vec<String>>` 記錄 prompts。

## 驗收標準

1. `cargo test` 全綠（新 crate + 既有三柱）
2. `cargo clippy --all-targets -- -D warnings` 乾淨
3. workspace 只產出一個 `wukong` bin（gateway 不再產出 `wukong`）
4. `wukong --agent-cmd "printf fixer" --db "sqlite://$PWD/scratch.db" "hello"` → stderr 顯示 `🐵 悟空·fixer`、跑完兩段、該回合寫入記憶
5. exec prompt 同時含人格、角色卡、記憶 context（由 `lib.rs` 測試斷言）
