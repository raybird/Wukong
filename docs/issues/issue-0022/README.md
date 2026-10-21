# Issue 22 - schedulerd 重啟後：跳過中斷的那次排程、標記殘留的 running run、清掉孤兒 scope

## 概述
`wukong-schedulerd` 在回合執行到一半時結束之後：
- 中斷的那一次排程不再重跑，等下一個 cron 時間點；
- 殘留的 running run 在 lease 到期後的下一輪掃描就標成 interrupted；
- daemon 啟動時，清掉找不到對應排程的 `agent:schedule-<id>` scope。

## 快速導覽

### 當前流程
- 取得排程時不推進 `next_run_at`，要等回合完成，`complete_claimed_job` 才推進。daemon 在回合中途結束後，lease 一到期，同一次排程就會被重新取得、從頭重跑。
- 只有 daemon 啟動時（非 `--once`）會把「排程 lease 已到期」的 running run 標成 interrupted。如果在 lease 到期前重啟，舊 run 不會被標記，之後也沒有其他時間點再檢查。
- 排程在執行中被刪除後，如果行程在回合結束前結束，它的 scope 就沒有人清（#17 TBD-1）。

### 變更後流程
- Turn 排程**開始執行時**就把 `next_run_at` 推進到開始時間之後的下一個 cron 時間點。中斷的那次不會再被掃描取得；回合正常完成時，照原本的方式再算一次。
- 已經取得、還在排隊、還沒開始執行的排程，以及維護排程，不推進。daemon 結束後，lease 到期就照常補跑（D-2）。
- 每輪掃描開始前（包括 `--once`）都跑一次原本啟動時的那個檢查；啟動時不再另外檢查（D-1）。
- daemon 啟動時（非 `--once`），列出 `agent:schedule-<id>` 形式的 scope；後綴是排程 id 寫法（小寫、有連字號的 UUID）而且對應的排程不存在時，用 `Memory::delete_scope` 清掉。

## 關鍵差異
| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| 回合中途 daemon 結束 | lease 到期後重跑同一次 | 不重跑，等下一個 cron 時間點 |
| 已取得、還沒開始的排程，或維護排程，daemon 結束 | lease 到期後補跑 | 不變 |
| lease 到期前就重啟，舊的 running run | 一直停在 running | 下一輪掃描標成 interrupted |
| 執行中被刪除又剛好當掉的排程 scope | 永遠留著 | 下次啟動時清掉 |
| 正常完成的排程、手動 trigger 還沒到期的排程 | - | 下次執行時間不變 |

## 已確認證據（2026-10-21 讀碼，main `c08c03d`）
- **取得時不推進**：`claim_due_jobs` 只設定 `locked_by`／`locked_until`（`crates/wukong-scheduler/src/store.rs:175-221`）。`next_run_at` 只在 `complete_claimed_job` 推進為 `next_after(cron, finished_at)`（`store.rs:336-358`）。
- **lease 到期就可以再取得**：`claim_due_jobs` 的條件是 `next_run_at <= now AND (locked_until IS NULL OR locked_until <= now)`。daemon 預設 lease 300 秒，執行中每 `min(30, lease/3)` 秒續租一次（#13），行程結束後就不再續租。
- **標記 interrupted 只在啟動時**：`interrupt_stale_runs` 把「排程 lease 已到期或沒有 lease」的 running run 標成 interrupted（`store.rs:361-384`），唯一的正式呼叫端是 daemon 啟動時的 `recover_interrupted_runs`，而且 `--once` 不呼叫（`crates/wukong-schedulerd/src/main.rs:69`、`168-180`）。
- **標記不會誤傷正在收尾的 run**：`run_claimed_job` 先 `finish_run` 寫入最終狀態，再 `complete_claimed_job` 釋放 lease（`crates/wukong-scheduler/src/executor.rs:191` 起）。只要回合還活著並持有 lease，`interrupt_stale_runs` 就不會碰它。
- **一輪掃描取得多個排程**：`run_scan` 一次取得最多 `--limit` 個排程，再逐一執行（`main.rs:198` 起）。排在後面的排程，取得之後可能要等前面的回合跑完才開始。
- **排程 id 一律是 UUID v4**：`add_job` 以 `uuid::Uuid::new_v4()` 產生（`store.rs:86`）。只有 executor 的 `scheduled_turn_scope` 會寫入 `agent:schedule-<id>`。
- **排程與記憶在同一個資料庫**：daemon 用同一個 `cfg.db_url` 開啟 `SchedulerStore` 與 `Memory`（`main.rs:64-68`），所以「找不到排程」不會是因為開錯資料庫。
- **正式環境還沒有排程 scope**：#14、#17 在 v0.22.1（2026-10-07）之後才合併，尚未發版。

## 涉及檔案
- `crates/wukong-scheduler/src/store.rs`：開始執行時推進 `next_run_at`（只在自己持有 lease 時）。
- `crates/wukong-scheduler/src/executor.rs`：`run_claimed_job` 在 `start_run` 之後推進；新增清孤兒 scope 的函式。
- `crates/wukong-memory/src/store/mod.rs`、`src/lib.rs`：列出某個前綴的所有 scope（記憶、`sessions`、兩張 session 指向表）。
- `crates/wukong-schedulerd/src/main.rs`：每輪掃描前呼叫 `interrupt_stale_runs`；啟動時清孤兒 scope。

## 文件清單
- [implementation-plan.md](./implementation-plan.md)

## Gherkin 驗收劇本
```gherkin
Feature: schedulerd 重啟後的處理

  @SCN-001
  Scenario: 回合中途 daemon 結束，中斷的那次不重跑
    Given 一個到期的 Turn 排程，daemon 已經開始執行它的回合
    And daemon 在回合結束前結束
    When lease 到期之後，daemon 再掃描
    Then 這個排程沒有被取得，回合沒有再執行
    And 它的下次執行時間是回合開始時間之後的下一個 cron 時間點
    And 那一次的 run 標成 interrupted

  @SCN-002
  Scenario: 已取得但還沒開始執行的排程，照常補跑
    Given daemon 同一輪掃描取得了多個排程，這個排程排在後面
    And daemon 在輪到它之前結束
    When lease 到期之後，daemon 再掃描
    Then 這個排程被取得並執行

  @SCN-003
  Scenario: 正常完成與手動 trigger，下次執行時間不變
    When daemon 正常跑完一個到期的排程
    Then 下次執行時間是完成時間之後的下一個 cron 時間點，和變更前相同
    When 手動 trigger 一個還沒到期的排程
    Then 它的下次執行時間不變

  @SCN-004
  Scenario: lease 到期前就重啟，舊的 run 在下一輪掃描標成 interrupted
    Given 一個 run 停在 running，它的排程 lease 已到期
    And 另一個 run 停在 running，它的排程 lease 還在別人手上
    When daemon 做一輪掃描
    Then 第一個 run 標成 interrupted，訊息說明行程在 run 完成前結束
    And 第二個 run 不變

  @SCN-005
  Scenario: daemon 啟動時清掉孤兒排程 scope
    Given 下列 scope 都有記憶與 session 指向：
      | scope                              | 說明                     |
      | agent:schedule-<不存在的 uuid>      | 排程已刪除               |
      | agent:schedule-<存在的排程 uuid>    | 排程還在                 |
      | agent:schedule-<不存在的 uuid>-x    | 後綴不是 uuid            |
      | agent:schedule-notes               | 後綴不是 uuid            |
      | user:tg-42                         | 擁有者                   |
    When daemon 以常駐模式啟動
    Then 只有第一個 scope 的記憶與 session 指向被清掉，其他 scope 逐列不變
    And daemon 印出清掉的 scope 數量
    When daemon 以 `--once` 啟動
    Then 不清任何 scope
```

## Gherkin 核准紀錄
- **核准 commit**: c8efc62
- **核准來源**：2026-10-21 使用者逐題確認：
  - Q1 範圍：run 停在 running、啟動時清孤兒 scope、中斷後重跑的策略，三項都做；
  - Q2「跳過中斷的那次」；
  - Q3 原本選「接手同一排程時」，得知跳過之後要等到下一個 cron 時間點才會接手，改選「每次掃描都檢查」；
  - Q4「開新 issue」，同日開立 GitHub #22。

  SCN-001 的推進時機是由 Q2 推導出來的：選「開始執行時」而不是 issue 內文寫的「取得時」，理由見「推進時機」。同日使用者回覆「核准」，核准 SCN-001～005（含推進時機與手動 trigger 的說明）。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-10-21 | 已核准 |
| SCN-002 | 2026-10-21 | 已核准 |
| SCN-003 | 2026-10-21 | 已核准 |
| SCN-004 | 2026-10-21 | 已核准 |
| SCN-005 | 2026-10-21 | 已核准 |

### 推進時機
- Q2 的目的，是避免中斷前已經做過的事（送出的訊息、寫入的檔案）在重跑時重複。
- 還在排隊、沒開始的排程什麼都沒做，補跑不會重複。如果在取得時就推進，這些排程會被跳過，而且沒有任何 run 紀錄，等於無聲地漏掉。
- 所以改在 `start_run` 之後推進。這樣每一次被跳過的排程，都會留下一筆 interrupted 的 run。
- 手動 trigger 也走 `run_claimed_job`。排程還沒到期時，「開始時間之後的下一個 cron 時間點」就是它原本的下次執行時間，所以不變（SCN-003）。排程已經到期時，手動 trigger 這一次就算是那一次的執行；這和現在完成後推進的結果相同。

## 風險與首要驗證
- **最大風險**：清孤兒 scope 是無法回復的刪除。判斷寫錯（前綴比對太寬、把查不到排程當成排程不存在、`LIKE` 的大小寫與萬用字元）會刪掉還在用的排程或其他 scope 的記憶。
- **風險等級與理由**：High。不可逆的資料刪除；而且 daemon 每次啟動都會跑，寫錯會反覆發生。
- **首要驗證**：先寫清孤兒 scope 的測試再實作，資料用 SCN-005 那五種 scope，外加大小寫不同的 `AGENT:SCHEDULE-<uuid>`。斷言只有排程不存在的那個 scope 被清掉，其他逐列不變。
- **選擇理由**：推進時機與標記 interrupted 改錯，最多是多跑或少跑一次、狀態顯示錯誤；刪錯 scope 則是使用者資料損失。
- **完成證據**：
  - 清孤兒 scope 的測試先紅後綠，並用變異確認比對條件放寬時會紅燈；
  - SCN-001～004 的測試在現行程式碼下先重現紅燈（SCN-002、SCN-003 作為 characterization），實作後轉綠；
  - 用真實行程驗證：daemon 在回合中途被 `kill -9`，重啟後同一次不再執行、舊 run 被標成 interrupted，並和 main 對照。

## 待確認事項
| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-AC | SCN-001～005 的具體行為 | 已解決 | 2026-10-21 使用者回覆「核准」 |
| TBD-1 | 一次性提醒在執行中被中斷，要等到下一個符合 cron 的時間（可能是明年）才會再觸發，排程也會留著 | 不影響本次交付 | 使用者 Q2 選定的代價 |
| TBD-2 | 回合總時間上限（#13 TBD-2）、逾時後接續與成果另外驗證（#13 TBD-3） | 不影響本次交付 | 不在 #22 範圍 |
| D-1 | 審查 S-1：每輪掃描都標記之後，`--once` 也會標記，和啟動時「`--once` 不標記」的檢查矛盾 | 已解決 | 2026-10-21 使用者選「接受並統一」：`--once` 也標記（只看 lease 是否到期），刪掉啟動時重複的檢查 `recover_interrupted_runs` 與它的測試 |
| D-2 | 審查 S-2：開始執行就推進，原本對維護排程也生效 | 已解決 | 2026-10-21 使用者選「只對 Turn 推進」：維護排程補跑不會重複對外的動作，維持中斷後補跑 |
| TBD-3 | 當掉的回合留下的 session lease 最多占用 900 秒，這段期間同一個 scope 的回合會在規劃階段以「session lease unavailable」失敗 | 不影響本次交付 | 步驟 4 實測發現，是既有行為（main 也一樣）。排程頻率低於 15 分鐘一次時不受影響；要處理需要另開 issue |

## Timeline
| 日期 | 異動 |
|---|---|
| 2026-10-21 | 讀碼盤點；使用者回答 Q1～Q4（Q3 得知跳過的影響後重新選擇）；開立 GitHub #22 |
| 2026-10-21 | 使用者核准 SCN-001～005；GitHub #22 內文的推進時機改為開始執行時 |
| 2026-10-21 | 實作完成；真實行程對照 main：回合中途 kill -9 後重啟，不再重跑同一次，舊 run 標成 interrupted，孤兒 scope 被清掉 |
| 2026-10-21 | 開 PR #23；獨立審查 `review-1d65567.md` 判定 RETURN TO execute-task（M-1：非正規寫法的 UUID 會被誤刪）。使用者決定 D-1、D-2，修正見 implementation-plan 步驟 6 |

---
**建立日期**: 2026-10-21
**分級**: Medium — 跨 wukong-memory、wukong-scheduler、wukong-schedulerd，沿既有相依方向，不改 schema
**風險**: High
**狀態**: 實作中

GitHub：https://github.com/raybird/Wukong/issues/22
