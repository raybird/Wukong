# 2026-10-26 排程模型額度事故調查

## 範圍與結論

2026-10-26 對 RunWuKong 正式容器進行唯讀查核，未修改設定、重啟服務或手動觸發排程。直接原因為 `opencode/big-pickle` 免費額度耗盡；Wukong 最終只顯示 1200 秒逾時，錯誤傳遞的具體缺口仍需契約探測。

## 取得的證據

2026-10-26 執行以下唯讀查詢：

```bash
docker ps -a
docker inspect wukong-schedulerd --format '{{json .Config.Cmd}} {{json .State}}'
docker exec wukong-schedulerd wukong schedule list
docker exec wukong-schedulerd wukong schedule runs --limit 35
docker exec wukong-schedulerd tail -c 22000 /home/wukong/.local/share/opencode/log/opencode.log
docker top wukong-schedulerd -eo pid,ppid,etime,args
```

- 映像 `ghcr.io/raybird/wukong:v0.23.0`；OpenCode 日誌版本 `1.19.3`。
- 正式 `/data/settings.json` 的 `agent.default_model` 為 `opencode/big-pickle`。
- run 900～904 全為 failure，規劃中約 1200～1201 秒後回報「本機 OpenCode 回合逾時，已中止程序」。此前 run 898、899 為 success，故並非所有歷史排程都失敗。
- 2026-10-25 08:00、10:30、12:30、14:00 UTC 的 plan 請求日誌明確顯示 `providerID=opencode modelID=big-pickle`，隨即記錄 `AI_APICallError: Free usage limit reached`。
- 2026-10-25 23:40:32.826 UTC（2026-10-26 07:40:32.826 台灣時間），run 905 啟動時段的 plan session `ses_e8a90198affeW1KsZUcWTE7IAO` 也出現相同額度錯誤；查詢時 run 905 仍是 running。
- `WUKONG_AGENT_TIMEOUT_SECS=1200`，`WUKONG_AGENT_SERVER_URL` 空白，`WUKONG_AGENT_CMD=opencode run`。程序清單顯示實際子程序為本機 `opencode serve --hostname 127.0.0.1 --port 0`，與 `local_process.rs` 路徑一致。

日誌位於共用 volume，不能將全部內容一概歸屬 scheduler；上述結論以排程起始時段、plan 請求、模型及錯誤交叉對照。未擷取真實 SSE payload，故尚不能宣稱特定 event parser 已被證明有錯。查核過程曾對該日誌使用 grep，但因含二進位資料只回報 binary matches，未將該結果當成錯誤次數證據。

## 後續追蹤

模型額度恢復／替代模型決策、錯誤契約探測及立即失敗修正統一追蹤於 [Issue #24](https://github.com/raybird/Wukong/issues/24) 與 [本機 issue 文件](issues/issue-0024/README.md)。正式操作尚未授權，具體模型未選定。
