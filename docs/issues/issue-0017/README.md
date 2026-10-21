# Issue 17 - 刪除排程後，排程 scope 的 session 指向與記憶留下無法清理

## 概述
刪除 Turn 排程時，一併清掉它的 scope（`agent:schedule-<job id>`）：該 scope 的所有記憶，以及 `agent_sessions`／`agent_session_state` 的 session 指向。排程如果正在執行，就先刪排程，等回合結束後再清 scope；這個回合的結果照常送到 Telegram。opencode 那邊的 session 本體不直接刪，清掉指向後交給既有的保留期清理。

## 快速導覽

### 當前流程
- `schedule rm` 和 Web 的刪除只刪 `scheduler_jobs` 那一列，執行紀錄會因為 `ON DELETE CASCADE` 一起刪掉。
- 排程 scope 的記憶與 session 指向留著，沒有任何流程會清。
- 排程在執行中被刪除時，回合照樣跑完，但 `complete_claimed_job` 找不到那一列，結果被當成 `LeaseLost`：
  - daemon 印「lease was taken by another worker」，不送 Telegram 通知；
  - 手動 trigger 回報錯誤「排程 lease 已被其他 worker 接手」。

### 變更後流程
- 刪除排程（CLI 或 Web）時，如果排程沒在執行，就立刻清掉它的 scope。
- 如果排程正在執行，或已被取得、還在排隊，就先刪排程，不清 scope。executor 之後發現排程已經不在了，再清 scope：
  - 正在執行：等回合跑完後清，並照常送出結果；
  - 還在排隊：輪到時跳過，然後清。

## 關鍵差異
| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| 刪除排程時的 scope 記憶與 session 指向 | 永遠留著 | 刪除 |
| 執行中被刪除的回合 | 跑完後被當成 lease 被接手，不送通知 | 跑完後清 scope，照常送通知 |
| `schedule rm` 撞到執行中的排程 | 「已刪除排程」 | 「已刪除排程」，並說明回合結束後才清它的記憶與 session |
| 手動 trigger 執行中被刪除 | 回報 lease 被接手的錯誤 | 印出回合結果，依結果決定 exit code |
| 維護排程、找不到排程 | - | 不變 |

## 已確認證據（2026-10-21 讀碼，main `a758f7e`）
- **刪除只刪 job**：`SchedulerStore::remove_job` 只執行 `DELETE FROM scheduler_jobs WHERE id = ?1`（`crates/wukong-scheduler/src/store.rs:136-142`）。呼叫端是 CLI 的 `schedule rm`（`crates/wukong-cli/src/main.rs:257`）和 Web 的 `delete_schedule`（`crates/wukong-web/src/lib.rs:473-495`）。
- **執行紀錄跟著刪**：`scheduler_runs` 的 `job_id` 是 `ON DELETE CASCADE`（`store.rs:76`）。
- **scope 規則**：Turn 排程的 scope 是 `agent:schedule-<job id>`（`crates/wukong-scheduler/src/executor.rs:319-321`）。只有 executor 會寫入這個 scope。
- **記憶清不掉**：
  - 回合寫入的記憶，importance 是預設的 1.0；prune 的 fallback 路徑只刪 importance 低於 0.5 的 event／note（`crates/wukong-memory/src/store/mod.rs:1243-1252`）。
  - 已經被 consolidate 的 event 會被 prune 掉，但 summary 永遠不在 prune 的範圍內。
  - Web 的記憶頁列出所有 scope（`crates/wukong-web/src/memory_api.rs`），所以孤兒記憶會一直出現在那裡。
- **session 指向**：`Memory::clear_agent_session` 已經會在同一個 transaction 刪掉兩張表的那一列（`store/mod.rs:742-754`），但目前沒有任何正式路徑呼叫它。保留期清理只要有 scope 指向某棵 session 樹，就整棵保留。
- **執行中清 session 狀態會讓回合失敗**：回合結束時，`finish_agent_session_turn` 要求 lease 仍在自己手上；找不到那一列就回「session lease lost」錯誤（`crates/wukong-runtime/src/turn.rs:393-401`）。所以執行中不能清 session 狀態。
- **執行中被刪除就不送通知**：`run_scan` 遇到 `LeaseLost` 會 `continue`，不送 Telegram（`crates/wukong-schedulerd/src/main.rs:232-238`）。
- **agent 會自己刪排程**：每回合最後一棒的排程能力提示，教 agent 用 `schedule rm --id <id>` 刪除排程（`crates/wukong-runtime/src/persona.rs:147`），排程回合也會收到。所以排程有可能在自己執行時刪掉自己。
- **正式環境沒有既有的孤兒資料**：#14（排程在自己的 scope 執行）在 2026-10-20 合併，最新發布的是 v0.22.1（2026-10-07），還不包含它。正式環境沒有 `agent:schedule-*` scope，所以不需要掃描既有的孤兒資料。

## 涉及檔案
- `crates/wukong-memory/src/store/mod.rs`、`src/lib.rs`：在一個 transaction 裡刪掉單一 scope 的記憶與 session 指向。
- `crates/wukong-scheduler/src/store.rs`：刪除 job 時，用同一句 SQL 回報刪除當下 job 是不是正被持有。
- `crates/wukong-scheduler/src/executor.rs`：
  - 新增一個 CLI 和 Web 共用的刪除函式；
  - 回合結束後或排隊跳過時，如果排程已經不在了，就清 scope；
  - 新增「執行中被刪除」的結果類型。
- `crates/wukong-cli/src/main.rs`：`schedule rm` 改用共用的刪除函式；trigger 處理新的結果類型。
- `crates/wukong-web/src/lib.rs`：`delete_schedule` 改用共用的刪除函式。
- `crates/wukong-schedulerd/src/main.rs`：處理新的結果類型，並送出通知。

## 文件清單
- [implementation-plan.md](./implementation-plan.md)

## Gherkin 驗收劇本
```gherkin
Feature: 刪除排程時清掉它的 scope

  @SCN-001
  Scenario: 刪除沒有在執行的排程，清掉它的記憶與 session 指向
    Given 一個 Turn 排程，它的 scope `agent:schedule-<id>` 有 event、note、summary 記憶，以及 session 指向
    And 擁有者 scope、另一個排程的 scope，以及名稱以這個 scope 開頭的 scope，也各有記憶與 session 指向
    When 用 `wukong schedule rm --id <id>` 刪除
    Then 印出「已刪除排程」
    And 這個 scope 沒有任何記憶與 session 指向，全文搜尋也找不到它的記憶
    And 其他 scope 的記憶與 session 指向都不變

  @SCN-002
  Scenario: 從 Web 刪除，結果和 CLI 相同
    Given 和 SCN-001 相同的資料
    When 呼叫 `DELETE /api/schedules/<id>`
    Then 回應 200
    And 資料的結果和 SCN-001 相同

  @SCN-003
  Scenario: 刪除正在執行的排程，回合結束後才清
    Given 一個 Turn 排程正在執行
    When 刪除這個排程
    Then 排程立刻消失，`schedule list` 看不到，trigger 印「找不到排程」
    And `schedule rm` 說明排程正在執行，回合結束後才清它的記憶與 session
    And 回合照常跑完，結果是成功，不會因為 session 狀態被清而失敗
    And 回合結束後，這個 scope 沒有任何記憶與 session 指向，包含這個回合剛寫入的

  @SCN-004
  Scenario: 執行中被刪除的回合，結果照常送出
    Given SCN-003 的回合由 daemon 執行，而且這個排程的擁有者是 Telegram 對話
    When 回合跑完
    Then daemon 把結果送到 Telegram，並印出排程在執行中被刪除
    And 不再印「lease was taken by another worker」
    And 如果是手動 trigger 執行的，就印出回合結果，exit code 依結果決定

  @SCN-005
  Scenario: 已被取得、還在排隊的排程被刪除，輪到時跳過並清理
    Given daemon 同一次掃描取得了多個排程，這個排程排在後面
    When 它在排隊時被刪除
    Then 輪到它時不執行回合
    And 這個 scope 沒有任何記憶與 session 指向

  @SCN-006
  Scenario: 維護排程與找不到的排程，行為不變
    When 刪除一個維護排程
    Then 印出「已刪除排程」，記憶不受影響
    When 刪除一個不存在的 id
    Then 印出「找不到排程」
```

## Gherkin 核准紀錄
- **核准 commit**: 22652cf
- **核准來源**：2026-10-21 使用者逐題確認：
  - Q1「刪除排程時立即清」；
  - Q2「記憶與 session 指向都清」；
  - Q3「交給保留期清理」；
  - Q4 原本選「執行中拒絕刪除」，得知排程會刪掉自己之後，改選「先刪排程，回合結束後再清」；
  - Q5「照常送出」。

  上列 Scenario 由這些答案推導；同日使用者回覆「核准」，核准 SCN-001～006。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-10-21 | 已核准 |
| SCN-002 | 2026-10-21 | 已核准 |
| SCN-003 | 2026-10-21 | 已核准 |
| SCN-004 | 2026-10-21 | 已核准 |
| SCN-005 | 2026-10-21 | 已核准 |
| SCN-006 | 2026-10-21 | 已核准 |

## 風險與首要驗證
- **最大風險**：刪除記憶無法回復。scope 比對寫錯（例如用了前綴比對，或 id 為空時變成 `agent:schedule-`），就會刪到擁有者或其他排程的記憶。另外，FTS 索引如果和資料表不同步，會留下搜得到、讀不到的殘留。
- **風險等級與理由**：High。這是不可逆的資料刪除。程式改動本身不大，但刪錯 scope 會直接造成使用者的記憶損失。
- **首要驗證**：先寫 memory 層的刪除測試再實作。資料包含：目標 scope、擁有者 scope、另一個排程的 scope，以及名稱以目標 scope 開頭的 scope（`agent:schedule-<id>-x`）。斷言只有目標 scope 的 memories、FTS 與 session 指向消失，其他三個 scope 逐列不變。
- **選擇理由**：刪錯資料的後果最嚴重，而且只要 memory 層的範圍正確，上層呼叫怎麼組合都刪不到別人的資料。
- **完成證據**：
  - memory 層刪除測試先紅後綠；
  - SCN-001～006 的測試先紅後綠，SCN-006 作為 characterization；
  - 在臨時目錄用真實行程跑一次執行中自我刪除的排程，確認回合成功、scope 清空、daemon 印出被刪除的訊息。

## 待確認事項
| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-AC | SCN-001～006 的具體行為 | 已解決 | 2026-10-21 使用者回覆「核准」 |
| TBD-1 | 執行中刪除之後、回合結束之前，行程如果當掉，就沒有人清這個 scope | 不影響本次交付 | 使用者 Q1 選不做定期掃描。殘留只發生在「執行中刪除」又剛好當掉的時候。追蹤：#13 的 TBD-1（重啟後的處理）另開 issue 時一併考慮 |
| TBD-2 | 排程已經刪掉、清 scope 卻失敗（資料庫錯誤）時，scope 會殘留，而且無法再用 `schedule rm` 重試 | 不影響本次交付 | 錯誤會照實回報給 CLI 和 Web，不會假裝成功 |
| TBD-3 | opencode session 本體交給保留期清理；用 `opencode run` 的部署不跑保留期清理，所以不會被刪 | 不影響本次交付 | 使用者 Q3 選定。這是所有 session 原本就有的限制，不是本 issue 造成的 |
| TBD-4 | 開發機上已經存在的孤兒 scope 不處理 | 不影響本次交付 | 正式環境還沒有 `agent:schedule-*` scope（#14 未發版） |
| TBD-5 | 排程回合寫進擁有者聊天紀錄的內容不刪 | 不影響本次交付 | 那些內容屬於擁有者的對話，不屬於排程的 scope |

## Timeline
| 日期 | 異動 |
|---|---|
| 2026-10-20 | 由 #14 審查 SHOULD FIX 1 開立 |
| 2026-10-21 | 讀碼盤點；使用者回答 Q1～Q5（Q4 得知排程會刪掉自己之後重新選擇） |
| 2026-10-21 | 使用者核准 SCN-001～006 |

---
**建立日期**: 2026-10-20
**分級**: Medium — 跨 wukong-memory、wukong-scheduler、wukong-cli、wukong-web、wukong-schedulerd，沿既有相依方向，不改 schema
**風險**: High
**狀態**: 待實作

GitHub：https://github.com/raybird/Wukong/issues/17
