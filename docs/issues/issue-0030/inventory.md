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

2026-11-23 起腳本自己判定：副本等於「主檔＋WAL」且大於「只有主檔」才印 PASS。正式資料會持續寫入，所以要在取副本後立刻執行；WAL 剛好已全部 checkpoint 時「只有主檔」也會相等，這時會判 FAIL，要換個時間重取。改版後重取副本再檢查，memory.db 與 opencode.db 都是 PASS。

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

反向自檢，用 sed 改出下列三種壞掉的版本，對照組都會 FAIL：

| 改壞的地方（sed 指令） | 對照組結果 |
|---|---|
| 前幾大 session 的查詢跳過第一名（`s/limit ?/limit ? offset 1/`） | FAIL：top_sessions_by_parts、top_sessions_by_messages 不符 |
| 查不到的表回傳 0 而不是 unknown（`s/return UNKNOWN$/return 0/`） | FAIL：missing table 拿到 0 |
| 「不屬於 session 的 event」條件寫反（`s/where aggregate_id not in/where aggregate_id in/`） | FAIL：events_outside_sessions 拿到 2，應為 1 |

對照組沒有涵蓋各表占用的 byte 數（dbstat）。另外交叉驗算：opencode.db 各表加總 42,057,728 byte，page_count×page_size 為 42,078,208 byte，相差的 20 KB 是 sqlite_master 等系統頁。

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

在 flashaim 目前的資料規模下（opencode.db 42 MB、最大 session 2,114 個 part），資料庫查詢與召回都在毫秒級，最慢約 16 ms，比 flashaim 自己的回合耗時（步驟 5，10～25 秒）小了約三個數量級，不是 flashaim 上的瓶頸候選。raybird-desktop 的資料規模沒有取得，這個結論不能推到那台。查詢耗時會隨單一 session 的 part 與 event 成長，長期 session 的成長速度留給步驟 5 量測。

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
| GET /global/health（gateway 的 health_check；選用的 opencode-server 容器另有 compose healthcheck `curl -fsS http://localhost:4096/global/health`） | 不存在；GET 回 SPA 網頁 HTML、狀態 200，沒帶認證也一樣是 200 與 text/html。替代是 GET /api/info（要認證） | gateway 的 health_check 走 send_json，回應不是 JSON 會判失敗；compose healthcheck 只看狀態碼，server 模式下會顯示 healthy（假綠燈） |
| POST /session、GET /session?limit=、GET／DELETE /session/{id} | 改到 /api/session…；回應包在 data 裡，列表改成 cursor 分頁 | 路徑與解析都要改；列表要跟著 cursor 走完，不然會重演 W-6 |
| POST /session/{id}/message（同步）、POST /session/{id}/prompt_async | 只剩 POST /api/session/{id}/prompt，一律非同步排入；body 是 text，不是 parts | 同步呼叫要改成送出後等事件 |
| GET /session/{id}/message?limit= | GET /api/session/{id}/message，回 data 加 cursor，訊息形狀是 type＋content，沒有 info／parts | 回覆擷取、模型擷取（providerID／modelID）的解析都要改 |
| GET /event（SSE），事件 message.part.updated、session.idle、permission.* | GET /api/event；V1 的路徑回 HTML。事件全部改名：session.text.delta、session.execution.succeeded／interrupted、session.tool.*、permission.asked／replied 等 | 串流與完成判定要重寫 |
| POST /session/{id}/abort | POST /api/session/{id}/interrupt，回 interrupted=true | 實測可用，路徑要改 |
| POST /session/{id}/summarize | POST /api/session/{id}/compact（排入下一個 step 邊界） | 端點接受請求（200），有沒有真的壓縮沒有驗證；路徑要改 |
| POST /permission/{id}/reply | GET /api/session/{id}/permission 取得待決請求，POST …/permission/{requestID}/reply，body 是 decision（once、always、reject） | 實測可用，路徑與 body 要改；權限動作名稱也改了（bash 改成 shell 等，見遷移文件） |
| POST /api/session/{id}/question/{qid}/reply、reject | 沒有 question，改成 form：POST /api/session/{id}/form/{formID}/reply | 要改 |
| 模型選擇放在 prompt body | POST /api/session/{id}/model 切換，或建立 session 時帶 model | 要改；探測只切到原本的同一個模型（回 204），實際切換沒有驗證 |

啟動訊息、認證（含 OPENCODE_SERVER_USERNAME 不再生效）、V1 路徑、/api/info、openapi 與遷移／回復（含 V1 讀回最大 session 的 message 與 part 數）都由 [scripts/v2-migration.sh](scripts/v2-migration.sh) 重現，全程 `--network none`、不呼叫模型。2026-11-23 重跑的輸出與下文相符：

```bash
# V2_PREFIX 是 npm install -g --prefix 裝 @opencode/cli@2.3.2 的目錄；WORK_DIR 是空目錄
bash docs/issues/issue-0030/scripts/v2-migration.sh "$DST" "$V2_PREFIX" "$WORK_DIR"
```

會呼叫模型的契約項目由 [scripts/v2-probe.sh](scripts/v2-probe.sh) 檢查，在有網路、設了 OPENCODE_SERVER_PASSWORD 的 V2 容器裡用 `docker exec -e PW=... 容器 sh v2-probe.sh` 執行，啟動方式同 v2-migration.sh 的 start，但不加 `--network none`，並另外掛上腳本目錄（例如 `-v "$PWD/docs/issues/issue-0030/scripts":/scripts:ro`，再執行 `sh /scripts/v2-probe.sh`）。2026-11-23 第二次執行的結果，模型為 V2 預設的免費模型 opencode/big-pickle：

| 項目 | 結果 |
|---|---|
| 認證：沒帶密碼 | PASS，401 |
| V1 路徑 GET /global/health、/session、/event | FAIL，三條都回 200 與網頁 HTML |
| 權限：shell 設為 ask，出現 permission.asked，回覆 once 後完成 | PASS |
| 取消：長回答中途 interrupt | PASS，interrupted=true，出現 session.execution.interrupted |
| 讀訊息、刪除後再讀 | PASS（6 則訊息；delete 204 後 404） |
| 壓縮、換模型 | 端點接受請求（compact 200、model 204）；只看狀態碼，壓縮結果與實際切換沒有驗證 |

反向自檢：用錯誤的密碼跑同一個腳本，權限項目就判 FAIL（沒有 permission.asked），探測器不會在請求失敗時假綠燈；這次的輸出沒有保存在 repo。第一次執行時取消項目的判定太寬，看到已經出現的 session.execution.started 也算通過；已改成必須看到 session.execution.interrupted，第二次執行才是上表的結果。

### 遷移與回復

| 動作（同一份副本，依序） | 結果 |
|---|---|
| V2 第一次啟動 | 就地遷移同一個 opencode.db（路徑不變）：套用 24 個 migration，新增 session_v2、session_part、session_inbox、automation、job、blob 等 12 張表，並刪掉 V1 的 data_migration、session_context_epoch、session_input 三張表（這份副本裡三張都是空表）；其餘 V1 的表保留。/api/experimental/migration/v1 回 completed，6 個 session 都看得到 |
| 停 V2，V1 1.19.6 開同一個檔 | 可以啟動，health 正常，6 個 session、最大 session 523 則 message／2,114 個 part 都讀得到；V1 再建一個 session |
| 再開 V2 | 沒有再跑 migration，只看得到原本的 6 個 session，V1 剛建的看不到；V2 再建一個 session |
| 再開 V1 | 看得到 7 個（原本 6 個加上 V1 建的），V2 建的看不到 |

V2 的歷史遷移只跑一次，之後 V1 與 V2 各寫各的表：

- 退回 V1 時程式可以跑，但 V2 期間的對話 V1 都看不到。
- 退回後再升級，V1 期間的對話 V2 也看不到。

所以回復方案是「升級前備份 opencode.db，退回時還原備份」，接受備份之後的對話紀錄會遺失。只換回 V1 版本號而不還原，會留下兩套互不相通的歷史；而且 V1 少了被 V2 刪掉的三張表，這次探測沒用到它們，用到的 V1 功能可能出錯。

V1 與 V2 預設讀同一個 opencode.db 與同一組設定目錄。正式部署只要有一個容器開了 V2，就會遷移三個入口共用的 runwukong_opencode-state。所以不能用滾動升級，三個入口要同時換版，或先讓 V2 用另一份資料目錄。

### 封裝

V2 的套件名稱從 opencode-ai 改成 @opencode/cli，原生執行檔在各平台的子套件裡。release/package.json、scripts/resolve-opencode-version.sh（`npm view opencode-ai version`）與 scripts/test-release-image.sh（檢查 package.json 釘的是 opencode-ai）都寫死舊套件名稱；換版要一起改，integrity 也要釘在 @opencode/cli 與平台子套件上。

### 結論

SCN-002 的最小契約：V2 的建立、送出、串流、取消、權限、讀訊息、刪除都實測可用，壓縮與換模型的端點接受請求但效果沒有驗證。Wukong 用到的每一條路徑、事件名稱與回應形狀都變了，啟動偵測與認證也不相容。不改 adapter 就換版，本機模式每個回合都會卡到逾時；server 模式的 gateway 健康檢查會失敗，但 compose healthcheck 仍顯示 healthy。資料遷移是就地、單向的，還會刪掉 V1 的三張表。阻礙與後續範圍交給步驟 6 的 go/no-go。

## 步驟 5：版本對照

取樣：2026-11-23 03:05～03:09 UTC，flashaim。每個映像各用一份步驟 1 副本的再複製，容器限制與正式部署相同（1.5 CPU、3 GiB、256 PID）。[scripts/bench-turn.sh](scripts/bench-turn.sh) 只跑一次 entrypoint，之後在同一個容器連跑 3 個回合：第 1 回合算冷，第 2、3 回合算暖。prompt 是固定的合成內容，模型是設定檔沒有指定時的預設免費模型 opencode/big-pickle。

```bash
# RUN_DIR 內放 data/memory.db、opencode/opencode.db、config/（opencode-config 的複本）、workspace/
bash docs/issues/issue-0030/scripts/bench-turn.sh ghcr.io/raybird/wukong:v0.25.1 "$RUN_DIR" short 3
```

| 映像（內含 OpenCode） | 任務 | 回合耗時 ms（冷、暖、暖） | 結果 | CPU 秒 | throttled 次數／秒 | 容器記憶體峰值 | opencode.db 增量 KB |
|---|---|---|---|---|---|---|---|
| v0.23.0（1.19.3） | short | 24,597／11,409／10,303 | 3 次成功 | 9.5／7.6／7.2 | 24～25／0.9～1.0 | 658 MiB | 788／64／72 |
| v0.25.1（1.19.6） | short | 16,112／15,405／10,063 | 3 次成功 | 5.6／5.4／5.3 | 11～13／0.3～0.4 | 600 MiB | 760／100／48 |
| v0.23.0（1.19.3） | tool | 15,032／16,363／19,994 | 3 次成功 | 7.5／7.6／7.3 | 22～24／0.8～0.9 | 661 MiB | 1,239／44／92 |
| v0.25.1（1.19.6） | tool | 2,230／1,926／2,133 | 3 次失敗：上游回 Free usage limit reached | 2.6／2.4／2.3 | 5／0.1 | 599 MiB | 314／20／8 |

- memory.db 每回合增量都是 0：CLI 回合沒有寫入記憶，寫入等待無從量測。
- 記憶體峰值是整個容器的累計值，包含 entrypoint 初始化；cgroup 檔案系統唯讀，不能逐回合重設。
- v0.25.1 short 的 opencode 日誌（bench 副本的 err-1～3.txt）：每回合啟動兩個 opencode 程序，plan 與 build 各一個（不同的 `run=`）。各自從載入設定到第一次呼叫上游分別是 1.25／0.95、1.05／0.97、0.95／0.99 秒，合計每回合 1.9～2.2 秒，占回合耗時的 13～19%。每個程序從 `message=stream`（開始呼叫上游）到 `message=loop … step=1`（這一步結束）就是一次 LLM step：三個回合的 plan／build 分別是 4.6／8.0、8.1／3.9、4.3／2.3 秒，合計 12.6、12.0、6.6 秒，占回合耗時的 78%、78%、66%。LLM step 的時間也包含 opencode 自己處理串流的時間，所以這是上游耗時的上限，不是純粹等待上游的時間。v0.23.0 的日誌沒有時間戳，不能拆階段。
- 不呼叫模型，單獨量 opencode 的啟動成本（[scripts/boot-cost.sh](scripts/boot-cost.sh)，v0.25.1，同樣 1.5 CPU，5 次）：從啟動到印出 listening 0.89～0.99 秒、1.21～1.32 CPU 秒。每回合啟動兩次，約 2.5 CPU 秒，占每回合 5.3～5.6 CPU 秒的一半左右；其餘約 3 CPU 秒花在串流處理、Wukong 本身或程序結束，這次的量測分不出來。
- 成功的判準：rc=0、輸出內容符合任務（short 輸出 OK；tool 在 workspace 留下內容為 hello 的 bench.txt）。v0.23.0 tool 三次都留下 bench.txt。
- v0.25.1 tool 任務的 3 次失敗是本次探測與對照把免費模型的額度用完：上游回 Free usage limit reached，Wukong 回報「上游模型錯誤（上游限流）」，2 秒內結束，沒有變成逾時（#24 的修正有效）。這 3 筆保留為失敗樣本，不算進成功回合的耗時。
- 同一段時間，正式部署的 schedulerd 正在用同一個模型跑排程。查日誌（`docker logs --since 40m`）沒有出現額度錯誤；發現額度用完後，已停止所有會呼叫模型的量測。
- 量測前試過每回合起一個新容器，結果混入了 entrypoint 的固定開銷（只跑 `wukong --help`：v0.23.0 4.8 秒，v0.25.1 11.2 秒），正式部署的常駐容器不付這筆開銷，所以那組數字棄用。

### 沒有量測的項目

2026-11-23 使用者決定以現有資料收尾（見 README 的 Gate 豁免紀錄），下列項目沒有量測：

| 項目 | 原因 | 對結論的影響 |
|---|---|---|
| v0.25.1 的 tool 任務成功樣本 | 3 次都碰到免費模型額度用完 | 不知道 v0.25.1 跑工具的回合耗時；同版 short 任務與 v0.23.0 tool 任務都顯示耗時主要在上游 |
| OpenCode V1 與 V2 本身（不經 Wukong）的效能對照 | 需要再呼叫模型 | V2 有沒有比較快仍是未知；不影響 go/no-go，因為不論快慢都要先改 adapter |
| Wukong＋V2 | 步驟 4 契約不相容，接不起來 | 依計畫記為不可比較 |
| 大型 session 的完整回合 | 要把對話副本送到外部模型，所以沒跑；改用步驟 3 的本機載入時間（最大 session 約 15 ms）代替 | 上游處理長上下文的耗時未知 |
| 多個入口同時寫入的 SQLite 等待 | 對照是單一回合，CLI 回合不寫 memory.db | 寫入競爭仍是假設 |

## 步驟 6：根因排序與升級 go/no-go

依據步驟 1～5，結論只適用 flashaim 上的 v0.25.1。raybird-desktop 的根因不在範圍內（README 的 TBD-4）。

### 交接紀錄的假設在 flashaim 上的結果

| 假設或既有修正 | 結果 | 證據 |
|---|---|---|
| 回合耗時主要在上游模型 | 大致成立：v0.25.1 每回合呼叫上游兩次（plan、build），兩次 LLM step 合計 6.6～12.6 秒，占回合耗時的 66～78%（含 opencode 處理串流的時間，是上游耗時的上限）；本機的 opencode 啟動合計約 2 秒；上游限流時 2 秒內失敗 | 步驟 5 |
| 長期 session 的事件與工具輸出讓資料量變大 | 資料量確實集中在少數長期 session 的 event 與 part；但在目前規模下，讀取最大 session 約 15 ms，不構成瓶頸 | 步驟 2、3 |
| 按需啟動 opencode 的重複成本 | 成立，不可忽略：每回合啟動兩次，合計約 2 秒（回合耗時的 13～19%）、約 2.5 CPU 秒（每回合 CPU 的一半左右） | 步驟 5 |
| 共用 SQLite 的寫入競爭 | 未驗證，仍是假設 | 步驟 5「沒有量測的項目」 |
| 1.5 CPU 限制造成 throttling | 有但小：v0.25.1 每回合 11～13 次、0.3～0.4 秒；v0.23.0 約 24 次、0.9 秒 | 步驟 5 |
| 記憶體壓力 | 這台上限 3 GiB，容器峰值約 600 MiB，沒有壓力；raybird-desktop 的 2 GiB 上限沒有量到 | 步驟 1、5 |
| 資料庫查詢與召回成本 | 不成立：毫秒級，召回 p95 32 ms | 步驟 3 |
| #24 上游錯誤不再被掩蓋成逾時 | 已修復：額度用完時回報「上游模型錯誤（上游限流）」 | 步驟 5 |
| #28 保留期清理 | 已修復且在運作：每 6 小時一次，過期但被 scope 指向的 session 保留 | 步驟 2 |

### 根因排序（flashaim，v0.25.1）

1. 上游模型：每回合兩次 LLM step，合計 6.6～12.6 秒，占回合耗時的 66～78%，這是上游耗時的上限（含 opencode 處理串流的時間）；免費模型的額度與限流會直接讓回合失敗。
2. 每回合按需啟動 opencode 兩次（plan、build）：合計約 2 秒、約 2.5 CPU 秒，在 1.5 CPU 上限下還有 0.3～0.4 秒的 throttling。改用常駐的 opencode server 可以省掉這筆，但要另外評估。
3. opencode.db 的成長：新 session 的第一回合增加約 0.8 MB，之後每回合 50～100 KB，大多是 event 表；目前不影響查詢，長期要看單一 session 的大小。

v0.25.1 比 v0.23.0 每回合少用約 33% CPU（short 任務三次平均 8.1 秒對 5.4 秒），throttling 減半。

### 升級 go/no-go：暫緩（no-go）

理由：

- 不相容：Wukong 用到的每一條 server 路徑、事件名稱與回應形狀都變了，啟動偵測與認證也不相容；直接換版，本機模式每個回合都會卡到逾時，server 模式的 compose healthcheck 卻因為 SPA 回 200 而顯示 healthy（步驟 4）。
- 資料單向：V2 就地遷移三個入口共用的 opencode.db，刪掉 V1 的三張表，遷移後 V1 與 V2 各寫各的表，回復只能還原升級前的備份（步驟 4）。
- 沒有效能理由：瓶頸在上游模型，換 OpenCode 版本不會改變它；V2 本身是否較快沒有量測（步驟 5）。
- 不急：V1 仍在維護，1.19.6 於 2026-11-17 發布，是 GitHub 的 Latest。release CI 解析的是 opencode-ai 套件，V2 改名為 @opencode/cli，所以發版不會自動升到 V2；opencode-ai 目前沒有 2.x 版本（`npm view opencode-ai versions`，2026-11-23）。

將來要升級時的前提（另開 issue 定範圍）：

1. adapter 改寫並補回歸測試：/api 路徑與 data 包裝、cursor 分頁（列表要走完，避免重演 W-6）、事件名稱、prompt 一律非同步、interrupt、compact、permission 與 form 回覆、啟動訊息 `server listening on`、本機模式自設密碼並帶 basic auth。
2. 健康檢查改打 GET /api/info 並帶認證：gateway 的 health_check 改路徑；compose healthcheck（`curl -fsS`，只看狀態碼）也要改，否則 SPA 回 200 會讓它顯示 healthy。
3. 封裝改用 @opencode/cli 與平台子套件，同步改 release/package.json、scripts/resolve-opencode-version.sh、scripts/test-release-image.sh 的釘選與 integrity。
4. 三個入口同時換版，換版前備份 runwukong_opencode-state 的 opencode.db。
5. 回復方案：停三個入口，還原備份的 opencode.db，映像退回前一版；接受備份之後的對話紀錄遺失。

