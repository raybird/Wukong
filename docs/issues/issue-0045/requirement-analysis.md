# 需求分析：Wukong 瘦身的現況

> ← 回到 [README](README.md)

2026-11-26 讀 main（b65b487，#44 V2 adapter 已合併）的程式碼整理。行號以這個版本為準。

## 需求描述

GitHub #45：Wukong 只做 harness 做不到的部分。planner 加接力鏈、技能選擇與注入、每回合的固定提示、依回合數的壓縮改交給 OpenCode V2 的原生 agent、skill 與 instructions。範圍與「留在 Wukong」的清單見 GitHub issue 內文。2026-11-24 使用者決定拆成兩個 issue，本 issue 是第二個；前置的 #44 已於 2026-11-26 合併。

## 現況分析

### 1. 規劃與接力鏈

- **規劃怎麼做**：`run_turn_traced_with_attachments` 先呼叫 `plan_skill_chain_with_preferences`（`crates/wukong-runtime/src/turn.rs:188`）。
  - 它以 `run_ephemeral` 對 OpenCode 內建的 `plan` agent 送一次 prompt，並關掉 question 工具（`crates/wukong-orchestrator/src/router.rs:245-262`）。這個 session 用完即刪。
  - prompt 內容是五個角色、八個技能，加上選用的使用者偏好與上一回合，要求模型每行輸出一個「角色|技能」，最多 3 行（router.rs:159-228）。
  - planner 看到的是含入口附加區塊的原始 input（turn.rs:189）。
- **解析**（router.rs:118-153）：
  - 未知的技能當成沒有技能。
  - 一行都解析不出來時，改從文字裡找角色名稱；還是沒有，就只派一棒 Oracle。
  - planner 呼叫本身出錯時，回合直接失敗（turn.rs:195），沒有退路。
- **執行順序**：planner 在拿 scope lease 之前執行（turn.rs:188 對 197）。同一個 scope 的第二個回合要等 planner 跑完，才會因為 lease 被拒絕。
- **每一棒**：prompt 是 `persona::build_prompt_with_skill` 加上 `[前序協作]`，前面各棒的輸出全文放入，沒有上限（`crates/wukong-orchestrator/src/lib.rs:40`）。
  - 輔助棒用 `run_streaming_ephemeral` 執行：沒有 session、`model: None`（不套用 `default_model`），但會帶附件（turn.rs:273-287）。
  - 最後一棒用 scope 的 session 與 `cfg.default_model` 執行（turn.rs:258-272）。
- **上一回合補償**：`previous_turn_records`（turn.rs:508）取這個 scope 最新的 2 筆 Event 記憶。
  - `with_previous_turn`（turn.rs:534）把它併進輔助棒的召回結果。最後一棒只有在 session 被輪替時才併入（turn.rs:225）。
  - `planner_previous_turn`（turn.rs:521）截成 800 字交給 planner。
- **呼叫端**：
  - planner 只從 turn.rs:188 呼叫。
  - `run_turn*` 有五個呼叫端：CLI REPL（`crates/wukong-cli/src/repl.rs:93`）、CLI 單次（`crates/wukong-cli/src/main.rs:488`）、Web（`crates/wukong-web/src/chat_api.rs:313`）、Telegram（`crates/wukong-telegram/src/dispatch.rs:1494`）、排程（`crates/wukong-scheduler/src/executor.rs:423`）。
  - `crates/wukong-orchestrator/src/main.rs` 是用舊版 `orchestrate_chain` 的 demo binary。

### 2. 角色與規劃偏好

- **角色**：`Role` 是五個固定值，各有寫死的中文 `description()` 與 `card()`（`crates/wukong-orchestrator/src/role.rs:3-64`），每張角色卡約 50 字。
- **使用者能設的只有規劃偏好**：`planner_preferences` 的 `enabled`、`roles`、`skills`。
  - 存在 `/data/settings.json`，或 `WUKONG_SETTINGS_FILE` 指定的檔案（`crates/wukong-settings/src/lib.rs:44-48`）。
  - CLI、Web、Telegram 讀它；schedulerd 傳 `None`。Web 有讀寫 API 與介面（`crates/wukong-web/src/lib.rs` 約 744-783）。
- **正式部署**：2026-11-26 以唯讀方式查 wukong-web 的 `/data/settings.json`，`enabled` 是 false。
- **文件錯誤**：根目錄 AGENTS.md 寫成 `.wukong/settings.toml`，與實際不符。

### 3. 技能

- 八個 Superpowers 技能以 `include_str!` 內嵌（`crates/wukong-skills/src/catalog.rs:58-120`），只靠 planner 輸出的「角色|技能」選用。
- 每個回合都由 `materialize_default_runtime_skills`（turn.rs:211）寫到工作區的 `.wukong/skills/superpowers/{名稱}/SKILL.md`（`crates/wukong-runtime/src/skill_assets.rs:25-78`）。entrypoint 另從映像複製一份（`scripts/docker-entrypoint.sh:33-91`）。
- prompt 裡用 `[技能規範指引]` 加上 SKILL.md 的絕對路徑，叫模型自己去讀（`crates/wukong-runtime/src/persona.rs:27-33`），技能內文不放進 prompt。
- `workspace/AGENTS.md` 的「技能動態加載機制」段落再交代一次。

### 4. 每回合附加的固定提示

| 區塊 | 約略字數 | 出現條件 | 位置 |
|---|---|---|---|
| 角色卡 | 50 | 每一棒 | role.rs:46 |
| `[技能規範指引]` | 120 | 該棒有技能 | persona.rs:27 |
| `[相關記憶]` | 最多 5 筆 × 800 | 有召回結果 | `crates/wukong-gateway/src/prompt.rs:9` |
| `[前序協作]` | 前面各棒的全文 | 第 2 棒起 | orchestrator lib.rs:40 |
| `[其他回合正在執行]` | 150 起 | 其他 scope 有 lease | turn.rs:250、persona.rs:106 |
| `[輸出要求]` | 110 | 最後一棒，一律 | persona.rs:48 |
| `[排程能力]` | 900 | 最後一棒，一律；內含這個回合的 `--scope` | persona.rs:138 |
| `[長期記憶能力]` | 550 | 最後一棒，`WUKONG_MEMORIA_HINT` 開啟時 | persona.rs:67-101 |
| `[Wukong 檔案互動規則]` | 170 | Telegram 入口加在 input 裡 | `crates/wukong-telegram/src/dispatch.rs:933-940` |
| `[無人值守排程]` | 110 | 排程入口加在 input 裡 | `crates/wukong-scheduler/src/executor.rs:11,383` |

`remember` 寫入 `User: {user_text}` 與 `Assistant: {answer}`（turn.rs:367-386）。`user_text` 是 input 在檔案互動規則標頭之前的部分（turn.rs:168）：檔案互動規則不寫入記憶，`[無人值守排程]` 則會寫入並參與召回。

### 5. 人格與 SOUL.md

- **SOUL.md 沒有被讀**：Rust、OpenCode 設定、`instructions` 都沒有用到它。映像與 installer 只是把它複製到工作區（Dockerfile:71、docker-entrypoint.sh:94-98、install.sh:384）。
- **`WUKONG_PERSONA` 常數**（persona.rs:8）沒有任何地方使用。
- **人格實際的來源**：`workspace/AGENTS.md` 第 3 行「你目前作為孫悟空」。OpenCode 會把它當專案指示自動讀入。
- **既有部署收不到範本更新**：`workspace/AGENTS.md` 只在工作區沒有這個檔案時才複製（docker-entrypoint.sh:99-103、install.sh:385）。
  - 2026-11-26 比對正式部署 RunWuKong 的工作區：AGENTS.md 與 ac60ebe 版的範本完全相同，比目前的範本少了「兩套記憶不要混用」等段落。
  - SOUL.md 與目前的範本相同。

### 6. 壓縮與輪替

- **依回合數壓縮**：`prepare_final_session_with_policy`（`crates/wukong-runtime/src/session.rs:69`）先拿 lease。
  - `turn_count` 達到 `WUKONG_SESSION_COMPACT_EVERY_TURNS`（預設 20，設 0 關閉）時，以 `default_model` 壓縮。
  - 回 404 或 405 時改成輪替；其他失敗就延後 N 回合（session.rs:161-196）。
- **7 天輪替（#28）**：判斷只在壓縮檢查點進行，依 `session_created_ms`（session.rs:101-127）。CLI backend 沒有建立時間，因此不會輪替。
- **保留期**：schedulerd 每 6 小時刪除沒有被引用、超過 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS`（預設 30 天）的 session（`crates/wukong-runtime/src/session_retention.rs`）。
- **OpenCode baseline**：`compaction.auto` 為 true、`background` 為 false（docker-entrypoint.sh:195-235）。

### 7. 角色與中間步驟的顯示

- **兩個回呼**：
  - `on_role(Role)` 在每一棒開始時觸發（turn.rs:220）。
  - `on_step` 只在輔助棒有非空輸出時觸發（turn.rs:295）。
- **各入口怎麼顯示**：

| 入口 | 現在看到的 |
|---|---|
| CLI | stderr 印「🐵 悟空·{角色}」（main.rs:489） |
| Web | SSE `role` 與 `step` 事件。前端顯示「🐵 悟空·X 思考中…」，加一張可收合的「悟空·角色 + 技能 的產出」卡片（`crates/wukong-web/static/components/wukong-chat.js:966-1006`）。步驟存進 `turn_steps` 表（`crates/wukong-chat-history/src/lib.rs:164,329`） |
| Telegram | 狀態泡泡改成「🐵 悟空·{角色} 思考中…」；`on_step` 不做事（dispatch.rs:1575-1580） |
| 排程 | 執行訊息的階段是「規劃中」→「第 k 棒 {角色}」（executor.rs:270-330） |

### 8. 同一 scope 的並行回合

- 每個 scope 有一個排他的 lease（`agent_session_state`，`crates/wukong-memory/src/store/mod.rs:563-603`）。
- 同一個 scope 的第二個回合立即以「session lease unavailable」失敗（session.rs:80），不排隊，而且已經花掉一次 planner 呼叫。
- 回合進行中每 lease/3 續約一次（預設 900 秒）；出錯時也會釋放。
- 不同 scope 之間只附加提示，不互相阻擋。

### 9. 空輸出補救與上游錯誤

- **空輸出補救**（turn.rs:305-361）：
  1. 最後一棒沒有文字時，改用最近一個有輸出的輔助棒。
  2. 全部都空時，在最後一棒的 session 補送一次 `[修復回覆]`。
  3. 還是空就回「(本回合未產生文字輸出)」。
- **上游錯誤分類**在 gateway 裡完成：`backend.rs:601`、`upstream_error.rs`、`local_process.rs:417-424`。步驟迴圈只用 `?` 往外傳。排程在執行訊息裡顯示分類。

### 10. 依賴假 planner 輸出的測試

- **共用的假 agent**：`scripts/test-support/cli_backend.rs:26` 在非串流呼叫時印出 `fixer|none`。
  - 用到它的有 `crates/{wukong-cli,wukong-web,wukong-telegram,wukong-scheduler}/tests/cli_backend.rs` 與 `scripts/test-support/managed_actual.rs`。
  - `crates/wukong-cli/tests/opencode_version.rs:24` 也用 `printf 'fixer|none'`。
- **mock 回覆佇列的第一筆是 planner 的輸出**：turn.rs 的測試約 38 個，web 約 17 個、telegram 約 11 個、scheduler 約 12 個、repl 約 4 個，另有 orchestrator 的單元測試。
- 合計約 110～130 個測試的寫法要改。

### 11. 效能基準

- 量測工具是 `docs/issues/issue-0030/scripts/bench-turn.sh`（不在 `scripts/`），由 `docs/issues/issue-0044/scripts/bench-v1-v2.sh` 呼叫。
- 每回合輸出一行 NDJSON：耗時、成功與否、CPU、throttle、記憶體峰值、opencode.db 增量。沒有逐步的拆解。
- V2 的基準在 [issue-0044/bench-v1-v2.md](../issue-0044/bench-v1-v2.md)：NVIDIA nemotron，short 與 tool 各 7 回合全部成功。#30 量到每回合呼叫上游兩次（plan 與 build），本機模式啟動兩次 opencode，合計約 2 秒。

## 問題點總結

1. 每回合多一次 planner 的 LLM 呼叫，本機模式還要多啟動一次 opencode；同一個 scope 的回合被 lease 拒絕前，planner 呼叫已經花掉了。
2. 輔助棒沒有 session，只好另外補上一回合；也不套用 `default_model`，可能用到預設模型（W-7 的同類風險）。
3. 固定提示每回合寫進 session 歷史，占用 context，也讓 opencode.db 多存一份。
4. 壓縮做兩次：Wukong 依回合數壓縮，OpenCode 也依 context 壓力自動壓縮。
5. 人格不是來自宣稱的 SOUL.md，而是 AGENTS.md。範本只在工作區沒有這個檔案時才複製，既有部署收不到更新，正式部署還叫模型去讀舊的技能路徑。
6. 角色顯示與排程的階段文字都綁在 `Role` 與「棒次」上。

## 目標

- 每回合只呼叫一個 primary agent（悟空）。由它決定直接回答，或派工給五個 subagent 之一。
- 技能改走 V2 的 skill。
- 固定提示移到 agent 定義與 session 指令層。
- Wukong 不再依回合數壓縮。
- 人格的來源明確，定義由 Wukong 管理並隨版本更新。
- 入口顯示的角色改從 V2 的 subagent 資訊取得。
- 「留在 Wukong」清單裡的行為維持不變。
