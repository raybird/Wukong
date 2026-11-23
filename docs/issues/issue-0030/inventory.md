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
