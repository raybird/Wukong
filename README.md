<p align="center">
  <strong>🐵 Wukong / 孫悟空</strong><br>
  全知全能的個人 AI 助手 — 有記憶、會分身、以齊天大聖之姿執行
</p>

<p align="center">
  <img alt="lang" src="https://img.shields.io/badge/lang-Rust-orange">
  <img alt="status" src="https://img.shields.io/badge/status-v1%20四柱完成-brightgreen">
  <img alt="tests" src="https://img.shields.io/badge/tests-63%20passing-blue">
</p>

---

## 簡介

**Wukong** 是一個用 Rust 從零打造的個人 AI 助手，取材自中國神話的孫悟空 ——
**齊天大聖**的執行力、**七十二變**的角色分身、**鬥戰勝佛**歷劫不忘的記憶。

它把三個概念融成一個 `wukong` 指令：

> 你問一句 → 助手**回想**相關記憶 → **判斷**該用哪個專家角色 → 以**孫悟空人格 + 角色 + 記憶**驅動底層 AI agent → 回答 → 把這一回合**記下來**。

跨對話的連續性由記憶層承載，且**依工作目錄自動分專案**隔離。

本專案是 [Memoria](https://github.com/raybird)、TeleNexus、tao-of-coding 三個專案概念的 Rust 重生版。

---

## 神話對應 → 系統四柱

| 柱 | 神話身份 | crate | 職責 |
| :- | :------- | :---- | :--- |
| 1 | 鬥戰勝佛・本我 | `wukong-memory` | 持久記憶：SQLite + FTS5、keyword/tree/hybrid 召回、scope 隔離、時間衰減 |
| 2 | 齊天大聖・肉身 | `wukong-gateway` | 驅動可設定的 agent CLI、記憶注入回合（函式庫） |
| 3 | 七十二變・分身 | `wukong-orchestrator` | LLM 路由到五專家角色（Explorer/Oracle/Librarian/Fixer/Designer）並以該角色執行 |
| 4 | 修成正果・金箍棒 | `wukong-cli` → **`wukong`** | 合體：人格 + 記憶 + 角色調度於一回合，統一進入點 |

```
        wukong (金箍棒 / 統一 CLI)
        ├── 人格層（孫悟空口吻）
        ├── recall ─────────────►  wukong-memory   (本我)
        ├── route  ─────────────►  wukong-orchestrator (分身)
        ├── execute ────────────►  wukong-gateway → agent CLI (肉身)
        └── remember ───────────►  wukong-memory   (本我)
```

依賴方向（單向、無循環）：`wukong-cli → { memory, gateway, orchestrator }`，`orchestrator → gateway → memory`。

### 一回合的資料流

```
你: wukong "幫我修這個 bug"
      │
      ▼
┌─────────────────────── wukong-cli (金箍棒) ───────────────────────┐
│                                                                    │
│  1. recall ───────────────►  wukong-memory     回想此 scope 的相關記憶
│                              （SQLite + FTS5）   ◄─── hits[]
│                                                                    │
│  2. route  ───────────────►  wukong-orchestrator  第 1 次 agent 呼叫：
│         （判斷專家角色）          └─► agent CLI       「該找哪個角色？」
│                                                  ◄─── role = Fixer
│                                                                    │
│  3. build_prompt = 人格(悟空) + 角色卡(Fixer) + 記憶 hits + 你的輸入     │
│                                                                    │
│  4. execute ──────────────►  wukong-gateway      第 2 次 agent 呼叫：
│                              └─► agent CLI         以該角色執行
│                                                  ◄─── 回答文字
│                                                                    │
│  5. remember ─────────────►  wukong-memory      落盤 User + Assistant
│                                                                    │
└────────────────────────────────────────────────────────────────────┘
      │
      ▼
  stderr: 🐵 悟空·fixer
  stdout: <回答>
```

> 每回合對底層 agent 進行兩次呼叫（路由 + 執行）。各 crate 另有自己的 README。

---

## 快速開始

### 先決條件

- **Rust**（stable，≥ 1.96）。若未安裝：
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  . "$HOME/.cargo/env"
  ```
- 一個可用的 **AI agent CLI**（預設 `opencode run`；可用 `--agent-cmd` 換成其他）。

### 建置與測試

```bash
cargo build            # 編譯整個 workspace
cargo test             # 全部測試（v1：63 passing）
cargo clippy --all-targets -- -D warnings
```

### 使用

```bash
# 基本：問一句（預設驅動 `opencode run`）
wukong "幫我重構這個函式"

# 指定底層 agent 指令
wukong --agent-cmd "opencode run" "這段程式為什麼會 panic？"

# 接續底層 agent 的上一個 session
wukong -c "那再幫我補上單元測試"

# 覆寫記憶 scope（預設依工作目錄為 project:<資料夾名>）
wukong --scope "global" "記住：我偏好 4 空格縮排"
```

每次執行會在 stderr 顯示這回合化身的角色，例如：

```
🐵 悟空·fixer
<agent 的回答…>
```

---

## CLI 參數

| 參數 | 說明 | 預設 |
| :--- | :--- | :--- |
| `<PROMPT>...` | 要問的內容（位置參數，以空白接回） | 必填 |
| `-c`, `--continue` | 把接續旗標透傳給底層 agent CLI | off |
| `--scope <SCOPE>` | 記憶 scope（`global` / `project:X` / `agent:X` / `user:X`） | `project:<cwd 資料夾名>` |
| `--db <URL>` | 記憶資料庫位置 | `$HOME/.wukong/memory.db` |
| `--agent-cmd <CMD>` | agent 指令（空白分隔） | `opencode run` |

環境變數：`WUKONG_MEMORY_DB`、`WUKONG_AGENT_CMD`、`WUKONG_AGENT_CONTINUE_ARGS`。

---

## 記憶服務（選用）

`wukong-memory` 同時提供一個獨立的 HTTP 服務 `wukong-memoryd`，供跨語言或外部工具存取：

```bash
WUKONG_MEMORY_PORT=3917 cargo run -p wukong-memoryd
curl -s http://127.0.0.1:3917/v1/health        # {"status":"ok"}
```

| Method | Path | 說明 |
| :--- | :--- | :--- |
| GET | `/v1/health` | 健康檢查 |
| GET | `/v1/stats` | 統計（總數、各 scope 分布） |
| POST | `/v1/remember` | 寫入記憶 |
| POST | `/v1/recall` | 召回記憶 |

回應信封：`{ data, evidence[], confidence, latency_ms }`。

---

## 專案結構

```
wukong/
├── Cargo.toml                      # workspace
├── crates/
│   ├── wukong-memory/              # 柱1：記憶核心（lib）
│   ├── wukong-memoryd/             # 記憶 HTTP 服務（bin）
│   ├── wukong-gateway/             # 柱2：執行閘道（lib）
│   ├── wukong-orchestrator/        # 柱3：角色調度（lib + demo bin wukong-orchestrate）
│   └── wukong-cli/                 # 柱4：統一 CLI（lib + bin wukong）
└── docs/superpowers/
    ├── specs/                      # 各柱設計 spec
    └── plans/                      # 各柱逐步實作計畫
```

---

## 記憶模型

- **儲存**：SQLite + FTS5（BM25 關鍵字檢索），啟用 WAL。
- **召回模式**：`keyword`（FTS5）、`tree`（依 scope 階層取近期）、`hybrid`（合併重排，預設）。
- **排序**：`score = α·詞彙相關 + β·時間衰減 + γ·重要度`，時間衰減半衰期 90 天，常被取用者加成。
- **Scope 階層**：`project:X` / `agent:X` / `user:X` 召回時自動含 `global`。
- **Adaptive gate**：過短／全停用詞的瑣碎查詢直接略過召回。

---

## 開發

```bash
cargo test -p wukong-memory          # 單一 crate 測試
cargo test -p wukong-cli persona::   # 指定模組
cargo run -p wukong-orchestrator --bin wukong-orchestrate -- --agent-cmd "printf fixer" "fix the bug"
```

開發遵循 TDD（測試先行）、frequent commits；每一柱皆有 `docs/superpowers/specs/` 設計與 `docs/superpowers/plans/` 計畫可循。

> 提示：以 `printf fixer` 或 `echo` 當「假 agent」可在無真實 LLM 下驗證完整流程。

---

## Roadmap（v2+）

- Telegram bot / Web Console 進入點（TeleNexus 完整願景）
- 即時串流輸出、互動 REPL
- 平行多角色調度、角色協作鏈、技能路由
- 語意向量召回（本機 embedding）
- 記憶 markdown/wiki 雙持久化、consolidation/prune、可觀測性快照

---

## 授權

MIT
