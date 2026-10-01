# Issue 0003 - 實作計畫

基準版本：`ghcr.io/raybird/wukong:v0.21.11`（內含 opencode 1.18.29）

關聯文件：`docs/2026-08-08-system-freeze-opencode-resource-handover.md`（P2「建立 OpenCode session retention 與 DB 維護流程」）；對照 raybird/telenexus#9。

## 現況查核

### 對照原始碼（2026-10-01）

| 事實 | 依據 |
|---|---|
| Server backend 建立的 session 標題固定為 `Wukong` | `crates/wukong-gateway/src/opencode_server.rs:129-143` |
| 輔助棒與 helper 呼叫跑完即刪 | `opencode_server.rs:720-764`；`crates/wukong-runtime/src/turn.rs:242-257` |
| 輪替後刪除舊 session | `turn.rs:377-390` |
| REPL `/new` 先刪 session 再清 mapping | `crates/wukong-cli/src/command.rs:46-52` |
| `--new` 旗標只清 mapping、不刪 session | `crates/wukong-cli/src/main.rs:35-39` |
| CLI backend 沒有實作 `delete_session`，沿用 trait 的空實作 | `crates/wukong-gateway/src/backend.rs:64-66`；`401` 起的 `impl` 內無此方法 |
| 末棒 session 建立後、寫回 `agent_sessions` 之前回合失敗，該 session 即成無主 | `turn.rs:358-376` |
| `opencode-server` 的 healthcheck 打 `/global/health`，不建立 session | `docker-compose.yml:97` |
| 環境變數由 compose 逐項傳入各服務 | `docker-compose.yml:238-241`、`docker-compose.release.yml:180-183` |

### 對照產物（2026-10-01，小型複本）

在 `opencode.db` 的複本（0.5 MB、9 個 session）上，以拋棄式容器執行，未碰任何運行中的資料。

| 問題 | 結果 |
|---|---|
| `opencode session delete <id>` 是否連帶清除歷史 | 是。刪 1 個 session 後 `message` 24→15、`part` 60→36、`event` 190→121、`event_sequence` 9→8，該 id 在 `event` 的殘留為 0 |
| HTTP `DELETE /session/{id}`（Wukong 實際使用的入口）是否等價 | 是，四張表的差異與 CLI 相同；對已刪除的 id 再打一次回 404 |
| `event` 是否靠外鍵串聯到 session | 否。`event` 的外鍵指向 `event_sequence`；歷史被清掉是 opencode 刪除邏輯的行為，不是 schema 保證，升級 opencode 後需重驗 |
| 刪除後檔案會不會變小 | 不會。空間進 freelist（111 頁中 16 頁），`auto_vacuum=0` |
| `GET /session` 不帶參數回幾筆 | 最多 100 筆。建立 150 個 session 後不帶參數回 100、`?limit=100000` 回 158 |
| `GET /session` 支援的查詢參數 | `directory`、`workspace`、`scope`、`path`、`roots`、`start`、`search`、`limit`（取自 server 的 `/doc`） |
| server 運行（閒置）時能否 `VACUUM` | 能，0.01 秒完成，之後 `/global/health` 與 `/session` 正常 |
| 映像內可用的 SQLite 工具 | 沒有 `sqlite3` CLI；`python3` 的 `sqlite3` 模組為 3.40.1；`wukong` binary 在 `/usr/local/bin` |

一個測試部署的樣態：9 個 session 中 `agent_sessions` 只指向 2 個，其餘 7 個無主——4 個是人工探測（標題不是 `Wukong`），3 個是末棒 session。

## 設計方案

### 挑選規則

一個 session 必須同時滿足以下全部條件才會被刪：

1. `time.updated` 早於保留期。
2. 不被 `agent_sessions` 或 `agent_session_state` 的任何 scope 指向。
3. 是根 session。列表帶 `roots=true`，取回後再排除帶 `parentID` 的項目，不單靠 server 端的過濾。子 session 隨父 session 一起消失（步驟 1 已確認）。

列表一律明確帶 `limit`，並以回傳筆數是否等於 `limit` 判斷是否被截斷。opencode 沒有「早於某時間」的過濾，所以整批取回再挑。

### 失效方向

任何一步不確定就不刪：

- 讀不到指向表、列不出 session、回應解析失敗 → 整輪跳過。列表可能因單一異常資料列而每一輪都失敗（步驟 1 的額外發現），所以失敗紀錄要帶上游回應，讓人看得出是同一個原因在重複。
- 保留期下限 1 天。進行中的回合所建立、尚未寫回 `agent_sessions` 的 session 必然是新的，下限確保它們不可能成為候選。
- 單次刪除失敗只記錄，下一輪自然會再遇到它。

### 執行位置

掛在 `wukong-schedulerd` 既有的維護迴圈旁（`crates/wukong-schedulerd/src/main.rs:106-127`），獨立的 interval。它已同時持有 memory 與 backend，且是 compose 內常駐的單一實例。

只在 Server backend 生效。容器部署的 `opencode-state` volume 專屬於 Wukong；CLI 模式的 `opencode.db` 與使用者自己的 opencode 使用共用，Wukong 無從分辨哪些是自己建的，因此不自動清掃。

每 6 小時一輪，每輪最多刪 500 個，由舊到新刪。

### 空間回收

在 `opencode-server` 容器啟動、`exec opencode serve` 之前執行（`scripts/docker-entrypoint.sh:271-292`）。此時 server 尚未開啟資料庫，而既有的離峰閒置重啟讓這個時點大約每天出現一次。

- freelist 佔比達 25% 才做。
- 剩餘磁碟空間不小於資料庫大小的 2 倍才做。
- 設 busy timeout；被鎖或任何失敗都只記錄，不阻擋 server 啟動。
- 由 `wukong` 的子命令執行，才能用 `cargo test` 覆蓋。

即使從不 `VACUUM`，釋出的頁也會被 SQLite 重用，檔案會停止成長；`VACUUM` 只負責讓檔案變小。

### 設定

| 變數 | 用途 | 預設 |
|---|---|---|
| `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` | 保留天數；`0` 停用清理 | `30` |

清理間隔、每輪上限與回收門檻是程式內的固定值（見步驟 1 的定案），沒有人要求調整它們，不另開環境變數。

### 可觀測性

每輪輸出一行：列出數、是否被截斷、受保護數、過期候選數、已刪數、失敗數，以及受保護但已超過保留期的數量。最後一項是被棄置 scope 的規模訊號。

## 使用方式對照

| 情境 | 變更前 | 變更後 |
|---|---|---|
| 無主的舊 session | 永久留存 | 超過保留期後由 schedulerd 刪除 |
| 想知道會刪哪些 | 無 | 執行預覽，列出待刪與受保護清單 |
| `wukong --new` | 舊 session 留在 opencode | 舊 session 被刪除 |
| 資料庫檔案大小 | 只增不減 | `opencode-server` 重啟時回收 |

## 實作步驟

1. ✅ **複本量測**（SCN-001、SCN-005、SCN-007；無相依）
   - 產出：本步驟下方的量測紀錄；TBD-1、TBD-2 的定案值。
   - 完成判準：在數百 MB 規模的真實 `opencode.db` 複本上記錄 (a) 刪除含子 session 的父 session 前後，父與子在各表的列數；(b) 單次刪除與連續刪除一批的耗時；(c) `VACUUM` 前後檔案大小、耗時、所需暫存空間，以及 server 開著且有連線時的行為；(d) 帶 `roots=true` 與明確 `limit` 的列表回傳是否完整。
   - 完成證據（2026-10-01）：樣本是開發機 `~/.local/share/opencode/opencode.db` 以 `sqlite3 .backup` 取得的複本（831,176,704 bytes、962 個 session、其中 124 個子 session、`message` 24,444、`part` 106,574、`event` 68,258），放在 repo 之外；以 `ghcr.io/raybird/wukong:v0.21.11` 的拋棄式容器對複本執行 `opencode serve`，刪除走 HTTP `DELETE /session/{id}`，列數以 `sqlite3 -readonly` 查詢。只記彙總數字，複本與容器已於量測後移除。
     - (a) 刪除一個有 30 個子 session 的父 session：父 `message` 167→0、`part` 682→0；子 `session` 30→0、`message` 334→0、`part` 1,712→0；HTTP 200，0.18 秒。另以空資料庫建立父子各一確認：子 session 的列表項帶 `parentID`；刪父之後子的 `GET` 回 404。
     - (b) 連續刪除最舊的 50 個：共 0.48 秒，中位數 9 ms、p95 22 ms、最大 48 ms，全數 200。連續刪除 484 個超過 30 天的根 session：共 4.43 秒，中位數 7 ms、p95 22 ms、最大 86 ms，全數 200。片段最多的 session（4,236 筆、15 MB）0.05 秒；事件最多的（5,837 筆、48 MB）0.05 秒，刪後 `event` 與 `event_sequence` 殘留皆為 0。全部刪完後，`message`、`part`、`event` 中找不到對應 session 的列數皆為 0。
     - (c) 刪完後 freelist 為 200,333／202,924 頁（98%），檔案仍是 831 MB；`VACUUM` 後 9,756,672 bytes，耗時 0.03 秒（成本隨存活資料量而非檔案大小）。在全新複本上對 831 MB 全為存活資料的情況 `VACUUM`：2.59 秒，期間 WAL 峰值 824,374,952 bytes（約等於資料庫大小），checkpoint 0.39 秒，`integrity_check` 為 ok；server 開著時同時輪詢 `/session`，10 次全數成功、最大延遲 2.47 秒。另一連線持有寫鎖時，`VACUUM` 在 5 秒 busy timeout 後回 `database is locked`，檔案不變。另一連線只持有讀取交易時 `VACUUM` 仍成功。暫存檔的用量未量測。
     - (d) server 目錄為非 git 的 `/workspace` 時，列表查的是 `project_id = 'global'`：不帶參數回 100 筆，`?limit=100000` 與 `?limit=100000&roots=true` 都回 304 筆，等於資料庫中該 project 的 304 個 session，且依 `time.updated` 由新到舊。`roots=true` 會濾掉子 session（空資料庫實測：不帶時回父與子、帶時只回父）。`start` 是 `time.updated` 的下界，沒有「早於」的過濾，所以只能整批取回再挑。
     - 額外發現：樣本中有 2 筆 `directory` 不是絕對路徑的資料列，只要它落在 `limit` 範圍內，整個列表請求就回 500（server 日誌為 `Path is not absolute: .`）；`limit=200` 成功、`limit=400` 起失敗。量測時在複本上把這 2 筆改成 `/tmp` 才得以繼續。這類資料列不會出現在只由 Wukong 寫入的資料庫，但它說明列表失敗必須整輪不刪（SCN-004），且同一筆壞資料會讓清理每一輪都失敗，紀錄必須看得出來。
     - 定案：刪除夠快，不需要小批次。TBD-1 定為固定每 6 小時一輪、每輪上限 500 個，不做成環境變數；TBD-2 定為 freelist 佔比達 25% 且剩餘磁碟空間不小於資料庫大小的 2 倍才回收（WAL 峰值量到 1 倍，暫存檔未量測，取保守值），同樣不做成環境變數。
2. 📝 **挑選邏輯**（SCN-001、SCN-002、SCN-003；相依：步驟 1 確認子 session 行為）
   - 產出：純函式，輸入 session 列表、受保護 id 集合、現在時間與保留期，輸出待刪清單與統計。
   - 完成判準：測試斷言被選中與被保留的具體 id；涵蓋保留期邊界、受保護的過期 session、保留期為 0、低於下限的設定。
3. 📝 **列表能力與受保護集合**（SCN-004、SCN-005；無相依）
   - 產出：`AiBackend` 列出 session 的能力（Server backend 實作）；`wukong-memory` 列出所有被 scope 指向的 session id。
   - 完成判準：對腳本化 HTTP server 的測試斷言請求帶有 `limit` 與 `roots`、回傳筆數等於 `limit` 時標示截斷、非預期回應回傳錯誤而非空清單；memory 測試斷言兩張表的 id 都被納入。
4. 📝 **清理執行與預覽**（SCN-001、SCN-002、SCN-004、SCN-006、SCN-009；相依：步驟 2、3）
   - 產出：`wukong-runtime` 的清理函式與報告；`wukong` 的預覽／手動清理子命令。
   - 完成判準：測試斷言實際送出 `DELETE` 的 id 集合；指向表或列表失敗時送出 0 個 `DELETE`；單一刪除失敗後其餘照常；預覽送出 0 個 `DELETE`，且其清單等於同一 fixture 實際執行所刪的集合。
5. 📝 **掛進 schedulerd 與設定**（SCN-003、SCN-006；相依：步驟 4）
   - 產出：schedulerd 的定期清理、環境變數解析、compose 與 `.env.example` 的對應項。
   - 完成判準：設定解析的測試涵蓋預設、停用與無效值；清理回傳錯誤時 schedulerd 迴圈繼續；compose 兩份檔案都傳入新變數。
6. 📝 **啟動前空間回收**（SCN-007；相依：步驟 1 定出門檻）
   - 產出：`wukong` 的空間回收子命令；entrypoint 在 `opencode serve` 前呼叫。
   - 完成判準：測試在暫存 SQLite 檔上斷言超過門檻時檔案變小、未達門檻時不動、檔案被鎖時回傳錯誤；entrypoint 在回收失敗時仍 `exec` server。
7. 📝 **修正 `--new` 漏刪**（SCN-008；無相依）
   - 產出：`main.rs` 的 `--new` 與 `/new` 行為一致。
   - 完成判準：先有重現漏刪的失敗測試，修正後斷言舊 session id 被刪且 mapping 被清。
8. 📝 **端到端驗證與文件**（SCN-001、SCN-002、SCN-007；相依：步驟 4、5、6）
   - 產出：在資料庫複本上以真實 opencode 跑完整清理與重啟回收的紀錄；`docs/docker.md`、`CHANGELOG.md`、`AGENTS.md` 的「關鍵設計細節」；08-08 文件驗證清單中已回答項目的勾選。
   - 完成判準：複本上過期無主 session 的 `message`／`part`／`event` 列數歸零、受保護 session 不變且可續接、重啟後檔案變小；文件中的環境變數表與程式的預設值一致。

## 測試策略

| 驗收 | 外迴圈 | 內迴圈 | 說明 |
|---|---|---|---|
| SCN-001 | 步驟 8 的真實 opencode 複本驗證 | 步驟 2、4 的單元與腳本化 server 測試 | 串聯刪除是 opencode 的行為，mock 回答不了，外迴圈不可省 |
| SCN-002 | 步驟 8 續接受保護 session | 步驟 2、4 | 同上 |
| SCN-003 | 步驟 5 設定解析測試 | 同層合併 | 行為完全由解析結果決定，無額外整合面 |
| SCN-004 | 步驟 4 腳本化 server 測試 | 步驟 3 | 斷言 `DELETE` 請求數為 0 |
| SCN-005 | 步驟 4 腳本化 server 測試 | 步驟 3 | 另以步驟 1 的真實列表確認預設上限 |
| SCN-006 | 步驟 5 schedulerd 測試 | 步驟 4 | |
| SCN-007 | 步驟 8 的容器重啟驗證 | 步驟 6 | |
| SCN-008 | 步驟 7 CLI 測試 | 同層合併 | 以記錄刪除呼叫的 backend 斷言 |
| SCN-009 | 步驟 4 | 同層合併 | |

斷言一律指名被刪與被留的 id，不只比對數量。驗證命令沿用 `AGENTS.md` 的 `cargo test -p <crate>` 與 `cargo clippy --all-targets -- -D warnings`。

## 風險與首要驗證

見 [README.md](./README.md#風險與首要驗證)。

## 檢查清單

- [x] 步驟 1 的量測已回寫，TBD-1、TBD-2 已定案
- [ ] 每個 Scenario 有對應的外迴圈證據
- [ ] `cargo test` 與 `cargo clippy --all-targets -- -D warnings` 全綠
- [ ] compose 兩份檔案、`.env.example` 與 `docs/docker.md` 的新變數一致
