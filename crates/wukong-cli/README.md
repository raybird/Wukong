# wukong-cli

> 柱 4 ──「修成正果・金箍棒」：統一 CLI（`wukong` 二進位）

把三柱合為一個產品。一回合 = recall → route → execute（人格 + 角色 + 記憶）→ remember。

## 進入點

產生 `wukong` 二進位（workspace 內唯一的 `wukong` bin）：

```bash
wukong "幫我重構這個函式"     # 單次：stderr 顯示 🐵 悟空·fixer，stdout 顯示答案
wukong                        # 無參數 → 進入互動 REPL（多輪、session 接續、記憶累積）
```

REPL meta 指令：`/exit`、`/quit`、Ctrl-D 離開；`/scope <x>` 切換本 session 記憶 scope；空行略過。

旗標與 `wukong-gateway::cli::Cli` 共用：`[PROMPT]...`、`-c/--continue`、`--scope`、`--db`、`--agent-cmd`、`--no-stream`。

## 活動渲染（串流）

預設開啟：execute 步驟以 `opencode run --format json` 解析事件——文字片段印到 **stdout**、工具活動（`▸ 使用工具 …`）印到 **stderr**，管線相容。`--no-stream` 或 `WUKONG_STREAM=0` 退回純文字一次輸出。

> 注意：opencode 目前不吐逐 token delta，故串流顆粒度為「訊息片段／步驟」級而非逐字打字機。

## 公開 API（lib）

```rust
use wukong_cli::{run_turn, TurnOutput};

// on_event 收串流事件、on_role 於路由後回呼角色
let out: TurnOutput = run_turn(&memory, &backend, &cfg, "fix the bug", &mut |_| {}, &mut |_| {}).await?;
// out.role（化身角色）、out.text（回答）

// 互動迴圈（注入式輸入，可測）
use wukong_cli::repl::run_repl_loop;
```

- `persona::WUKONG_PERSONA` — 孫悟空人格系統 prompt
- `persona::build_prompt(role, hits, input)` — 人格 + 角色卡 + 記憶 context + 輸入
- `render::StreamRenderer` — 事件 → stdout/stderr 分流
- `repl::{run_repl_loop, classify_line, LineAction}` — REPL 迴圈與行解析
- `WukongError` — 收斂 memory / orchestrator / backend 三柱錯誤

依賴：`wukong-cli → { wukong-memory, wukong-gateway, wukong-orchestrator }`（單向，頂層）。

詳見 [`docs/superpowers/specs/2026-06-05-wukong-cli-design.md`](../../docs/superpowers/specs/2026-06-05-wukong-cli-design.md)。
