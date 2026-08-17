# WuKong Runtime 資源與穩定性 Handover

日期：2026-08-16

部署目錄：~/Documents/RunWuKong

執行映像：ghcr.io/raybird/wukong:v0.21.5

OpenCode：1.18.18

## 摘要

本次檢查只讀取執行中容器、cgroup、OpenCode API、日誌與原始碼，沒有重啟服務、
修改設定或刪除資料。

目前沒有可直接套用的版本升級：執行映像 v0.21.5 與本地 Wukong checkout
的 tag/HEAD 一致，CHANGELOG.md 的 Unreleased 目前沒有待發布項目。現階段
最有效的資源改善應從 runtime lifecycle 與連線清理著手。

主要結論：

1. wukong-opencode-server 是 WuKong 的資源瓶頸，cgroup 目前約 1.88 GB / 2 GB，
   歷史 peak 約 2.00 GB，但尚未 OOM kill。
2. idle restart 最近持續因 4096 有 established connection 而跳過；該連線是
   wukong-schedulerd，而最新 session 已閒置約 79 分鐘，疑似長駐 HTTP/SSE 或
   keep-alive 連線阻止回收。
3. OpenCode 日誌出現 MaxListenersExceededWarning，需要確認是否有 listener
   累積造成長時間記憶體上升。
4. Web、Scheduler、Telegram 三個 Rust service 合計約數十 MiB，並非目前主要
   佔用來源。

## 目前執行狀態

| 項目 | 觀察值 |
|---|---:|
| 容器健康狀態 | 全部 healthy |
| Restart / OOMKilled | 0 / false |
| wukong-opencode-server Docker memory | 約 1.09 GiB |
| wukong-opencode-server cgroup current | 約 1.88 GB |
| wukong-opencode-server cgroup peak | 約 2.00 GB |
| cgroup memory limit | 2 GiB |
| cgroup memory.events max | 11046 |
| cgroup OOM / OOM kill | 0 / 0 |
| wukong-opencode-server PIDs | 17 |
| wukong-opencode-server CPU snapshot | 約 4.0–4.4% |

Docker memory 與 cgroup memory 的差異主要來自 cache accounting。cgroup 拆解約為
anonymous 709 MiB、file cache 937 MiB、kernel 147 MB；file cache 可回收，
但 anonymous memory 已足以讓 2 GiB 上限成為實際風險。

Compose 目前已對 OpenCode 設定 1.5 CPU / 2 GiB / 256 PIDs。在確認 cleanup
修正前，不建議直接把 memory limit 再調低；應先讓服務能正常回收，再以實測決定。

## 最近運行與異常

- 最近排程大多成功，但日誌至少有一次 OpenCode send_message timeout。
- idle-restart 最近一輪摘要顯示 connection_skips=87、db_skips=0、
  session_skips=0、signals=0。
- 4096 目前只看到 schedulerd → opencode-server 的 established connection；
  沒有證據顯示當下仍有活躍 agent turn。
- OpenCode 日誌出現 MaxListenersExceededWarning。
- Telegram 日誌有外部 HTTP 錯誤；日誌曾包含未遮罩的 bot URL/token，應立即輪替
  token，並避免將完整 URL 寫入錯誤日誌。
- OPENCODE_SERVER_PASSWORD 未設定，server 目前沒有認證保護。這是安全問題，
  不是資源問題，但應在後續處理。

## 建議處理順序

### P0：修正 idle-restart 的錯誤阻擋

相關位置：

- scripts/opencode-idle-restart.sh
- crates/wukong-gateway/src/... 的 OpencodeServerBackend event/stream client
  路徑
- docker-compose.yml 的 restart 環境變數

目前判斷邏輯把「任何 established connection」當成不可重啟。建議改為：

1. 以實際 active job、active session 或 scheduler run marker 作為主要條件。
2. 確認 SSE response 在 session idle/結束後確實 drop。
3. 對 scheduler 的長連線設定明確 idle timeout，或讓 client 在 idle 後重新建立。
4. 保留「有活躍工作時禁止重啟」的安全閘門，避免為了省記憶體中斷正在執行的 turn。

驗收條件：

- 沒有 active session 時，即使 scheduler 保有 TCP connection，也能在 restart window
  正常回收 OpenCode。
- 有 active turn 時不會被 idle-restart 中斷。
- restart 後 cgroup memory 回到可解釋的 baseline。
- connection_skips 不再無限累積。

### P1：調查 listener 累積

針對 MaxListenersExceededWarning：

- 記錄 listener 數量與事件名稱的成長趨勢。
- 對 event stream、session subscription、health/event hook 逐一確認註冊與解除是否
  成對。
- 不要只提高 setMaxListeners；那只能隱藏症狀。
- 以一個完整排程週期與一次 idle-restart 觀察 anonymous memory 是否回落。

### P1：把 cgroup 指標納入長期監控

目前只看 docker stats 不足以解釋 2 GiB 上限。建議監控：

- memory.current
- memory.peak
- memory.events 的 high、max、oom_kill
- memory.stat 的 anon、file、inactive_file
- cpu.stat 的 throttling
- pids.current

### P2：磁碟與映像整理

RunWuKong 約 5.9 GiB，其中 workspace/projects 約 5.7 GiB。Docker 仍保留
多個未使用的舊版 Wukong images，可回收數 GiB；刪除前需先確認 rollback 與
.wukong-backups 需求，不能直接執行廣泛 prune。

## 後續驗證命令

    cd ~/Documents/RunWuKong
    docker compose ps
    docker stats --no-stream
    docker logs --since 24h wukong-opencode-server
    docker logs --since 24h wukong-schedulerd
    curl -fsS http://127.0.0.1:4096/session?limit=100

每次調整後至少觀察一個完整 restart window 與一個排程週期，並記錄：

- OpenCode cgroup current/peak
- idle-restart skip/signal 統計
- active session 數量
- timeout 數量
- MaxListenersExceededWarning 是否再次出現

## 追補：2026-08-17 回覆失敗調查

本次追查仍為唯讀，沒有重啟容器、修改設定或刪除資料。

### 直接根因

OpenCode 持久化 log 在 `2026-08-16T20:00Z` 左右反覆出現 provider HTTP 429：
`Rate limit exceeded`。受影響 model 包含 `big-pickle` 與 `deepseek-v4-flash-free`。

實際流程是：

    provider rate limit
      -> OpenCode 內部反覆重試/等待
      -> gateway 收不到有效 event
      -> 20 分鐘 agent timeout
      -> scheduler 回報 stream timeout
      -> Telegram 仍成功送出錯誤結果

最新失敗 job 在 `2026-08-16T20:22:57Z`（台灣時間 2026-08-17 04:22:57）被記錄，
但後續仍有 `result delivered to telegram`，所以這次不是 Telegram 回覆傳輸故障。

目前 `WUKONG_AGENT_TIMEOUT_SECS=1200` 只控制整體等待時間，沒有把 provider 429
快速、明確地傳回排程層。現行 OpenCode 設定已允許 `/tmp`，本次沒有看到 permission
request；因此先前的 permission hang 不是本次主要根因。

### 資源與恢復面的補充

2026-08-17 早上的快照仍顯示所有容器無 restart/OOM；OpenCode cgroup current 約
1.0 GiB、歷史 peak 約 2.0 GiB，`oom`/`oom_kill` 仍為 0。主機上的
`jy-analysis-windows` QEMU container 約使用 6 GiB、CPU 約 119%，且沒有資源上限，
會增加主機延遲與 CPU contention，但不能解釋 provider HTTP 429。

idle-restart 仍因 scheduler 保持 established connection 而跳過；這是故障後的恢復
阻塞點，不是本次 provider rate limit 的直接原因。後續應以 active session/job marker
判斷是否能重啟，不應把任何 TCP connection 視為活躍工作。

### 後續處置優先順序

1. 為 OpenCode provider 加入可觀測的 429 原始錯誤、`retry-after` 與 bounded backoff，
   並準備 model/provider fallback 或降低同帳號併發。
2. 讓 gateway 在 provider 429 時快速結束該 job，避免等滿 20 分鐘才產生模糊的
   `no events` timeout。
3. 修正 idle-restart 的 active-work 判斷與 connection cleanup。
4. 以一次受控測試確認 quota reset 後的成功率，再恢復全部排程。

---

## 追補：2026-08-17 中午的落地驗證與更正

把上面每一條宣稱拿去問產物（`docker ps` / `docker inspect` / cgroup / 持久化 log）
之後的結果。已修的部分見 CHANGELOG `[Unreleased]`。

### 三處事實更正

| 原文宣稱 | 實測 |
|---|---|
| 執行映像 `v0.21.5` | **`v0.21.2`**（四個容器皆是） |
| cgroup memory limit `2 GiB` | **3 GiB**（`docker-compose.memoria.yml` 的 overlay 自 08-13 起生效） |
| 「目前沒有可直接套用的版本升級」 | **落後三個 release** |

三者同源：讀了描述檔（本地 checkout 的 git tag、base compose）而不是問產物。第三項
是本文頭條結論，而漏掉的 v0.21.3 正是把 image 從 2.15 GB 縮到 1.26 GB 的那一版——
一份資源 handover 判定沒有可用升級，漏掉的偏偏是唯一直接減資源的版本。

另有一組內部矛盾：`memory.events max = 11046` 只有在使用量真的頂到上限時才累加，
若上限一直是 3 GiB 而 peak 只有 2.00 GB，該值應為 0。兩個數字至少有一個錯，而舊
容器已不存在，現在無法判別是哪一個。

當前實測（容器 08-17 09:29 重建，restarts=0，僅 3 小時）：`memory.current` 312 MB、
`memory.peak` 426 MB、`memory.events` 全 0。這**不推翻**記憶體長期成長的觀察，只是
現在的容器還太年輕，證不了也駁不了。

### 429 的證據不在本文引用的來源裡

追補段落寫「OpenCode 持久化 log 在 2026-08-16T20:00Z 左右反覆出現 provider HTTP
429」。該檔（`~/.local/share/opencode/log/opencode.log`，涵蓋 08-13 至 08-17T01:29Z，
完整包住事故時段）實際內容是：

- ERROR 共 128 行，**訊息只有一種**：`Failed to fetch models.dev`
  （`GET https://models.opencode.ai/api.json` 連不上）
- `429`、`rate limit`、`quota`、`too many requests`、`big-pickle`、`deepseek` 的出現
  次數皆為 **0**

429 可能出現在 `docker logs`（隨 09:29 重建被清空），但本文明確把它歸給持久化 log，
而那裡沒有。**目前既無法證實也無法否證 429 是根因**，倒是「連不到 models.dev」有
128 次實證，值得單獨追。

因此原「後續處置優先順序」第 1、2 項（針對 429 的 backoff 與快速失敗）**前提未定**，
不宜先做。要定案只需一行證據：當時 gateway 吐的是 `no events arrived for this
session` 還是 `events seen: N, last X`——前者代表 SSE 根本沒東西，補 `session.error`
無用；後者代表事件有到但被丟掉。`event_map.rs` 目前只認 `session.idle`、
`session.status`、`permission.asked`、`message.part.updated` 四種，確實沒有
`session.error`。

### 本文沒抓到、但更該修的一項

`WUKONG_TG_TOKEN` 只傳給 `wukong-telegram`，**沒有傳給 `wukong-schedulerd`**（兩份
compose 皆然）。`build_notifier()` 因此靜靜停用通知：job 照跑、結果照產生，但永遠送
不出去，唯一訊號是一行啟動日誌。本文追補寫「Telegram 仍成功送出錯誤結果」，但當前
容器的啟動日誌是
`🐵 scheduler 通知停用：未設定 Telegram token`，且 `/workspace/.wukong/` 下沒有
`settings.toml` 可以提供替代來源。

### 安全項的優先序應該提前

本文把 token 洩漏與 `OPENCODE_SERVER_PASSWORD` 未設定列在「最近運行與異常」的 bullet
裡，P0 給了記憶體回收。但 token 洩漏是**結構性**的，不是「日誌曾經有」：

`TgError::Http` 直接包 `reqwest::Error`，而後者的 Display／Debug 都會印出完整 URL，
URL 裡就是 `/bot<token>/`。`log_send`（12 處）與 schedulerd 的 delivery 警告每一次
網路層失敗都會寫出來。實測（reqwest 0.12.28）確認會洩漏。**已修**——改在型別內遮蔽；
但已經進日誌的 token 收不回來，**必須輪替**。
