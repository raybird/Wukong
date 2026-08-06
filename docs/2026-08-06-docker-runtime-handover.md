# Docker Runtime Handover

日期：2026-08-06

部署目錄：`~/Documents/RunWuKong`

事故版本：`ghcr.io/raybird/wukong:v0.18.7`

關聯規劃：`docs/superpowers/specs/2026-08-06-opencode-permission-hang-remediation-design.md`

## 摘要

最近幾次 scheduler 任務失敗的主要原因不是 Docker container crash，而是
OpenCode server 在無人值守任務中提出 `external_directory` 權限詢問，scheduler
沒有回覆，導致 session 一直等待，最後被 Wukong 的 1200 秒 agent timeout 中止。

根因已由三段資料互相對上：

1. OpenCode log 出現 `permission=external_directory patterns=["/tmp/*"]`。
2. 同一個 SSE stream 之後沒有 permission reply，並在 stream 開始後達到 1200 秒
   時被記錄為 `stream timed out before session became idle`。
3. scheduler 呼叫 `run_turn` 時把 event callback 設為 no-op，沒有處理
   `QuestionRequest`。

這份文件只記錄本次事故的分析與交接，不包含 runtime 修復程式碼。

## 觀測到的 Runtime 狀態

最後觀測時間：2026-08-06 08:19 (+08:00)

- `wukong-opencode-server`、`wukong-schedulerd`、`wukong-telegram`、
  `wukong-web` 都是 running。
- `opencode-server` 與 `wukong-web` healthcheck 通過。
- 所有 Wukong container `RestartCount=0`。
- 沒有 OOM kill，Docker 狀態不是 container crash。
- `WUKONG_AGENT_TIMEOUT_SECS=1200`。
- OpenCode 版本是 `1.17.18`。
- OpenCode database 約 `1.4 GiB`，主機檔案系統尚有約 `55 GiB` 可用。
- SQLite `integrity_check` 當時回報 `ok`。
- `scheduler_runs.id=574` 的 `GitHub Trending 每日推送` 在最後觀察時仍是
  `running`，並已出現同樣的 permission request。

## 根因分析

### 1. Server backend 沒有使用 CLI 的 skip-permissions

Compose 為長駐服務設定了：

```text
WUKONG_AGENT_SERVER_URL=http://opencode-server:4096
```

Wukong 因此選擇 `OpencodeServerBackend`，而不是執行
`WUKONG_AGENT_CMD` 的 CLI backend。雖然 `WUKONG_AGENT_CMD` 預設包含：

```text
opencode run --dangerously-skip-permissions
```

這個 flag 不會套用到獨立執行的 `opencode serve` process。

依據：

- `crates/wukong-gateway/src/backend.rs:189-195`
- `docker-compose.yml` 的 `WUKONG_AGENT_SERVER_URL` 設定
- `opencode serve --help` 沒有提供同等的 `--dangerously-skip-permissions` 選項

### 2. 實際 opencode.json 沒有 external_directory 規則

容器內目前的 config 只有 `$schema` 與 `permission.bash` 規則，沒有
`permission.external_directory`。OpenCode 對外部工作目錄預設採用 `ask`，因此
agent 嘗試存取 `/tmp/*` 時會停在 permission prompt。

官方說明：<https://opencode.ai/docs/permissions/>

OpenCode log 在近期至少記錄了以下四次同類詢問：

```text
2026-08-04 19:01:03  run=801422e0  /tmp/*
2026-08-05 08:02:10  run=2b634746  /tmp/*
2026-08-06 07:56:19  run=808fb0ad  /tmp/*
2026-08-06 08:13:28  run=808fb0ad  /tmp/*
```

這些 permission id 沒有對應的 reply log。

### 3. Scheduler 忽略了 permission event

OpenCode 的 `permission.asked` 會被轉成 `StreamEvent::QuestionRequest`：

- `crates/wukong-gateway/src/opencode_server/event_map.rs:269-275`

Telegram 互動路徑具備 `reply_permission`，但 scheduler 執行排程工作時使用
空 callback：

- `crates/wukong-scheduler/src/executor.rs:85-103`
- `crates/wukong-gateway/src/opencode_server.rs:375-391`

因此排程工作既沒有允許，也沒有拒絕，而是持續等待。

### 4. 固定 1200 秒 deadline 將等待轉成失敗

`consume_event_stream` 在每次 event-stream consumption 開始時建立一次
`agent_timeout()` deadline，並等待 `session.idle`。它不是單純的 idle-gap timeout，
而是該次 streaming backend call 的固定 wall-clock deadline；一個多步驟 turn 的
不同 stream 會各自計時：

- `crates/wukong-gateway/src/opencode_server.rs:480-531`
- `crates/wukong-gateway/src/backend.rs:12`
- `crates/wukong-gateway/src/backend.rs:453-459`

因此錯誤訊息雖然寫成「before session became idle」，實際上本次是 permission
prompt 長時間未回覆後觸發總時限。

## 失敗時間線

以下時間均為主機時區 `+08:00`。

| Run | 任務 | 時間 (+08:00) | 結果 | 判斷 |
|---|---|---|---|---|
| 563 | Mythos Atlas 衝刺深化 | 08-04 18:43:15-19:12:02 | stream timeout | 19:01:03 先出現 `/tmp/*` permission ask |
| 566 | Mythos Atlas 深化 | 08-05 07:52:45-08:13:00 | stream timeout | 08:02:10 先出現 `/tmp/*` permission ask |
| 567 | Mythos Atlas 衝刺深化 | 08-05 08:13:01-08:26:12 | SSE unexpected EOF | 前一輪 timeout 後的連線/清理連帶錯誤 |
| 568 | Mythos Atlas 分析 | 08-05 08:26:13-08:26:18 | DNS failure | OpenCode server 切換窗口的暫時性錯誤 |
| 569 | GitHub Trending 每日推送 | 08-05 08:26:37-08:27:27 | session 404 | 舊 session 已不存在，與前一輪中止後的 session 狀態有關 |
| 570 | Mythos Atlas 分析 | 08-06 07:31:58-07:45:53 | SSE unexpected EOF | 與 v0.18.7 container rollout 時間重疊 |
| 571 | Mythos Atlas 深化 | 08-06 07:45:54-07:45:59 | DNS failure | 部署啟動期間的暫時性解析失敗 |
| 572 | Mythos Atlas 衝刺深化 | 08-06 07:46:00-07:46:18 | interrupted | scheduler process 被替換，啟動時已 recovery |
| 573 | Mythos Atlas 衝刺深化 | 08-06 07:46:18-08:08:15 | stream timeout | 07:56:19 permission ask，08:08:14 被 deadline 中止 |
| 574 | GitHub Trending 每日推送 | 08-06 08:12:34-最後觀察仍 running | 尚未完成 | 08:13:28 再次出現 `/tmp/*` permission ask |

`run=808fb0ad` 的 OpenCode log 顯示，run 573 在 07:56:15 跑到 step 40，
四秒後發生 permission ask，直到 08:08:14 被 cancel，中間沒有 idle event。

## 其他風險與放大因素

### v0.18.7 CPU guardrail 尚未套用

`opencode-config` 是持久 volume，seed script 只會在 `opencode.json` 不存在時
寫入。這次部署使用的是 6 月建立的舊 config，因此以下設定都不存在：

- `snapshot: false`
- `compaction.auto` / `compaction.prune`
- `tool_output` 上限
- `watcher.ignore`

這不直接造成 permission hang，但會讓長對話持續增加 event log 與 SQLite 寫入
成本。當時 OpenCode database 約 1.4 GiB，event 約 354,608 筆，part 約 123,314
筆。

### Session 清理一致性問題

較早的 OpenCode log 曾出現：

```text
FOREIGN KEY constraint failed
Session not found: ses_0d26d72abffeF0N0i4HhS66V7A
```

這與 timeout/abort 後 session 被清理，但 OpenCode 仍嘗試寫入 part 的可能競態
一致。當時的整體 SQLite integrity check 仍通過，因此目前不能判定為整個 DB 損壞，
應視為需要持續監控的次要問題。

## 2026-08-07 追記：v0.19.0 修復後狀態

### 已套用的修復

- `main` 已 fast-forward 至 `7b5f20a`，包含 permission hang 的完整修復鏈。
- `~/Documents/RunWuKong` 現在使用 `ghcr.io/raybird/wukong:v0.19.0`，OpenCode
  版本為 `1.18.14`。
- `opencode.json` baseline、`user.json`、`.wukong-baseline` 與舊設定 backup 都已
  存在，`external_directory` 的 `/tmp` 三種 pattern、compaction 與 CPU guardrail
  已實際載入。
- `WUKONG_SCHED_PERMISSION=reject` 已生效；streaming scheduler turn 的 permission
  request 會即時拒絕，不再被動等待 1200 秒。非 streaming 路徑的限制見下方高風險 2。
- 四個 container 啟動後 healthcheck 通過，`RestartCount=0`，沒有 OOM。

### 高風險 1：回覆失敗時可能跳過 abort 與 cleanup

Scheduler 在 permission 回覆重試 3 次仍失敗時，會在
`crates/wukong-scheduler/src/executor.rs:209-225` 跳出 `select!`，並 drop 正在執行
的 `run_turn` future。

但 OpenCode server 只有在 `consume_event_stream` 正常回傳 error 後，才會執行
`abort_session` 與 session cleanup：

- `crates/wukong-gateway/src/opencode_server.rs:637-645`
- `crates/wukong-runtime/src/turn.rs:393-398`

因此 cancellation path 可能不會送出 `/session/{id}/abort`，也可能跳過 runtime
session lease release，留下仍在 server 執行的 prompt 或 zombie session，重新引發
session 404、foreign key constraint 或後續 lease 卡住。現有 regression test 使用 fake
backend，只驗證 scheduler 很快返回，尚未驗證真實 OpenCode server 的 abort/cleanup。

處理方向：為 streaming backend 加 cancellation-safe cleanup，或在 scheduler 中止 turn
前顯式 abort、釋放 session lease，並新增真實 server cancellation integration test。

### 高風險 2：非 streaming 路徑仍繞過 permission policy

新的 `PermissionPolicy` 只接在 `run_unattended_turn` 的 streaming event channel。
以下路徑仍使用 `run_ephemeral`，沒有 `QuestionResponder`：

- Planner：`crates/wukong-runtime/src/turn.rs:172-179`、
  `crates/wukong-orchestrator/src/router.rs:226-244`
- Memory summarizer：`crates/wukong-gateway/src/summarize.rs:25-42`
- Automatic maintenance：`crates/wukong-schedulerd/src/main.rs:107-129`

這些呼叫透過同步的 `POST /session/{id}/message` 等待回覆；若工具觸發
`external_directory` permission，scheduler 沒有事件 channel 可以處理，仍可能等待
到 20 分鐘 client timeout。Planner 的 `tool_overrides` 只關閉 `question` 工具，不能
阻止 OpenCode server 在工具執行前攔截 permission。

處理方向：讓非 streaming backend call 共用 unattended permission responder，或明確
禁止這些路徑使用會觸碰外部目錄的工具；新增 planner、summarizer、maintenance 的
permission hang regression test。

### 中風險：拒絕 permission 後仍可能被記為成功

如果 OpenCode 在收到 reject 後仍能完成回合，scheduler 會把 job 記為 success，僅在
輸出附加 `[無人值守權限]`。這可能把「部分工作被拒絕」誤看成完整成功，監控與通知
需同時檢查該區塊，或另訂 permission-denied 的 job status。

### Production 驗證缺口

本次已完成 workspace tests、clippy、fmt、Docker runtime checks 與 release workflow
checks；但 v0.19.0 啟動後尚未觀察到新的 scheduler run。OpenCode log 最後一筆
`external_directory` ask 發生在 v0.19.0 啟動前，因此目前只能確認 baseline 已載入，
不能宣稱修復已完成 production end-to-end 驗證。

## 建議處理順序

### P0：解除 scheduler permission hang（v0.19.0 已實作，待 production 驗證）

v0.19.0 已由 entrypoint baseline 自動寫入規則；以下保留給仍在 v0.18.7 或未完成
baseline migration 的部署使用，先處理反覆出現的 `/tmp/*`：

```json
{
  "permission": {
    "external_directory": {
      "/tmp/**": "allow"
    }
  }
}
```

實際操作時要保留既有的 `permission.bash` deny 規則，合併後重啟
`opencode-server`。這是共用 OpenCode server 的全域 policy，會同時影響 Web、
Telegram 與 Scheduler；`/tmp/**` 也會允許所有會觸碰該路徑的 path-based tools，
不只目前這個 scheduler job。如果後續 log 出現 `/proc/*`、`/etc/*`、
`/home/wukong/*` 或其他路徑，應逐項審核後再放行，不要直接無限制允許所有外部
目錄。若需要更寬鬆的 scheduler policy，應考慮獨立 server/config，而不是放寬
所有共用 client。

### P0：修正 scheduler 的無人值守權限策略（v0.19.0 已實作，仍需補 cancellation）

v0.19.0 已將 no-op callback 改為明確的 `PermissionPolicy`；以下列出仍需完成的
cancellation 與驗收事項：

- 對不允許的 permission request 立即 reject 並記錄路徑與 permission id。
- 如果部署明確信任 container 隔離，再提供可配置的 auto-allow policy。
- 對 scheduler job 回報「permission denied」而不是等待到 1200 秒才失敗。

### P1：重新評估 agent timeout

權限問題修好後，如果單一 Mythos build stream 本身仍經常接近或超過 20 分鐘，
再將 `WUKONG_AGENT_TIMEOUT_SECS` 提高到 2400 或 3600。多步驟 turn 的總時間可以
超過 20 分鐘而不代表單一 stream 逾時；單純提高 timeout 也不能解決 permission
hang，只會把等待時間拉長。

### P1：套用長對話護欄（v0.19.0 baseline 已套用）

仍在 v0.18.7 或未完成 baseline migration 的部署，需手動合併 `snapshot`、
`compaction`、`tool_output`、`watcher.ignore` 設定並重啟 OpenCode server；v0.19.0
應確認 baseline 已載入。注意 `snapshot: false` 會停用 OpenCode 自己的 revert/undo，
但不會影響 workspace 的 Git 歷史。

### P2：保留狀態後再處理 database 成長

不要直接使用 `docker compose down -v`，避免刪除 `opencode-config`、
`opencode-state`、`agent-reach-state`、`gh-config` 與 `wukong-data`；其中
`agent-reach-state`、`gh-config` 可能包含登入或認證狀態。在任何 DB cleanup、
VACUUM 或 volume 重建前，先備份現有 state，並保留 `opencode.log` 與
`scheduler_runs` 作為事故證據。

## 驗證清單

修復後應確認：

- 新 scheduler job 不再停在 `message=asking permission=external_directory`。
- 若仍出現 permission request，OpenCode log 對 permission id 應有 reply 或明確
  reject；規則正確放行時，預期不再出現該 request。
- scheduler run 能在 OpenCode 回傳 `session.idle` 後完成。
- 不再出現同一 job 的 1200 秒 stream timeout。
- `opencode.db` 增長速度、CPU 與 block I/O 下降。
- 若發生 timeout，錯誤能清楚指出是模型/工具逾時，而不是模糊的 idle timeout。
- permission reply 失敗時，OpenCode session 有收到 abort，Wukong session lease 有釋放。
- planner、memory summarizer、automatic maintenance 觸發 permission 時不會等待到
  1200 秒。
- permission 被拒絕但 job 仍成功時，監控能辨識 `[無人值守權限]` 並標記為部分成功。
