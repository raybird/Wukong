# Issue 14 - 排程回合寫進使用者的記憶 scope，排程產出會稀釋並永久佔據使用者記憶

## 概述
排程回合改在自己的記憶 scope 執行，不再把提示詞與報告寫進使用者的記憶；Telegram 回送與聊天紀錄照舊送到建立排程的使用者。

問題機制、正式部署現況與外部參照見 GitHub issue 本文；2026-10-20 的決定與兩個新發現見 [#14 留言](https://github.com/raybird/Wukong/issues/14#issuecomment-6284844309)。

## 快速導覽

### 當前流程
`schedule add-turn --scope <使用者 scope>` → 觸發時 `cfg.scope = <使用者 scope>` → 召回、session、記憶寫入都用使用者 scope → 依同一個 scope 還原 chat id 回送 Telegram，並寫入該使用者的聊天紀錄。

### 變更後流程
job 的 `scope` 改稱「擁有者 scope」，只用來決定回送對象與聊天紀錄 → 觸發時召回、session、記憶寫入改用 `agent:schedule-<job id>` → 回送與聊天紀錄仍依擁有者 scope。

## 關鍵差異
| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| 排程回合寫入的記憶 | 使用者 scope | `agent:schedule-<job id>` |
| 排程回合讀得到的記憶 | 使用者 scope + global | 自己的 scope + global |
| 排程回合的 opencode session | 與使用者對話共用 | 每個排程各自一個，跨次觸發延續 |
| Telegram 回送、聊天紀錄 | 依 job scope | 依擁有者 scope（同一個欄位，行為不變） |
| 對話中查排程結果 | 只能從記憶召回 | agent 依提示執行 `wukong schedule runs --id <id>` |

## 涉及檔案
- `crates/wukong-scheduler/src/executor.rs` - Turn job 執行時改用衍生 scope。
- `crates/wukong-runtime/src/persona.rs` - 排程能力提示補上查詢執行結果的指令，並說明 `--scope` 是擁有者。
- `crates/wukong-schedulerd/src/notify.rs` - 預期不修改；以測試確認回送仍依擁有者 scope。

## 文件清單
- [implementation-plan.md](./implementation-plan.md)

## Gherkin 驗收劇本
```gherkin
Feature: 排程回合使用獨立的記憶 scope，排程產出不再寫入使用者的記憶，回送照舊送達建立者

  Background:
    Given 使用者 scope 為 user:tg-42
    And 一個 Turn 排程由該使用者建立，擁有者 scope 為 user:tg-42

  @SCN-001
  Scenario: 排程回合不寫入使用者的記憶
    When 排程觸發並完成一個回合
    Then 該回合的提問與回覆寫入 agent:schedule-<job id>
    And user:tg-42 沒有新增任何記憶

  @SCN-002
  Scenario: 排程回合不讀使用者的記憶，也不接續使用者的對話 session
    Given user:tg-42 已有與排程提示詞相關的記憶，以及進行中的對話 session
    When 排程觸發
    Then 排程回合的召回只涵蓋 agent:schedule-<job id> 與 global
    And 排程回合使用自己的 session，不取得 user:tg-42 的 session lease

  @SCN-003
  Scenario: Telegram 回送與聊天紀錄照舊送到建立者
    When 排程成功或失敗
    Then 結果回送到 chat id 42
    And 結果寫入 user:tg-42 的聊天紀錄

  @SCN-004
  Scenario: 同一個排程的多次觸發延續同一個 scope，不同排程互不相通
    Given 另一個排程也由 user:tg-42 建立
    When 兩個排程各觸發兩次
    Then 每個排程的第二次觸發讀得到自己第一次的記憶與 session
    And 讀不到另一個排程的記憶

  @SCN-005
  Scenario: 對話中可以查到排程的執行結果
    When 使用者在對話中詢問某個排程上次回報了什麼
    Then agent 收到的排程能力提示包含查詢執行結果的指令 `wukong schedule runs --id <id>`
```

## Gherkin 核准紀錄
- **核准 commit**: 待提交
- **核准來源**: 2026-10-20 使用者對 #14 TBD-1 選「(a)」；同日逐題確認 Q1「執行時衍生」、Q2「不讀」、Q3「提示 agent 用 CLI 查」、Q4「不在本 issue 處理」。上列 Scenario 由這些答案推導，2026-10-20 使用者回覆「全部核准，TBD-4 照建議」。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-10-20 | 已核准 |
| SCN-002 | 2026-10-20 | 已核准 |
| SCN-003 | 2026-10-20 | 已核准 |
| SCN-004 | 2026-10-20 | 已核准 |
| SCN-005 | 2026-10-20 | 已核准 |

## 風險與首要驗證
- **最大風險**：執行時換 scope 後，仍有某條路徑用 `cfg.scope` 代表「使用者」（回送、聊天紀錄、session 保留期清理、排程能力提示），結果靜默失效且沒有錯誤；既有排程下次觸發就會切換，沒有灰度。
- **風險等級與理由**：Medium — 核心改動只在 executor 一處，但 `cfg.scope` 在一回合中被多處使用，任何一處預期它是使用者 scope 就會壞；失敗不會報錯（例如回送 `Ok(false)` 直接略過）。
- **首要驗證**：主要實作前先盤點一個 Turn job 執行期間所有讀 `cfg.scope` 與 job scope 的地方，並以 characterization 測試鎖住現行回送與聊天紀錄的行為（依 job scope 送到 chat id）。
- **選擇理由**：失敗面是「誰在用 scope」這個既有行為，不是外部契約或效能；盤點加上回送的 characterization 測試最直接，能在改 executor 前確定改動邊界。
- **完成證據**：盤點表列出每個使用點與改後應使用的 scope；回送／聊天紀錄的 characterization 測試在改動前後都綠；SCN-001～004 的測試先紅後綠。

## 待確認事項
| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-1 | 處理方向 | 已解決 | 2026-10-20 使用者選 (a)：排程回合預設使用獨立 scope |
| TBD-2 | 選 (a) 時，使用者想在對話中問「排程昨天報了什麼」要怎麼取得 | 已解決 | 2026-10-20 使用者選 Q3「提示 agent 用 CLI 查」，對應 SCN-005 |
| TBD-3 | 排程 Scenario SCN-001～005 的核准 | 已解決 | 2026-10-20 使用者回覆「全部核准」 |
| TBD-4 | 排程回合內建立的新排程，擁有者會是 `agent:schedule-<job id>`，結果不會回送 Telegram | 不影響本次交付 | 2026-10-20 使用者回覆「TBD-4 照建議」，接受為已知限制。追蹤：無；排程回合再建立排程目前沒有使用情境，出現需求時另開 issue |
| TBD-5 | 使用者 scope 中既有的排程紀錄 | 不影響本次交付 | 2026-10-20 使用者選 Q4「不在本 issue 處理」。追蹤：無；2026-10-16 正式庫只有 8 筆 event、沒有 summary，分辨哪些由排程寫入需比對提示詞，誤刪風險高於殘留的影響 |

## 關聯議題
- #11（已合併，PR #15）：planner 帶入同 scope 的上一回合；排程改用獨立 scope 後，上一回合不會再是排程產出。
- #12：聊天與排程共用 scope 造成 session lease 競爭。本 issue 合併後，預設排程不再與聊天共用 session，#12 的 SCN-001 前提只剩「使用者明確指定同一 scope」的情況；共用工作目錄（#12 SCN-003）不受影響。#12 的範圍需在本 issue 合併後重新界定。

## Timeline
| 日期 | 異動 |
|---|---|
| 2026-10-16 | 在 GitHub 建立 issue，處理方向待決定 |
| 2026-10-20 | 使用者選 (a)；逐題確認 Q1～Q4；建立 README 與實作計畫 |
| 2026-10-20 | 使用者核准 SCN-001～005，TBD-4 接受為已知限制 |

---
**建立日期**: 2026-10-20  
**分級**: Medium — 跨 wukong-scheduler 與 wukong-runtime 兩個 crate，沿既有相依方向，沒有 schema 變更  
**風險**: Medium  
**狀態**: 待實作

GitHub：https://github.com/raybird/Wukong/issues/14
