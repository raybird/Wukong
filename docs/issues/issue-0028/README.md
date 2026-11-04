# Issue 28 - opencode.db 與聊天即時事件長期無上限成長

## 概述
長期運行時，下列三處資料沒有上限地成長。本 issue 為三者各自補上清理：
- **A**：本機模式下，有明確設定保留天數時，也執行 opencode session 保留期清理。
- **B**：長期 scope 的 opencode session 建立超過固定天數後，在下一個 compaction 時點輪替，舊 session 刪除。
- **C**：定期刪除超過 1 天的 `chat_live_events`。

不在範圍：Wukong 記憶（已有自動摘要封存，`wukong-runtime/src/maintenance.rs`）、`chat_messages`／`turn_steps`／`turn_events`／`scheduler_runs`／`recall_telemetry` 的保留期、`memory.db` 的 vacuum、本機模式的 `opencode.db` vacuum。

## 文件清單
- [implementation-plan.md](./implementation-plan.md)

## 快速導覽

### 當前流程
- v0.22 起預設是按需本機模式（`WUKONG_AGENT_SERVER_URL` 留空，`AgentBackend::Local`）。`wukong-schedulerd` 的保留期清理只對 `AgentBackend::Server` 啟用，所以預設部署的 `opencode.db` 完全不清。
- 保留期清理不刪仍被 scope 指向的 session。Telegram chat 與每個排程（`agent:schedule-<id>`）都綁著一個一直續接的 session，每 20 回合做一次 compaction。compaction 只縮短送給模型的 context，資料庫裡的 event/part 不會減少。只有 server／本機 backend 的 summarize 回 404／405（例如 session 已不存在）時才會輪替。
  - NOTE（2026-11-03 更正）：原本寫成「只有 compaction 不被支援時（純 CLI backend）才會輪替」，那是只讀 runtime 的 `is_unsupported_compaction` 就下的推論。純 CLI backend 的錯誤碼是行程結束碼（`backend.rs:470`、`:518`），不會是 404／405；它的 compaction 失敗時走一般失敗路徑，會把回合計數歸零，不會輪替。
- Telegram 回合把即時事件寫進 `chat_live_events`，Web 從最新的 id 開始讀。`prune_live_events_before` 已經實作，但沒有任何呼叫端。

### 變更後流程
- 本機模式：有明確設定 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` 時，schedulerd 照 server 模式的規則清理（間隔、每輪上限、受保護的 session 都相同）。compose 會為每個服務設定這個變數，所以容器部署會清；主機上沒有設定就不清。
- 回合數到達 compaction 門檻（預設每 20 回合）時，先查詢 scope 目前 session 的建立時間（opencode 的 `time.created`）。超過輪替門檻（暫定 7 天）就不做 compaction，改用新的 session，取得新 session 後刪除舊的。輪替的那一回合，最後一棒另外注入上一回合。其他回合不查年齡。
- schedulerd 的維護迴圈（預設每 900 秒）同時刪除建立超過 1 天的 live event。

## 關鍵差異
| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| 本機模式，有設定保留天數（容器部署） | 不清 | 依保留天數清理 |
| 本機模式，沒有設定保留天數（主機上執行） | 不清 | 不變 |
| server 模式 | 沒有設定時預設 30 天 | 不變 |
| 本機模式的 `opencode.db` vacuum | 不做 | 不變（刪除後空出的頁面會被新資料重複使用） |
| scope 的 session 建立超過輪替門檻 | 一直續接 | 下一個 compaction 時點（預設每 20 回合）改為輪替，舊 session 刪除 |
| 關閉 compaction（`WUKONG_SESSION_COMPACT_EVERY_TURNS=0`） | 一直續接 | 不變：沒有 compaction 時點，也就不輪替 |
| 純 CLI backend（`opencode run` 帶額外旗標或其他命令） | 每 20 回合 compaction，不輪替 | 不變：查不到 session 年齡，照原本 compaction |
| `chat_live_events` | 只寫不刪 | 超過 1 天刪除 |

## 已確認證據（2026-11-03 讀碼，main `ad29d32`）
- **本機模式不清**：`active_policy` 只在 `AgentBackend::Server` 時回傳保留政策（`crates/wukong-schedulerd/src/session_retention.rs:46-51`）。註解寫的理由是「CLI 模式的 opencode.db 與使用者自己的 opencode 共用」；容器的 `opencode-state` 是 Wukong 專屬的 volume，這個理由在容器內不成立。
- **本機 backend 只能刪、不能列**：`LocalProcessBackend` 實作了 `delete_session`（`local_process.rs:235`），在按需啟動的本機 `opencode serve` 上執行；`list_sessions` 沒有覆寫，用的是 trait 預設的錯誤「目前只有 opencode server backend 支援列出 session。」（`crates/wukong-gateway/src/backend.rs:89-94`）。
  - NOTE（2026-11-03 更正）：原本寫成「實作了 `list_sessions` 與 `delete_session`」，是只看 `AgentBackend` 的 match 分派就下結論。步驟 1 的探測（3）實際執行 `wukong opencode prune --dry-run` 時拿到上述錯誤，才發現寫錯。A 因此要替本機 backend 補上列出 session 的實作。
- **未設定時的預設值**：`RetentionPolicy::from_value(None)` 回傳 30 天（`crates/wukong-runtime/src/session_retention.rs:118-125`），所以要區分「未設定」與「明確設定」，不能只看 `enabled()`。
- **compose 會設定這個變數**：`docker-compose.release.yml` 的 web、telegram、schedulerd 都有 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS=${WUKONG_OPENCODE_SESSION_RETENTION_DAYS-30}`。
- **既有輪替路徑**：`prepare_final_session_with_policy` 回傳 `session_id: None, rotated_from: Some(old)` 時，`run_turn` 取得新 session 後才刪除舊的（`crates/wukong-runtime/src/session.rs:125-135`、`turn.rs:402-415`）。
- **上一回合注入**：輔助棒用 `previous_turn_records`（最新兩筆回合記憶）加上 `with_previous_turn`，每筆上限 800 字（`crates/wukong-runtime/src/turn.rs:181-183`、`503-550`）。
- **session 清單只帶 `time.updated`**：`SessionSummary` 只有 `id`、`updated_ms`、`parent_id`（`crates/wukong-gateway/src/backend.rs:49-55`），目前沒有取得建立時間的方法。
- **live event 沒人清**：`prune_live_events_before` 只在測試裡被呼叫；Web 用 `latest_live_event_id` 當起點（`crates/wukong-web/src/chat_api.rs:541`）。schedulerd 已經依賴 `wukong-chat-history`。
- **成長規模（正式站，2026-08-08，server 模式）**：100 個 session、356,530 筆 event、123,864 筆 part，`opencode.db` 1.33 GiB（`docs/2026-08-08-system-freeze-opencode-resource-handover.md:39`、`:237`）。目前正式主機的數字沒有量測（TBD-1）。
- **opencode 版本**：RunWuKong 的 v0.24.0 映像為 opencode 1.19.4（`docker exec wukong-web opencode --version`，2026-11-03）。`Dockerfile` 的 `OPENCODE_VERSION` 預設 `latest`。

## 涉及檔案
### wukong-schedulerd
- `crates/wukong-schedulerd/src/session_retention.rs` - `active_policy` 接受 Local backend（需有明確設定）
- `crates/wukong-schedulerd/src/main.rs` - 啟動日誌；維護 tick 加上 live event 清理
### wukong-runtime
- `crates/wukong-runtime/src/session_retention.rs` - 區分「未設定」與「明確設定」
- `crates/wukong-runtime/src/session.rs` - 依 session 年齡輪替
- `crates/wukong-runtime/src/turn.rs` - 輪替回合的最後一棒注入上一回合
### wukong-gateway
- `crates/wukong-gateway/src/local_process.rs` - 本機 backend 補上列出 session（A）
- `crates/wukong-gateway/src/backend.rs`、`opencode_server.rs`、`local_process.rs` - 取得單一 session 的建立時間（B）
### 文件
- `docs/docker.md`、`docs/entrypoints.md`、`AGENTS.md`「關鍵設計細節」、`CHANGELOG.md` `[Unreleased]`
- `.env.example` - 保留天數的說明改為也適用本機模式
- `crates/wukong-cli/src/main.rs` - `opencode prune` 在本機模式沒有明確設定時只允許 `--dry-run`（SCN-011）
- `docs/cli-reference.md` - `opencode prune` 的支援範圍

## Gherkin 驗收劇本

```gherkin
Feature: 長期運行時 opencode 與聊天即時事件的資料有上限

  @SCN-001
  Scenario: 本機模式有明確設定保留天數時清理過期且無主的 session
    Given schedulerd 使用本機 backend，且明確設定 WUKONG_OPENCODE_SESSION_RETENTION_DAYS=30
    And opencode 中有一個 40 天沒有更新、沒有任何 scope 指向的 session
    And 另一個 40 天沒有更新、但被某個 scope 指向的 session
    When 保留期清理執行一輪
    Then 無主的過期 session 被刪除
    And 被 scope 指向的 session 保留
    And schedulerd 啟動日誌寫明保留期清理已啟用

  @SCN-002
  Scenario: 本機模式沒有設定保留天數時不清理
    Given schedulerd 使用本機 backend，且沒有設定 WUKONG_OPENCODE_SESSION_RETENTION_DAYS
    When schedulerd 啟動並經過清理間隔
    Then 不刪除任何 opencode session
    And schedulerd 啟動日誌寫明保留期清理停用及原因

  @SCN-003
  Scenario: server 模式的保留期行為不變
    Given schedulerd 使用 server backend，且沒有設定 WUKONG_OPENCODE_SESSION_RETENTION_DAYS
    When schedulerd 啟動
    Then 保留期清理以 30 天啟用

  @SCN-004
  Scenario: 到 compaction 時點且 session 建立超過輪替門檻時，改為輪替並帶上一回合
    Given 某個 scope 指向的 session 建立已超過輪替門檻（暫定 7 天）
    And 該 scope 的回合數已達 compaction 門檻（預設每 20 回合）
    And 該 scope 有上一回合的紀錄
    When 該 scope 執行下一回合
    Then 這一回合不做 compaction
    And 最後一棒在新的 session 中執行，scope 改為指向新 session
    And 最後一棒的 prompt 含上一回合的內容
    And 取得新 session 後，舊 session 從 opencode 刪除

  @SCN-005
  Scenario: 到 compaction 時點但 session 未超過輪替門檻時照常 compaction
    Given 某個 scope 指向的 session 建立未滿輪替門檻
    And 該 scope 的回合數已達 compaction 門檻
    When 該 scope 執行下一回合
    Then 照原本的規則做 compaction，最後一棒續接同一個 session
    And 最後一棒的 prompt 不因輪替而額外注入上一回合

  @SCN-010
  Scenario: 未到 compaction 時點時不查 session 年齡
    Given 某個 scope 指向的 session 建立已超過輪替門檻
    And 該 scope 的回合數未達 compaction 門檻
    When 該 scope 執行下一回合
    Then 不查詢 session 的建立時間
    And 最後一棒續接同一個 session

  @SCN-006
  Scenario: 查不到 session 建立時間時不輪替
    Given 某個 scope 的回合數已達 compaction 門檻
    And 該 scope 指向的 session，其建立時間查詢失敗或回應中沒有建立時間
    When 該 scope 執行下一回合
    Then 照原本的規則做 compaction，回合照常完成並續接原 session
    And 日誌記錄一行無法判斷 session 年齡的警告

  @SCN-007
  Scenario: 輪替的回合失敗時不刪舊 session
    Given 某個 scope 的 session 已超過輪替門檻，且回合數已達 compaction 門檻
    When 該 scope 的下一回合在取得新 session 之前失敗
    Then 舊 session 沒有被刪除，scope 仍指向它
    And 再下一回合會重新嘗試輪替

  @SCN-008
  Scenario: 定期刪除超過 1 天的 live event
    Given chat_live_events 中有建立超過 24 小時的事件與 24 小時內的事件
    When schedulerd 的維護迴圈執行一次
    Then 超過 24 小時的事件被刪除
    And 24 小時內的事件保留

  @SCN-009
  Scenario: 關閉記憶自動維護時仍清理 live event
    Given WUKONG_MEMORY_AUTO_MAINTENANCE=0
    And chat_live_events 中有建立超過 24 小時的事件
    When schedulerd 的維護迴圈執行一次
    Then 超過 24 小時的事件被刪除
    And 不執行記憶摘要

  @SCN-011
  Scenario: 本機模式沒有設定保留天數時，手動 prune 只允許預覽
    Given backend 是按需本機模式
    And 沒有設定 WUKONG_OPENCODE_SESSION_RETENTION_DAYS
    When 執行 wukong opencode prune
    Then 拒絕清理、結束碼為 1，錯誤訊息說明要先明確設定或改用 --dry-run
    And opencode.db 的 session 一個都沒有被刪除
    When 改執行 wukong opencode prune --dry-run
    Then 照常列出將刪除與受保護的 session，不刪除任何 session
```

## Gherkin 核准紀錄
- **核准 commit**: d0d6f0a（前一版 `445b34c`）
- **核准來源**: 2026-11-03 對話。使用者先接受分析建議「好開成 issue」，接著四題都選建議答案：A「明確設定才啟用」（不新增變數，本機模式不做 vacuum）、B 觸發「依 session 年齡」（暫定 7 天，量測後定案）、B 接續「帶上一回合」、C「1 天」（固定值、不開放設定）。同日使用者回覆「都核准」，核准 SCN-006、SCN-007、SCN-009。步驟 1 量到本機模式每次查年齡要多啟動一次 opencode serve（約 0.7 秒），使用者對「B 要多久查一次 session 年齡」選「只在 compaction 時查（建議）」。該選項的說明寫明「每 20 回合到 compaction 時才查年齡，超過 7 天就輪替、不做那次 compaction……超過 7 天之後最多還會續接 19 回合，SCN-004 的『下一回合輪替』要改寫」，因此改寫 SCN-004～007、新增 SCN-010，以這個答案為核准來源。
- **SCN-011 核准來源**：2026-11-03 PR #29 獨立審查（`review-e63beeb.md`）發現，本機 backend 有了 `list_sessions` 之後，主機上手動 `wukong opencode prune` 在沒有設定保留天數時也會以預設 30 天刪除使用者自己的 opencode session（改版前在列表那一步就失敗）。使用者對「要不要仍允許 `--dry-run`」選「允許 dry-run（建議）」：沒有設定時 `--dry-run` 照常列出，實際刪除拒絕。

| Scenario | 核准日期 | 狀態 |
|----------|---------|------|
| SCN-001 | 2026-11-03 | 已核准 |
| SCN-002 | 2026-11-03 | 已核准 |
| SCN-003 | 2026-11-03 | 已核准 |
| SCN-004 | 2026-11-03 | 已核准 |
| SCN-005 | 2026-11-03 | 已核准 |
| SCN-006 | 2026-11-03 | 已核准 |
| SCN-007 | 2026-11-03 | 已核准 |
| SCN-010 | 2026-11-03 | 已核准 |
| SCN-008 | 2026-11-03 | 已核准 |
| SCN-009 | 2026-11-03 | 已核准 |
| SCN-011 | 2026-11-03 | 已核准 |

SCN-003 是「server 模式不變」，由 A 的答案直接涵蓋。SCN-006、SCN-007、SCN-009 是規劃時推導出的失敗路徑與邊界，經使用者確認後核准（TBD-2）。

## 實作步驟概要
權威清單見 [implementation-plan.md](./implementation-plan.md#實作步驟)。
1. ✅ **首要驗證：opencode session 建立時間的契約探測**
2. ✅ **A：本機模式保留期清理**
3. ✅ **C：live event 清理**
4. ✅ **B：依年齡輪替與上一回合注入**
5. ✅ **文件與全套驗證**
6. ✅ **審查修正：CLI prune 比照 schedulerd 擋本機模式**（SCN-011）

## 風險與首要驗證
- **最大風險**：B 依賴 opencode 回報的 session 建立時間，這個契約還沒驗證過。若單位錯（秒與毫秒）、compaction 會重設它、或本機與 server backend 取得方式不同，輪替會變成每回合都發生（每回合失去 context，並刪掉剛建立的 session），或者永遠不發生。A 會在既有容器部署第一次開啟刪除，刪除不可逆。
- **風險等級與理由**：High。會刪除 opencode session（不可逆），而且核心外部行為（建立時間的欄位、單位、compaction 後是否不變）還沒確認。
- **首要驗證**：在拋棄式容器中對真實 opencode 1.19.4 探測：(1) 單一 session 的建立時間怎麼取得、單位為何；(2) compaction 前後建立時間是否不變；(3) 經由本機 backend 列出超過 100 個 session 時，最舊的仍會出現（W-6）。
- **選擇理由**：輪替判斷與清理範圍都由這些外部回應決定。單元測試只能驗證我們對回應的假設，驗證不了回應本身。
- **完成證據**：implementation-plan 步驟 1 記錄探測命令、opencode 版本與實際回應；三項都有明確結論，B 的實作依結論選定欄位與單位。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|------|------|------|------|
| TBD-1 | 輪替門檻的最終天數。需要正式主機的唯讀量測：各 session 的 part/event 筆數與建立時間、`opencode.db` 大小。正式主機不是 RunWuKong（使用者 2026-10-21 說明），我無法存取；量測查詢見 implementation-plan 步驟 1 附錄 | 不影響本次交付 | SCN-004 的門檻值。2026-11-04 結論：以暫定的 7 天交付，PR #29 已揭露。追蹤：無——量測查詢保留在 implementation-plan 步驟 1 附錄，之後要調整天數時另開 issue（使用者 2026-11-04 選擇） |
| TBD-2 | SCN-006（查不到建立時間時不輪替、回合照常）、SCN-007（輪替回合失敗時保留舊 session）、SCN-009（live event 清理不受 `WUKONG_MEMORY_AUTO_MAINTENANCE` 開關影響）是否照寫 | 已解決 | 2026-11-03 使用者回覆「都核准」，三條照原文核准 |
| TBD-3 | 本機模式查 session 年齡要多啟動一次 opencode serve（約 0.7 秒），要多久查一次 | 已解決 | 2026-11-03 使用者選「只在 compaction 時查」；SCN-004～007、SCN-010 |
| TBD-4 | 本機模式每刪一個 session 都要啟動一次 opencode serve（約 0.7 秒），一輪 5 分鐘預算大約只刪得完 400 個 | 不影響本次交付 | 2026-11-03 結論：預算用完時這一輪停止，沒刪的留到 6 小時後的下一輪；只有舊部署第一次清理時量大，之後每輪只會有少量新過期的 session。追蹤：無，PR 揭露即可。改成一次啟動、批次刪除，要讓 runtime 的清理知道 gateway 的程序生命週期，跨層改動不值得 |
| TBD-5 | 輪替後 `turn_count` 沒有歸零（`finish_agent_session_turn` 換 session 時只加 1，既有行為），下一回合會對新 session 多查一次年齡、多做一次 compaction | 不影響本次交付 | 2026-11-03 步驟 4 端到端時發現。每次輪替（約每 7 天一次）只多一次 compaction 與一次 opencode 啟動。追蹤：無，PR 揭露。輪替的下一回合對新 session 做 compaction 成功後，計數就歸零，所以不會連續輪替。NOTE（2026-11-03 更正）：原本還寫「同一個原因讓純 CLI backend 在 20 回合後每回合都輪替」，經查不成立（純 CLI 的錯誤碼不會是 404／405），使用者原本同意另開的 issue 因此沒有開 |
| TBD-6 | PR #29 審查的 NICE TO HAVE 1、2、4 沒有處理 | 不影響本次交付 | 2026-11-03。1：本機模式舊部署前幾輪清理會用滿 5 分鐘、期間排程掃描暫停，逾時那輪沒有摘要行——已在 PR 揭露。2：純 CLI backend 每到 compaction 時點記一行 `session_age_unknown` 警告——只是雜訊。4：輪替時若沒取得新 session 仍會刪舊的（既有程式碼）——server 與本機 backend 一定回 session id，走不到。追蹤：無 |

## Timeline

| 日期 | 異動 | 負責人 |
|------|------|--------|
| 2026-11-03 | 讀碼盤點與負擔分析；使用者回答 A／B／C 四題；開立 GitHub #28；核准 SCN-001～005、SCN-008 | - |
| 2026-11-03 | 使用者核准 SCN-006、SCN-007、SCN-009，全部 Scenario 已核准 | - |
| 2026-11-03 | 步驟 1 契約探測完成；更正「本機 backend 已能列出 session」的錯誤證據；使用者選擇只在 compaction 時點查年齡，改寫 SCN-004～007、新增 SCN-010 並核准 | - |
| 2026-11-03 | 步驟 2～5 完成（A、C、B 與文件），workspace 全套檢查通過 | - |
| 2026-11-03 | PR #29 獨立審查 RETURN（CLI prune 未比照 schedulerd 擋本機模式）；新增 SCN-011 並經使用者核准，步驟 6 修正 | - |
| 2026-11-03 | PR #29 重審（`review-ab44d7c.md`）PASS，報告提交 `eb42e72` 只新增這份報告 | - |
| 2026-11-03 | PR #29 以 `eb42e72` squash 合併為 main `7d479b4`（07:13Z），合併內容與審查範圍一致；GitHub #28 已關閉 | - |
| 2026-11-04 | TBD-1 記為不影響本次交付（追蹤：無）；結案 | - |

---
**建立日期**: 2026-11-03
**分級**: Medium — 跨 wukong-gateway、wukong-runtime、wukong-schedulerd，沿既有相依方向，不改 schema
**風險**: High
**狀態**: 已完成（PR #29 已於 2026-11-03 合併）

GitHub：https://github.com/raybird/Wukong/issues/28
