# Issue 7 執行路徑比較

決策整理日期：2026-10-04。核准規格為 `335f987:docs/issues/issue-0007/README.md`，使用者於 2026-10-03 要求 CLI 保留問答，並說明目標是降低常駐 server 的資源佔用。

| 方案 | 問答能力 | 閒置成本與維護 | 決策 |
|---|---|---|---|
| 原生 `opencode run --format json` | 非互動 session 禁用 question，沒有 reply 通道 | 每次退出；可保留任意自訂 CLI 命令 | 保留相容路徑，不能交付互動要求 |
| 每次執行管理本機 `opencode serve` | 重用官方 question／permission API 與現有事件 adapter | 收尾回收程序、client 與待答路由；有啟動成本 | 採用，符合使用者目標 |
| 修改／出貨 OpenCode fork | 需要自行設計 run 雙向協定 | 維護第三方修改與版本出貨 | 不採用 |
| ACP stdio | 固定版本未轉送一般 question，且本身仍建立 HTTP listener | 尚無完整問答契約證據 | 不採用 |

官方固定版本查核見 [requirement-analysis.md](requirement-analysis.md)。真實工具結果與退出證據見 [verification.md](verification.md)；設定文字與假事件不能取代這些證據。

## 邊界與取捨

- 無非空 server URL 且命令為一般 `opencode run` 時選本機 backend；模型旗標仍由 Wukong 解析後送入請求。其餘 run 旗標及任意命令沿用純 CLI，避免丟棄自訂參數。
- 每次 backend 執行各啟動一個控制程序，包含規劃／輔助棒；不是整個 Wukong 回合只啟動一次。`check_ready` 不啟動程序。session 使用既有資料目錄；輔助 session 刪除後不暴露可續接 ID，壓縮走原生 summarize API。
- 只接受子程序回報的 `http://127.0.0.1:<port>`，不發布 Docker host 埠。`--port 0` 使用 OpenCode 的可用埠選擇，測試觀察的埠寫入探針結果，不宣稱必定隨機。
- 回合 deadline 包含啟動、健康檢查及執行；正常收尾送 SIGINT，必要時強制終止。Unix 建獨立 process group，drop 也會終止同組程序；容器使用 init 回收孤兒。工具自行 daemonize 到其他 process group 不在此保證內。
- 待答路由由 session ID、request ID 與該次 server client 綁定；回合結束／drop 移除。CLI 的單一 stdin reader 同時供 REPL 與問題使用；逾時不等待阻塞式 stdin 工作。
- 無互動 callback 的內部呼叫禁用 question，若仍收到權限詢問則拒絕；Scheduler 沿用 Reject／AllowOnce，一般 question 一律拒絕。這不將權限改成全域自動允許。
- 每個入口容器承擔自己的 agent 成本；平行入口的峰值會累加。省下的是閒置控制程序，並非消除執行時的 CPU／記憶體成本。Memoria overlay 因此也掛到各入口。
- 共用 server 仍明確 opt-in。profile 停用不會停止舊容器，即使加 `--remove-orphans` 也相同；installer 只停止同 project 已停用的 server，保留使用者 `.env`。既有額外 CLI 旗標／URL 必須按操作文件遷移。
- 本機模式不做保留期 session 刪除或啟動前 vacuum，避免擴大既有 cleanup 的所有權假設；顯式專用 server 的原清理路徑維持。
