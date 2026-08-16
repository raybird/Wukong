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
