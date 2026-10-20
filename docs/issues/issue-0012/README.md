# Issue 12 - 聊天與排程同時在同一個工作目錄執行，彼此不知道對方存在

## 概述
回合開始執行時，若另有回合正在執行，就在 prompt 中告知 agent，讓它避開同一批檔案與提交；不阻擋任何一方執行。

2026-10-20 重新界定範圍。原標題「Telegram 對話與排程共用 scope，競爭 session 導致排程失敗」描述的 session 競爭已由 #14（PR #16）解決：排程回合改在 `agent:schedule-<job id>` 執行，不再與聊天搶同一個 session lease。拆開之後，聊天與排程會在同一個 `WUKONG_WORKSPACE` 同時執行，兩者之間沒有任何互斥或告知。原 issue 的 SCN-003（共用工作目錄操作安全）因此成為主要問題。原本的 2026-10-08 證據與驗收草案見 GitHub issue 本文；草案從未核准，本文件取代它。

## 快速導覽

### 當前流程
聊天回合（scope `user:tg-42`）與排程回合（scope `agent:schedule-<id>`）各自取得自己的 session lease，同時在同一個工作目錄執行。兩邊的 agent 都不知道對方存在。

### 變更後流程
回合取得 session lease 後，查詢其他 scope 目前仍有效的 lease。有的話，在這個回合每一棒的 prompt 加上告知區塊，列出正在執行的回合，並要求 agent 避開對方可能在改的檔案與提交。執行照常進行，不等待、不排隊。

## 關鍵差異
| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| 聊天與排程同時執行 | 可以，彼此不知道 | 可以；後開始的回合在 prompt 中得知對方 |
| 沒有其他回合在執行 | - | prompt 與現行逐字相同 |
| 是否阻擋或排隊 | 否 | 否 |

## 已確認證據與重現（2026-10-20）
同一個工作目錄與記憶庫，用假 agent（每次呼叫睡 3 秒，並在工作目錄記下開始與結束時間）同時跑聊天回合 `wukong --scope user:tg-42 "…"` 與 `wukong schedule trigger`（擁有者 `user:tg-42`）。腳本與紀錄在本機 scratchpad 的 `repro12/`，結果抄錄如下：

| | #14 之前（`e9af283`） | #14 之後（`1af4528`） |
|---|---|---|
| 聊天 | exit 0 | exit 0 |
| 排程 | exit 1：`session lease unavailable for scope user:tg-42`（與 2026-10-08 run #869 同一個錯誤） | exit 0 |
| 工作目錄中的時間交錯 | 排程的 planner 仍與聊天重疊約 3 秒，接著在取得 lease 時失敗 | 兩邊每一棒都交錯執行 |

- 原 session 競爭已不會發生（右欄排程成功）。
- #14 之前的 lease 也只在 planner 之後才取得，所以互斥本來就不完整；#14 之後完全沒有。
- 尚未觀察到兩個 agent 實際改壞同一批檔案；目前證明的是會同時執行。

## 程式碼現況（2026-10-20 讀碼）
- `run_turn_traced_with_attachments`（`crates/wukong-runtime/src/turn.rs`）在 planner 之後呼叫 `session::prepare_final_session` 取得 `agent_session_state` 的 lease，之後才逐棒執行，結束時釋放。
- lease 預設 900 秒（`WUKONG_SESSION_LEASE_SECS`），執行中不續租。
- 所有進入點（CLI、Web、Telegram、Scheduler）都走 `run_turn`，工作目錄都是同一個 `WUKONG_WORKSPACE`。
- `wukong-runtime` 不依賴 `wukong-scheduler`，只能從 scope 名稱判斷是不是排程（`agent:schedule-<id>`），拿不到排程名稱。

## 涉及檔案
- `crates/wukong-memory/src/store/mod.rs`、`crates/wukong-memory/src/lib.rs` - 新增查詢：列出 lease 仍有效的 scope。
- `crates/wukong-runtime/src/turn.rs` - 取得 lease 後查詢其他回合，把告知區塊加到每一棒的 prompt。
- `crates/wukong-runtime/src/persona.rs` - 告知區塊的文字。

## 文件清單
- [implementation-plan.md](./implementation-plan.md)

## Gherkin 驗收劇本
```gherkin
Feature: 回合開始執行時，若另有回合正在同一個工作目錄執行，agent 會被告知；任何一方都不被阻擋

  @SCN-001
  Scenario Outline: 另一個回合執行中時，新回合的每一棒都收到告知
    Given <其他 scope> 持有仍有效的 session lease
    When <本回合 scope> 開始一個回合
    Then 這個回合每一棒的 prompt 都包含「其他回合正在執行」區塊
    And 區塊把 <其他 scope> 標示為 <類型>
    And 區塊要求 agent 避免修改對方可能正在修改的檔案，也不要提交對方的變更

    Examples:
      | 本回合 scope         | 其他 scope           | 類型 |
      | user:tg-42           | agent:schedule-job-1 | 排程 |
      | agent:schedule-job-1 | user:tg-42           | 對話 |

  @SCN-002
  Scenario: 沒有其他回合執行時，prompt 不變
    Given 除了本回合，沒有任何 scope 持有有效的 session lease
    When 開始一個回合
    Then 每一棒的 prompt 與變更前逐字相同

  @SCN-003
  Scenario: 過期的 lease 與自己的 lease 不列入
    Given 某個 scope 的 lease 已過期
    When 開始一個回合
    Then 告知區塊不列出那個 scope，也不列出本回合自己的 scope

  @SCN-004
  Scenario: 告知不阻擋執行
    Given 聊天回合與排程回合在同一個工作目錄同時執行
    When 兩者都完成
    Then 兩者都成功結束，沒有任何一方等待或失敗
```

## Gherkin 核准紀錄
- **核准 commit**: 802cb07
- **核准來源**: 2026-10-20 使用者對重新界定選「縮小為工作目錄併行」，對處理策略選「提示告知」。上列 Scenario 由這兩個答案推導；同日使用者回覆「都接受」，核准 SCN-001～004 並接受 TBD-2～4 的建議。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-10-20 | 已核准 |
| SCN-002 | 2026-10-20 | 已核准 |
| SCN-003 | 2026-10-20 | 已核准 |
| SCN-004 | 2026-10-20 | 已核准 |

## 風險與首要驗證
- **最大風險**：把「有效的 session lease」當成「正在執行的回合」，但兩者可能對不上，告知就會漏報或誤報。已知的不一致有兩種：回合超過 900 秒後 lease 過期而且不續租（#863 跑了 1506 秒），以及回合當掉時 lease 殘留到過期。另一個未知是模型會不會理會告知。
- **風險等級與理由**：Medium — 失敗後果是告知不準或被模型忽略，等於回到現狀，不會比現在更差。改動也有界：新增一個查詢，並加一段 prompt。由 Large／High 調降，因為範圍已從「排隊與續接」縮小成告知，而且不阻擋任何執行。
- **首要驗證**：在臨時目錄跑一個長時間的假 agent 回合，執行期間從另一個行程查詢 `agent_session_state`，確認 lease 在整個回合期間都可見、結束後釋放。
- **選擇理由**：整個設計建立在「lease 可以代表執行中」這個假設上，最直接的驗證就是在真實的回合生命週期中觀察它。
- **完成證據**：記錄回合執行中與結束後的查詢結果；SCN-001～003 的單元測試先紅後綠；SCN-004 以本文件的重現腳本在變更後再跑一次，兩邊都是 exit 0。

## 待確認事項
| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-1 | 安全排隊或隔離方式、等待與取消語意 | 已解決 | 2026-10-20 使用者選「提示告知」：不排隊、不隔離、不阻擋 |
| TBD-AC | Scenario 的具體行為 | 已解決 | 2026-10-20 使用者回覆「都接受」，核准 SCN-001～004 |
| TBD-2 | 回合超過 lease 時間（預設 900 秒）後，其他回合看不到它 | 不影響本次交付 | 2026-10-20 使用者接受為已知限制；要解決得在執行中續租，屬於長時間排程的範圍。追蹤：#13 |
| TBD-3 | 回合當掉時 lease 殘留到過期，這段期間會誤報 | 不影響本次交付 | 2026-10-20 使用者接受。追蹤：無；誤報的後果只是 agent 多一分謹慎，lease 最多殘留 900 秒 |
| TBD-4 | 模型是否理會告知 | 不影響本次交付 | 2026-10-20 使用者接受：驗收只要求 prompt 有告知，不驗證模型行為。追蹤：無；告知是軟性防護，使用者已選擇不採用排隊等硬性做法 |

## Timeline
| 日期 | 異動 |
|---|---|
| 2026-10-08 | 依使用者授權建立議題與驗收草案，尚未實作 |
| 2026-10-20 | #14 合併後重新界定：session 競爭已解決，範圍縮小為工作目錄併行的告知；重現前後對照 |
| 2026-10-20 | 使用者核准 SCN-001～004，接受 TBD-2～4 |

---
**建立日期**: 2026-10-08  
**分級**: Medium — 跨 wukong-memory 與 wukong-runtime，沿既有相依方向，沒有 schema 變更（2026-10-20 由 Large 調降）  
**風險**: Medium  
**狀態**: 實作中

GitHub：https://github.com/raybird/Wukong/issues/12
