# 盤點報告：flashaim RunWuKong

對應 [README](README.md) 的 SCN-001。只記聚合數值，不記對話內容、token 或使用者識別資料。raybird-desktop 的觀測只在[交接紀錄](../../2026-11-11-runtime-performance-handover.md)，本報告的數字都不是那台機器的。

## 步驟 1：版本矩陣與一致性副本

取樣：2026-11-23（UTC 02:37～02:38），主機 flashaim（`hostnamectl` 的 Static hostname），部署目錄 ~/Documents/RunWuKong，compose 專案 runwukong。

### 版本矩陣

| 項目 | 值 | 來源 |
|---|---|---|
| Wukong 映像 | ghcr.io/raybird/wukong:v0.25.1 | 三個容器的 `docker inspect` Config.Image |
| 映像 digest | sha256:898b73a149598e2a4a98ffc09f7fffb513f7ec27a67bdbe1f06ac1d1513bfb76 | `docker image inspect` RepoDigests；與 .wukong-release 的 imageDigest 相同 |
| 映像 ID | sha256:c1f70d8088f4f3fa829da344031ea505dfa5e551d79b817efd202b41b34c63c0 | 三個容器相同 |
| 容器內 OpenCode | 1.19.6（npm 套件 opencode-ai） | `docker exec wukong-web opencode --version` |
| 原始碼釘選的 OpenCode | 1.17.18 | `git show v0.25.1:release/runtime-inputs.env` |
| 原始碼基線 | bd001e6（issue 分支，產品程式碼與 v0.25.1 tag 相同） | `git log` |
| 執行模式 | 按需本機 OpenCode（WUKONG_AGENT_SERVER_URL 空值） | `docker exec wukong-web env` |

容器內是 1.19.6，原始碼的 runtime-inputs.env 卻是 1.17.18。這不是部署漂移：release CI 在建置前用 scripts/resolve-opencode-version.sh 把 npm latest 改寫進釘選檔，所以 tag 裡的 1.17.18 不是映像的實際版本（地雷 W-1 的同類情況）。對照與回復要以 1.19.6 為 V1 基準。

### 執行限制（2026-11-23 起動的容器）

| 項目 | web、telegram、schedulerd 各自 | raybird-desktop（交接紀錄） |
|---|---|---|
| CPU | 1.5（cpu.max 150000 100000） | 1.5 |
| 記憶體上限 | 3 GiB（memory.max 3221225472） | 2 GiB |
| PID 上限 | 256 | 256 |
| Memoria overlay | 有（/memoria、/opt/memoria） | 無 |

記憶體上限不同，raybird-desktop 的記憶體壓力結論不能直接套到這台。

### 資料卷與資料庫

三個容器掛同一組 volume：runwukong_wukong-data（/data）、runwukong_opencode-state（/home/wukong/.local/share/opencode）、runwukong_opencode-config、runwukong_gh-config、runwukong_agent-reach-state、runwukong_memoria-data（/memoria），/workspace 是 bind mount。

| 資料庫 | 主檔 | WAL | 主檔最後修改 | 副本大小 | 資料表 | integrity_check |
|---|---|---|---|---|---|---|
| /data/memory.db | 1,925,120 | 3,135,352 | 2026-11-21 | 1,925,120 | 19 | ok |
| opencode.db | 41,889,792 | 4,420,792 | 2026-11-23 | 42,078,208 | 20 | ok |
| Memoria sessions.db | 450,560 | 無 | 2026-10-07 | 450,560 | 34 | ok |
| Memoria vectors.db | 450,560 | 無 | 2026-08-13 | 450,560 | 3 | 未知：索引函式 libsql_vector_idx 要 libsql 才驗得了 |

大小單位為 byte。opencode-state 另有 tool-output 目錄 3.3 MB（32 個檔）與 log 目錄 5.0 MB。

### 副本取法與驗證

副本放在本機暫存目錄，不進 repo。來源 volume 一律以 `:ro` 掛進一次性容器，連線用 `mode=ro`，容器加 `--network none`，所以正式資料不會被寫入或遷移。

```bash
# 在 repo 根目錄執行；DST 換成要放副本的目錄
docker run --rm --user 1000:1000 --network none --entrypoint python3 \
  -v runwukong_wukong-data:/src/data:ro -v runwukong_opencode-state:/src/opencode:ro \
  -v runwukong_memoria-data:/src/memoria:ro -v "$DST":/dst \
  -v "$PWD/docs/issues/issue-0030/scripts":/scripts:ro \
  ghcr.io/raybird/wukong:v0.25.1 -I /scripts/snapshot.py
```

反向自檢（[scripts/wal-check.py](scripts/wal-check.py)，另掛 `-v "$WORK":/work`、`-v "$DST":/dst:ro`）：

| 資料庫 | 副本總筆數 | 主檔＋WAL 原始複本 | 只有主檔 |
|---|---|---|---|
| memory.db | 2,295 | 2,295 | 2,226 |
| opencode.db | 19,422 | 19,422 | 19,293 |

只有主檔時筆數較少，副本與「主檔＋WAL」相等，證明副本包含 WAL 裡尚未 checkpoint 的資料。若副本只抄到主檔，這項檢查會失敗。兩次執行（02:37、02:38）的副本 sha256 相同。

### 限制

- 這台有排程：副本的 scheduler_jobs 2 筆、scheduler_runs 25 筆，執行量遠少於 raybird-desktop 交接紀錄裡的尾端樣本，兩台的負載不能直接比。
- 容器於 2026-11-23 00:54 UTC 重啟，cgroup 的 cpu.stat 只涵蓋重啟之後，留給步驟 5 在負載下量測。

## 步驟 2：資料庫聚合盤點

取樣：2026-11-23，步驟 1 的副本（02:38 UTC）。腳本 [scripts/inventory.py](scripts/inventory.py)，只輸出筆數與 byte 數。

```bash
docker run --rm --user 1000:1000 --network none --entrypoint python3 \
  -v "$DST":/dst:ro -v "$PWD/docs/issues/issue-0030/scripts":/scripts:ro \
  ghcr.io/raybird/wukong:v0.25.1 -I /scripts/inventory.py control
# 對照組通過後再盤點副本
docker run ...（同上） -I /scripts/inventory.py /dst/opencode.db /dst/memory.db /dst/memoria-sessions.db
```

### 取數方法的驗證

對照組（`inventory.py control`）造一個內容已知的資料庫：3 個 session（其中 1 個是子 session）、part 7／3／0 筆、message 2／1／0 筆、event 3 筆（1 筆不屬於任何 session），逐項比對筆數、byte 數、分組排序與查不到的表。結果 PASS。

反向自檢，下列三種改壞的版本都會讓對照組 FAIL：

| 改壞的地方 | 對照組結果 |
|---|---|
| 前幾大 session 的查詢跳過第一名（`offset 1`） | FAIL：top_sessions_by_parts、top_sessions_by_messages 不符 |
| 查不到的表回傳 0 而不是 unknown | FAIL：missing table 拿到 0 |
| 「不屬於 session 的 event」條件寫反 | FAIL：events_outside_sessions 拿到 2，應為 1 |

### 檔案與頁面

| 資料庫 | 副本 byte | page_size | page_count | freelist_count |
|---|---|---|---|---|
| opencode.db | 42,078,208 | 4096 | 10,273 | 0 |
| memory.db | 1,925,120 | 4096 | 470 | 101 |
| Memoria sessions.db | 450,560 | 4096 | 110 | 0 |

memory.db 的 freelist 占 21%（101／470 頁，約 0.4 MB），總量小，不影響效能判斷。vectors.db 要 libsql 才讀得了索引，沒有盤點，列為未知。

### opencode.db

| 表 | 筆數 | 占用 byte（含索引） | 占全檔 |
|---|---|---|---|
| event | 14,033 | 27,701,248 | 66% |
| part | 4,254 | 13,496,320 | 32% |
| message | 1,056 | 675,840 | 2% |
| session | 6 | 20,480 | 低於 1% |

其餘 16 張表都在 25 KB 以下。

- session 6 個，沒有子 session；最早建立於 2026-09-08，最新於 2026-11-19。
- 最大的 session 有 2,114 個 part、data 共 4.8 MB；第二、三名是 1,161 個 part（1.2 MB）、767 個 part（1.2 MB）。依 message 數排是 523、279、201 則。
- part 依類型：tool 1,147 筆占 6.2 MB（part data 總量的 81%），reasoning 691 筆 0.9 MB，text 439 筆 0.4 MB，compaction 14 筆。
- event 依類型：message.part.updated 9,931 筆 14.4 MB，message.updated 3,016 筆 1.4 MB，session.updated 1,080 筆 0.6 MB，session.created 6 筆。所有 event 都屬於現存的 6 個 session（不屬於 session 的 0 筆）。
- event 表是 part 更新的事件紀錄：message.part.updated 的 data 總量（14.4 MB）比 part 表本身的 data（7.7 MB）還大，佔用量主要在這裡，不在 session 數。

### Wukong memory.db

| 表 | 筆數 | 占用 byte（含索引） |
|---|---|---|
| turn_events | 1,800 | 749,568 |
| memories | 84 | 356,352 |
| chat_messages | 82 | 135,168 |
| memories_fts_data | 34 | 126,976 |
| recall_telemetry | 52 | 12,288 |
| scheduler_runs | 25 | 20,480 |
| chat_live_events | 0 | 8,192 |

agent_sessions、agent_session_state 各 4 筆，chat_threads 2 筆。chat_live_events 為 0，#28 的 live events 清理之後沒有殘留。

### Memoria sessions.db

全庫 110 頁，最大的表 memory_nodes 3 筆、sessions 1 筆；最後修改在 2026-10-07，這台的 Memoria 幾乎沒有使用，不列入瓶頸候選。

### 保留期清理的實際行為

schedulerd 日誌（`docker logs wukong-schedulerd`，2026-11-20 起動後）每 6 小時一筆，最近 5 筆都是 `listed=6 protected=4 stale_protected=1 expired=0 deleted=0 failed=0`。

2026-09-08 建立、之後沒再更新的那個 session 已超過 30 天，但仍被某個 scope 指向，所以列在 stale_protected 而沒有刪。另一個 2026-09-08 建立的 session 在 2026-11-19 仍有更新，不算過期。保留期清理照 #28 的設計運作，opencode.db 的大小來自少數幾個長期 session 的 part 與 event，不是過期 session 沒清。

## 步驟 3：查詢計畫與耗時

取樣：2026-11-23，步驟 1 的副本。腳本 [scripts/queries.py](scripts/queries.py)；容器加上與正式部署相同的 `--cpus 1.5 --memory 3g`，連跑兩次。

```bash
docker run --rm --user 1000:1000 --network none --cpus 1.5 --memory 3g --entrypoint python3 \
  -v "$DST":/dst:ro -v "$PWD/docs/issues/issue-0030/scripts":/scripts:ro \
  ghcr.io/raybird/wukong:v0.25.1 -I /scripts/queries.py /dst/memory.db /dst/opencode.db
```

冷是開新連線後第一次執行，暖是同一連線 20 次的中位數。作業系統的檔案快取沒有清（清快取要 root），所以「冷」只排除了 SQLite 自己的頁快取。opencode 的查詢是依 opencode.db 的索引推測它載入 session 時會用的形狀，不是從 opencode 原始碼抄出來的。

| 查詢 | 資料規模 | 回傳筆數 | 冷 ms（兩次） | 暖 ms（兩次） | 計畫 |
|---|---|---|---|---|---|
| recall.keyword_candidates（FTS5） | memories 84 筆 | 20 | 0.64／0.60 | 0.20／0.20 | FTS 索引＋主鍵，排序用暫存 B-tree |
| recall.cjk_fallback_candidates | 同上 | 20 | 0.25／0.29 | 0.06／0.07 | 走 memories_created_at_idx 掃描 |
| recall.recent_candidates | 同上 | 20 | 0.15／0.15 | 0.005／0.005 | 覆蓋索引 |
| session.state_by_scope | agent_session_state 4 筆 | 1 | 0.14／0.15 | 0.002／0.002 | 主鍵索引 |
| chat.messages_by_thread | 最大的 thread 82 則 | 82 | 0.17／0.22 | 0.02／0.02 | 覆蓋索引 |
| opencode.session_list | session 6 個 | 6 | 0.17／0.21 | 0.005／0.007 | 全表掃描，排序用暫存 B-tree |
| opencode.messages_of_session | 最大的 session | 523 | 0.66／0.60 | 0.21／0.22 | message_session_time_created_id_idx |
| opencode.parts_of_session | 同上 | 2,114 | 11.8／12.4 | 8.4／8.4 | part_session_idx |
| opencode.parts_by_message | 同上 | 2,114 | 16.3／15.4 | 13.8／13.7 | 兩個索引，排序用暫存 B-tree |
| opencode.events_of_session | 同上 | 6,550 | 15.1／15.3 | 14.5／14.9 | event_aggregate_seq_idx |
| 對照：part 依 json 類型全表掃描 | part 4,254 筆 | 1 | 10.8／10.9 | 10.8／11.3 | SCAN part |

核心查詢都走索引，只有 session_list 是全表掃描，而它只有 6 筆。最貴的是載入最大 session 的全部 part 或 event，約 15 ms，跟全表掃描 part 的對照組同一個量級，因為那個 session 本來就占了 part 的一半。耗時大致跟 session 的資料量成正比，不是缺索引造成的。

### 正式部署的召回耗時（recall_telemetry）

memory.db 的 recall_telemetry 是正式部署自己寫的量測：relevant 模式 49 次，p50 7 ms、p95 32 ms、最大 42 ms；hybrid 模式 3 次，最大 3 ms。沒有被略過的召回。

### 結論與限制

在 flashaim 目前的資料規模下（opencode.db 42 MB、最大 session 2,114 個 part），資料庫查詢與召回都在毫秒級，跟交接紀錄的整個任務 1,218～2,390 秒差了五個數量級以上，不是 flashaim 上的瓶頸候選。raybird-desktop 的資料規模沒有取得，這個結論不能推到那台。查詢耗時會隨單一 session 的 part 與 event 成長，長期 session 的成長速度留給步驟 5 量測。

## 步驟 4：OpenCode V2（2.3.2）最小契約與遷移／回復探測

取樣：2026-11-23。V2 以 `npm install -g --prefix` 裝在暫存目錄（@opencode/cli 2.3.2，原生執行檔在 @opencode/cli-linux-x64，約 253 MB），在 wukong:v0.25.1 映像的一次性容器裡執行，HOME 與 XDG 目錄都指向暫存目錄。遷移探測用步驟 1 副本的再複製，原副本的 sha256 探測前後都是 2d2f3518…，正式容器沒有重啟或改動。

### 官方文件

https://opencode.ai/docs/migrate-v1/ 只寫「V1 server API 的整合要改用 V2 API」，指向的 API 參考頁（https://opencode.ai/api）回 404。端點對照全部是實測結果，V2 server 自己提供的 /openapi.json（142 條路徑，標示 Experimental）是格式來源。

### Wukong 用到的介面對照

Wukong 用到的介面取自 crates/wukong-gateway/src/local_process.rs 與 opencode_server.rs。

| Wukong 現在的用法（V1） | V2 2.3.2 實測 | 影響 |
|---|---|---|
| `opencode serve --hostname 127.0.0.1 --port 0 --print-logs` | 旗標都在，`--port 0` 可用 | 無 |
| 等待 stdout 出現 `opencode server listening on ` | 印的是 `server listening on http://…` | 啟動偵測永遠等不到，每次都會卡到 agent timeout |
| 移除 OPENCODE_SERVER_PASSWORD，本機模式不帶認證 | 沒給密碼時自動產生隨機密碼，/api 沒帶認證回 401；給 OPENCODE_SERVER_PASSWORD 就用它。使用者名稱固定為 opencode，OPENCODE_SERVER_USERNAME 不再支援 | 本機模式要自己設密碼再帶 basic auth |
| GET /global/health | 不存在，但 GET 回 SPA 網頁 HTML、狀態 200；改用 GET /api/info | **假綠燈**：現有健康檢查會判成功 |
| POST /session、GET /session?limit=、GET／DELETE /session/{id} | 改到 /api/session…；回應包在 data 裡，列表改成 cursor 分頁 | 路徑與解析都要改；列表要跟著 cursor 走完，不然會重演 W-6 |
| POST /session/{id}/message（同步）、POST /session/{id}/prompt_async | 只剩 POST /api/session/{id}/prompt，一律非同步排入；body 是 text，不是 parts | 同步呼叫要改成送出後等事件 |
| GET /session/{id}/message?limit= | GET /api/session/{id}/message，回 data 加 cursor，訊息形狀是 type＋content，沒有 info／parts | 回覆擷取、模型擷取（providerID／modelID）的解析都要改 |
| GET /event（SSE），事件 message.part.updated、session.idle、permission.* | GET /api/event；V1 的路徑回 HTML。事件全部改名：session.text.delta、session.execution.succeeded／interrupted、session.tool.*、permission.asked／replied 等 | 串流與完成判定要重寫 |
| POST /session/{id}/abort | POST /api/session/{id}/interrupt，回 interrupted=true | 實測可用，路徑要改 |
| POST /session/{id}/summarize | POST /api/session/{id}/compact（排入下一個 step 邊界） | 實測可用，路徑要改 |
| POST /permission/{id}/reply | GET /api/session/{id}/permission 取得待決請求，POST …/permission/{requestID}/reply，body 是 decision（once、always、reject） | 實測可用，路徑與 body 要改；權限動作名稱也改了（bash 改成 shell 等，見遷移文件） |
| POST /api/session/{id}/question/{qid}/reply、reject | 沒有 question，改成 form：POST /api/session/{id}/form/{formID}/reply | 要改 |
| 模型選擇放在 prompt body | POST /api/session/{id}/model 切換，或建立 session 時帶 model | 要改 |

[scripts/v2-probe.sh](scripts/v2-probe.sh) 的結果（2026-11-23 第二次執行，模型為 V2 預設的免費模型 opencode/big-pickle）：

| 項目 | 結果 |
|---|---|
| 認證：沒帶密碼 | PASS，401 |
| V1 路徑 GET /global/health、/session、/event | FAIL，三條都回 200 與網頁 HTML |
| 權限：shell 設為 ask，出現 permission.asked，回覆 once 後完成 | PASS |
| 取消：長回答中途 interrupt | PASS，interrupted=true，出現 session.execution.interrupted |
| 壓縮、換模型、讀訊息、刪除後再讀 | PASS（compact 200、model 204、6 則訊息、delete 204 後 404） |

反向自檢：用錯誤的密碼跑同一個腳本，權限項目就判 FAIL（沒有 permission.asked），探測器不會在請求失敗時假綠燈。第一次執行時取消項目的判定太寬，看到已經出現的 session.execution.started 也算通過；已改成必須看到 session.execution.interrupted，第二次執行才是上表的結果。

### 遷移與回復

| 動作（同一份副本，依序） | 結果 |
|---|---|
| V2 第一次啟動 | 就地遷移同一個 opencode.db（路徑不變）：套用 24 個 migration，新增 session_v2、session_message、session_part、automation、job、blob 等表；V1 的表保留。/api/experimental/migration/v1 回 completed，6 個 session 都看得到 |
| 停 V2，V1 1.19.6 開同一個檔 | 可以啟動，health 正常，6 個 session、最大 session 523 則 message／2,114 個 part 都讀得到；V1 再建一個 session |
| 再開 V2 | 沒有再跑 migration，只看得到原本的 6 個 session，V1 剛建的看不到；V2 再建一個 session |
| 再開 V1 | 看得到 7 個（原本 6 個加上 V1 建的），V2 建的看不到 |

V2 的歷史遷移只跑一次，之後 V1 與 V2 各寫各的表：

- 退回 V1 時程式可以跑，但 V2 期間的對話 V1 都看不到。
- 退回後再升級，V1 期間的對話 V2 也看不到。

所以回復方案是「升級前備份 opencode.db，退回時還原備份」，接受備份之後的對話紀錄會遺失。只換回 V1 版本號而不還原，會留下兩套互不相通的歷史。

V1 與 V2 預設讀同一個 opencode.db 與同一組設定目錄。正式部署只要有一個容器開了 V2，就會遷移三個入口共用的 runwukong_opencode-state。所以不能用滾動升級，三個入口要同時換版，或先讓 V2 用另一份資料目錄。

### 封裝

V2 的套件名稱從 opencode-ai 改成 @opencode/cli，原生執行檔在各平台的子套件裡。release/package.json、scripts/resolve-opencode-version.sh（`npm view opencode-ai version`）與 scripts/test-release-image.sh（檢查 package.json 釘的是 opencode-ai）都寫死舊套件名稱；換版要一起改，integrity 也要釘在 @opencode/cli 與平台子套件上。

### 結論

SCN-002 的最小契約：V2 的功能面（建立、送出、串流、取消、權限、壓縮、換模型、刪除）都可用，但 Wukong 用到的每一條路徑、事件名稱與回應形狀都變了。啟動偵測與認證也不相容，健康檢查還會假綠燈。不改 adapter 就換版，本機模式每個回合都會卡到逾時，server 模式的健康檢查卻顯示正常。資料遷移是就地、單向的。阻礙與後續範圍交給步驟 6 的 go/no-go。
