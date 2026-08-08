# OpenCode Server 常駐累積修復規劃

日期：2026-08-08

關聯文件：

- `docs/2026-08-08-system-freeze-opencode-resource-handover.md`（事故與現場調查）
- `docs/2026-08-06-docker-runtime-handover.md`（前一次 runtime 事故）
- `docs/docker.md`

## 背景

交接文件記錄部署主機的 `wukong-opencode-server` 長期維持 14-25% idle CPU、
RSS 約 782 MiB、21 個 PID，並把最可能的解釋歸給「OpenCode/Bun 的常駐 runtime 或
idle housekeeping」，同時聲明因主機禁止非特權 `perf` 與 `ptrace` 而無法證實。

2026-08-08 在開發機上以**同版本 opencode 1.18.14** 做了對照實驗，結論與該假設不符。
量測方式為 `/proc/<pid>/stat` 的 `utime+stime` 差分（真實區間 CPU），而非
`ps -eo %cpu`（後者是程序存活期平均，交接文件已指出該欄位不可用於定案）：

| 條件 | idle CPU | RSS | threads |
|---|---:|---:|---:|
| 空目錄啟動 `opencode serve` | 0.5-1.1% | 305 MiB | 14-16 |
| 於含 23 GB `target/` 的 repo 啟動 | 0.6-1.0% | 299 MiB | 11-14 |
| 部署主機（交接文件記錄值） | 14-25% | 782 MiB | 21 |

開發機的 `opencode.db` 為 787 MB / 646 sessions，與主機的 1.33 GiB / 100 sessions
屬同一量級。由此排除三項既有假設：

1. **非 Bun/opencode 常駐 runtime 的固有成本**——同版本、同量級 DB 的全新實例只有約 1%。
2. **非 watcher 掃到 build artifact**——刻意在 23 GB `target/`（比主機 workspace 的
   5.7 GB 大四倍）下啟動，CPU 無變化。
3. **非 2 秒 healthcheck**——實測純閒置 0.95%、加上 2 秒 healthcheck 為 1.45%，
   server 端僅多約 0.5 個百分點；client 端 100 次呼叫共 0.6 秒牆鐘。合計低於 1%。

開發機實例與主機實例的關鍵差異是**執行時間與工作負載**：前者為全新啟動、僅執行 60 秒；
後者長時間執行並持續處理 Wukong 回合。累積的跡象存在於交接文件自己的數字中：
RSS 由 305 MiB 成長為 782 MiB（2.6 倍）、thread 由 11-16 成長為 21，而每 session
事件數由開發機的 87（互動式使用）成長為主機的 3,565（Wukong 回合），相差 40 倍。

該 40 倍差距與本 repo 已記錄的機制一致。`scripts/docker-entrypoint.sh` 的 baseline
註解載明：opencode 1.18 把每一次串流更新都以**完整快照**寫入持久 event log，實測
單一 session 有約 430 倍寫入放大（91 KB 的最終 part 造成 38 MB 寫入）。Wukong 的
多棒自主回合正是產生這種事件量的工作負載。

因此本規劃採用的假設是：**idle CPU 來自常駐程序內隨回合累積而不被釋放的狀態，
而非 `opencode serve` 本身的固有成本。**

### 為何 CLI 模式沒有同樣現象

`opencode run` 每回合生成一個程序，回合結束即退出，heap、快取、DB handle 與
event loop 狀態全部隨程序消滅，下一回合從零開始——累積在結構上不可能發生。
`opencode serve` 以 `restart: unless-stopped` 常駐，每回合殘留全部留存，idle 成本
成為「上次重啟以來累積工作量」的函數。

兩者的差異是**程序生命週期**，不是效率高低。server 模式並未引入新成本，而是移除了
原本在掩蓋既有成本的週期性重置。server 模式換得的是暖啟動延遲，這個好處是真實的
（`docs/docker.md` 稱其為「低延遲模式」）。

## 目標

1. 以可重複的量測證實或推翻「常駐累積」假設，並取得可比較的基線。
2. 在**保留暖啟動延遲優勢**的前提下，為 server 模式補回 CLI 模式免費獲得的週期性重置。
3. 為所有 service 加上 cgroup 硬邊界，使單一失控回合無法耗盡主機資源。
4. 讓上述變更透過已驗證的 installer 通道送達既有部署，而非停留在維運待辦清單。

## 非目標

- 不定位 opencode 上游的具體 leak 或 CPU call stack。若累積假設成立，週期性重置
  對任何根因皆有效，上游定位可延後並另案處理。
- 不在本規劃內把 Web / Telegram 切換為 CLI backend。互動路徑正是暖啟動有價值的地方。
- 不處理主機散熱、BIOS 與 APU 韌體。交接文件的 P0 判斷正確，那是獨立且更高優先的
  硬體工作，與本規劃並行但不相依。
- 不執行 OpenCode session retention 或 DB VACUUM。見 W6 的重新評估。

## 現況查核

| 項目 | 位置 | 現況 |
|---|---|---|
| server 啟動命令 | `docker-compose.yml:59` | `["opencode", "serve", "--hostname", "0.0.0.0", "--port", "4096"]` |
| server healthcheck | `docker-compose.yml:61-70` | `interval: 2s`、`retries: 30`、`start_period: 2s` |
| release 範本 healthcheck | `docker-compose.release.yml:44-49` | 同上 |
| cgroup 限制 | 兩份 compose | 皆無 `cpus` / `mem_limit` / `pids_limit` |
| installer 檔案擁有權 | `scripts/install.sh:21` | `DOCKER_RELEASE_OWNED=(docker-compose.yml .env.example LICENSE scripts/install.sh)` |
| 使用者覆寫層 | `scripts/test-installer-upgrade.sh:171,180` | `compose.override.yml` 於升級時保留 |
| backend 選擇 | `crates/wukong-gateway/src/backend.rs:189-195` | 僅依 `WUKONG_AGENT_SERVER_URL` 是否存在，per-process |
| CLI session 續接 | `crates/wukong-gateway/src/backend.rs:130-132` | `-s <id>`，與 server 模式讀同一份 `opencode.db` |

部署主機已確認由 `install.sh --mode docker` 管理，因此 `docker-compose.yml` 屬
bundle 擁有、`--upgrade` 會覆寫，W3/W4 的變更可經此通道送達。

## 工作項目

### W1（P0）建立可比較的常駐基線量測

**問題**：交接文件的驗證計畫是「升級到 1.18.15 後 A/B 量測 idle baseline」。
**該實驗在設計上無法產生結論**：升級必然重啟容器，若真正機制是累積，重啟本身就會
讓 idle CPU 回落，因此「版本修好了」與「重啟清掉了累積」會給出相同的觀測結果，
且兩者都會在數週後悄悄復發。

此外現有 freeze-monitor 以 `ps -eo comm=,%cpu=` 取樣，該欄位是程序存活期平均而非
區間使用率，不能用於定案（交接文件已指出，但未更換量測方式）。

**變更**：

1. 以 `/proc/<pid>/stat` 的 `utime+stime` 差分取樣區間 CPU，取代 `ps %cpu`。
2. 在部署主機上，於**下一次重啟前**先記錄一組長時間執行的基線（這是即將被 W2
   破壞的證據，必須先取得）。
3. 重啟後於 +0h、+6h、+24h、+72h 各記錄一次：區間 idle CPU、RSS、thread 數、
   cgroup `memory.current`、期間執行的回合數（由 `scheduler_runs` 與聊天歷史推得）。
4. 取樣期間須無進行中回合，且取樣前 30 秒不得執行 `docker exec`——交接文件記錄過
   158-209% 的峰值其實是被調查用的 `du`、SQLite 查詢與 `docker exec` 污染。

**進度（2026-08-08）**：`scripts/collect-opencode-baseline.sh` 已完成並於開發機
對真實 `opencode serve` 程序驗證通過。要點：

- CPU、RSS、thread 與 cgroup 數值全部由 host 端讀 `/proc` 與 cgroup 檔案取得，
  **不需要 `docker exec`**，因此量測本身零污染。
- 分兩階段：先靜置（預設 30 秒）量 CPU，量完才做 SQLite 與檔案大小等侵入式查詢。
- 輸出為 `key: value` 純文字，多份樣本可直接 `diff`。
- 已含 `memory.events` 的 `oom`／`oom_kill` 與 `cpu.stat` 的 throttling 欄位，
  因此同一支腳本也是 W3 的驗收工具。
- `--pid` 模式供開發測試；該模式下 cgroup 屬呼叫者的 login session 而非單一程序，
  輸出會標註 `cgroup_scope: SHARED` 以免誤讀。

**驗收**：取得一組時間序列，可判定 idle CPU 與 RSS 是否隨累積回合數單調上升。
若成立，W2 為正確且充分的處置；若不成立，則將 profiler 權限調查（見 W7）提前。

**相依**：**必須在 W2 生效前完成基線擷取**，否則週期性重啟會永久抹除可觀測的累積現象。

### W2（P0）為 server 模式補回週期性重置

**問題**：`opencode serve` 常駐且無任何重置機制。CLI 模式因程序生命週期而免費獲得
的「每回合重置」，在 server 模式下完全不存在。

**變更**：於映像內加入輕量 supervisor，使 server 在達到設定的執行時間後**於閒置時**
自行退出，交由既有的 `restart: unless-stopped` 拉起。

採自行退出而非外部排程，理由是它能經由 compose 與映像檔走已驗證的 installer 通道
送達既有部署；相對地 host cron 位於 repo 之外，會重蹈「repo 端修好但部署端拿不到」
的覆轍——`docs/docker.md:24` 記載 v0.18.7 的 CPU guardrail 正是這樣失效的。

**已定案的策略（2026-08-08）**：閒置優先＋離峰時段。只在指定窗口內檢查，且**必須
完全閒置才退出**；窗口內始終不閒置就跳過，等隔天。這條路徑永遠不會中斷進行中的
回合，代價是極端忙碌時可能連續數日不重啟——以目前的使用型態，這個代價可以接受。

實作要點：

- `WUKONG_OPENCODE_RESTART_WINDOW`，預設 `03:00-05:00`（主機時區 +08:00）；
  設為空字串停用整個機制。
- 僅在窗口內輪詢。**不設硬上限、不強制退出**：窗口結束時仍不閒置就放棄本次，
  不留待稍後重試。
- 閒置判定須**三項同時成立**：
  1. opencode `/session` 顯示最近 5 分鐘無活躍 session；
  2. 4096 埠無 TCP 連線；
  3. `opencode.db` 的大小與 mtime 連續 N 分鐘（預設 5）未變化。
  第 3 項是為了涵蓋 compaction 與任何背景寫入——它不需要知道 opencode 內部如何
  運作，就能避免在寫入途中退出。交接文件已驗證閒置時這三項分別為「最近 5 分鐘無
  活躍 session」「無連線」與「20 秒內大小與 mtime 未變」。
- 退出須為 graceful（SIGTERM 後給予寬限期），確保 SQLite WAL 正常 checkpoint。
- 退出與重啟事件須寫入 log，供 W1 的時間序列對齊。

**注意**：重啟期間 server 有數十秒不可用。窗口選在 03:00-05:00 是因為互動流量最低；
若日後有排程任務集中在凌晨，需重新選擇窗口，否則兩者會互相排擠——排程回合讓
server 一直不閒置，重啟就永遠不會發生。

**進度（2026-08-08）**：repo 端已完成，`scripts/opencode-idle-restart.sh` 由
entrypoint 在 `opencode serve` 時以背景 sibling 程序啟動（不是 wrapper，opencode
仍是 PID 1，其訊號處理不受影響）。

實作時量測發現三件事，都改變了設計：

- **必須送 SIGINT，不能送 SIGTERM。** opencode 的 `SigCgt` 是 `0x10002`，只註冊了
  SIGINT 與 SIGCHLD，沒有攔 SIGTERM。而核心對 PID 1 有特殊規則：**只有裝了處理常式
  的訊號才會送達**，採預設動作的一律丟棄。容器內 opencode 正是 PID 1，所以 SIGTERM
  會石沉大海，同 namespace 內連 SIGKILL 都殺不掉它。SIGINT 實測 0.1 秒乾淨退出。
- **這順帶暴露一個既有問題**：compose 原本沒設 `stop_signal`，所以每次
  `docker stop` / `docker compose restart` 送的 SIGTERM 都被忽略，空等 10 秒寬限期
  後被 Docker 從外部 SIGKILL——**現行部署的每一次重啟都不是優雅關閉**，SQLite WAL
  沒有乾淨 checkpoint 的機會。兩份 compose 已補上 `stop_signal: SIGINT`。
- **連線檢查只能數 `ESTABLISHED`（state `01`）。** 起初寫成「非 LISTEN」，但
  healthcheck 與 supervisor 自己的探測結束後會留下 TIME_WAIT（`06`），那樣永遠判不
  出閒置。條件順序也調整為由便宜到昂貴，把會發 HTTP 請求的 `sessions_idle` 放到最後，
  避免探測本身建立的連線干擾前一項判斷。

另外兩個容易踩的坑：

- **`${VAR:-default}` 不能用在「空字串即停用」的開關上**——空值會被當成未設定而套回
  預設，等於停用開關是壞的。compose 與腳本兩層都改用 `${VAR-default}`，並加了測試
  斷言鎖住這件事。
- **窗口是容器本地時間，而容器預設是 UTC。** 不設 `TZ` 的話 03:00-05:00 會落在台灣
  時間上午 11 點，而唯一的症狀是「重啟沒有在我以為的時間發生」——很難察覺。Dockerfile
  因此補裝 `tzdata`（缺它時 `TZ` 會靜默退回 UTC），且**預設值 `TZ=Asia/Taipei` 放在
  compose 而非 `.env`**：compose 是 bundle 擁有、升級時覆寫的檔案，預設放那裡才會隨
  版本送達每一個既有部署；`.env` 是使用者覆寫層，既有部署永遠不會自動獲得那裡的新值。
  這與 v0.19.0 把 opencode baseline 從 seed-if-missing 改為每次覆寫是同一個道理。
  supervisor 啟動時也會把解析後的窗口與時區印進 log，讓設錯的人第一眼就看得到。

**驗收**：

1. 重啟後 idle CPU 回到 W1 記錄的初始基線量級（開發機實測為 1% 上下）。
2. 以人工延長的回合驗證：達到門檻但回合仍在進行時，server 不退出。
3. 重啟後既有 session 仍可續接——`opencode.db` 為持久 volume，session 不隨程序消滅。
4. 連續 7 日觀察，idle CPU 不再單調上升。

第 2 項已在開發機以真實 `opencode serve` 驗證：持有一條 ESTABLISHED 連線時
supervisor 正確跳過且未動到程序；連線關閉後於下一輪判定閒置並送出 SIGINT，
opencode 乾淨退出。停用、格式錯誤、窗口外三條邊界路徑亦已驗證。

部署後確認實際有沒有重啟過：

```bash
docker logs wukong-opencode-server 2>&1 | grep wukong-idle-restart | tail -20
```

每次跳過都會寫明是哪一項條件沒過，因此「從未重啟」與「重啟過但沒效果」可以分辨。

### W3（P1）將 cgroup 硬邊界做成出貨預設

**問題**：四個 service 的 Docker HostConfig 皆為未限制，任一 agent 或工具可用盡
主機全部 8 個 logical CPU、29.31 GiB 記憶體與無限 PID。交接文件將此列為手動維運
動作，但該類動作只保護做過的那一台，新部署與重裝會回到無上限狀態。

**變更**：於 `docker-compose.yml` 與 `docker-compose.release.yml` 直接寫入限制，
使其隨 `install.sh --upgrade` 送達所有 installer 管理的部署。

`opencode-server`：

```yaml
cpus: "1.5"
mem_limit: 2g
mem_reservation: 512m
pids_limit: 256
```

其餘 service 給較小值（例如 `cpus: "0.5"`、`mem_limit: 768m`、`pids_limit: 128`）。
`wukong` CLI profile（`docker-compose.yml:4-5`）同樣需設限——交接文件的限制表只列出
四個執行中 service，照表施工會漏掉它，且該 profile 無 `WUKONG_AGENT_SERVER_URL`，
會在自身容器內另起 `opencode run`。

**注意**：加上 `mem_limit` 會引入一個原本不存在的失敗模式——容器遭 OOM kill 後由
`restart: unless-stopped` 拉起，進行中的回合遺失，Web/Telegram/Scheduler 端表現為
串流中斷。目前 `memory.current` 約 902 MiB（其中約 321 MiB 為可回收 file cache），
2g 留有緩衝，但驗收必須涵蓋此風險。

**進度（2026-08-08）**：repo 端已完成，待隨下一次 `install.sh --upgrade` 送達部署主機。

實作時修正了原提案的兩點：

- **`wukong` CLI profile 不能用「較小值」。** 它沒有 `WUKONG_AGENT_SERVER_URL`，會在
  自己的容器內跑 `opencode run`，因此需要與 `opencode-server` 同級的上限；給
  `cpus 0.5` 會直接把它掐死。
- **上限一律經 env 變數指定**（`WUKONG_OPENCODE_*` / `WUKONG_SVC_*`），不寫死數值。
  因為 `docker-compose.yml` 是 bundle 擁有、升級時會被覆寫，寫死的話使用者唯一的
  調整途徑就是手改該檔，而那個調整會在下次升級無聲消失；`.env` 才是保留的那一層。

實際值：`opencode-server` 與 `wukong` 為 `1.5` / `2g` / `256`（server 另加
`mem_reservation: 512m`）；`wukong-web`、`wukong-telegram`、`wukong-schedulerd`
為 `0.5` / `768m` / `128`。已同步寫入 `.env.example` 與 `docs/docker.md` 的變數表。

**驗收**：

1. `docker inspect` 顯示各 service 的 `NanoCpus`、`Memory`、`PidsLimit` 不再為 0 或空值。
2. 壓力測試期間 CPU throttling 為預期值，且 cgroup `memory.events` 的 `oom` 與
   `oom_kill` 維持為 0。
3. `compose.override.yml` 可覆寫上述值且於 `--upgrade` 後保留。

前兩項可直接用 `scripts/collect-opencode-baseline.sh` 取得（已含
`limit_*`、`cgroup_memory_events_*` 與 `cgroup_cpu_*` 欄位）。第 3 項與 `.env`
的保留行為由 `scripts/test-installer-upgrade.sh` 的 `test_docker` 涵蓋。

### W4（P1）調降 healthcheck 頻率

**問題**：`opencode-server` healthcheck 為 2 秒一次，每日約 43,200 次；每次於容器
cgroup 內生成 shell + curl，該 CPU 會計入 `docker stats` 而被誤讀為 opencode 自身負載。

**變更**：`interval` 由 2s 調整為 30s，兩份 compose 同步修改。

**進度（2026-08-08）**：已完成。實作時發現原提案漏了一件事——`wukong-web`、
`wukong-telegram`、`wukong-schedulerd` 三個 service 都以 `depends_on` 搭配
`service_healthy` 等待 opencode-server，**所以 2 秒間隔其實兼任了啟動排序的角色**；
單純改成 30s 會讓每次 `compose up` 的依賴服務多等最多 30 秒。

改用 `start_interval: 2s` 搭配 `start_period: 60s`：啟動期維持 2 秒快探（依賴服務
照樣立刻解除等待），進入穩態後才放寬到 30s。`retries` 由 30 降為 3、`timeout` 由
2s 提高為 5s，故障偵測時間與原本的 2s×30 相當。`start_interval` 需要 Docker Engine
25.0+（已於 28.1.1 / Compose v2.35.1 驗證）。

**注意**：本項已量化為約 0.5-0.8 個百分點，是**降噪與正確歸因**，不是對 14-25% 的
修復。不得以本項的完成宣稱問題已解決。

**驗收**：service 於調整後仍穩定 healthy；`compose up` 時依賴服務的等待時間沒有
明顯變長；啟動期不因 `retries` 不足而誤判為 unhealthy。

### W5（P2）將現場調查沉澱為可重複執行的診斷產出

**問題**：`docs/2026-08-08-...-handover.md` 全部 441 行的數據皆為人工逐項蒐集，
包含 cgroup `cpu.stat`/`memory.stat`、`docker stats`、opencode session/event/part
筆數、DB 與 WAL 大小、溫度、PSI、rootfs 使用率。代價是下次事故需重複相同人力、
兩次取樣的採集方式不保證一致（「約 1.4 GiB」指目錄、「1.33 GiB」指單檔即為一例），
且資料僅存在於該主機，本 repo 無從驗證。

**變更**：新增診斷蒐集器，輸出帶時戳的結構化快照。CLI 目前僅有 `memory` 與
`schedule` 兩組操作，無診斷子命令。可先以 `scripts/collect-diagnostics.sh` 落地，
待介面穩定再評估收進 `wukong` binary。欄位以交接文件「改善 freeze-monitor 歸因能力」
一節列出的清單為規格，並一律採用 W1 的區間 CPU 量測方式。

**驗收**：於部署主機執行一次即可產出交接文件中的對應表格；兩次執行的輸出可直接 diff。

### W6（P2）重新評估 OpenCode session retention 的優先級

**問題**：交接文件將「event DB 無 retention」列為中風險並規劃清理流程，但依 08-06
（event 354,608 / part 123,314）與 08-08（event 356,530 / part 123,864）兩次取樣，
兩日間僅增加約 1,922 個 event、550 個 part，約每日 960 個 event。若該速率成立，
1.33 GiB 主要是 v0.19.0 護欄生效前的**歷史存量**，而非持續滲漏。

此外交接文件將 DB 成長與 idle CPU 列為兩個獨立風險，但依本規劃的分析，兩者可能是
同一股流量的兩種表現——把 DB 灌大的寫入放大，也是把常駐程序灌大的輸入。

**變更**：不立即執行清理。先由 W5 的診斷產出連續蒐集 event/part 成長率，確認是
存量或流量後再決定。若為存量，retention 降級為一次性清理；若 W2 使 idle CPU 回落
而 DB 成長率不變，則可確認兩者相互獨立。

**驗收**：取得至少兩週的成長率序列，據以判定優先級。任何清理動作前須先完成
`opencode-state` volume 備份，且不得使用 `docker compose down -v`。

### W7（P2）解除 profiling 封鎖（W1 判定不支持累積假設時提前）

**問題**：交接文件宣告因 `kernel.perf_event_paranoid=4` 與 ptrace 遭拒而無法取得
CPU call stack，並據此將主要問題列為「尚未證實」。但兩者皆為主機可調參數
（`kernel.perf_event_paranoid`、`kernel.yama.ptrace_scope`），容器端亦可以
`--cap-add=SYS_PTRACE` 放行。目前等同於把一項可調設定當成不可跨越的邊界。

**變更**：若 W1 顯示 idle CPU 與累積回合數無關，則於受控時段暫時調降上述 sysctl，
對 opencode 取一次 profile 後還原。

**驗收**：取得可歸因到函式層級的 CPU 分佈，或明確記錄嘗試後仍失敗的原因。

## 執行順序與相依

```
W1 基線擷取（重啟前）──┬─→ W2 週期性重置 ──→ W1 後續取樣（+0h/+6h/+24h/+72h）
                      │
                      └─→ W7（僅當 W1 不支持累積假設）

W3 cgroup 限制 ──┐
W4 healthcheck ──┴─→ 同一次 install.sh --upgrade 部署

W5 診斷產出 ──→ W6 retention 優先級判定
```

關鍵相依：

1. **W1 的重啟前基線必須先於 W2 取得**，否則累積現象將被永久抹除。
2. W3 與 W4 同屬 compose 變更，應合併於同一次升級窗口，減少 container recreate 次數。
3. W2 的重啟會使 W3 的 OOM 觀察窗口重置，兩者的驗收數據需標註所屬的執行區間。
4. W3/W4 的部署會 recreate container，須於無進行中回合時執行。

## 測試與驗證

### 原始碼

- W2 的閒置判定與退出邏輯需單元測試涵蓋：達門檻但有進行中回合時不退出、
  閒置後退出、`WUKONG_OPENCODE_MAX_UPTIME=0` 時完全停用。
- `scripts/test-docker-runtime.sh` 增加斷言：兩份 compose 的 `opencode-server`
  皆含 `cpus`/`mem_limit`/`pids_limit`，且 healthcheck `interval` 不為 2s。
- `scripts/test-installer-upgrade.sh` 增加斷言：升級後主機 compose 帶有資源限制，
  且既有 `compose.override.yml` 未被覆寫。

### 部署後

```bash
# 1. 確認資源限制已生效（不應出現 0 或空值）
docker inspect wukong-opencode-server \
  --format '{{.HostConfig.NanoCpus}} {{.HostConfig.Memory}} {{.HostConfig.PidsLimit}}'

# 2. 區間 idle CPU（非 ps %cpu）——取樣前 30 秒不得執行 docker exec
PID=$(docker inspect -f '{{.State.Pid}}' wukong-opencode-server)
C0=$(awk '{print $14+$15}' /proc/$PID/stat); sleep 10
C1=$(awk '{print $14+$15}' /proc/$PID/stat)
awk -v a="$C0" -v b="$C1" -v k="$(getconf CLK_TCK)" \
  'BEGIN{printf "idle CPU: %.2f%% of one core\n", ((b-a)/k)/10*100}'

# 3. OOM 事件應維持為 0
docker exec wukong-opencode-server cat /sys/fs/cgroup/memory.events | grep -E '^oom'

# 4. 確認週期性重置有發生且為 graceful
docker inspect -f '{{.RestartCount}} {{.State.StartedAt}}' wukong-opencode-server
```

## 風險

| 風險 | 等級 | 處置 |
|---|---|---|
| W2 的重啟窗口中斷進行中回合 | 中 | 閒置後才退出；門檻搭配離峰時段 |
| W3 的 `mem_limit` 引入 OOM kill | 中 | 2g 相對現況 902 MiB 留有緩衝；驗收納入 `memory.events` |
| 累積假設不成立，W2 無效 | 中 | W1 先行驗證；不成立則轉 W7 |
| W4 被誤讀為已修復主要問題 | 低 | 已於該項明確標註其量化貢獻上限 |
| 部署主機非 installer 管理 | 低 | 已確認為 `install.sh --mode docker` 管理 |

## 已定案事項（2026-08-08）

| 項目 | 決策 |
|---|---|
| W2 重啟策略 | 閒置優先＋離峰時段；窗口內不閒置就跳過，不強制中斷回合 |
| W2 重啟窗口 | 每日 03:00-05:00（+08:00），依 W1 時間序列再調整 |
| W2 閒置判定 | session 活躍狀態 ＋ TCP 連線數 ＋ `opencode.db` 寫入靜止（三項同時成立） |
| W3 資源上限 | `opencode-server` 為 `cpus 1.5` / `mem_limit 2g` / `pids_limit 256` |
| 實作順序 | W1 先行（已完成腳本），再 W3/W4，最後 W2 |

## 待確認事項

- 重啟窗口 03:00-05:00 是否與既有排程任務衝突。若排程集中於凌晨，會讓 server 在
  窗口內始終不閒置，重啟將永遠不觸發——需查 `scheduler_runs` 的實際執行時段分布。
- 若 W1 證實累積，是否值得同時向上游回報。本規劃不以上游修復為前提。
- 閒置判定第 3 項的靜止時長（初值 5 分鐘）需依實際 compaction 耗時調整；過短會在
  寫入空檔誤判為閒置，過長則在忙碌日永遠湊不齊三項條件。
