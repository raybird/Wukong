# System Freeze and OpenCode Resource Handover

日期：2026-08-08

部署目錄：`~/Documents/RunWuKong`

執行中映像：`ghcr.io/raybird/wukong:v0.19.0`

OpenCode：`1.18.14`（npm 最新版為 `1.18.15`）

關聯文件：

- `docs/2026-08-06-docker-runtime-handover.md`
- `docs/docker.md`
- `docs/superpowers/specs/2026-08-08-opencode-server-residency-remediation-design.md`
  （修復規劃；含 2026-08-08 對照實驗，已推翻本文第 4 點的主要假設——見下方追記）

## 摘要

本次調查的目標是釐清整機凍結、歷史高溫，以及
`wukong-opencode-server` 長期 CPU/記憶體使用量的關係。

結論如下：

1. Wukong 的 Docker Compose 目前沒有任何 CPU、記憶體或 PID cgroup 上限。這是
   明確的風險，應優先加入硬限制；但它是風險緩解措施，尚未被證實是本次凍結的直接
   根因。
2. 凍結前最後一筆監控資料沒有 CPU、記憶體或 I/O 壓力：CPU/GPU 約 63C、load
   2.71、可用記憶體約 23.1 GiB、Swap 使用 1 MiB、I/O PSI 為 0。因此不能將該次
   凍結直接歸因於容器滿載或瞬時過熱。
3. 主機歷史資料多次記錄 CPU/GPU 90-103C。這是獨立且嚴重的長期熱風險，需由
   Docker 限制與硬體散熱/韌體調整共同處理。
4. `opencode-server` 的 14-25% 閒置 CPU 基線是真實的，不是 Docker 統計誤差。
   調查時沒有進行中的 agent turn、沒有 4096 連線、資料庫也沒有持續寫入；其精確
   CPU call stack 無法取得，因主機禁止非特權 `perf` 與 `ptrace`。目前最符合證據的
   解釋是 OpenCode/Bun 的常駐 runtime、heap 維護或其他 idle housekeeping，而不是
   Rust 編譯。
5. OpenCode state 的主要成長來源已確認：100 個 session 產生 356,530 筆 event 與
   123,864 筆 part，使 `opencode.db` 成長到 1.33 GiB。現有 compaction 會縮減對話
   context，卻不會提供歷史 event/session 的 retention policy。

本文件記錄調查結果與交接建議；本次調查沒有修改 Compose、OpenCode 設定、資料庫或
Docker volumes。

## 調查範圍與限制

### 已檢查的資料來源

- `docker compose config`、`docker inspect`、`docker stats`、`docker events`。
- 容器 cgroup v2 的 `cpu.stat`、`memory.stat`、`memory.events`、`pids.current`。
- `sensors`、前一次 boot 的 kernel journal、`last -x`、freeze-monitor metrics/events。
- OpenCode health/session API、OpenCode SQLite metadata、OpenCode runtime/config 狀態。
- Wukong Compose 與 runtime source 中的 session/concurrency 路徑。

### 調查限制

- `kernel.perf_event_paranoid=4`，非特權帳號無法以 `perf` profile `opencode`。
- `strace -p` 被 ptrace 權限拒絕。因此無法從 native stack 精確定位 OpenCode idle CPU
  的函式；不能把它描述成已證實的單一 bug。
- 最後一個 freeze-monitor sample 是 2026-08-07 11:27:49 (+08:00)，而下一次 boot
  是 17:34:54。這段觀測空窗使系統最後失去回應前的狀態不可得。

## 主機與 Docker 現況

### 主機資源

| 項目 | 觀察值 |
|---|---|
| CPU | AMD Ryzen 5 3400G with Radeon Vega Graphics，4 cores / 8 logical CPUs |
| 可用邏輯 CPU | 8 |
| Docker 可見記憶體 | 29.31 GiB |
| CPU governor | `schedutil`，power profile 為 `balanced` |
| CPU frequency boost | `lscpu` 顯示 enabled |
| CPU frequency driver | `acpi-cpufreq` |
| 核心韌體訊息 | `amd_pstate: the _CPC object is not present in SBIOS or ACPI disabled` |
| 根檔案系統 | 468 GiB，已用 395 GiB，剩餘約 50 GiB（89% used） |

根檔案系統 inode 使用僅 17%，不是 inode 耗盡。

### Docker 使用量

| 類型 | 總量 | 可回收 |
|---|---:|---:|
| Images | 9.249 GiB | 5.356 GiB |
| Local volumes | 10.19 GiB | 8.63 GiB |
| Containers | 508.1 MiB | 0 B |

可回收空間合計約 14 GiB。回收可將根檔案系統餘裕增加至約 64 GiB，但不會清除正在執行的
process，也不會直接降低 CPU 溫度。任何 volume prune 前都必須確認沒有需要保留的
OpenCode session、Wukong memory、GitHub/Agent Reach 登入狀態。

### 容器 cgroup 限制

下列 Wukong services 的 Docker HostConfig 都是未限制狀態：

| Service | CPU | Memory | PID |
|---|---:|---:|---:|
| `wukong-opencode-server` | `NanoCpus=0` / quota 0 | 0 | 未設定 |
| `wukong-web` | `NanoCpus=0` / quota 0 | 0 | 未設定 |
| `wukong-telegram` | `NanoCpus=0` / quota 0 | 0 | 未設定 |
| `wukong-schedulerd` | `NanoCpus=0` / quota 0 | 0 | 未設定 |

因此任何 agent 透過 OpenCode 啟動的子程序，都可以使用所有 8 個 logical CPUs、29.31 GiB
記憶體與無限 PID。對應設定位置為 `docker-compose.yml` 與
`docker-compose.release.yml` 的各 service。

## 凍結與溫度調查

### 最後可用的 freeze-monitor sample

最後一筆資料位於 2026-08-07 11:27:49 (+08:00)：

| 指標 | 值 |
|---|---:|
| load 1 / 5 / 15 | 2.71 / 4.17 / 4.17 |
| CPU temperature | 63.375C |
| GPU temperature | 63.0C |
| NVMe temperature | 48.85C |
| 可用記憶體 | 24,204,352 KiB（約 23.1 GiB） |
| Swap 使用量 | 1,024 KiB |
| CPU PSI avg10 | 1.99 |
| Memory PSI avg10 | 0 |
| I/O PSI avg10 | 0 |
| 根檔案系統使用率 | 89% |

這筆樣本不支持下列說法：

- 「凍結當下 CPU 被容器跑滿」。
- 「凍結是由記憶體耗盡、swap thrashing 或 I/O 壓力直接造成」。
- 「凍結當下仍處於 90C 以上高溫」。

`last -x` 將前一個使用者 session 記為 `crash`，目前 boot 起始時間為
2026-08-07 17:34:54 (+08:00)。前一次 boot 的 kernel journal 未發現 OOM kill、panic、
soft lockup、NVMe/EXT4 錯誤或 AMDGPU reset。這不足以排除硬體/電源/韌體問題，僅表示
現有 journal 沒有留下可歸因的核心錯誤。

### 歷史熱風險

`~/.local/state/freeze-monitor/events.log` 曾多次記錄 CPU/GPU 超過 90C，包括
96.1C、99.5C、100.8C 與最高 103.1C。這些高溫不必然與本次 freeze 同時發生，但已
足以構成需要立即處理的長期穩定性風險。

Docker CPU limit 可以降低高負載 agent 對溫度的貢獻，但不能取代以下硬體處理：

- 清理散熱器與進出風路徑，確認風扇曲線與風扇轉速。
- 檢查散熱器壓力與散熱膏狀態。
- 更新主機板 BIOS/UEFI，特別是與 Ryzen power management、`_CPC`、APU 韌體相關的
  版本。
- 視 BIOS 能力降低 PPT/TDC/EDC，或關閉/限制 Core Performance Boost。

## OpenCode Server CPU 調查

### CPU 統計的正確解讀

Docker CPU percentage 以一個 logical CPU 為 100%。在本主機上：

- `20%` 約等於 0.2 個 logical CPU，約為整機 8 threads 總算力的 2.5%。
- `100%` 約等於一個 logical CPU 全滿。
- `200%` 約等於兩個 logical CPUs 同時全滿。

因此長期 14-25% 不等於整台主機 20% 滿載，但它會讓 APU 持續有工作、減少進入低功耗
狀態的機會。agent 工具或編譯工作發生時仍可能短暫超過 100% 並提高溫度。

### 已驗證的閒置基線

為避免診斷命令本身影響容器，以下兩次取樣前均等待 30 秒，期間沒有 `docker exec`：

| 取樣 | CPU | Memory | PIDs |
|---|---:|---:|---:|
| 1 | 24.99% | 782.5 MiB | 21 |
| 2 | 14.40% | 782.4 MiB | 21 |

這確認 14-25% 是真實 idle baseline。先前出現的 158-209% CPU、PID 36-37 峰值包含
本次調查同時進行的 `du`、SQLite 查詢與 `docker exec` 工作，不能拿來歸因為 OpenCode
server 的純閒置負載。

### 排除中的工作負載

觀察時：

- `/session` 回傳 100 個 session，但最近 5 分鐘沒有活躍 session；最近一天只有 1 個
  session 更新。
- 4096 port 當下沒有 TCP 連線。
- `opencode.db` 在 20 秒間檔案大小與 mtime 都未變化。
- process baseline 是 21 PID；主要 `opencode` process 之外可見 Bun pool、HeapHelper、
  HTTP client 與 `notify-rs` inotify/debounce threads。
- OpenCode config 目前沒有 plugin/MCP 設定鍵，且 `watcher.ignore` 已排除
  `node_modules`、`target`、`.git`、`dist`、`build`。

因此持續 CPU 不能歸因於正在執行的 Wukong turn、連線中的 API client、持續 SQLite
寫入，或 Rust `target` 目錄 watcher。

目前可支持的結論是：CPU 來自 OpenCode 1.18.14/Bun 的常駐 runtime 或 idle
housekeeping。這需要升級與受控重啟 A/B 測試來定位；不能在沒有 profiler stack 的情況下
宣稱已找到上游的單一 CPU bug。

### Healthcheck 的次要成本

`opencode-server` 的 healthcheck 目前每 2 秒呼叫一次：

```yaml
test: ["CMD-SHELL", "curl -fsS http://localhost:4096/global/health || exit 1"]
interval: 2s
```

近期 healthcheck 每次約 53-68 ms 完成。它每天會產生約 43,200 次 local request，會增加
喚醒與 PID 波動，但現有證據不足以把 14-25% CPU 全數歸因於這個 healthcheck。將 interval
調整為 30 秒是低風險的降噪措施，不應被視為完整修復。

## OpenCode Memory 與資料庫調查

### Cgroup 記憶體拆解

擷取 `memory.stat` 時：

| 類型 | 大小 |
|---|---:|
| `memory.current` | 945,790,976 bytes（約 902 MiB） |
| anonymous memory | 543,412,224 bytes（約 518 MiB） |
| file cache | 336,678,912 bytes（約 321 MiB） |
| kernel memory | 65,699,840 bytes（約 63 MiB） |
| cgroup OOM events | 0 |

`docker stats` 的 Linux memory 顯示會依 cache 狀態扣除部分 inactive file cache，所以其
約 782 MiB 的數值與 `memory.current` 約 902 MiB 並不矛盾。部分 file cache 也可能在
目錄/資料庫檢查後暫時增加。

這表示 700-800 MiB 並非全部是未受控 heap：其中約三分之一是可回收的檔案快取，但約
518 MiB anonymous memory 仍由長壽命的 OpenCode/Bun process 持有。

### Persistent OpenCode state

`/home/wukong/.local/share/opencode` 使用約 1.4 GiB，其中：

| 項目 | 大小 |
|---|---:|
| `opencode.db` | 1,425,854,464 bytes（約 1.33 GiB） |
| `opencode.db-wal` | 約 25.9 MiB |
| `log/` | 約 41.7 MiB |
| `tool-output/` | 約 2.3 MiB |

SQLite 結構：

| 資料表/索引 | 配置大小 |
|---|---:|
| `event` | 700,407,808 bytes（約 668 MiB） |
| `part` | 478,404,608 bytes（約 456 MiB） |
| event indexes | 約 72 MiB |
| `message` | 約 12 MiB |
| 其他 session/todo/index 資料 | 小於 10 MiB |

資料庫共有：

| 項目 | 數量 |
|---|---:|
| sessions | 100 |
| events | 356,530 |
| parts | 123,864 |
| `message.part.updated.1` events | 246,748（payload 約 465 MiB） |
| `message.updated.1` events | 81,968（payload 約 34.5 MiB） |

`message.part.updated.1` 是逐段串流輸出更新造成的主要歷史資料來源。即使
`tool_output.max_lines=500`、`tool_output.max_bytes=65536`、`snapshot=false`、
`compaction.auto=true`、`compaction.prune=true` 與 `tail_turns=8` 已生效，這些設定仍
不會自動刪除舊 session 的 event history。

`freelist_count=30,996` pages，每 page 4 KiB，表示 DB 內約有 121 MiB 可重用空間。
即使立刻 `VACUUM`，在不刪除舊 history 的前提下也只能回收這部分，無法解決 1.33 GiB
的主要資料量。

### 工作區大小

掛載到 OpenCode 的 `/workspace` 約 5.7 GiB，主要是：

| 路徑 | 大小 |
|---|---:|
| `/workspace/projects/Wukong` | 約 5.56 GiB |
| `/workspace/projects/Wukong/target/debug/deps` | 約 2.84 GiB |
| `/workspace/projects/Wukong/target/debug/incremental` | 約 2.59 GiB |

目前 watcher 已忽略 `**/target/**`，所以這個目錄不是已證實的 idle CPU 根因；但縮小掛載
workspace 或定期移除不需要的 Rust build artifacts，仍可降低首次掃描、備份與檔案快取的
負擔。

## Wukong 端的並行風險

### 現有保護範圍

Wukong 的 session lease 只會保護同一個 scope：

- `crates/wukong-runtime/src/session.rs:72-81`

這可以避免同一 session 的兩個 turn 同時寫入，但不是全域 agent concurrency limit。

### 可同時送入 OpenCode 的入口

- Web chat 對每個 request 建立 dedicated thread：
  `crates/wukong-web/src/chat_api.rs:233-249`。
- Telegram 對每個訊息使用 `spawn_local`：
  `crates/wukong-telegram/src/main.rs:154-168`。
- Telegram 以每個 chat 建立不同 scope，因此不同 chat 可以同時執行。
- Web 可接受 requested scope：`crates/wukong-web/src/lib.rs:265-270`。
- Scheduler 自身會循序處理已 claim 的 jobs：
  `crates/wukong-schedulerd/src/main.rs:181-201`；但它仍可以與 Web、Telegram、CLI
  同時使用同一個 OpenCode server。
- `wukong` CLI profile 預設沒有 `WUKONG_AGENT_SERVER_URL`，可能改走自身容器內的
  `opencode run`，因此也要設資源上限或明確導向共享 server。

若需要「同一時刻只允許一個 agent turn」，不能只在單一 service 放
`tokio::Semaphore`。Web、Telegram、Scheduler 是不同 containers，應使用共享 SQLite
資料庫支援的全域 lease/queue，或在 OpenCode server 前加入可跨程序協調的排程層。

## 已排除與尚未證實的假設

### 已排除或不支持

- Docker images/volumes 是殘留 process 的原因。
- 上次凍結前最後一筆資料是 CPU/RAM/I/O 飽和。
- Wukong 預設 Docker image 在容器內自行編譯 Rust。映像內沒有 `cargo`、`rustc`、
  `gcc` 或 `make`。
- Rust `target` 目錄正被 OpenCode watcher 遞迴監看。現有 watcher ignore 已排除它。
- 觀察期間有持續執行的 agent session 或持續寫入的 OpenCode DB。
- Docker OOM、container crash 或 restart loop 是上次整機凍結的直接證據。

### 尚未證實

- OpenCode 1.18.14 的哪個 runtime call stack 導致 14-25% idle CPU。
- 高溫是否曾直接觸發任一次凍結。
- 主機是否有 BIOS、電源、記憶體、APU 或 kernel driver 層面的問題。
- OpenCode session delete 是否會完整 cascade 清除所有對應 event/part；清理前必須先在
  備份或副本驗證。
- 1.18.15 是否修復此 CPU 行為。版本存在，但未在本次調查中宣稱有相關修復。

## 2026-08-08 追記：對照實驗結果

本文成稿後，在開發機上以**同版本 opencode 1.18.14** 做了對照實驗。量測方式為
`/proc/<pid>/stat` 的 `utime+stime` 差分（真實區間 CPU），而非本文批評過的
`ps -eo %cpu`。開發機 `opencode.db` 為 787 MB / 646 sessions，與主機的
1.33 GiB / 100 sessions 屬同一量級。

| 條件 | idle CPU | RSS | threads |
|---|---:|---:|---:|
| 空目錄啟動 `opencode serve` | 0.5-1.1% | 305 MiB | 14-16 |
| 於含 23 GB `target/` 的 repo 啟動 | 0.6-1.0% | 299 MiB | 11-14 |
| 本文記錄的部署主機 | 14-25% | 782 MiB | 21 |

據此**推翻本文第 4 點的主要假設**（「最符合證據的解釋是 OpenCode/Bun 的常駐
runtime、heap 維護或其他 idle housekeeping」）：若成立，同版本的全新實例應呈現
相同基線，實測僅約 1%。同時排除另外兩項：

- **非 watcher 掃到 build artifact**——刻意在 23 GB `target/`（比主機 workspace 大
  四倍）下啟動，CPU 無變化。本文「排除中的工作負載」一節的判斷因此得到獨立佐證。
- **非 2 秒 healthcheck**——實測純閒置 0.95%、加上 2 秒 healthcheck 為 1.45%，
  server 端僅多約 0.5 個百分點。本文將其列為「低」風險是正確的，現有量化數字。

開發機實例與主機實例的關鍵差異是**執行時間與工作負載**：前者為全新啟動、僅執行
60 秒。累積的跡象存在於本文自己的數字中——RSS 由 305 MiB 成長為 782 MiB、thread
由 11-16 成長為 21，而每 session 事件數由開發機的 87（互動式使用）成長為主機的
3,565（Wukong 回合），相差 40 倍。後者與 `scripts/docker-entrypoint.sh` 記載的
約 430 倍串流寫入放大一致。

修正後的假設是：**idle CPU 來自常駐程序內隨回合累積而不被釋放的狀態，而非
`opencode serve` 的固有成本。** CLI 模式沒有同樣現象，是因為 `opencode run`
每回合退出，累積在結構上不可能發生——差異在程序生命週期，不在效率高低。

### 對本文「建議處理順序」的一項修正

P1 的「升級 1.18.15 後以受控重啟的 idle baseline 進行 A/B 量測」**在設計上無法
產生結論**：升級必然重啟容器，若真正機制是累積，重啟本身就會讓 idle CPU 回落，
因此「版本修好了」與「重啟清掉了累積」會給出相同的觀測結果，且兩者都會在數週後
復發。正確做法是先取得重啟前基線，再於 +0h / +6h / +24h / +72h 連續取樣並對齊
期間執行的回合數。

完整規劃見
`docs/superpowers/specs/2026-08-08-opencode-server-residency-remediation-design.md`。

## 風險評估

| 風險 | 等級 | 說明 |
|---|---|---|
| 歷史 APU 90-103C | 高 | 長期高溫直接威脅整機穩定性，Docker 不是唯一來源。 |
| 無 cgroup 上限 | 高 | 任一 agent/tool 可耗盡 host CPU、記憶體或 PID。 |
| 無全域 agent queue | 高 | 多個入口與 scope 可平行工作，峰值可跨多個 core。 |
| OpenCode event DB 無 retention | 中 | session history 已佔 1.33 GiB，會持續成長。 |
| 14-25% idle OpenCode CPU | 中 | 不會單獨飽和 8 threads，但會持續耗電、升溫並縮小尖峰餘裕。 |
| 2 秒 healthcheck | 低 | 會持續喚醒與產生大量 local request，但非已證實的主因。 |
| 根檔案系統 89% used | 中 | 本次最後 sample 的 I/O PSI 為 0，但可用空間較低會放大未來 log/DB 成長風險。 |

## 建議處理順序

### P0: 保留事故證據並處理散熱

1. 在任何清理前備份 `opencode-state` volume，至少保留 `opencode.db`、`-wal`、`-shm`、
   config、Wukong memory DB 與必要 logs。
2. 不要執行 `docker compose down -v`、`docker volume prune` 或直接刪除
   `opencode.db`。
3. 先檢查實體散熱、風扇、散熱膏與 BIOS/UEFI。對曾達 100C 以上的 APU，這比 Docker
   cleanup 更優先。

### P1: 為 Docker 加硬資源邊界

在 `docker-compose.yml` 與 `docker-compose.release.yml` 對 `opencode-server` 加入：

```yaml
cpus: "1.5"
mem_limit: 2g
mem_reservation: 512m
pids_limit: 256
```

`1.5` 是適合先測試的保守上限；若仍持續高溫可降至 `1.0`，若 agent 回合明顯過慢且溫度
可接受再調至 `2.0`。`mem_limit: 2g` 留下高於目前 0.8-0.9 GiB 使用量的緩衝，避免因
尖峰或 file cache 立即 OOM。

為其他 services 加較小限制，例如 `cpus: "0.5"`、`mem_limit: 768m`、
`pids_limit: 128`；CLI profile 必須同樣設限，或在 server 已啟動時明確使用共享 server。

這類變更會 recreate container，應在沒有進行中 agent turn 的維護窗口執行。部署後以
`docker inspect` 確認 `NanoCpus`、`Memory` 與 `PidsLimit` 不再是 0/空值。

### P1: 降低不必要喚醒並升級 OpenCode

1. 將 `opencode-server` healthcheck `interval` 從 2 秒提高至 30 秒；保留較短
   `start_period` 與合理 retries，避免啟動期間誤判。
2. 將映像內 OpenCode 從 1.18.14 升級到 1.18.15，並以受控重啟後的 idle baseline
   進行 A/B 量測。不可在升級前宣稱 patch 已修復 CPU。
3. 重啟後等待至少 2 分鐘、沒有 user turn 的情況下，連續多次記錄：

```bash
docker stats --no-stream wukong-opencode-server
```

若 idle CPU 從目前 14-25% 顯著下降，表示長壽命 Bun heap/session cache 或版本行為是
重要因素；若沒有下降，應將 profiler 權限調查、OpenCode upstream issue 與硬體層調查
提至更高優先級。

### P1: 實作跨容器 agent concurrency 控制

1. 在 Wukong shared SQLite 加入全域 agent execution lease/queue，預設一次一個 turn。
2. 對 queued/rejected turn 提供明確使用者回饋，避免 Web/Telegram 看起來無回應。
3. 保留 scope session lease，因它解決的是同一 session 一致性，不能取代全域 queue。
4. 對真正包含 Rust toolchain 的環境設定 `CARGO_BUILD_JOBS=2`；此環境不是目前
   Wukong runtime image，但可能是 host 上其他 OpenCode/開發工作負載。

### P2: 建立 OpenCode session retention 與 DB 維護流程

1. 先列出並審核舊 session，保留仍需在 Wukong memory 中續接的 session。
2. 使用 OpenCode 原生命令刪除已確認可移除的 session：

```bash
opencode session delete <sessionID>
```

3. 在 DB backup 已完成、server 停止且沒有其他 client 的維護窗口測試 cleanup 是否
   cascade 清除 event/part。
4. 僅在已刪除 history 後再執行 SQLite `VACUUM`，否則最多只會回收目前約 121 MiB
   freelist，無法大幅縮小資料庫。
5. 將 session 數、`opencode.db` 大小、event/part row count 納入週期性監控，設定明確
   retention 週期與容量閾值。

### P2: 改善 freeze-monitor 歸因能力

現有 script 的 `ps -eo comm=,%cpu=` 顯示的是 process 存活期間的平均 CPU，不是單次
10 秒 interval 的 CPU 使用率。因此 top process 欄位只能作為線索，不能用來定案。

後續監控應記錄：

- container cgroup `cpu.stat` 的時間差、`memory.current`、`memory.stat`、PID count。
- `docker stats --no-stream` 的 CPU、memory、PID、block I/O。
- OpenCode active session count、DB/WAL 大小與新增 event/part 速率。
- CPU/GPU/NVMe 溫度、CPU/memory/I/O PSI、root filesystem 使用率。
- kernel/journal 最後成功寫入時間，以辨識監控本身停止與整機失去回應的時間差。

## 驗證清單

完成後應逐項確認：

- [ ] Docker inspect 顯示 server/CLI 的 CPU、memory、PID 上限已生效。
- [ ] 在無 agent turn 的受控期間，連續 idle CPU 樣本已記錄並與目前 14-25% 基線比較。
- [ ] OpenCode 1.18.15 升級後重複相同量測，沒有把版本差異誤判為硬體改善。
- [ ] healthcheck interval 已降低，service 仍穩定 healthy。
- [ ] Web、Telegram、Scheduler、CLI 無法同時啟動超過設計值的 agent turn。
- [ ] 壓力測試時 CPU throttling 是預期值，主機溫度不再長時間超過安全範圍。
- [ ] OpenCode DB cleanup 前已有可還原 backup，並驗證 session delete 的實際 cascade 行為。
- [ ] DB retention 後 `opencode.db`、event/part row count 與磁碟餘裕已重新量測。
- [ ] 新版 monitor 能將 Docker/host CPU、溫度、PSI、DB 寫入與 session 活動對齊至同一時間線。
