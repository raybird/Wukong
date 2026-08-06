# OpenCode Permission Hang 修復規劃

日期：2026-08-06

關聯事故文件：`docs/2026-08-06-docker-runtime-handover.md`

基準版本：`ghcr.io/raybird/wukong:v0.18.7`

## 背景

事故本身已由 handover 完整記錄：共用的 `opencode serve` 對 `/tmp/*` 提出
`external_directory` 權限詢問，scheduler 不會回覆，session 持續等待，最後被
`WUKONG_AGENT_TIMEOUT_SECS=1200` 的固定 deadline 中止。

本文件在該分析上補三點範圍修正，並據此排定修復項目：

1. **會 hang 的不只 scheduler。** `permission-` 前綴的解碼只存在於 Telegram，
   Web 與 CLI 都無法回覆權限詢問；Web 甚至會把回覆送到錯誤的端點。
2. **只熱修容器內的 `opencode.json` 不算修好。** seed script 缺同一條規則，
   任何全新部署或重建 volume 都會完整重演本次事故。
3. **既有的 non-interactive 防護在結構上擋不住 permission。** 那是 prompt 層
   約束，permission 由 OpenCode server 在工具執行前攔截，模型再聽話也無效。

## 目標

- 讓無人值守的 scheduler 工作不再因未回覆的權限詢問而停到 1200 秒。
- 讓 Web 與 CLI 具備與 Telegram 一致的權限回覆能力。
- 讓新部署（全新 volume）預設就帶有本次事故所需的權限規則。
- 讓逾時錯誤能指出真正原因，而不是統一顯示為 idle timeout。
- 保留現有 `permission.bash` 破壞性刪除護欄與所有持久 volume 狀態。

## 非目標

- 不無限制放行所有外部目錄；只逐項放行已觀測到的路徑。
- 不在本次調高 `WUKONG_AGENT_TIMEOUT_SECS`；權限修好前調高只會拉長等待。
- 不重構 `consume_event_stream` 的 deadline 模型（固定 wall-clock 改 idle-gap
  另案評估）。
- 不做 OpenCode database 的 VACUUM 或 volume 重建。
- 不追溯重跑歷史失敗的 run 563–574。

## 現況查核

以下皆已對照原始碼確認：

| 事實 | 依據 |
|---|---|
| 設了 `WUKONG_AGENT_SERVER_URL` 就走 Server backend，`WUKONG_AGENT_CMD` 整組被忽略 | `crates/wukong-gateway/src/backend.rs:189-196`；`docker-compose.yml:83,128,181` |
| seed 的 `opencode.json` 只有 CPU guardrails 與 `permission.bash` | `scripts/docker-entrypoint.sh:139-183` |
| `permission.asked` 轉成 `QuestionRequest`，`request_id` 帶 `permission-` 前綴 | `crates/wukong-gateway/src/opencode_server/event_map.rs:182,269-276` |
| 剝前綴的 `permission_id()` 只在 Telegram | `crates/wukong-telegram/src/dispatch.rs:147-148,168-176,197-199` |
| Web 把 `permission-xxx` 當成 question id 送出 | `crates/wukong-web/src/lib.rs:96-134` → `question_reply_url` (`opencode_server.rs:941-945`)，正確端點應為 `permission_reply_url` (`opencode_server.rs:955-957`) |
| CLI 直接忽略 `QuestionRequest` | `crates/wukong-cli/src/render.rs:37` |
| scheduler 使用空 callback | `crates/wukong-scheduler/src/executor.rs:94-102` |
| 現有 non-interactive 防護是 prompt hint | `crates/wukong-scheduler/src/executor.rs:8`（commit `ca7d703`） |
| 1200 秒是每次 stream 的固定 wall-clock deadline，非 idle-gap | `crates/wukong-gateway/src/opencode_server.rs:491-503`；`backend.rs:453-460` |

## 工作項目

### W1（P0）補上 `external_directory` 規則，並同步 seed script

**問題**：容器內 config 與 seed script 都沒有 `permission.external_directory`，
OpenCode 對外部工作目錄預設 `ask`。

**變更**：

- 修改 `scripts/docker-entrypoint.sh` 的 seed 區塊，在 `permission` 下加入
  `external_directory` 規則。
- 以相同內容合併進 `~/Documents/RunWuKong` 既有的 `opencode-config` volume，
  保留現有 `permission.bash` 全部規則，重啟 `opencode-server`。

**前置驗證**：實作前必須先確認 opencode `1.17.18` 的 `external_directory`
接受的形態。log 回報的 pattern 是 `/tmp/*`，而直覺寫法是 `/tmp/**`，兩者在多數
glob 實作下涵蓋範圍不同（`/tmp/**` 通常不匹配 `/tmp` 本身）。可能的形態：

```json
{ "permission": { "external_directory": { "/tmp": "allow", "/tmp/**": "allow" } } }
```

或僅接受純字串 `"external_directory": "allow"`。確認前不要寫死任一種。

**範圍聲明**：這是共用 OpenCode server 的全域 policy，會同時影響 Web、Telegram
與 Scheduler，也會放行所有觸碰該路徑的 path-based tools。若之後 log 出現
`/proc/*`、`/etc/*`、`/home/wukong/*`，逐項審核後再放行。需要更寬鬆的 scheduler
policy 時，應開獨立 server/config，不要放寬共用 client。

**驗收**：新容器（全新 volume）啟動後 config 即含該規則；重放同類任務不再出現
`permission=external_directory patterns=["/tmp/*"]`。

### W2（P0）權限回覆邏輯上收 gateway，Web 與 CLI 共用

**問題**：`permission_id()` 只存在於 Telegram。Web 會把 `permission-xxx` 送到
`/api/session/{id}/question/permission-xxx/reply`，正確端點是
`/permission/{id}/reply`，因此使用者在 Web Console 按「允許」也回不去，權限依然
懸著，一樣撞 1200 秒。CLI 則連詢問都不顯示。

**變更**：

- 把 `permission_id()` 與 `permission_reply_from_answers()` 的分派邏輯從
  `crates/wukong-telegram/src/dispatch.rs` 抽到 `wukong-gateway`，成為
  `AgentBackend` 層的共用行為：`reply_question` / `reject_question` 收到帶
  `permission-` 前綴的 `request_id` 時自動改走 `reply_permission`。
- `wukong-web` 改用該共用路徑（`lib.rs:96-134`）。
- `wukong-telegram` 移除重複實作，行為不變。
- CLI 至少讓 `QuestionRequest` 可見（`render.rs:37`），互動回覆能力視 REPL 現況
  另行決定。

**驗收**：新增 gateway 層單元測試涵蓋「`permission-` 前綴 → permission 端點」與
「一般 question id → question 端點」兩條分支；Web Console 對權限詢問按下允許／
拒絕後，OpenCode log 對該 permission id 有對應 reply。

### W3（P0）scheduler 的無人值守權限策略

**問題**：`executor.rs:94-102` 兩個 callback 都是 no-op，排程工作既不允許也不
拒絕。`SCHEDULED_TURN_AUTONOMY_HINT` 只約束模型呼叫 `question` 工具，對 server
端攔截的 permission 無效。

**變更**：

- scheduler 改用具名 callback，收到 `QuestionRequest` 時依策略處置：
  - 預設 **立即 reject**，並記錄 permission id、permission 類型與 patterns。
  - 提供可設定的 auto-allow policy，僅在部署明確信任 container 隔離時開啟。
- job 結果回報「permission denied（含路徑）」，而不是等 1200 秒後回報逾時。
- 另一條可選路徑：透過 `AgentRequest.tool_overrides`
  (`crates/wukong-gateway/src/backend.rs:33-34`) 對排程回合停用會觸發外部目錄
  存取的工具。此為補充手段，不取代上述策略。

**驗收**：新增 `wukong-scheduler` 回歸測試——mock backend 送出 `QuestionRequest`
後，job 應在秒級以 permission denied 收尾，而非等到 timeout。

### W4（P1）讓逾時錯誤可診斷

**問題**：`opencode_server.rs:499-503` 對所有逾時都回報
`stream timed out before session became idle`，掩蓋了「有未回覆的 permission」
這個真正原因。

**變更**：逾時訊息附帶最後一個已知 event 型別，以及是否存在未回覆的 permission
request（含其 id）。不改動 deadline 的計時模型。

**驗收**：在權限被刻意擱置的情境下，錯誤訊息能指出未回覆的 permission id。

### W5（P1）套用 v0.18.7 長對話護欄

**問題**：`opencode-config` 是持久 volume，seed 只在缺檔時寫入，現行部署用的是
6 月建立的舊 config，缺 `snapshot: false`、`compaction`、`tool_output`、
`watcher.ignore`。

**變更**：手動把這四項合併進既有 config 後重啟 OpenCode server。可與 W1 的
config 編輯合併為同一次操作與同一次重啟。

**注意**：`snapshot: false` 會停用 OpenCode 自己的 revert/undo，不影響 workspace
的 Git 歷史。

**驗收**：`opencode.db` 增長速度、CPU 與 block I/O 下降。

### W6（P2）保留狀態後再處理 database 成長

**問題**：OpenCode database 約 1.4 GiB（event 約 354,608 筆、part 約 123,314
筆）；另有 `FOREIGN KEY constraint failed` / `Session not found` 的清理競態跡象，
但 `integrity_check` 通過。

**變更**：先備份，再評估清理。**不得使用 `docker compose down -v`**，避免刪除
`opencode-config`、`opencode-state`、`agent-reach-state`、`gh-config` 與
`wukong-data`；其中 `agent-reach-state`、`gh-config` 可能含登入或認證狀態。
保留 `opencode.log` 與 `scheduler_runs` 作為事故證據。

**驗收**：備份存在且可還原後，才進行任何 cleanup 或 VACUUM。

## 執行順序與相依

```text
W1（config + seed）──┬─→ 部署驗證 ─→ W5（可併同一次重啟）
W2（gateway 共用）──┴─→ W3（scheduler 策略，依賴 W2 的共用分派）
W4（錯誤訊息）獨立，可隨時進行
W6 需在 W1/W5 生效、系統穩定後再排
```

建議實作順序：**W2 → W3 → W1 → W5 → W4 → W6**。

W2 是純程式碼修正、有明確可寫測試的 bug、且橫向影響三個進入點，適合先做；
W1 需要先做 opencode 設定語法的外部確認，不宜擋住其他工作。

## 測試與驗證

### 原始碼

```bash
cargo test -p wukong-gateway
cargo test -p wukong-scheduler
cargo test -p wukong-web
cargo clippy --all-targets -- -D warnings
```

### 部署後

```bash
# 1. 確認 config 已含新規則（且保留既有 bash deny）
docker compose exec opencode-server cat /home/wukong/.config/opencode/opencode.json

# 2. 觸發一次排程工作並觀察結果
wukong schedule runs --limit 10
wukong schedule trigger --id <job-id>

# 3. 檢查 permission 詢問與回覆
docker compose logs opencode-server | grep -i "permission="
```

### 通過條件

- 新 scheduler job 不再停在 `message=asking permission=external_directory`。
- 若仍出現 permission request，log 對該 permission id 應有 reply 或明確 reject；
  規則正確放行時，預期不再出現該 request。
- scheduler run 能在 OpenCode 回傳 `session.idle` 後正常完成。
- 同一 job 不再出現 1200 秒 stream timeout。
- Web Console 對權限詢問的允許／拒絕能實際送達 OpenCode。
- 全新 volume 啟動的容器，config 預設即含 `external_directory` 規則。
- 若仍發生逾時，錯誤訊息能指出是模型／工具逾時或未回覆的 permission。

## 風險與回滾

| 風險 | 處置 |
|---|---|
| `external_directory` 規則語法猜錯，OpenCode 啟動失敗或規則不生效 | 修改前備份 `opencode.json`；重啟後先確認 server healthcheck 通過再觸發任務 |
| 放行 `/tmp` 擴大共用 server 的權限面 | 只放行已觀測路徑；新路徑逐項審核；必要時改為 scheduler 專用 server/config |
| scheduler 預設 reject 造成原本可完成的任務改為失敗 | 失敗訊息帶上被拒的路徑，據此決定是加白名單還是改任務寫法；auto-allow 為可設定選項 |
| gateway 分派邏輯上收造成 Telegram 行為回歸 | 移轉時保留原有測試（`dispatch.rs:1714` 等），並在 gateway 層補等價測試 |
| `snapshot: false` 停用 OpenCode revert/undo | 已知取捨；workspace 的 Git 歷史不受影響 |

## 待確認事項

1. opencode `1.17.18` 的 `permission.external_directory` 實際接受的設定形態與
   pattern 語法（阻擋 W1 實作，其餘工作不受影響）。
2. handover 記錄 `scheduler_runs.id=574` 在最後觀察時仍是 `running`，需補上其最終
   狀態，事故證據鏈才完整。
3. scheduler auto-allow policy 的設定介面：環境變數、`.wukong/settings.toml`，
   或 per-job 欄位。
4. CLI／REPL 是否要做到完整互動回覆，或僅顯示權限詢問即可。
