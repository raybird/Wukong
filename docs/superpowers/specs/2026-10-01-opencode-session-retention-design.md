# OpenCode Session 保留期清理設計

日期：2026-10-01

狀態：待核准（尚未實作）

關聯文件：`docs/2026-08-08-system-freeze-opencode-resource-handover.md`（P2「建立 OpenCode
session retention 與 DB 維護流程」，至今未實作）

對照案例：<https://github.com/raybird/telenexus/issues/9>

基準版本：`ghcr.io/raybird/wukong:v0.21.11`（內含 opencode 1.18.29）

## 背景

opencode 把每個 session 的訊息、片段與事件歷史存進 `opencode.db`。Wukong 只在三個時點
刪除 session（輔助棒跑完、session 輪替、使用者下 `/new`），沒有任何依年齡的清理，所以
凡是漏出這三個時點的 session 都永久留存。08-08 事故當時部署主機上的資料庫為 1.33 GiB，
`event` 表 668 MiB、`part` 表 456 MiB。

Wukong 自己另存對話（`wukong-memory` 與 `wukong-chat-history`），不讀取舊的 opencode
session；唯一會被續接的是 `agent_sessions` 表內每個 scope 目前指向的那一個。

## 目標

- 定期刪除超過保留期、且沒有任何 scope 指向的 opencode session。
- 保留期可設定、可停用，預設 30 天。
- 刪除後在 server 未運行時回收檔案空間。
- 補上已確認的漏刪路徑（`--new` 旗標）。
- 首次套用到既有部署前，能先預覽會刪哪些 session。

## 非目標

- **不輪替長壽的 scope session。** 這會改變對話延續行為，另案評估。見「最大未知」：
  這可能才是 Wukong 資料量的主體。
- 不直接以 SQL 改寫 opencode 的資料表。刪除一律走 opencode 自己的刪除入口；`VACUUM`
  只重整檔案、不改資料列。
- 不在 CLI backend 自動清掃。CLI 模式的 `opencode.db` 是使用者本機的那一份，與使用者
  自己的 opencode 使用共用（本開發機該檔 831 MB、962 個 session，絕大多數與 Wukong
  無關），Wukong 無從分辨哪些是自己建的。
- 不改 `opencode.json` 的 compaction 設定；它縮的是 context，不是歷史。

## 現況查核

### 對照原始碼

| 事實 | 依據 |
|---|---|
| Server backend 建立的 session 標題固定為 `Wukong` | `crates/wukong-gateway/src/opencode_server.rs:129-143` |
| 輔助棒與 helper 呼叫跑完即刪 | `opencode_server.rs:720-765`；`crates/wukong-runtime/src/turn.rs:242-257` |
| 輪替後刪除舊 session | `turn.rs:377-390` |
| REPL `/new` 先刪 session 再清 mapping | `crates/wukong-cli/src/command.rs:46-52` |
| **`--new` 旗標只清 mapping、不刪 session** | `crates/wukong-cli/src/main.rs:35-39` |
| **CLI backend 沒有實作 `delete_session`**，沿用 trait 的空實作 | `crates/wukong-gateway/src/backend.rs:64-66`、`401` 起的 `impl` 內無此方法 |
| CLI backend 非串流路徑回傳 `session_id: None`，呼叫端拿不到 id 可刪 | `backend.rs:456` |
| 末棒 session 建立後、寫回 `agent_sessions` 之前回合失敗，該 session 即成無主 | `turn.rs:358-376`（任一 `?` 提前返回時 session 已存在） |
| `opencode-server` 的 healthcheck 打 `/global/health`，不建立 session | `docker-compose.yml:97` |
| repo 內沒有任何依年齡刪除 session 的程式 | `grep -rn "session.*delete\|retention"` 僅命中上列即時刪除 |

### 對照產物（2026-10-01 實測）

以下皆在 `opencode.db` 的**複本**上、以拋棄式容器執行，未碰任何運行中的資料。

| 問題 | 結果 |
|---|---|
| `opencode session delete <id>` 是否連帶清除歷史 | 是。刪 1 個 session 後 `message` 24→15、`part` 60→36、`event` 190→121、`event_sequence` 9→8，該 id 在 `event` 的殘留為 0 |
| HTTP `DELETE /session/{id}`（Wukong 實際使用的入口）是否等價 | 是，四張表的差異與 CLI 完全相同；對已刪除的 id 再打一次回 404 |
| `event` 是否靠外鍵串聯 | 否。`event` 的外鍵指向 `event_sequence`，不指向 `session`；歷史能被清掉是 opencode 刪除邏輯的行為，不是 schema 保證。**升級 opencode 後需重驗** |
| 刪除後檔案會不會變小 | 不會。空間進 freelist（111 頁中 16 頁），`auto_vacuum=0` |
| `GET /session` 不帶參數回幾筆 | **最多 100 筆**。建立 150 個 session 後不帶參數回 100、`?limit=100000` 回 158 |
| server 運行（閒置）時能否 `VACUUM` | 能，0.01 秒完成，之後 `/global/health` 與 `/session` 正常。僅在 0.5 MB 的複本上測過 |
| 映像內可用的 SQLite 工具 | 沒有 `sqlite3` CLI；`python3` 的 `sqlite3` 模組為 3.40.1；`wukong` binary 在 `/usr/local/bin` |

本機 RunWuKong 部署的樣態：`opencode.db` 內 9 個 session，`agent_sessions` 只指向其中
2 個，其餘 7 個無主。7 個之中 4 個是 `Reply with exactly: OK` 的人工探測（標題不是
`Wukong`，來自 `docs/2026-09-08-model-eol-silent-failure-handover.md` 的驗證步驟），
3 個是末棒 session。

### 尚未證實

- **真正膨脹的那份資料庫裡，無主 session 佔多少。** 08-08 的 1.33 GiB 在另一台主機上，
  本機沒有副本。見下節。
- 08-08 文件記錄的 session 數恰好是 100，與 `GET /session` 的預設上限相同。若當時是用
  API 數的，實際數量可能更多。
- 刪除父 session 是否連帶刪除子 session（`parent_id`）。樣本中沒有子 session。
- 大型資料庫上單次刪除與 `VACUUM` 的耗時與鎖定行為。

## 最大未知：保留期清理能收回多少

TeleNexus 的案例是 3,000 個 session、123 MB，平均每個約 41 KB，而且 97% 是一次性的
排程與探針 session——刪舊 session 幾乎就是全部的解法。

Wukong 的 08-08 數字是 100 個 session、1.33 GiB，平均每個約 13 MiB，相差兩個數量級。
而 Wukong 的排程回合沿用 scope 的 session、輔助棒當場就刪，結構上不會大量產生一次性
session。這個形狀指向另一種成因：**少數長壽的 scope session 累積了全部歷史**，而它們
正是本設計刻意保護、不刪的對象。

所以本設計能收回多少，取決於那份資料庫裡無主 session 的佔比，目前無從得知。W1 的
預覽指令就是為了在動手前回答這個問題：若預覽顯示可刪的只佔一小部分，就該把「長壽
session 輪替」從非目標提前，而不是把本設計做完後宣稱問題已解。

## 設計

### 挑選規則

一個 session 必須同時滿足以下全部條件才會被刪：

1. `time.updated` 早於保留期。
2. 不被 `agent_sessions` 或 `agent_session_state` 的任何 scope 指向。
3. 是根 session（以 `roots=true` 列出）。子 session 只隨父 session 一起消失，不單獨判定。

被 scope 指向的 session **不論多舊都保留**，對應 telenexus#9 的「最新的聊天 session
一律保留」。

列表時一律明確帶 `limit`，並以回傳筆數是否等於 `limit` 判斷是否被截斷；被截斷就記錄
下來，不當成「已全部看過」。

### 失效方向

任何一步不確定就不刪：

- 讀不到 `agent_sessions`、列不出 session、回應解析失敗 → 整輪跳過，等下一輪。
- 保留期下限 1 天。進行中的回合所建立、尚未寫回 `agent_sessions` 的 session 必然是新
  的，下限確保它們永遠不可能成為候選。
- 單次刪除失敗只記錄、不重試，下一輪自然會再遇到它。

### 執行位置

掛在 `wukong-schedulerd` 既有的維護迴圈旁（`crates/wukong-schedulerd/src/main.rs:106-127`），
獨立的 interval。它已經同時持有 memory 與 backend，而且是 compose 內常駐的單一實例。

只在 Server backend 生效。容器部署的 `opencode-state` volume 專屬於 Wukong；容器內
`wukong` CLI profile 以 `opencode run` 建立的 session 落在同一個 volume、同一個目錄，
會被 server 列出並一併涵蓋。

每輪設刪除上限，由舊到新刪，避免首次套用時一口氣刪上千個。

### 空間回收

在 `opencode-server` 容器啟動、`exec opencode serve` **之前**執行
（`scripts/docker-entrypoint.sh:271-292`）。此時 server 尚未開啟資料庫，而既有的離峰
閒置重啟（預設 03:00-05:00）讓這個時點大約每天出現一次。

- freelist 佔比超過門檻才做。
- 檢查剩餘磁碟空間不小於資料庫大小（`VACUUM` 需要等量暫存）。
- 設 busy timeout；被鎖或任何失敗都只記錄，**不得阻擋 server 啟動**。
- 由 `wukong` 的子命令執行而非 shell 內嵌腳本，這樣能用 `cargo test` 覆蓋。

即使從不 `VACUUM`，刪除後釋出的頁也會被 SQLite 重用，檔案會停止成長；`VACUUM` 只負責
讓檔案變小。因此閒置重啟被停用的部署不會壞，只是不縮。

### 設定

| 變數 | 用途 | 預設 |
|---|---|---|
| `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` | 保留天數；`0` 停用清理 | `30` |
| `WUKONG_OPENCODE_SESSION_RETENTION_INTERVAL_SECS` | 清理間隔 | `21600` |
| `WUKONG_OPENCODE_SESSION_RETENTION_BATCH` | 每輪最多刪除數 | `200` |
| `WUKONG_OPENCODE_VACUUM_MIN_FREE_RATIO` | freelist 佔比門檻；`0` 停用回收 | `0.25` |

間隔、批次與門檻的預設值是暫定的，由 W1 在真實資料上量到的耗時修正。

### 可觀測性

每輪輸出一行，欄位包含：列出數、是否被截斷、受保護數、過期候選數、已刪數、失敗數，
以及**受保護但已超過保留期**的數量。最後一項是「被棄置的 scope」的規模訊號——它們
永遠不會被本設計刪除，這個數字讓人看得到累積。

## 工作項目

### W1 預覽指令，並在受影響的資料庫上量測

新增唯讀的預覽（例如 `wukong opencode-sessions prune --dry-run`），列出會被刪除的
session id、最後更新時間與受保護的清單。先在受影響主機的資料庫**複本**上執行，記錄：

- 無主且過期的 session 佔總資料量的比例（回答「最大未知」）。
- 單次與批次刪除的耗時；`VACUUM` 的耗時與所需暫存空間。
- 刪除父 session 後子 session 的去向。

這一步的結果決定 W2 之後的預設值，以及是否需要把輪替提前。

### W2 挑選邏輯

純函式：輸入 session 列表、受保護 id 集合、現在時間與保留期，輸出待刪清單。

### W3 列表與執行

`AiBackend` 新增列出 session 的能力（Server backend 實作，帶明確 `limit` 與
`roots=true`）；`wukong-memory` 新增列出所有被指向的 session id；`wukong-runtime` 的
maintenance 串接兩者並呼叫既有的 `delete_session`。

### W4 掛進 schedulerd 與設定

### W5 啟動前空間回收

### W6 修正 `--new` 旗標漏刪

讓 `main.rs:35-39` 與 `command.rs:46-52` 的 `/new` 行為一致：先刪 session 再清 mapping。

### W7 文件

`docs/docker.md` 的環境變數表、`CHANGELOG.md`，並回頭勾選 08-08 文件驗證清單中已由本次
實測回答的項目。

## 驗收條件

- **A1** 超過保留期且無 scope 指向的 session 被刪除，其 `message`／`part`／`event` 列數
  歸零；保留期內的 session 完全不變。
- **A2** 被 `agent_sessions` 指向的 session 不論多舊都不被刪，下一回合仍能續接。
- **A3** 保留期可由環境變數設定；設為 `0` 時不刪除任何 session；未設定時為 30 天。
- **A4** 讀取 `agent_sessions` 失敗或列表失敗時，該輪不刪除任何 session。
- **A5** session 總數超過 100 時，清理仍能看到並處理第 100 筆之後的 session。
- **A6** 清理失敗不影響進行中的聊天與排程回合。
- **A7** freelist 超過門檻時，`opencode-server` 重啟後資料庫檔案變小；回收失敗時 server
  照常啟動。
- **A8** `wukong --new` 之後，原 scope 的舊 session 不再存在於 opencode。
- **A9** 預覽指令不刪除任何資料，其列出的清單與實際執行會刪的集合一致。

測試依 `AGENTS.md` 的驗證紀律撰寫：斷言**哪些 id 被刪、哪些被留**，不只斷言數量；A1 與
A7 以真實的 opencode 在資料庫複本上驗證，不以 mock 取代——串聯刪除是 opencode 的行為，
mock 回答不了。

## 待決定

| 編號 | 事項 | 建議 |
|---|---|---|
| U1 | 被棄置的 scope（`agent_sessions` 指向、但早已無人使用）的 session 要不要也過期 | 本次不做，先靠可觀測性的計數看規模 |
| U2 | 長壽 scope session 的輪替要不要納入 | 等 W1 的量測結果再決定 |
| U3 | CLI backend 的輔助棒 session 要不要即時刪除 | 另案。需要先讓非串流路徑取得 session id，改動面較大 |
| U4 | 是否比照 TeleNexus 在 GitHub 開 issue 追蹤 | 待使用者決定 |
