# Issue 0003 - opencode session 保留期清理：避免 opencode.db 無限成長

GitHub：<https://github.com/raybird/Wukong/issues/3>

## 概述

opencode 把每個 session 的訊息、片段與事件歷史存進 `opencode.db`。Wukong 只在輔助棒跑完、session 輪替與使用者下 `/new` 時刪除 session，沒有依年齡的清理，漏出這三個時點的 session 永久留存。本 issue 加入保留期清理：定期刪除過期且無 scope 指向的 session、清理前可預覽、`opencode-server` 重啟時回收檔案空間，並修正 `wukong --new` 的漏刪。

Wukong 自己另存對話（`wukong-memory` 與 `wukong-chat-history`），不讀取舊的 opencode session；只有 `agent_sessions` 內每個 scope 目前指向的那一個會被續接。

## 文件清單

- [implementation-plan.md](./implementation-plan.md)

## 涉及檔案

```text
.
├── crates/wukong-gateway/src/           # 可改：列出 session 的能力、CLI 子命令定義
├── crates/wukong-memory/src/            # 可改：列出所有被 scope 指向的 session id
├── crates/wukong-runtime/src/           # 可改：挑選與清理邏輯
├── crates/wukong-schedulerd/src/        # 可改：定期執行與設定
├── crates/wukong-cli/src/               # 可改：預覽／手動清理、空間回收子命令、--new 漏刪
├── scripts/docker-entrypoint.sh         # 可改：opencode serve 啟動前呼叫空間回收
├── scripts/test-docker-runtime.sh       # 可改：對應的 entrypoint 檢查
├── docker-compose.yml                   # 可改：只新增環境變數傳遞
├── docker-compose.release.yml           # 可改：同上
├── .env.example                         # 可改：新增設定
├── docs/docker.md、docs/cli-reference.md、CHANGELOG.md、AGENTS.md  # 可改：設定說明、指令參考與變更紀錄
├── docs/2026-08-08-system-freeze-opencode-resource-handover.md   # 可改：只加註已回答的未知
├── Cargo.lock、各 crate 的 Cargo.toml   # 可改：wukong-cli 新增 sqlx、rustix；wukong-runtime 測試用 sqlx
├── scripts/opencode-idle-restart.sh     # 不可觸及：閒置重啟的判定不變
├── crates/wukong-runtime/src/session.rs # 不可觸及：compaction 與輪替政策不變
└── opencode.db 的資料表                 # 不可觸及：刪除只走 opencode 的刪除入口，不直接改寫資料列
```

## Gherkin 驗收劇本

```gherkin
Feature: opencode session 的保留期清理

  @SCN-001
  Scenario: 過期且無 scope 指向的 session 被刪除
    Given opencode 中有最後更新早於保留期、且沒有任何 scope 指向的 session
    And 也有保留期內的 session
    When 清理執行
    Then 過期的無主 session 及其訊息、片段與事件歷史都不再存在
    And 保留期內的 session 完全不變

  @SCN-002
  Scenario: 被 scope 指向的 session 一律保留
    Given 某個 scope 目前指向的 session 已超過保留期
    When 清理執行
    Then 這個 session 不被刪除
    And 該 scope 的下一回合仍續接同一個 session

  @SCN-003
  Scenario: 保留期可設定，清理可停用
    Given 以環境變數設定保留天數，或設為 0 停用
    When 到達清理時間
    Then 清理依設定的天數執行；停用時不刪除任何 session
    And 未設定時保留 30 天

  @SCN-004
  Scenario: 無法確認哪些 session 受保護時不刪除
    Given 讀取 scope 指向表失敗，或 opencode 的 session 列表取不到、解析不了
    When 清理執行
    Then 這一輪不刪除任何 session
    And 失敗原因被記錄，下一輪照常再試

  @SCN-005
  Scenario: session 數量超過列表預設上限時仍完整處理
    Given opencode 中的 session 超過 100 個，其中過期的無主 session 排在第 100 筆之後
    When 清理執行
    Then 這些 session 仍被看到並刪除
    And 列表若被截斷，紀錄中會標示

  @SCN-006
  Scenario: 清理失敗不影響服務
    Given 清理途中某個 session 刪除失敗，或 opencode server 無回應
    When 清理執行
    Then 失敗被記錄，其餘可刪的 session 照常處理
    And 進行中的聊天與排程回合照常完成

  @SCN-007
  Scenario: server 重啟時回收檔案空間
    Given 清理刪除了大量 session，資料庫內可回收的空間超過門檻
    When opencode-server 容器重新啟動
    Then 資料庫檔案在 server 開始服務前變小
    And 回收失敗、資料庫被鎖或磁碟空間不足時，server 照常啟動

  @SCN-008
  Scenario: wukong --new 不留下舊 session
    Given 某個 scope 已有指向的 session
    When 以 wukong --new 開新 context
    Then 原本的 session 已從 opencode 刪除
    And 該 scope 不再指向任何 session

  @SCN-009
  Scenario: 預覽不刪除任何資料
    Given opencode 中有會被清理刪除的 session
    When 執行清理的預覽
    Then 列出會被刪除與受保護的 session，且沒有任何 session 被刪除
    And 之後實際執行清理所刪除的集合與預覽列出的相同

  @SCN-010
  Scenario: 記憶庫與 server 對不上時不刪除
    Given 記憶庫指向的 session 沒有任何一個出現在 opencode server 的清單裡
    When 清理或預覽執行
    Then 不刪除任何 session，也不列出任何待刪項目
    And 輸出說明原因與所用的記憶庫
```

## Gherkin 核准紀錄

- **核准 commit**: cbd65ef（SCN-001 至 SCN-009 的原核准版本為 c613028，其內容未變）
- **核准來源**: 使用者於 2026-10-01 對話指出 Wukong 與 raybird/telenexus#9 有相同的 `opencode.db` 膨脹問題；我提出設計草稿與九項驗收條件後，使用者同日在確認題中選擇「9 項全部核准」，並選擇由我開立 GitHub issue、以 dev-cycle 推進。九項即 SCN-001 至 SCN-009。「被棄置 scope 的過期」與「長壽 session 輪替」在同一題中列為不在範圍。

- **SCN-010 的核准來源**: 2026-10-01 的獨立審查（[review-6037672.md](./review-6037672.md) 的 M-2）指出清理倚賴「server 上的 session 都屬於這一份記憶庫」這個未被防護的前提，並重現了記憶庫接錯時受保護 session 被列為待刪。使用者同日在確認題的四個選項中只選了「接錯記憶庫時整輪不刪」；「compose 以外預設停用」與「只清標題為 Wukong 的 session」未被選擇，因此不實作。

| Scenario | 核准日期 | 狀態 |
|----------|---------|------|
| SCN-001 | 2026-10-01 | 已核准 |
| SCN-002 | 2026-10-01 | 已核准 |
| SCN-003 | 2026-10-01 | 已核准 |
| SCN-004 | 2026-10-01 | 已核准 |
| SCN-005 | 2026-10-01 | 已核准 |
| SCN-006 | 2026-10-01 | 已核准 |
| SCN-007 | 2026-10-01 | 已核准 |
| SCN-008 | 2026-10-01 | 已核准 |
| SCN-009 | 2026-10-01 | 已核准 |
| SCN-010 | 2026-10-01 | 已核准 |

## 重構步驟概要

任務清單、狀態與證據只維護在 [implementation-plan.md](./implementation-plan.md) 的「實作步驟」：

1. 在真實規模的資料庫複本上量測刪除與空間回收
2. 挑選待刪 session 的邏輯
3. 列表能力與受保護集合
4. 清理執行與預覽
5. 掛進 schedulerd 與設定
6. 啟動前空間回收
7. 修正 `--new` 漏刪
8. 複本上的端到端驗證與文件
9. 審查退回的修正

## 風險與首要驗證

- **最大風險**：刪除不可逆。若挑選規則或受保護集合判斷錯誤，會刪掉仍在續接的對話；若 opencode 的刪除在大型資料庫上很慢或鎖表，清理會拖住進行中的回合。小型複本上的行為已確認（見 implementation-plan 的「現況查核」），但大型資料庫的耗時、子 session 的去向、`VACUUM` 的鎖定行為仍未知。
- **風險等級與理由**：High。涉及資料刪除與不可逆操作，且外部工具在真實規模下的行為尚未確認。
- **首要驗證**：寫任何清理程式之前，把一份數百 MB 規模的真實 `opencode.db` 複製到 repo 之外，在拋棄式容器裡對複本實際執行列表、刪除與空間回收，量測並記錄結果。
- **選擇理由**：風險來自外部工具的行為與資料規模，兩者只能靠真實資料試跑確認；mock 與小型複本回答不了耗時與鎖定。
- **完成證據**：父／子 session 刪除前後的列數差異；單次與批次刪除的耗時；`VACUUM` 前後檔案大小、耗時與期間其他連線的行為。證據只記彙總數字，不含對話內容。

> **2026-10-01 追記**：首要驗證已完成，上述未知都已有量測值，見 [implementation-plan.md](./implementation-plan.md) 步驟 1 的完成證據。樣本是開發機的 opencode 資料庫，不是受影響部署的那一份，所以 TBD-3 仍未回答。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|------|------|------|------|
| TBD-1 | 清理間隔與每輪刪除上限的預設值 | 已解決 | 2026-10-01：固定每 6 小時一輪、每輪上限 500 個。連續刪除 484 個 session 共 4.43 秒，不需要小批次，也不另開環境變數 |
| TBD-2 | 空間回收的觸發門檻 | 已解決 | 2026-10-01：freelist 佔比達 25% 且剩餘磁碟空間不小於資料庫大小的 2 倍才回收。831 MB 全量 `VACUUM` 耗時 2.59 秒、WAL 峰值約 1 倍資料庫大小 |
| TBD-3 | 受影響部署的資料庫裡，無主 session 佔多少資料量。08-08 記錄的 1.33 GiB 在另一台主機，本機沒有複本；Wukong 的資料量可能集中在少數長壽的 scope session，而它們是本 issue 保留的對象 | 待確認 | 不阻塞實作。步驟 4 的預覽就是量測工具，要在受影響的主機上執行才有答案；若可刪的只佔一小部分，另開 issue 評估輪替 |
| TBD-5 | 把 Wukong 接到使用者自己也在用的 `opencode serve` 時，只要 Wukong 在上面跑過回合，使用者自己超過保留期的 session 就會被當成無主而刪除（review-6037672 的 M-2 輸入一） | 不影響本次交付 | 2026-10-01：使用者未選擇「compose 以外預設停用」，清理維持預設啟用。改由文件要求這種用法把保留天數設為 0；compose 部署的 volume 專屬於 Wukong，不受影響 |
| TBD-4 | 08-08 文件記錄的 session 數恰為 100，與 `GET /session` 的預設上限相同，當時的實際數量可能更多 | 待確認 | 不影響本次交付；與 TBD-3 一併在受影響主機上確認 |

## Timeline

| 日期 | 異動 | 負責人 |
|------|------|--------|
| 2026-10-01 | 建立；SCN-001 至 SCN-009 依使用者同日對話核准 | - |
| 2026-10-01 | 獨立審查 review-6037672 判定 RETURN TO execute-task（兩項 MUST FIX） | - |
| 2026-10-01 | 規格修訂：新增 SCN-010，依使用者對 M-2 的選擇核准；SCN-001 至 SCN-009 不變 | - |

---
**建立日期**: 2026-10-01  
**分級**: Medium（跨五個 crate，但沿既有相依方向、邏輯直觀，不涉及 schema 或架構變更）  
**風險**: High\
**狀態**: 實作完成，待審查
