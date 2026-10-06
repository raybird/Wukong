# Issue 9 - 回合記憶只寫入每個 session 的第一回合；修正後同步調整召回與合併

## 概述

依 [GitHub issue #9](https://github.com/raybird/Wukong/issues/9)：`run_turn` 寫入回合記憶的防重複 key 是 `runtime:{session_id}:user|assistant`，而同一 scope 的 session 會長期沿用，所以每個 session 只有第一個回合被記住，之後的回合靜默略過。自 `e9a9e85`（2026-07-05）起存在。

2026-10-06 在 RunWuKong（v0.22.0）實測：Telegram 回合完成、`chat_messages` 有寫入，但 `memories` 沒有新增；該 scope 只有 2026-09-08 第一個回合的兩筆，dedupe key 為 `runtime:ses_f7f91dc0…:user/assistant`。

因為每個 scope 目前最多 2 筆，召回注入與自動合併在正式環境從未以真實資料量運作。修正寫入會同時啟動這兩條路徑，所以本 issue 先調整召回、補齊合併的安全性，最後才修正寫入。

## 文件清單
- [implementation-plan.md](./implementation-plan.md)

## 關鍵差異

| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| 回合記憶寫入 | 每個 session 只寫第一回合 | 每個回合都寫入；同一次回合只寫一次 |
| 最後一棒的記憶 | 關鍵字＋最近 50 筆＋向量，固定取前 5 筆 | 只取與輸入相關的記憶，不相關就不注入；最近對話交給 session resume |
| 輔助棒的記憶 | 同最後一棒 | 相關記憶＋最近一個回合，讓無 session 的輔助棒看得懂指代 |
| 注入長度 | 全文 | 每筆有上限，超過截斷並標示 |
| 合併失敗 | 一個 scope 失敗，本輪後續 scope 全部跳過 | 失敗的 scope 跳過並記錄，其他 scope 照常合併 |

## 涉及檔案

```text
crates/
├── wukong-runtime/src/turn.rs          # 可改：寫入 key、最後一棒／輔助棒的召回
├── wukong-runtime/src/maintenance.rs   # 可改：scope 失敗隔離
├── wukong-memory/src/lib.rs            # 可改：召回模式、合併的空摘要防護
├── wukong-memory/src/recall/mod.rs     # 可改：召回來源與相關度門檻
├── wukong-gateway/src/prompt.rs        # 可改：注入長度上限
├── wukong-schedulerd/                  # 可改：按需模式合併的驗證
├── wukong-telegram/src/dispatch.rs     # 不可觸及：固定說明文字另行追蹤
└── wukong-memory/src/store/            # 不可觸及：不改 schema 與防重複機制本身
AGENTS.md                               # 常青文件：「一回合資料流」的召回描述
```

## Gherkin 驗收劇本

```gherkin
Feature: 每個回合都被記住，且記憶只在有用時注入

  @SCN-001
  Scenario: 同一 session 的連續回合都寫入記憶
    Given 一個已有 session 的 scope
    When 在同一 session 連續完成兩個回合，第二回合的輸入與第一回合相同
    Then 該 scope 的回合記憶共有四筆，兩個回合的 User 與 Assistant 各一筆
    And 同一次回合的寫入只產生兩筆

  @SCN-002
  Scenario: 最後一棒不注入只因時間接近而入選的記憶
    Given scope 內最近的記憶與這次輸入沒有關鍵字或語意關聯
    When 執行一個回合
    Then 最後一棒的 prompt 不含這些記憶

  @SCN-003
  Scenario: 最後一棒仍注入與輸入相關的舊記憶
    Given scope 內有一筆與這次輸入關鍵字相符的舊記憶，以及多筆較新但不相關的記憶
    When 執行一個回合
    Then 最後一棒的 prompt 含有那筆相關的舊記憶

  @SCN-004
  Scenario: 輔助棒取得最近一個回合
    Given scope 內已完成至少兩個回合，且最近一回合與這次輸入沒有關鍵字關聯
    When 執行一個包含輔助棒的回合
    Then 每個輔助棒的 prompt 含有最近一個回合的 User 與 Assistant
    And 不含更早回合中與輸入不相關的記憶

  @SCN-005
  Scenario: 注入的單筆記憶有長度上限
    Given 一筆被召回的記憶長度超過上限
    When 這筆記憶注入 prompt
    Then 注入內容截斷在上限內並標示已截斷
    And 資料庫中的原始記憶不變

  @SCN-006
  Scenario: 單一 scope 合併失敗不阻擋其他 scope
    Given 兩個 scope 的未合併記憶都達到門檻
    And 第一個 scope 的摘要呼叫失敗
    When 執行一輪自動維護
    Then 第二個 scope 完成合併
    And 第一個 scope 的原始記憶全部保留
    And 失敗被記錄在日誌

  @SCN-007
  Scenario: 空白摘要不刪除原始記憶
    Given 一個 scope 的未合併記憶達到門檻
    And 摘要呼叫成功但回傳空白內容
    When 執行一輪自動維護
    Then 不寫入摘要記憶
    And 該批原始記憶保留且未標記為已合併

  @SCN-008
  Scenario: 按需模式下 schedulerd 能完成合併
    Given schedulerd 使用按需 OpenCode（WUKONG_AGENT_CMD=opencode run，未設 server URL）
    And 一個 scope 的未合併記憶達到門檻
    When schedulerd 執行一輪自動維護
    Then 該 scope 產生非空摘要，被摘要的原始記憶被刪除
    And 維護結束後沒有殘留的 OpenCode 程序
```

## Gherkin 核准紀錄
- **核准 commit**: a51009c
- **核准來源**: 使用者 2026-10-06 對話。先同意範圍建議（修正寫入 key、合併的真實證據與 scope 失敗隔離、注入長度上限），再就「最後一棒拿掉最近來源並加相關度門檻、輔助棒保留小型最近視窗、先改召回再修寫入」回覆「好開 issue」。SCN-007 是建檔時才發現的空摘要刪除路徑，屬新增行為，使用者同日另行核准「保留原始記憶」方案。

| Scenario | 核准日期 | 狀態 |
|----------|---------|------|
| SCN-001 | 2026-10-06 | 已核准 |
| SCN-002 | 2026-10-06 | 已核准 |
| SCN-003 | 2026-10-06 | 已核准 |
| SCN-004 | 2026-10-06 | 已核准 |
| SCN-005 | 2026-10-06 | 已核准 |
| SCN-006 | 2026-10-06 | 已核准 |
| SCN-007 | 2026-10-06 | 已核准 |
| SCN-008 | 2026-10-06 | 已核准 |

## 風險與首要驗證

- **最大風險**：修正寫入後，合併會在 schedulerd 以按需 OpenCode 真正執行，並刪除原始記憶。這條路徑從未在正式環境運作；摘要失敗或為空白時，可能卡住所有 scope 或刪掉沒有被有效摘要的資料。
- **風險等級與理由**：High。涉及資料刪除，且召回調整會改變每一個回合的 prompt。
- **首要驗證**：在修正寫入前，先以測試鎖定合併的失敗路徑（SCN-006、SCN-007），再以真實的按需 OpenCode 程序跑通 schedulerd 合併（SCN-008）。
- **選擇理由**：合併是唯一會刪資料的步驟，也是唯一換了執行環境卻沒有真實證據的路徑；召回與寫入的改動都可用單元與整合測試直接驗證。
- **完成證據**：SCN-006、SCN-007 先紅後綠；SCN-008 有真實程序的執行紀錄，包含摘要內容、刪除筆數與程序收尾檢查。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|------|------|------|------|
| TBD-1 | SCN-007：空白摘要時是否保留原始記憶、不寫摘要 | 已解決 | 2026-10-06 使用者選擇保留原始記憶、不寫摘要（不採機械式摘要，也不延後處理） |
| TBD-2 | 向量召回的相似度門檻值 | 待確認 | SCN-002；未啟用 embedding 的部署不受影響，實作時以測試資料決定並記錄依據 |

## Timeline

| 日期 | 異動 | 負責人 |
|------|------|--------|
| 2026-10-06 | 建立；SCN-001～006、SCN-008 依使用者同日對話核准，SCN-007 待核准 | Claude |
| 2026-10-06 | 使用者核准 SCN-007：空白摘要時保留原始記憶、不寫摘要 | 使用者 |

---
**建立日期**: 2026-10-06  
**分級**: Medium — 跨四個 crate，沿既有相依方向，無 schema 變更  
**風險**: High\
**狀態**: 待實作
