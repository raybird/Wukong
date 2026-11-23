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
