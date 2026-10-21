# Issue 13 - 長時間排程逾時：看不到進度、lease 在執行中過期

## 概述
排程回合執行中，可以查到目前的階段與已執行時間；失敗時，訊息指出卡在哪個階段。回合執行期間持續為 job lease 與 session lease 續租，執行超過 lease 時間也不會被別的行程重複執行，或共用同一個 session。

2026-10-21 重新界定範圍。GitHub issue 本文是 2026-10-08 的草案，涵蓋三件事：執行階段可觀察、逾時後接續、成果另外驗證。三個 Scenario 都沒有核准過。使用者 2026-10-21 選「可觀察性＋lease 續租」，接續與成果驗證留待之後另開 issue（TBD-3）。本文件取代原草案。

## 快速導覽

### 當前流程
- 排程 run 從開始到結束，run 訊息都是空的，只在結束時寫入結果。執行中看不出在做什麼、跑了多久。
- 逾時只留下「本機 OpenCode 回合逾時，已中止程序。」，看不出是哪一棒。
- job lease 預設 300 秒，session lease 預設 900 秒，執行中都不續租；單次 backend 呼叫最多卻可以跑 1200 秒。
- lease 過期後，另一個行程可以再執行同一個排程。手動 trigger 會再跑一次；同一個 scope 的回合可能拿到 session lease，和原回合共用同一個 opencode session。原回合結束時拿不到 lease，被判為失敗，但記憶已經寫入。

### 變更後流程
- 執行中的 run 訊息顯示目前階段與已執行秒數，換階段時更新，之後至少每 30 秒更新一次。
- 失敗時，訊息開頭標出失敗時的階段與已執行秒數，後面保留原本的錯誤。
- 回合執行中定期續租 job lease 與 session lease。行程中止後不再續租，lease 照原本的時間到期。
- 手動 trigger 碰到執行中的排程時，印「排程正在執行中」。

## 關鍵差異
| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| 執行中的 run 訊息 | 空白 | 目前階段、已執行秒數 |
| 失敗訊息 | 只有原錯誤 | 先標出階段與已執行秒數，再接原錯誤 |
| job lease（預設 300 秒） | 執行中不續租 | 執行中續租 |
| session lease（預設 900 秒） | 不續租，所有進入點都一樣 | 所有進入點都續租 |
| 手動 trigger 撞到執行中的排程 | 「找不到排程」 | 「排程正在執行中」；停用的排程印「排程已停用」 |
| 回合在 lease 時間內結束 | - | 結果與 lease 行為不變 |

## 已確認證據（2026-10-21 讀碼，main `53f8243`）
- **證據來源**：原草案引用 2026-10-08 run #861～#870 的統計（9 筆中有 6 筆逾時）。那份資料來自另一台主機，本機的 `~/Documents/RunWuKong` 從來沒有跑過排程。本 issue 的證據一律以本機模擬為準。
- **backend 路徑**：部署預設 `WUKONG_AGENT_CMD=opencode run`，而且沒有設 `WUKONG_AGENT_SERVER_URL`，所以走 `LocalProcessBackend`。每次呼叫都會啟動一個 `opencode serve`，外層以 `WUKONG_AGENT_TIMEOUT_SECS`（預設 1200 秒）計時。逾時時殺掉整個行程群組，錯誤是「本機 OpenCode 回合逾時，已中止程序。」（`crates/wukong-gateway/src/local_process.rs:176-180`）。
- **沒有回合總上限**：一個回合最多約 6 次 backend 呼叫（planner、壓縮、最多 3 棒、空輸出修復），每次各自計時。
- **job lease**：`claim_due_jobs`／`claim_job` 寫入 `locked_until = now + lease`。daemon 預設 300 秒，手動 trigger 寫死 300 秒（`crates/wukong-cli/src/main.rs:292-302`）。`run_claimed_job` 執行中不續租，也沒有任何續租函式（`crates/wukong-scheduler/src/executor.rs:131-157`）。
- **session lease**：`renew_agent_session_lease` 已經存在，但只有 memory 自己的測試呼叫它（`crates/wukong-memory/src/store/mod.rs:605`）。回合結束時會再取得一次 lease（`crates/wukong-runtime/src/turn.rs:382-390`），這時如果被別人拿走，回合就失敗。另外，壓縮要求 lease 仍然有效（`store/mod.rs:700-705`），所以壓縮呼叫跑到 900～1200 秒之間時，回合也會失敗。
- **進度**：`start_run` 寫入空訊息，只有 `finish_run` 會寫訊息（`crates/wukong-scheduler/src/store.rs:243-273`）。排程的 `on_role` 是 no-op（`executor.rs:204`）。`run_turn` 每棒開始時呼叫 `on_role(role)`（`turn.rs:219`），但不會告知 planner 階段、棒次編號或總棒數。
- **手動 trigger**：`claim_job` 只要沒拿到 job 就印「找不到排程」，不管原因是不存在、已停用，還是正在執行。

## 涉及檔案
- `crates/wukong-scheduler/src/store.rs`：新增續租 job lease、更新執行中 run 訊息的函式；分辨 trigger 失敗的原因。
- `crates/wukong-scheduler/src/executor.rs`：`run_claimed_job` 在回合執行中定期續租並更新進度；失敗訊息加上階段。
- `crates/wukong-runtime/src/turn.rs`：回合執行中定期續租 session lease；提供階段給呼叫端。
- `crates/wukong-cli/src/main.rs`：trigger 的訊息；傳入 lease 時間。
- `crates/wukong-schedulerd/src/main.rs`：傳入 lease 時間。

## 文件清單
- [implementation-plan.md](./implementation-plan.md)

## Gherkin 驗收劇本
```gherkin
Feature: 長時間排程回合可以查到進度，執行中持續持有 lease

  @SCN-001
  Scenario: 執行中的排程 run 顯示目前階段與已執行時間
    Given 一個排程回合正在執行
    When 用 `wukong schedule runs` 查詢
    Then 這個 run 的狀態是 running
    And 訊息顯示目前階段（規劃中，或第幾棒與角色）與已執行秒數
    And 換到下一個階段時訊息跟著更新，同一階段內至少每 30 秒更新一次已執行秒數

  @SCN-002
  Scenario Outline: 排程回合失敗時，訊息指出失敗的階段
    Given 排程回合在 <階段> 失敗，例如 backend 呼叫逾時
    When 回合結束
    Then run 的狀態是 failure
    And 訊息開頭標出 <階段> 與已執行秒數
    And 後面保留原本的錯誤訊息

    Examples:
      | 階段         |
      | 規劃中       |
      | 第 2 棒 fixer |

  @SCN-003
  Scenario: 回合執行超過 session lease 時間，仍持有 session
    Given session lease 時間比回合執行時間短
    When 回合執行超過 lease 時間時，另一個行程在同一個 scope 開始回合
    Then 另一個回合拿不到 session lease，不會和原回合共用 session
    And 原回合成功結束，並寫回 session

  @SCN-004
  Scenario: 排程回合執行超過 job lease 時間，手動 trigger 不會重複執行
    Given job lease 時間比回合執行時間短
    When 回合執行超過 lease 時間時，手動 trigger 同一個排程
    Then trigger 印出「排程正在執行中」，不執行回合
    And 原回合結束後記為成功，並排定下一次執行時間

  @SCN-005
  Scenario: 行程中止後，lease 照原本的時間到期
    Given 回合執行中，執行它的行程被強制終止
    When 經過 lease 時間
    Then 這個排程可以再次被觸發
    And 這個 scope 可以再次取得 session lease

  @SCN-006
  Scenario: 回合在 lease 時間內結束時，行為不變
    Given 回合在 lease 時間內結束
    When 查看結果
    Then 成功的 run 訊息與變更前相同，是回合的最終輸出
    And 回合結束後 lease 已經釋放，和變更前一樣

  @SCN-007
  Scenario Outline: 手動 trigger 依原因回報
    Given 排程 <狀況>
    When 手動 trigger 這個排程
    Then 印出 <訊息>，不執行回合

    Examples:
      | 狀況     | 訊息             |
      | 不存在   | 找不到排程       |
      | 已停用   | 排程已停用       |
      | 正在執行 | 排程正在執行中   |

  @SCN-008
  Scenario: 同一批取得、還在排隊的 job，lease 已被別人拿走時不再執行
    Given daemon 同一次掃描取得了多個 job
    And 排在後面的 job 在等待期間 lease 過期，被另一個行程取得並執行
    When 輪到 daemon 執行這個 job
    Then daemon 跳過它，不執行回合，也不新增 run 紀錄
```

## Gherkin 核准紀錄
- **核准 commit**: 5a64393（SCN-001～007）；a5a0340（SCN-008）
- **核准來源**: 2026-10-21 使用者對範圍選「可觀察性＋lease 續租」；逐題確認 Q1「沿用 run 訊息」、Q2「區分原因」、Q3「所有回合」、Q4「先記入待確認事項」。上列 Scenario 由這些答案推導；同日使用者回覆「接受」，核准 SCN-001～007。SCN-008 來自 PR #20 審查 D-1：同日使用者選「本 PR 小修」，內容為「每個 job 開始執行前先續租一次；lease 已經不在自己手上就跳過」。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-10-21 | 已核准 |
| SCN-002 | 2026-10-21 | 已核准 |
| SCN-003 | 2026-10-21 | 已核准 |
| SCN-004 | 2026-10-21 | 已核准 |
| SCN-005 | 2026-10-21 | 已核准 |
| SCN-006 | 2026-10-21 | 已核准 |
| SCN-007 | 2026-10-21 | 已核准 |
| SCN-008 | 2026-10-21 | 已核准 |

## 風險與首要驗證
- **最大風險**：lease 問題目前只有讀碼證據。續租寫在 `run_turn` 裡，所有進入點的每一回合都會經過（#12 時 impact 為 CRITICAL）。如果續租在長時間 backend 呼叫期間沒有真的觸發，lease 照樣會過期；反過來，如果行程中止後續租沒有停，lease 就永遠不會到期，那個 scope 或排程會被鎖死。
- **風險等級與理由**：Medium。改動以加法為主，失敗的後果有限：續租沒有觸發，等於維持現狀；續租不停，只會發生在行程還活著的時候，因為續租和回合在同一個 task 裡，行程一結束續租就停了。但它碰到所有回合的核心路徑，所以要先拿到真實行程的證據。
- **首要驗證**：在臨時目錄用假 agent 重現。把 job lease 與 session lease 都設得很短，讓回合跑得比 lease 久，執行中另開行程手動 trigger 同一個排程。記錄現行版本是否真的重複執行、共用 session，以及原回合是否失敗。
- **選擇理由**：缺陷目前只有讀碼證據，先在真實行程重現，確認問題存在，同時取得 SCN-003／004 的紅燈基線。之後用同一支腳本驗證修正。
- **完成證據**：重現腳本修正前後的輸出；SCN-001～007 的測試先紅後綠；SCN-005 用強制終止行程的腳本，確認 lease 照原本的時間到期。

## 待確認事項
| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-AC | SCN-001～007 的具體行為 | 已解決 | 2026-10-21 使用者回覆「接受」 |
| TBD-1 | daemon 重啟後，下次執行時間沒有往後推，會立刻重跑同一個排程；重啟發生在 claim 後 300 秒內時，舊 run 會永遠停在 running | 不影響本次交付 | 2026-10-21 使用者選先記錄（Q4）。續租之後，殘留的 lease 最多再撐一個 lease 時間。追蹤：#13 完成後再決定是否另開 issue |
| TBD-2 | 整個回合沒有總上限，最多約 6 次呼叫，每次 1200 秒 | 不影響本次交付 | 2026-10-21 使用者選先記錄（Q4）。有了 SCN-001／002 的進度訊息，才知道總上限該設多少。追蹤：同上 |
| TBD-3 | 原草案的逾時後接續（原 SCN-002）與成果另外驗證（原 SCN-003） | 不影響本次交付 | 2026-10-21 使用者選範圍時排除。追蹤：同上 |
| TBD-5 | 進度的「規劃中」涵蓋召回、planner、取得 session lease 與壓縮；壓縮最長可跑一次 backend 逾時（1200 秒），這段期間都顯示「規劃中」，取得 session lease 失敗也標成「規劃中失敗」 | 不影響本次交付 | 2026-10-21 使用者選只在文件註明（PR #20 審查 D-2）：壓縮預設每 20 回合才一次。追蹤：有正式資料顯示「規劃中」常逾時再拆階段 |
| TBD-4 | 逾時的根本原因：任務太大、模型太慢，或工具卡住 | 不影響本次交付 | 本 issue 只負責讓原因看得到，不判定根因。正式資料在另一台主機，要等部署新版之後才看得到 |

## Timeline
| 日期 | 異動 |
|---|---|
| 2026-10-08 | 依使用者授權在 GitHub 建立議題與驗收草案，尚未核准 |
| 2026-10-21 | 讀碼盤點逾時與 lease 機制；使用者選範圍「可觀察性＋lease 續租」，並回答 Q1～Q4 |
| 2026-10-21 | 使用者核准 SCN-001～007 |
| 2026-10-21 | 重現 lease 過期後的重複執行與共用 session；實作續租與進度，重現腳本與中止測試複驗通過 |
| 2026-10-21 | PR #20 審查 PASS；使用者對 D-1 選本 PR 小修（新增 SCN-008），對 D-2 選只在文件註明（TBD-5） |

---
**建立日期**: 2026-10-08
**分級**: Medium — 跨 wukong-scheduler、wukong-runtime、wukong-cli、wukong-schedulerd，沿既有相依方向，不改 schema（2026-10-21 由 Large 調降）
**風險**: Medium
**狀態**: 實作中

GitHub：https://github.com/raybird/Wukong/issues/13
