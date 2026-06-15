# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 常用指令

```bash
# 建置整個 workspace
cargo build --release

# 執行所有測試
cargo test

# 只測單一 crate
cargo test -p wukong-memory

# 只測特定模組
cargo test -p wukong-cli persona::

# Lint 檢查
cargo clippy --all-targets -- -D warnings

# 執行 orchestrator demo（以假 agent 驗證流程）
cargo run -p wukong-orchestrator --bin wukong-orchestrate -- --agent-cmd "printf fixer" "fix the bug"

# 啟動 Web Console（開發用）
WUKONG_AGENT_CMD="opencode run" cargo run -p wukong-web

# 啟動 Telegram bot
WUKONG_TG_TOKEN="<token>" WUKONG_TG_ALLOWED="<chat_id>" cargo run -p wukong-telegram

# 啟動記憶 HTTP 服務
WUKONG_MEMORY_PORT=3917 cargo run -p wukong-memoryd

# 同步 Superpowers 技能（預覽）
scripts/sync-superpowers.sh <commit-or-tag> --dry-run
```

## 架構

Wukong 是 Rust Workspace，包含 13 個 crate，分為四柱核心與周邊進入點。

### 四柱核心（依賴方向單向）

```
wukong-cli → { wukong-runtime, wukong-memory, wukong-orchestrator }
wukong-runtime → { wukong-gateway, wukong-memory, wukong-orchestrator, wukong-skills, wukong-settings }
wukong-orchestrator → wukong-gateway → wukong-memory
```

| crate | 職責 |
|-------|------|
| `wukong-memory` | SQLite + FTS5 記憶儲存；keyword/tree/hybrid 召回；時間衰減計分 |
| `wukong-gateway` | 驅動底層 agent CLI（預設 `opencode run`）；inject 人格 + 記憶 + 技能 |
| `wukong-orchestrator` | LLM 路由規劃（最多 3 棒角色：Explorer/Oracle/Librarian/Fixer/Designer） |
| `wukong-runtime` | 串聯一回合完整執行流程（`run_turn`）：recall → plan → execute → remember；CLI、Web、Telegram、Scheduler 共用 |
| `wukong-cli` | 統一 CLI 進入點（`wukong` binary）；含 REPL、`memory` 子命令、`schedule` 子命令 |

### 周邊 crate

| crate | 職責 |
|-------|------|
| `wukong-skills` | 以 `include_str!` 內嵌 Superpowers 技能（`assets/superpowers/`） |
| `wukong-settings` | 讀寫 `.wukong/settings.toml` 專案設定 |
| `wukong-scheduler` | 排程核心 lib；SQLite lease 防止重複執行 |
| `wukong-schedulerd` | 排程 daemon binary |
| `wukong-memoryd` | 記憶 HTTP 服務（`/v1/recall`、`/v1/remember` 等） |
| `wukong-render` | Markdown → HTML / Telegram HTML 渲染（含 SafeHTML 防 XSS） |
| `wukong-telegram` | Telegram Long-Polling bot；重用 `run_turn` |
| `wukong-web` | Axum Web Console；SSE 串流；前端以 `include_str!` 內嵌單一 binary |

### 一回合資料流（`run_turn`）

1. `wukong-memory` recall（混合 BM25 + 語意向量，預設 hybrid）
2. `wukong-orchestrator` plan（LLM 規劃角色 + 技能鏈）
3. 逐棒 `wukong-gateway` execute（注入人格 + 角色 + 技能規範 + 記憶）
4. `wukong-memory` remember（落盤本輪 User + Assistant）

會話隔離：只有最後一棒才帶入 / 更新 scope 的 `session_id`，前面輔助棒為 stateless。

### 關鍵設計細節

- **假 agent 測試法**：`--agent-cmd "printf fixer"` 或 `echo` 可在無 LLM 下驗證完整流程。
- **embedding 選用**：cargo feature `embed` + `WUKONG_EMBED=1` 啟用本機 embedding（fastembed all-MiniLM-L6-v2），未啟用則退回 BM25。
- **Web 執行緒隔離**：`run_turn` 含非 `Send` callback，Web 後端以 `std::thread::spawn` + `current_thread` 隔離，進度透過 mpsc channel → SSE 回傳。
- **Superpowers 來源**：`crates/wukong-skills/assets/superpowers/SOURCE.md` 記錄上游版本；以 `scripts/sync-superpowers.sh` 更新。

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **Wukong** (2572 symbols, 4862 relationships, 199 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/Wukong/context` | Codebase overview, check index freshness |
| `gitnexus://repo/Wukong/clusters` | All functional areas |
| `gitnexus://repo/Wukong/processes` | All execution flows |
| `gitnexus://repo/Wukong/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
