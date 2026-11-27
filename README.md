<p align="center">
  <strong>🐵 Wukong / 孫悟空</strong><br>
  全知全能的個人 AI 助手 — 有記憶、會分身、以齊天大聖之姿執行
</p>

<p align="center">
  <img alt="lang" src="https://img.shields.io/badge/lang-Rust-orange">
  <img alt="status" src="https://img.shields.io/badge/status-v1%20四柱完成-brightgreen">
  <img alt="tests" src="https://img.shields.io/badge/tests-444%20passing-blue">
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
| 1 | 鬥戰勝佛・本我 | `wukong-memory` | 持久記憶：SQLite + FTS5、keyword/tree/hybrid/relevant 召回、scope 隔離、時間衰減 |
| 2 | 齊天大聖・肉身 | `wukong-gateway` | 驅動可設定的 agent CLI、記憶注入回合（函式庫） |
| 3 | 七十二變・分身 | `wukong-orchestrator` | 五專家角色（Explorer/Oracle/Librarian/Fixer/Designer）的定義，作為 OpenCode V2 的 subagent 由悟空派工 |
| 4 | 修成正果・金箍棒 | `wukong-cli` → **`wukong`** | 合體：人格 + 記憶 + 角色調度於一回合，統一進入點 |

```
        wukong (金箍棒 / 統一 CLI)
        ├── 人格層（孫悟空口吻）
        ├── recall ─────────────►  wukong-memory   (本我)
        ├── execute ────────────►  wukong-gateway → OpenCode V2 (肉身)
        │                           └─ 悟空派工 ─► 五個 subagent（wukong-orchestrator，分身）
        └── remember ───────────►  wukong-memory   (本我)
```

依賴方向（單向、無循環）：`wukong-cli → { runtime, gateway, memory }`，`runtime → { gateway, memory, orchestrator, skills }`，`gateway → memory`。

### 一回合的資料流

```mermaid
sequenceDiagram
    autonumber
    actor User as 使用者
    participant CLI as wukong-cli (金箍棒)
    participant Mem as wukong-memory (本我)
    participant GW as wukong-gateway (肉身)
    participant Agent as OpenCode V2：agent wukong
    participant Sub as subagent (分身，例如 fixer)

    User->>CLI: 輸入: wukong "幫我修這個 bug"
    activate CLI

    CLI->>Mem: 1. 取得 scope 的 session lease、recall 相關記憶
    Mem-->>CLI: 回傳 hits[]

    CLI->>GW: 2. 送出一次 prompt（使用者輸入 + 相關記憶）
    GW->>Agent: 以 agent wukong 執行（帶 session_id、模型與 thinking）
    opt 需要改檔、跑測試或專門分析
        Agent->>Sub: 用 subagent 工具派工
        Sub-->>Agent: 回報產出
    end
    Agent-->>GW: 回答與 session_id
    GW-->>CLI: 傳回結果

    CLI->>Mem: 3. remember (落盤 User + Assistant)

    CLI-->>User: 輸出結果 (stderr: 角色狀態 / stdout: 回答)
    deactivate CLI
```

> 2026-11-26（#45）起每回合只呼叫一次 primary agent `wukong`，不再另跑規劃；規劃、技能與壓縮交給 OpenCode V2。各 crate 另有自己的 README。

### 派工與 session

- **悟空決定自己回答或派工**：人格（工作區的 `SOUL.md`）、派工規則、輸出要求與排程能力寫在 Wukong 管理的 agent 定義（工作區的 `.opencode/agents/`），入口啟動時寫入、升級時跟著更新。悟空不能自己改檔，終端機只能跑排程與長期記憶指令，需要改檔、跑測試的工作一律派給 subagent。
- **入口看得到誰在處理**：Telegram 的狀態泡泡、Web 的進度與排程的執行訊息顯示「悟空」或「悟空·fixer」，Web 另外顯示 subagent 的產出卡片。
- **同一個 scope 接續同一個 session**：對話脈絡由 session 保存；session 開始超過 7 天時換新的，換新後的第一個回合另帶上一回合，追問照樣看得懂。壓縮交給 OpenCode 自己的自動壓縮。
- **空輸出補救**：最終回覆是空的（例如只用工具收尾）時，在同一個 session 補送一次修復提示；仍是空的才回 `(本回合未產生文字輸出)`，使用者不會收到空白，記憶與對話歷史也不會留下空字串。
- **輸出要求**：agent `wukong` 的定義裡有 `[輸出要求]`（`persona::final_answer_directive`；CLI backend 改附在每回合的 prompt），要求即使工作都用工具完成也必須以文字總結。

### 技能 (Skills)

- **本地技能庫**：Wukong 內建 selected Superpowers 技能於 `crates/wukong-skills/assets/superpowers/`，由 `wukong-skills` catalog 以 `include_str!` 內嵌進 runtime。
- **由 V2 的 skill 機制提供**：入口啟動時把技能寫到工作區的 `.opencode/skills/`，模型依技能的 description 自己用 `skill` 工具載入（2026-11-26，#45）；prompt 不再附技能規範。
- **更新來源**：使用 `scripts/sync-superpowers.sh <commit-or-tag> --dry-run` 預覽，再用 `scripts/sync-superpowers.sh <commit-or-tag>` 同步上游 Superpowers；來源版本記錄於 `crates/wukong-skills/assets/superpowers/SOURCE.md`。

---

## 快速開始

一行安裝（腳本會詢問 Docker 或 Binary 模式，詳見 [安裝指南](docs/installation.md)）：

```bash
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash
```

既有 Docker 部署升級時，請在 Runtime／部署目錄直接執行：

```bash
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode docker --upgrade
```

Binary 安裝可用 `--mode binary --upgrade` 保留既有元件、設定與 workspace 升級；加上 `--with-schedulerd` 可在 Linux 明確安裝 Scheduler user service，`--rollback` 可回復最後一個已驗證 transaction。Docker installer 驗證 release checksum、manifest 與 GHCR image digest，且不會移除 volumes；stable promotion 必須附上已提交的 RC rehearsal evidence。

基本使用：

```bash
# 互動 REPL（無參數）：多輪對話、session 接續、記憶持續累積
wukong

# 問一句（預設驅動 `opencode run`）
wukong "幫我重構這個函式"

# 覆寫記憶 scope（預設依工作目錄為 project:<資料夾名>）
wukong --scope "global" "記住：我偏好 4 空格縮排"
```

每次執行會在 stderr 顯示這回合化身的角色，例如：

```
🐵 悟空·fixer
<agent 的回答…>
```

完整參數、記憶維護與排程子命令請見 [CLI 參考](docs/cli-reference.md)。

---

## 文件導覽

| 主題 | 文件 | 內容 |
| :--- | :--- | :--- |
| 安裝 | [docs/installation.md](docs/installation.md) | Binary / Docker 模式選擇、快速安裝、prerelease/RC、從原始碼建置 |
| Docker 部署 | [docs/docker.md](docs/docker.md) | 容器化、opencode serve 低延遲模式、升級、volume 架構、Agent Reach、provider 授權、env 變數表 |
| CLI 參考 | [docs/cli-reference.md](docs/cli-reference.md) | CLI 參數、使用範例、記憶維護子命令、排程子命令、自然語言排程、歸檔剪枝機制 |
| 記憶模型 | [docs/memory.md](docs/memory.md) | 混合計分公式、語意向量召回、scope 階層、`wukong-memoryd` HTTP API |
| 進入點 | [docs/entrypoints.md](docs/entrypoints.md) | opencode session 控制、Telegram bot、Web Console、Chat control commands、執行緒隔離 |

各 crate 另有自己的 README（見下方專案結構），`docs/superpowers/` 收錄各模組詳細設計規格書與開發計畫。

---

## 專案結構

```
wukong/
├── Cargo.toml                      # workspace 設定檔
├── crates/
│   ├── wukong-memory/              # 柱1：記憶核心（lib）
│   ├── wukong-memoryd/             # 記憶 HTTP 服務（bin）
│   ├── wukong-gateway/             # 柱2：執行閘道（lib）
│   ├── wukong-orchestrator/        # 柱3：五個專家角色的定義（lib；派工交給 OpenCode 的 subagent）
│   ├── wukong-skills/              # 技能管理庫：內嵌 Superpowers 技能（lib）
│   ├── wukong-settings/            # 專案設定檔管理（lib）
│   ├── wukong-scheduler/           # 任務排程核心（lib）
│   ├── wukong-schedulerd/          # 任務排程守護行程（bin）
│   ├── wukong-runtime/             # 悟空執行時期：串聯記憶與協作鏈（lib）
│   ├── wukong-cli/                 # 柱4：統一 CLI 進入點（lib + bin wukong）
│   ├── wukong-render/              # 渲染層：markdown → HTML/Telegram HTML（lib）
│   ├── wukong-tg-client/           # Telegram 傳輸層：Bot API client + scope 解析（lib，bot/daemon 共用）
│   ├── wukong-telegram/            # 進入點：Telegram bot（lib + bin wukong-telegram）
│   └── wukong-web/                 # 進入點：Web Console（lib + bin wukong-web）
└── docs/superpowers/
    ├── specs/                      # 各模組詳細設計規格書
    └── plans/                      # 各模組逐步開發計畫
```

---

## 開發

```bash
cargo test -p wukong-memory          # 單一 crate 測試
cargo test -p wukong-cli persona::   # 指定模組
cargo test -p wukong-cli --test cli_backend   # 以假 agent（scripts/test-support/cli_backend.rs）跑真正的 wukong 回合
```

開發遵循 TDD（測試先行）、frequent commits；每一柱皆有 `docs/superpowers/specs/` 設計與 `docs/superpowers/plans/` 計畫可循。

> 提示：`scripts/test-support/cli_backend.rs` 的假 agent 會照 opencode 的 JSON 事件格式回覆，可在無真實 LLM 下驗證完整流程。

---

## Roadmap（v2+）

- ~~Telegram bot 進入點~~ ✅ v0.6.0;~~Web Console 進入點~~ ✅(TeleNexus 完整願景,持續)
- ~~角色協作鏈（多角色依序接力）~~ ✅ v0.5.0，#45 起改為悟空派工給 OpenCode V2 的 subagent;~~技能路由（Superpowers 本地技能注入）~~ ✅，#45 起改由 V2 的 skill 機制提供;平行多角色調度(後續)
- ~~記憶 markdown 雙持久化、consolidation/prune、可觀測性快照~~ ✅ v0.4.0
- ~~助手自然語言建立排程（提示詞注入排程能力）+ 排程結果回送 Telegram~~ ✅ 2026-06-16

---

## 授權

MIT
