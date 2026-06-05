# wukong-cli

> 柱 4 ──「修成正果・金箍棒」：統一 CLI（`wukong` 二進位）

把三柱合為一個產品。一回合 = recall → route → execute（人格 + 角色 + 記憶）→ remember。

## 進入點

產生 `wukong` 二進位（workspace 內唯一的 `wukong` bin）：

```bash
wukong "幫我重構這個函式"
# stderr: 🐵 悟空·fixer
# stdout: <agent 回答>
```

旗標與 `wukong-gateway::cli::Cli` 共用：`<PROMPT>...`、`-c/--continue`、`--scope`、`--db`、`--agent-cmd`。

## 公開 API（lib）

```rust
use wukong_cli::{run_turn, TurnOutput};

let out: TurnOutput = run_turn(&memory, &backend, &cfg, "fix the bug").await?;
// out.role（化身角色）、out.text（回答）
```

- `persona::WUKONG_PERSONA` — 孫悟空人格系統 prompt
- `persona::build_prompt(role, hits, input)` — 人格 + 角色卡 + 記憶 context + 輸入
- `WukongError` — 收斂 memory / orchestrator / backend 三柱錯誤

依賴：`wukong-cli → { wukong-memory, wukong-gateway, wukong-orchestrator }`（單向，頂層）。

詳見 [`docs/superpowers/specs/2026-06-05-wukong-cli-design.md`](../../docs/superpowers/specs/2026-06-05-wukong-cli-design.md)。
