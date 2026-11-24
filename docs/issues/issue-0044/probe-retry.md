# Task 2.3 端到端：上游連不到時的中止與恢復

取樣：2026-11-24，主機 flashaim，@opencode/cli 2.3.2，容器用 wukong:v0.25.1 映像、全新的 HOME 與 workspace。Wukong 為本分支建出的 `target/debug/wukong`，記憶庫與設定都放在暫存目錄。只送固定的合成提示詞給預設免費模型。

主機的 glibc（2.43）比映像（2.36）新，Wukong 不能放進容器裡跑，所以兩條路徑都是 Wukong 在主機、V2 在斷網的容器：

- server：V2 `serve` 跑在 `docker network create --internal` 的網路上（容器出不去，主機經 bridge IP 連得到），`WUKONG_AGENT_SERVER_URL=http://<容器 IP>:4096`
- CLI：`WUKONG_AGENT_CMD="<wrapper>/opencode run --dangerously-skip-permissions"`。wrapper 名為 opencode，內容是 `docker run --rm -i --network none … /v2prefix/bin/opencode "$@"`。另有兩點：
  - Wukong 目前傳 `--log-level ERROR`，wrapper 把它改成小寫的 `error`。這個改動屬於 Task 2.6
  - 多帶一個旗標，Wukong 才會選 CLI backend，不會選本機按需模式

## 結果

| 情境 | 結果 | 對照 |
|---|---|---|
| server，斷網，「Reply with exactly the word OK.」 | rc=1，91 秒結束：`上游模型錯誤（上游連不到）：provider.transport: Can't reach opencode.ai. …（重試 7 次、超過 90 秒仍未成功，已中止）` | 改動前 V2 不會自己結束（Task 1.1：60 秒後仍在執行），只能等 agent timeout（預設 20 分鐘）；V1 1.19.6 約 65 秒以 UnknownError 結束 |
| CLI，斷網，同一句 | rc=1，91 秒結束，訊息同上 | 改動前（同日、`run` 尚未改讀事件的建置）400 秒的外部 timeout 到了仍沒有輸出：路由走非串流的 `run`，純文字輸出看不到重試 |
| server，斷網後 30 秒把容器接上 `bridge` 網路（SCN-016） | rc=0，51 秒回覆 `PONG` | 重試期間恢復，回合沒有被中止 |

CLI 那一次結束後殘留 1 個容器：Wukong 結束的是 `docker` client，`--rm` 的容器不會跟著停。實際部署時 Wukong 直接結束 opencode 子程序，不會有這個問題；殘留的容器已在探測後刪除。
