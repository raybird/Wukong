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
3. 是根 session。刪除根 session 會連整棵樹一起帶走（步驟 1 已確認），所以判定單位是樹：樹裡任何一個 session 被 scope 指向就整棵保留；樹的最後活動取所有成員中最新的。為此列表不帶 `roots=true`，子 session 也要取回。

列表一律明確帶 `limit`，並以回傳筆數是否等於 `limit` 判斷是否被截斷。opencode 沒有「早於某時間」的過濾，所以整批取回再挑。

> **2026-10-01 修訂**：原設計只取根 session、子 session 一律略過。審查提出 CLI backend 記下的可能是子 session 的 id，其根 session 過期被刪時會連帶消失，違反 SCN-002，因此改為以樹判定。

### 失效方向

任何一步不確定就不刪：

- 讀不到指向表、列不出 session、回應解析失敗 → 整輪跳過。列表可能因單一異常資料列而每一輪都失敗（步驟 1 的額外發現），所以失敗紀錄要帶上游回應，讓人看得出是同一個原因在重複。
- 保留期下限 1 天。進行中的回合所建立、尚未寫回 `agent_sessions` 的 session 必然是新的，下限確保它們不可能成為候選。
- 單次刪除失敗只記錄，下一輪自然會再遇到它。
- 記憶庫指向的 session 沒有任何一個出現在 server 的清單裡 → 視為記憶庫接錯，整輪不挑也不刪（SCN-010）。記憶庫與 server 都是空的（全新部署）不算對不上。
- 保留天數寫了卻無法解析（`off`、`false`、`-1`）→ 視為停用並輸出警告，不退回預設的 30 天。

### 執行位置

掛在 `wukong-schedulerd` 既有的維護迴圈旁（`crates/wukong-schedulerd/src/main.rs:106-127`），獨立的 interval。它已同時持有 memory 與 backend，且是 compose 內常駐的單一實例。

只在 Server backend 生效。容器部署的 `opencode-state` volume 專屬於 Wukong；CLI 模式的 `opencode.db` 與使用者自己的 opencode 使用共用，Wukong 無從分辨哪些是自己建的，因此不自動清掃。

啟動 6 小時後第一輪，之後每 6 小時一輪；每輪最多刪 500 個、最長 5 分鐘，由舊到新刪。第一輪不在啟動當下，升級後才有時間先預覽。整輪設時間上限，是因為列表與刪除沿用 agent 的 20 分鐘逾時，而清理是在排程迴圈裡同步執行的。

這項清理假設 server 上的 session 都屬於這一套 Wukong，只在 compose 的專屬 volume 成立。SCN-010 的防護擋住「記憶庫接錯」，擋不住「Wukong 與使用者共用同一個 `opencode serve`」（README 的 TBD-5）。

### 空間回收

在 `opencode-server` 容器啟動、`exec opencode serve` 之前執行（`scripts/docker-entrypoint.sh:271-292`）。此時 server 尚未開啟資料庫，而既有的離峰閒置重啟讓這個時點大約每天出現一次。

- freelist 佔比達 25% 才做。
- 剩餘磁碟空間不小於資料庫大小的 2 倍才做。
- 設 busy timeout；被鎖或任何失敗都只記錄，不阻擋 server 啟動。
- entrypoint 設 `SQLITE_TMPDIR` 為資料庫所在目錄：`VACUUM` 的暫存複本預設寫在 `/var/tmp`，那是容器的根檔案系統，不在磁碟檢查的範圍內。
- 由 `wukong` 的子命令執行，才能用 `cargo test` 覆蓋。

即使從不 `VACUUM`，釋出的頁也會被 SQLite 重用，檔案會停止成長；`VACUUM` 只負責讓檔案變小。

### 設定

| 變數 | 用途 | 預設 |
|---|---|---|
| `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` | 保留天數；`0` 停用清理 | `30` |

清理間隔、每輪上限與回收門檻是程式內的固定值（見步驟 1 的定案），沒有人要求調整它們，不另開環境變數。（步驟 11、12：為了讓測試看得到真正的 daemon 跑完一輪，debug 建置認得一個縮短間隔的測試鉤子；release 建置不編譯它。）

### 可觀測性

每輪輸出一行：列出數、是否被截斷、記憶庫是否對得上（`anchored`）、受保護數、過期候選數、已刪數、失敗數，以及受保護但已超過保留期的數量。最後一項是被棄置 scope 的規模訊號。

## 使用方式對照

| 情境 | 變更前 | 變更後 |
|---|---|---|
| 無主的舊 session | 永久留存 | 超過保留期後由 schedulerd 刪除 |
| 想知道會刪哪些 | 無 | 執行預覽，列出待刪與受保護清單 |
| `wukong --new`（server backend） | 舊 session 留在 opencode | 舊 session 被刪除 |
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
2. ✅ **挑選邏輯**（SCN-001、SCN-002、SCN-003；相依：步驟 1 確認子 session 行為）
   - 產出：純函式，輸入 session 列表、受保護 id 集合、現在時間與保留期，輸出待刪清單與統計。
   - 完成判準：測試斷言被選中與被保留的具體 id；涵蓋保留期邊界、受保護的過期 session、保留期為 0、低於下限的設定。
   - 完成證據（2026-10-01）：`cargo test -p wukong-runtime --lib session_retention`。紅燈：挑選函式為空實作時 `selects_only_expired_sessions_no_scope_points_at` 與 `a_run_is_capped_and_takes_the_oldest_first` 失敗（期望 `["old_orphan", "just_past_cutoff"]`，得到 `[]`）。綠燈：實作後 3 項全過。`zero_retention_days_selects_nothing` 在空實作下也會過，另以變異確認它抓得到錯：拿掉天數為 0 的兩處判斷後，它與 `disabled_retention_deletes_nothing` 轉紅。保留天數是整數、0 代表停用，所以不存在「低於 1 天」的設定，下限由型別保證。單迴圈：挑選是純函式，沒有另一層整合面。
3. ✅ **列表能力與受保護集合**（SCN-004、SCN-005；無相依）
   - 產出：`AiBackend` 列出 session 的能力（Server backend 實作）；`wukong-memory` 列出所有被 scope 指向的 session id。
   - 完成判準：對腳本化 HTTP server 的測試斷言請求帶有 `limit` 與 `roots`、回傳筆數等於 `limit` 時標示截斷、非預期回應回傳錯誤而非空清單；memory 測試斷言兩張表的 id 都被納入。
   - 完成證據（2026-10-01）：`cargo test -p wukong-gateway --lib list_sessions`、`cargo test -p wukong-memory --lib referenced_session_ids`。紅燈：server backend 尚未覆寫時，三項 `list_sessions_*` 失敗——腳本化 server 收到的請求是 `[]`，期望 `["GET /session?roots=true&limit=10000"]`；memory 的空實作回傳 `[]`，期望三個 id。綠燈：實作後 gateway 4 項、memory 1 項全過（截斷判定的 `a_full_page_is_reported_as_truncated` 不符合這個篩選字串，在全量測試中執行）。`AiBackend` 以帶預設實作的方法擴充（GitNexus 對該 trait 回報 HIGH、27 個實作者，已事先告知）；既有實作皆未修改，`cargo test` 全 workspace 通過。
4. ✅ **清理執行與預覽**（SCN-001、SCN-002、SCN-004、SCN-006、SCN-009；相依：步驟 2、3）
   - 產出：`wukong-runtime` 的清理函式與報告；`wukong` 的預覽／手動清理子命令。
   - 完成判準：測試斷言實際送出 `DELETE` 的 id 集合；指向表或列表失敗時送出 0 個 `DELETE`；單一刪除失敗後其餘照常；預覽送出 0 個 `DELETE`，且其清單等於同一 fixture 實際執行所刪的集合。
   - 完成證據（2026-10-01）：`cargo test -p wukong-runtime --lib session_retention`。紅燈：清理函式為空實作時 6 項失敗（刪除、預覽、列表失敗、指向表讀不到、單一刪除失敗、截斷標示）。綠燈：實作後 13 項全過。`an_unreadable_scope_table_deletes_nothing` 以另一條連線 `DROP TABLE agent_session_state` 製造真實的讀取失敗，斷言回傳 `WukongError::Memory` 且刪除呼叫為空。預設天數另以變異確認（30 改 31 後 `policy_defaults_to_thirty_days_and_zero_disables` 轉紅）。子命令 `wukong opencode prune [--dry-run]` 的解析由 `parses_opencode_ops` 覆蓋，實際執行見步驟 8。
5. ✅ **掛進 schedulerd 與設定**（SCN-003、SCN-006；相依：步驟 4）
   - 產出：schedulerd 的定期清理、環境變數解析、compose 與 `.env.example` 的對應項。
   - 完成判準：設定解析的測試涵蓋預設、停用與無效值；清理回傳錯誤時 schedulerd 迴圈繼續；compose 兩份檔案都傳入新變數。
   - 完成證據（2026-10-01）：`cargo test -p wukong-schedulerd session_retention`。紅燈：`active_policy` 為空實作時，停用與 CLI backend 兩種情況都回傳 `Some`；`run_once` 為空實作時對連不上的 server 回傳 `Ok`。綠燈：實作後 2 項全過。迴圈以與 memory maintenance 相同的方式記錄錯誤後繼續。compose 一致性由 `scripts/test-docker-runtime.sh` 檢查，並確認它抓得到缺項：從 release compose 拿掉該行後腳本回報 `missing pattern`。設定只有 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` 一項（步驟 11 加入的測試鉤子不是設定，見步驟 12）。`--once` 模式不跑清理。
6. ✅ **啟動前空間回收**（SCN-007；相依：步驟 1 定出門檻）
   - 產出：`wukong` 的空間回收子命令；entrypoint 在 `opencode serve` 前呼叫。
   - 完成判準：測試在暫存 SQLite 檔上斷言超過門檻時檔案變小、未達門檻時不動、檔案被鎖時回傳錯誤；entrypoint 在回收失敗時仍 `exec` server。
   - 完成證據（2026-10-01）：`cargo test -p wukong-cli --lib opencode_db`。紅燈：回收函式為空實作時 4 項失敗（回收、未達門檻、磁碟不足、被鎖）。綠燈：實作後 6 項全過。以實際 binary 執行四種情況：檔案不存在回 `outcome=missing` 且不建立任何檔案；9,224,192 → 8,192 bytes 回 `outcome=reclaimed`；再跑一次回 `below_threshold`；另一連線持有寫鎖時回 `database is locked`、exit 1、檔案不變。子命令在開記憶庫之前處理，容器內 `/data` 沒有多出檔案。entrypoint 的真實容器驗證見步驟 8。**設計追加**：entrypoint 先以 `wukong opencode --help` 確認 binary 認得子命令才呼叫——舊版 binary 不會拒絕 `opencode vacuum`，而是把它當成 prompt 跑一個回合；以新 entrypoint 配 v0.21.11 的舊 binary 啟動容器實測：沒有任何 vacuum 紀錄、session 數不變、server 正常啟動。
7. ✅ **修正 `--new` 漏刪**（SCN-008；無相依）
   - 產出：`main.rs` 的 `--new` 與 `/new` 行為一致。
   - 完成判準：先有重現漏刪的失敗測試，修正後斷言舊 session id 被刪且 mapping 被清。
   - 完成證據（2026-10-01）：`cargo test -p wukong-cli --test new_session_flag`，執行真正的 binary 對一個記錄請求的假 server。紅燈：修正前 server 只收到 `["GET /global/health"]`，沒有任何 `DELETE`。綠燈：修正後收到 `DELETE /session/ses_old`，且該 scope 不再指向任何 session。單迴圈：漏掉的是一個對外請求，binary 層級的測試就是最外層。
8. ✅ **端到端驗證與文件**（SCN-001、SCN-002、SCN-007；相依：步驟 4、5、6）
   - 產出：在資料庫複本上以真實 opencode 跑完整清理與重啟回收的紀錄；`docs/docker.md`、`CHANGELOG.md`、`AGENTS.md` 的「關鍵設計細節」；08-08 文件驗證清單中已回答項目的勾選。
   - 完成判準：複本上過期無主 session 的 `message`／`part`／`event` 列數歸零、受保護 session 不變且可續接、重啟後檔案變小；文件中的環境變數表與程式的預設值一致。
   - 完成證據（2026-10-01）：以 `ghcr.io/raybird/wukong:v0.21.11`（opencode 1.18.29）的拋棄式容器，對本機測試部署 `opencode.db` 的複本執行；記憶庫是全新的，只寫入該部署實際指向的兩個 session id。複本、容器與為此拉取的編譯映像已於驗證後移除。
     - 佈置：9 個舊 session（22–23 天前）之外另建 150 個新的，使舊 session 排在列表第 151 筆之後；此時不帶參數的列表回 100 筆、其中 0 個是舊 session。
     - 預覽（SCN-005、SCN-009）：`WUKONG_OPENCODE_SESSION_RETENTION_DAYS=7 wukong opencode prune --dry-run` 列出 7 個將刪除、2 個受保護、共列出 159 個；執行前後四張表的列數相同（session 159、message 24、part 60、event 340）。
     - 設定（SCN-003）：預設 30 天時預覽為將刪除 0 個；設 0 時回報已停用且列數不變。
     - 清理（SCN-001、SCN-002）：以 `wukong-schedulerd` 常駐模式、保留期 7 天執行，日誌為 `opencode_session_retention dry_run=false listed=159 truncated=false protected=2 stale_protected=2 expired=7 deferred=0 deleted=7 failed=0`。7 個無主 session 的 session／message／part／event 由 7／19／46／144 全部歸零，正是預覽列出的那 7 個；2 個受保護 session 維持 2／5／14／46，`GET /session/{id}` 回 200、訊息數 3 與 2 不變，記憶庫的 scope 對應不變；150 個新 session 不變；被刪的 session `GET` 回 404。再跑一次：`expired=0 deleted=0`。
     - 重啟回收（SCN-007）：以真正的 entrypoint 啟動容器（掛入本分支的 entrypoint 與在 Debian bookworm 容器內編譯的 `wukong`）。資料庫被另一連線寫鎖時：日誌出現 `outcome=failed error=... database is locked` 與 `WARNING: opencode.db vacuum failed; starting the server anyway.`，server 隨後健康、檔案維持 606,208 bytes。正常重啟：`outcome=reclaimed before_bytes=606208 after_bytes=438272`，freelist 40／148 → 0／107，server 回報 152 個 session、受保護 session 的訊息數不變。第三次啟動：`outcome=skipped reason=below_threshold`。
     - 限制：SCN-002 的「下一回合仍續接」沒有以真實的模型回合驗證——拋棄式容器沒有 provider 認證。證據是 session 與其訊息完好、scope 對應未變。容器驗證用的是精煉前編譯的 binary；精煉只調整了回收模組內部的函式切分，之後以主機 binary 重跑四種情況、結果相同。
     - 全量檢查：`cargo test` 39 個測試套件、587 項通過、0 失敗；`cargo clippy --all-targets -- -D warnings` 無警告；`cargo fmt --all -- --check` 通過；`scripts/test-docker-runtime.sh` 通過。
     - 文件：`docs/docker.md`、`docs/cli-reference.md`、`CHANGELOG.md`、`.env.example`、`AGENTS.md` 已更新；08-08 交接文件在「尚未證實」的對應項下加註。該文件驗證清單的兩個相關勾選項都沒有勾：一項同時要求備份、另一項要求在那台主機上重新量測，本次都沒做。
     - 額外發現：`wukong --db X memory snapshot` 這類「全域旗標在子命令之前」的寫法不會報錯，而是把子命令當成 prompt 跑一個真的回合。驗證過程中我自己踩到一次，已記入 `AGENTS.md` 並以 `a_global_flag_before_a_subcommand_turns_it_into_a_prompt` 釘住。
9. ✅ **審查退回的修正（第一輪）**（SCN-002、SCN-003、SCN-006、SCN-007、SCN-008、SCN-010；相依：步驟 1 至 8）
   - 產出：對 [review-6037672.md](./review-6037672.md) 各項發現的修正與證據。
   - 完成判準：兩項 MUST FIX 各有先紅後綠的測試；採納的 SHOULD FIX 各有測試或實測；未採納的項目有理由；全量檢查通過。
   - 完成證據（2026-10-01）：
     - **M-1（`--new` 刪除失敗時帶著舊 context 繼續）**：`cargo test -p wukong-cli --test new_session_flag`。紅燈：假 server 對 `DELETE` 回 500 時，scope 事後仍指向 `ses_old`（期望 `None`）。綠燈：刪除失敗只記警告，對應照樣清除；2 項全過。
     - **M-2（未防護的前提）**：依使用者核准新增 SCN-010。`cargo test -p wukong-runtime --lib session_retention`，紅燈：`a_memory_that_matches_nothing_on_the_server_deletes_nothing` 在防護未實作時失敗；綠燈後 16 項全過。真實 opencode 複本上：空的記憶庫與指向別處 session 的記憶庫，實際執行 `wukong opencode prune` 都回報未清理、結束碼 1，四張表的列數不變（session 11、message 24、part 60、event 192）；補上該部署真正的兩筆對應後，預覽列出 7 個、實際刪除後受保護的 2 個 session 維持 2／5／14／46。`prune` 輸出第一行顯示所用的記憶庫。共用 server 的殘餘風險記為 README 的 TBD-5，並寫進 `docs/docker.md` 與 `CHANGELOG.md`。
     - **S-1（無法解析的天數被當成 30 天）**：改為停用並警告。紅燈：`policy_defaults_to_thirty_days_and_zero_disables` 對 `off`、`false`、`-5` 等值失敗；綠燈後通過。實測 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS=off` 時輸出警告與「已停用」。
     - **S-2（啟動當下就清理）**：第一輪改在啟動後一個完整間隔。實測：修正前 schedulerd 啟動 12 秒內即出現 `opencode_session_retention` 一行（步驟 8）；修正後同樣 12 秒內為 0 行，啟動日誌為 `opencode session retention enabled retention_days=7 first_run_in_secs=21600 interval_secs=21600`。迴圈呼叫清理的接線在步驟 8 已觀察過，這次只改了第一個 tick 的時間點。
     - **S-3（server 掛住時拖住排程迴圈）**：整輪加 5 分鐘上限。紅燈：對一個接受連線但永不回應的 server，`a_server_that_never_answers_gives_the_loop_back` 在 120 秒內沒有返回；綠燈：以 300 ms 上限執行時 0.31 秒內回傳 `timed out`。沒有另外加「連續失敗就中止」，整輪上限已涵蓋。
     - **S-4（`VACUUM` 暫存檔寫在容器根檔案系統）**：entrypoint 設 `SQLITE_TMPDIR`。以 `strace` 確認：未設時暫存檔開在 `/var/tmp/etilqs_*`，設定後開在資料庫所在目錄。
     - **子 session 被指向的情況**（審查列為未查證）：改以樹判定。紅燈：`a_scope_pointing_at_a_child_protects_the_whole_tree` 與 `a_root_is_only_as_old_as_its_most_recent_descendant` 失敗；綠燈後通過。列表改為不帶 `roots=true`，`list_sessions_asks_for_every_session_with_an_explicit_limit` 先紅（收到的仍是帶 `roots=true` 的請求）後綠。
     - **N-3**：上方步驟 3 的測試數與測試策略表的 SCN-008 已更正。**N-4**：`prune` 在拒絕清理或有刪除失敗時以 1 結束；拒絕清理（1）、成功（0）、停用（0）已實測，「有刪除失敗」這一條沒有實際執行到。**N-2**：runtime 測試補上清理後 scope 對應不變的斷言；`test-docker-runtime.sh` 的字串檢查維持原樣，行為由下方的真實容器驗證承擔。
     - **未採納**：N-1（時鐘往前跳的健全性檢查）——沒有實際的觸發情境，損害上限是尚未寫回對應的進行中 session。N-5（三份手寫 HTTP stub）——審查判定可接受。
     - **真實容器重驗**：以最終版 entrypoint 與在 Debian bookworm 容器內重新編譯的最終版 `wukong`，掛進 v0.21.11 映像啟動。資料庫被寫鎖時：`outcome=failed ... database is locked`、警告後 server 健康、檔案維持 454,656 bytes。正常啟動：`outcome=reclaimed before_bytes=454656 after_bytes=294912`，freelist 39／111 → 0／72，`integrity_check` 為 ok；資料庫目錄與容器的 `/var/tmp` 都沒有殘留暫存檔。步驟 8 記錄的「容器驗證用的是精煉前的 binary」這項限制因此解除。
     - **仍然成立的限制**：SCN-002 的續接沒有以真實模型回合驗證；樹狀保護沒有在真實 opencode 的舊資料上演練（樣本裡沒有夠舊的子 session），由單元測試承擔；容器用的是 gnu binary 而非 release 的 musl binary。
     - **全量檢查**：`cargo test` 39 個套件、592 項通過、0 失敗；`cargo clippy --all-targets -- -D warnings` 無警告；`cargo fmt --all -- --check` 通過；`scripts/test-docker-runtime.sh` 通過。
10. ✅ **審查退回的修正（第二輪）**（SCN-008、SCN-010；相依：步驟 9）
   - 產出：對 [review-46cd815.md](./review-46cd815.md) 各項發現的修正與證據。
   - 完成判準：同步驟 9。
   - 完成證據（2026-10-01）：
     - **M-1（SCN-010 的說明在 schedulerd 路徑沒有實作）**：schedulerd 在記憶庫對不上時，於摘要行之後另寫一行 `warning:`，內容含原因與所用的記憶庫。`cargo test -p wukong-schedulerd session_retention`，紅燈：日誌行尚未實作時 `a_memory_that_does_not_match_the_server_is_explained_in_the_log` 與 `a_normal_run_logs_its_summary_and_each_failed_delete` 失敗；綠燈後 6 項全過。日誌只由這個函式產生，沒有第二條輸出路徑。`anchored` 欄位的意義已寫進 `docs/docker.md`。這條路徑沒有以真正的 schedulerd 觀察（要等一個 6 小時的間隔）；審查者在第二輪以 3 秒間隔的複本確認過迴圈會呼叫清理，那是修正前的版本。
     - **M-2（兩份記憶庫共用一個 server）**：不加新的防護（那會改變 SCN-010，需使用者決定），更正說法並揭露。`docs/docker.md` 原本把「主機上的 `wukong` 指向容器的 server」寫成有防護的例子，與審查者的實測相反，已改為獨立一類並寫明防護擋不住、處置是保留天數設 0；`.env.example`、`CHANGELOG.md`、`AGENTS.md` 與 README 的 TBD-5 同步。這一項沒有測試：它是已知不防護的情況。
     - **S-1（`--new` 在 CLI backend 仍不刪）**：屬實，CLI backend 的 `delete_session` 是空實作。`CHANGELOG.md` 改為註明 server backend，並列入已知限制；README 記為 TBD-6，待使用者決定是否另案實作。
     - **S-2（四個行為沒有測試會紅燈）**：各補一個測試，並以變異確認會轉紅。延後首輪：`the_first_run_waits_a_full_interval`（暫停時鐘），把 `ticker()` 改回立即觸發後失敗。`prune` 的第一行與結束碼：新增 `crates/wukong-cli/tests/opencode_prune.rs`，執行真正的 binary 對假 server，涵蓋成功（0）、有刪除失敗（1）、記憶庫對不上（1）、空 server（0）；拿掉記憶庫那一行並讓結束碼恆為 0 後，4 項中 3 項失敗。父不在清單的子 session：`a_child_whose_parent_is_not_listed_is_never_deleted_on_its_own`，把這類子 session 當成根的變異下失敗（期望 `["old_orphan"]`，得到 `["old_orphan", "stray"]`）。另補成環的 fixture。這些測試除空 server 一項外都是先綠的——行為在上一輪就已存在，所以以變異取代紅燈。
     - **S-3（截斷時被截掉的受保護子 session）**：不修，記為 README 的 TBD-7。審查者建議的最小修法（截斷且有受保護 id 不在清單時整輪不刪）會讓超過 10,000 個 session、又有任何一筆失效對應的部署永遠不清理，而那正是最需要清理的情況。
     - **N-1**：寫進 `docs/docker.md` 與 `CHANGELOG.md`，README 記為 TBD-8。**N-2**：server 上沒有任何 session 時不再視為對不上；紅燈 `an_empty_server_is_not_a_mismatch`，綠燈後通過，binary 測試確認結束碼為 0。**N-3**：entrypoint 的 `SQLITE_TMPDIR` 改取資料庫所在目錄；以替身指令執行 entrypoint 片段：預設為 `/home/wukong/.local/share/opencode`，`WUKONG_OPENCODE_DB=/mnt/other/opencode.db` 時為 `/mnt/other`，回收失敗時印警告後繼續。這個改動沒有在真實容器重跑。**N-4**、**N-6**：文字已更正。**N-5**：不動，審查判定可接受。
     - **審查者未查證的兩項**：GitNexus 影響分析——`AiBackend`（HIGH，已事先告知）、兩個 `main` 與 `Command`（LOW）在動手前跑過；這次 PR 新增的符號不在索引內（索引停在 2026-08-06）。每次提交前都跑了 `detect_changes`。server 的 project id 改變後舊 session 是否還會被列出，我也沒有查證。
     - **全量檢查**：`cargo test` 40 個套件、602 項通過、0 失敗；`cargo clippy --all-targets -- -D warnings` 無警告；`cargo fmt --all -- --check` 通過；`scripts/test-docker-runtime.sh` 通過。
11. ✅ **第三輪審查後的修正**（SCN-001、SCN-003、SCN-008、SCN-010；相依：步驟 10）
   - 產出：對 [review-a09b2ad.md](./review-a09b2ad.md)（判定 PASS）所列建議的處理。使用者於 2026-10-01 逐題決定：修正 S-1、把 SCN-008 限定為 server backend（S-3）、為 schedulerd 補 binary 層級測試（S-2）、處理 N-1、N-3 與文件類的 N-2、N-4。
   - 完成判準：採納的每一項有先紅後綠的測試或實測；全量檢查通過。
   - 完成證據（2026-10-01）：
     - **S-1（空清單一律算對得上）**：條件收窄為「清單為空且記憶庫也沒有任何指向」才算沒東西可清。紅燈：`a_memory_with_pointers_and_an_empty_server_is_a_mismatch`（runtime）與 `a_memory_with_pointers_against_an_empty_server_refuses`（執行真正的 `wukong`）在收窄前失敗；綠燈後 runtime 20 項、`tests/opencode_prune.rs` 5 項全過。兩邊都是空的情況維持結束碼 0。欄位註解與三處文件同步。
     - **S-3（SCN-008 與 CLI backend）**：規格修訂於 `06b25d3`，只在 Given 加上 server backend 的限定；TBD-6 結案。程式不變。
     - **S-2（daemon 的執行路徑沒有測試會紅燈）**：新增 `crates/wukong-schedulerd/tests/retention_daemon.rs`，執行真正的 `wukong-schedulerd` 對假 server。為此加了一個只給這個測試用、不寫進文件的環境變數 `WUKONG_TEST_OPENCODE_RETENTION_INTERVAL_SECS` 來縮短間隔。紅燈：變數尚未實作時，daemon 30 秒內沒有跑清理，兩項都失敗，日誌停在 `first_run_in_secs=21600`。綠燈：實作後兩項 1.07 秒通過——一項斷言 daemon 送出 `DELETE /session/ses_orphan`、沒有送出 `ses_kept`，日誌含 `anchored=true` 與 `deleted=1`；另一項斷言記憶庫對不上時日誌含警告與該記憶庫的位置、沒有任何 `DELETE`。審查者列的三個變異都會轉紅：不寫出日誌行（兩項失敗）、`main` 傳空字串當記憶庫位置（對不上那一項失敗）、迴圈永不呼叫清理（兩項失敗）。
     - **N-1（`.env` 留空得到 30 天）**：compose 兩份檔案由 `${VAR:-30}` 改為 `${VAR-30}`。以 `docker compose config` 確認：沒有這個變數是 `"30"`、留空是 `""`、設 7 是 `"7"`；以真正的 `wukong` 確認空值時輸出警告與「已停用」。`scripts/test-docker-runtime.sh` 的比對字串同步。
     - **N-3（錯誤訊息帶出整筆 session）**：紅燈 `a_malformed_listing_is_reported_without_dumping_its_contents`——原訊息含 session 標題，且非陣列的回應整份寫出；綠燈後只留 session id，非陣列的回應截到 200 個字元。gateway 125 項全過。
     - **N-2、N-4（文件）**：截斷時的邊界補進 `docs/docker.md` 與 `CHANGELOG.md` 的已知限制；`CHANGELOG.md` 已知限制清單中多餘的空行已移除；README 的涉及檔案補上 schedulerd 測試用的 `tokio` `test-util`；本檔「使用方式對照」的 `wukong --new` 註明 server backend。交付範圍的提交清單先前漏列 `18cdda8`（只動 issue 文件）。
     - **未處理**：N-5（`scripts/test-docker-runtime.sh` 對 entrypoint 仍只比對字串）——行為由步驟 8、9 的真實容器驗證與審查者的替身指令執行承擔。
     - **審查者留下的未查證項**：opencode 是否接受 `limit=10000`——在真實 opencode 1.18.29 的拋棄式容器上建立 1,500 個 session：不帶參數回 100 筆、`limit=1000` 回 1,000 筆、`limit=10000` 回 1,500 筆，沒有發現 server 另設上限；沒有測到 10,000 筆。server 的 project 改變後舊 session 是否還會被列出，仍未查證；S-1 修正後，這種情況若導致清單為空，會被回報為對不上。
     - **全量檢查**：`cargo test` 41 個套件、607 項通過、0 失敗；`cargo clippy --all-targets -- -D warnings` 無警告；`cargo fmt --all -- --check` 通過；`scripts/test-docker-runtime.sh` 通過。
     - **限制**：這一輪的程式改動（不含上面那次 `limit` 的量測）沒有在真實 opencode 或真實容器上重跑；改到的是空清單的判定、錯誤訊息文字、compose 的預設值寫法與測試用的間隔，步驟 8、9 的真實環境證據所涵蓋的路徑沒有變。
12. ✅ **第四輪審查後的修正**（SCN-003；相依：步驟 11）
   - 產出：對 [review-8020271.md](./review-8020271.md)（判定 PASS）S-1 的修正。使用者於 2026-10-02 決定現在修。
   - 完成判準：測試鉤子不存在於 release 建置；debug 建置下它只能縮短間隔、任何值都不會讓 daemon 啟動失敗；有測試釘住；相矛盾的文字已更正。
   - 完成證據（2026-10-02）：
     - **S-1（測試用的間隔會被正式 binary 讀取，極大值讓 daemon 啟動即 panic）**：讀取環境變數的那一段改以 `#[cfg(debug_assertions)]` 編譯，並只接受 1 到 21,599 之間的整數。`cargo test -p wukong-schedulerd`，紅燈：解析函式為空實作時 `the_test_hook_only_ever_shortens_the_interval` 失敗（期望 `Some(1s)`，得到 `None`）；綠燈後 22 項全過，daemon 測試 2 項 1.07 秒通過。
     - **問產物**：分別編譯 release 與 debug 的 `wukong-schedulerd` 後直接執行。release：變數設為 `1`、`0`、`9223372036854775807`、`18446744073709551615`，啟動日誌一律是 `first_run_in_secs=21600 interval_secs=21600`，沒有 panic；`strings` 在 release binary 裡找不到這個變數名（debug binary 有 1 處）。debug：`1` 得到 `first_run_in_secs=1`；`21600`、`0` 與兩個極大值都是 21600，沒有 panic。
     - **`cargo test --release -p wukong-schedulerd`**：單元測試 21 項通過（鉤子的那一項隨鉤子一起不編譯），`tests/retention_daemon.rs` 以 `#![cfg(debug_assertions)]` 整檔略過、0 項。
     - **文字**：`INTERVAL` 的註解、README 的 TBD-1、本檔「設定」一節與步驟 5 都補上這個例外。另更正 review-8020271 N-4 指出的紀錄：SCN-008 的狀態改回 `已核准`；Timeline 的「三項小建議」改為與步驟 11 一致；步驟 11 的限制補上與 `limit` 量測的區別；TBD-6 的理由補上 compose `cli` profile 的情況。
     - **未處理**（review-8020271 的 NICE TO HAVE，使用者沒有要求）：N-1 保留天數只傳進 schedulerd 容器，在其他容器執行 `prune` 會用 30 天——文件的範例已指定 schedulerd 容器；N-2 回應不是合法 JSON 時既有的共用函式仍寫出整個 body，以及兩句錯誤訊息的措辭；N-3 daemon 測試在測試行程被訊號終止時會留下子行程、沿用呼叫端的 proxy 設定、不釘住第一輪的時間點。
     - **全量檢查**：`cargo test` 41 個套件、608 項通過、0 失敗；`cargo clippy --all-targets -- -D warnings` 無警告；`cargo fmt --all -- --check` 通過；`scripts/test-docker-runtime.sh` 通過。

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
| SCN-008 | 步驟 7、9 的 binary 層級測試 | 同層合併 | 執行真正的 `wukong`，對記錄請求的假 server 斷言。只適用 server backend |
| SCN-009 | 步驟 4 | 同層合併 | |
| SCN-010 | 步驟 9 的真實 opencode 驗證；步驟 10、11 的 `prune` 與 schedulerd binary 測試 | 步驟 9、11 的 runtime 測試；步驟 10 的 schedulerd 日誌測試 | `prune` 與 schedulerd 兩條路徑各有執行真正 binary 的證據 |

斷言一律指名被刪與被留的 id，不只比對數量。驗證命令沿用 `AGENTS.md` 的 `cargo test -p <crate>` 與 `cargo clippy --all-targets -- -D warnings`。

## 風險與首要驗證

見 [README.md](./README.md#風險與首要驗證)。

## 檢查清單

- [x] 步驟 1 的量測已回寫，TBD-1、TBD-2 已定案
- [x] 每個 Scenario 有對應的外迴圈證據（SCN-002 的續接有上述限制）
- [x] `cargo test` 與 `cargo clippy --all-targets -- -D warnings` 全綠
- [x] compose 兩份檔案、`.env.example` 與 `docs/docker.md` 的新變數一致
