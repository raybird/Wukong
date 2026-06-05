# wukong-gateway v1 設計

> 子專案 2／4 ──「齊天大聖・肉身」：CLI 互動閘道
> 日期：2026-06-05
> 狀態：已核可，待轉實作計畫

## 背景與定位

「孫悟空」四柱中的第二柱。柱 1 `wukong-memory`（持久記憶核心）已完成並合併 `main`。本柱是「肉身」── 接收使用者輸入、驅動 AI、並用記憶讓助手「歷劫不忘」。

| 子專案 | 神話對應 | 概念來源 | 狀態 |
|--------|---------|---------|------|
| 1. `wukong-memory` | 鬥戰勝佛／本我 | Memoria | ✅ 完成 |
| **2. `wukong-gateway`** | 齊天大聖／肉身 | TeleNexus | ← 本文件 |
| 3. `wukong-orchestrator` | 七十二變／分身 | tao-of-coding | 待做 |
| 4. `wukong`（金箍棒） | 修成正果 | 三者融合 | 待做 |

## 目標與非目標

### v1 目標（最小可用垂直切片）
一條垂直切片：**一個 CLI 進入點 → 驅動一個 agent CLI → 前置注入 `wukong-memory` recall、回合結束 persist remember**。

- 進入點：`wukong` CLI，one-shot（每次呼叫一回合）
- 接續：`-c`/`--continue` 旗標透傳給底層 agent CLI（借用 agent 自身的 session 接續，gateway 不自管 session 生命週期）
- AI 後端：`AiBackend` trait；v1 實作以子程序驅動**可設定的 agent 指令**（預設 `opencode run`），run-and-capture（非串流）
- 記憶：直接 `use wukong_memory`（函式庫，不走 HTTP）；每回合 recall 注入 → 跑 agent → remember 落盤
- scope：預設 `project:<cwd 資料夾名>`，`--scope` 覆寫

### v1 非目標（延後 v2+）
Telegram bot、Web Console、排程、可觀測性快照、即時串流輸出、runner 分離執行、互動 REPL、多 AI 後端同時並存。

## 技術棧

Rust + tokio + clap（derive）+ thiserror。`wukong-gateway` 以 path 依賴直接 `use wukong_memory`。

## 架構：crate 佈局

新增第三個 crate（lib + bin `wukong`），加入既有 workspace：

```
crates/wukong-gateway/
├── Cargo.toml              # lib + [[bin]] name="wukong"
└── src/
    ├── lib.rs              # 模組接線 + 重新匯出
    ├── cli.rs              # clap 參數解析
    ├── config.rs           # GatewayConfig：scope/db/agent 指令解析（CLI + env + 預設）
    ├── backend.rs          # AiBackend trait + AgentCliBackend + assemble_argv
    ├── prompt.rs           # 把 recall 命中 + 使用者輸入組成完整 prompt
    ├── pipeline.rs         # run_turn：recall → prompt → backend → remember
    └── main.rs             # bin 進入點（薄殼）
```

職責單一：`backend` 只管子程序、`prompt` 只管字串組裝、`pipeline` 只管一回合編排、`config`/`cli` 只管設定。`pipeline` 吃 `Memory` 與 `impl AiBackend`、`backend` 吃 trait，皆可獨立測試。

## CLI 與 Config

### CLI（clap derive）

```
wukong [OPTIONS] <PROMPT>...
  -c, --continue         把接續旗標透傳給 agent CLI
      --scope <SCOPE>     覆寫記憶 scope
      --db <URL>          覆寫記憶資料庫
      --agent-cmd <CMD>   覆寫 agent 指令（空白分隔）
```

`<PROMPT>...` 為位置參數（trailing），以單一空白接回成一句。

### GatewayConfig（優先序：CLI > env > 預設）

| 欄位 | 來源／預設 |
|------|-----------|
| `scope` | `--scope` ／ 由 cwd 推 `project:<資料夾名>` ／ 失敗則 `global` |
| `db_url` | `--db` ／ `WUKONG_MEMORY_DB` ／ `$HOME/.wukong/memory.db` |
| `agent_command` | `--agent-cmd` ／ `WUKONG_AGENT_CMD` ／ `["opencode","run"]` |
| `continue_args` | `WUKONG_AGENT_CONTINUE_ARGS` ／ `["-c"]` |
| `continue_session` | `-c`/`--continue` 旗標 |
| `recall_top_k` | 預設 `5` |

`agent_command` 與 `continue_args` 的 env／CLI 字串以空白切分成 argv。`db_url` 預設與 `wukong-memoryd` 一致（`$HOME/.wukong/memory.db`，自動建立父目錄）。

## AiBackend 與 agent 驅動

Rust 1.96 原生支援 trait 內 async fn；用泛型（`impl AiBackend`）避免 dyn 物件安全問題，不需 `async_trait`。

```rust
pub struct AgentRequest  { pub prompt: String, pub continue_session: bool }
pub struct AgentResponse { pub text: String }

pub trait AiBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError>;
}
```

純函式（可單測）：

```rust
/// 組出傳給子程序的 argv：command + (continue_args if continue_session) + [prompt]
pub fn assemble_argv(
    command: &[String],
    continue_args: &[String],
    continue_session: bool,
    prompt: &str,
) -> Vec<String>;
```

`AgentCliBackend { command: Vec<String>, continue_args: Vec<String> }`：
- 用 `assemble_argv` 組好 argv
- `tokio::process::Command::new(argv[0]).args(&argv[1..])`，`.stdin(Stdio::null())` 避免卡住，`.output().await` 捕捉 stdout
- 非零退出碼 → `GatewayError::AgentFailed { code, stderr }`
- 成功 → `AgentResponse { text: String::from_utf8_lossy(stdout).trim().to_string() }`
- **不經 shell**（無跳脫/注入問題）、不覆寫 cwd/env（讓 opencode 在使用者專案目錄、用自身設定執行）

## 回合管線與 prompt 組裝

```rust
pub async fn run_turn(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
) -> Result<String, GatewayError>;
```

流程：
1. `memory.recall(RecallQuery { query: input, top_k: cfg.recall_top_k, scope: Some(cfg.scope.clone()), mode: Hybrid })`
2. `compose_prompt(&hits, input)` → 完整 prompt
3. `backend.run(AgentRequest { prompt, continue_session: cfg.continue_session })` → 回應
4. `memory.remember(RememberInput { scope: cfg.scope.clone(), session_id: None, items: [Event "User: <input>", Event "Assistant: <resp>"] })`
5. 回傳回應文字

`compose_prompt(hits, input)`（純函式）：
- 無命中 → 原樣回傳 `input`
- 有命中 → 前置記憶區塊：

```
[相關記憶]
- (project:Wukong) 之前決定用 Rust…
- (global) …

[使用者輸入]
<input>
```

> 註：即使 `--continue`（agent 已有自身 session 脈絡），仍照常 recall 注入──`wukong-memory` 保存的是 agent session 之外的長期跨 session 記憶，兩者互補；v1 接受輕微重複。

## 錯誤處理

`GatewayError`（thiserror）：
- `Memory(#[from] wukong_memory::MemoryError)`
- `AgentFailed { code: Option<i32>, stderr: String }`
- `Io(#[from] std::io::Error)`

`main` 捕捉 `run_turn` 的錯誤，印到 stderr 並 `std::process::exit(1)`。

## 測試（TDD）

- `prompt.rs`：有／無記憶命中的組裝（純函式）
- `backend.rs`：`assemble_argv` 含/不含接續旗標（純函式）；`AgentCliBackend` 用真實 `echo` 指令驗證捕捉（`echo <prompt>` → 回應含 prompt）
- `pipeline.rs`（整合，temp db + Mock backend）：run_turn 後 ① 回傳 mock 回應 ② 記憶確實寫入（再 recall 找得到）③ 先植入一筆記憶後，傳給 backend 的 prompt 含 `[相關記憶]`
- `cli.rs`：clap 解析（prompt 接回、`-c`、`--scope`）

Mock backend：測試內定義一個實作 `AiBackend` 的型別，記錄收到的 prompt 並回傳固定回應。

## 驗收標準

1. `cargo test` 全綠（含上述單元 + 整合測試）
2. `cargo clippy --all-targets -- -D warnings` 乾淨
3. `wukong --agent-cmd "echo" "hi"` 能跑出 echo 結果並把該回合寫入記憶（之後 `recall` 找得到）
4. 預設 scope 由 cwd 推導為 `project:<資料夾名>`，`--scope` 可覆寫
5. `-c`/`--continue` 會把接續參數（預設 `-c`）插入 agent argv
6. agent 子程序非零退出 → 回傳 `AgentFailed` 並使 `main` 以 exit code 1 結束
