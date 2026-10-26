# 審查報告

- 日期：2026-10-26
- 範圍：完整 PR #25，TARGET=`main`
- Reviewed BASE SHA：`ff6eb18785f83bee2af10bb022c20a1c16d79a25`
- Reviewed HEAD SHA：`9f9b3a02da8eb7950e95026efbce1c8fb28be0a6`
- Reviewed patch-id：`d41cf48504afc90cb9f5fd0eb69a54ac3edde4a7`
- 獨立 reviewer：Codex 隔離 subagent `/root/review_9f9b3a0`，非原實作者；主代理實跑測試、交叉核對並保存報告
- Review artifact：`docs/issues/issue-0024/review-9f9b3a0.md`
- 2026-10-26 以 `gh pr view 25 --json headRefOid,baseRefOid` 確認遠端與本機範圍一致，收尾複查未變更。無未提交程式差異。

## 問題與風險

### MUST FIX — P2：文件提供不存在的模型設定命令

位置：`docs/docker.md:276`。

新增的 `wukong models set <model>` 並非現有 CLI 子命令。`crates/wukong-gateway/src/cli.rs:54` 的 `Command` 僅包含 Memory、Schedule、Opencode，其餘字詞由 `Cli.prompt` 收集，`wukong-cli/src/main.rs` 會將其當成真實 prompt 執行。因此依照文件操作會跑一個回合，無法更新 `agent.default_model` 或切離額度耗盡的模型。

既有入口是進入 Wukong REPL 後使用 `/set_models <model>`；`crates/wukong-cli/src/command.rs:120–130` 會將值保存到 `agent.default_model`。Web `/settings` 路由確實存在，可保留。建議更正文案為明確的 REPL 操作，無須新增 CLI 功能；本次未在正式部署執行錯誤命令。

### 前次發現覆核

- P1 子代理可恢復錯誤誤殺：已修復。`is_auxiliary_stream_error` 新增 `mode=subagent` 過濾；純單元與程序整合測試確認子代理錯誤不阻止主要回合成功。
- P2 Wukong 模型明確覆寫：說明方向已修正，已解釋 `agent.default_model` 優先於 OpenCode 預設模型，但新增的 CLI 操作有上述錯誤。
- 未見其他可確定功能缺陷。測試中 title／subagent 使用相似 mock server，屬可維護性議題，未見邏輯分歧造成產品錯誤，不列 MUST FIX。

## 已查核維度

- 規格：比對 `3e12253` 與 HEAD 的 issue README，SCN-001／SCN-002 的驗收語意未改，核准欄已回填；SCN-003／TBD-1 保持待核准。未將模型可用性恢復列為已完成。
- SCN-001：上游額度錯誤可由 stderr 擷取並立即失敗，原始原因與 RateLimited 分類保留；scheduler 測試驗證 execution output 的失敗標記與原因。未將該測試冒充正式 run 資料庫寫入驗證。
- SCN-002：正常回覆、title 錯誤、subagent 錯誤及主回合成功有測試；真正無回應仍受既有 deadline 保護。程序取消與啟動失敗的釋放測試通過。
- 錯誤解析：轉義雙引號不截斷、非 ERROR 日誌忽略、額度字詞大小寫分類皆有覆蓋。
- 相關安全與架構：沒有 schema、依賴版本、權限或反向依賴變更；未更改正式部署。檢查 stdout／stderr drain、程序回收與 scheduler 錯誤傳遞，未見新增可確定缺陷。
- 品質：完整 PR diff 含程式、文件與歷史報告均已查核，未見阻塞性過度設計。
- GitNexus compare 列 37 個變更符號／11 檔、零 affected processes；execute impact 未列 caller。此圖譜結果有限，另以原始碼確認影響範圍，不將零流程當成零影響。

## 2026-10-26 本輪測試證據

被測版本為上述固定 HEAD。

| 命令 | 結果 |
|---|---|
| `cargo test -p wukong-gateway -p wukong-scheduler --locked` | exit 0；gateway 137 passed、scheduler 51 passed、cli_backend 1 passed；原有 managed_actual 共 6 ignored |
| `cargo test -p wukong-gateway local_process:: -- --nocapture` | exit 0；9 passed；title／subagent／quota 三種 stderr 實際出現，未出現 sandbox skipping，socket 測試確實執行 |
| `cargo fmt --all -- --check` | exit 0 |

未重跑完整 workspace／clippy，不將先前版本結果列為本輪結果。managed_actual 仍需獨立 OpenCode／模型 fixture，本輪程序測試使用 mock server，不等於正式模型服務驗收。

## 流程判定

RETURN TO execute-task

前次程式 P1 已修復；模型處置文件仍有一項 P2 MUST FIX。報告綁定 9f9b3a0，尚未提交。本次審查不代表 SCN-003 完成或整個 issue 可以結案。
