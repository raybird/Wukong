# wukong-gateway

> 柱 2 ──「齊天大聖・肉身」：執行閘道（函式庫）

驅動可設定的 agent CLI（預設 `opencode run`），並提供統一 CLI 共用的設定/介面零件。
v1 起為**純 lib**（`wukong` 進入點已由 `wukong-cli` 接管）。

## 提供的零件

| 模組 | 內容 |
| :--- | :--- |
| `backend` | `AiBackend` trait、`AgentRequest`/`AgentResponse`、`AgentCliBackend`、`assemble_argv` |
| `cli` | `Cli`（clap 參數）、`prompt_text()` |
| `config` | `GatewayConfig`（CLI > env > 預設）、`resolve` |
| `prompt` | `compose_prompt(hits, input)` — 記憶 context + 使用者輸入 |
| `pipeline` | `run_turn` — 簡單的「記憶 + backend」回合（保留 API） |
| `error` | `GatewayError` |

## AiBackend

```rust
pub trait AiBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError>;
}
```

`AgentCliBackend` 以子程序執行 `command + (continue_args?) + [prompt]`，**不經 shell**（無跳脫/注入）、`stdin` 設為 null、捕捉 stdout；非零退出 → `AgentFailed`。

`route`/`orchestrate`（柱 3）與統一 CLI（柱 4）都建立在這個 trait 上，因此可用假指令（`echo`/`printf`）或 mock 測試。

詳見 [`docs/superpowers/specs/2026-06-05-wukong-gateway-design.md`](../../docs/superpowers/specs/2026-06-05-wukong-gateway-design.md)。
