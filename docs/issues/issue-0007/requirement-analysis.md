# CLI／Server 行為基線

盤點日期：2026-10-03。基線：`origin/main` 的 `b986493`。本文件描述實作，不把尚未執行的路徑寫成驗證通過。

## 需求與目標

Wukong 持有 scope、memory、orchestration 與 lifecycle；OpenCode 為 execution adapter。驗證 CLI 對必要入口可用才切部署預設，server 保留。來源見 README。

## 現況

| 契約 | CLI | Server | 呼叫依賴與驗證 |
|---|---|---|---|
| backend 選擇 | URL 未設或留空時使用 CLI | 非空 `WUKONG_AGENT_SERVER_URL` 選 server | `backend::build_backend_from_env`；Web／Telegram／schedulerd main 均使用它 |
| session | `assemble_argv` 加 `-s`；串流從 JSON 捕獲 sessionID；plain `run` 回 None | 建立／續接 session，失效時有重建路徑 | runtime 末棒一律 `run_streaming`，同 scope 讀寫記憶庫；CLI plain 的 None 不能直接推論入口無法續接 |
| streaming | `--format json`；解析 text、reasoning、tool_use、step_start／finish | SSE 映射 delta、tools、question、permission | `stream::parse_event` 不解析 question；Web callback → mpsc → SSE |
| tool override | `AgentRequest.tool_overrides` 未傳給 CLI | 作為 request tools 欄位 | router 關閉 question 的要求只在 server 生效；CLI `--agent plan` 已傳遞 |
| permission／question | stdin=null；無 reply endpoint | `AgentBackend::answer_question`／`cancel_question` 呼叫對應 HTTP endpoint | Web／Telegram 使用 responder；Scheduler 預設 Reject，AllowOnce 為 env opt-in，但只對收到的 QuestionRequest 生效 |
| final output | runtime directive／空輸出修復／fallback 共用 | 同左 | backend 換模式不改 runtime；仍須入口驗證 |
| memory | runtime recall／remember 共用 | 同左 | session 捕獲是否有效需跨回合驗證 |
| failure | 非零 exit 與結構化 upstream error 為 Err | HTTP／SSE error 與 timeout，呼叫 abort | CLI plain stderr 另有分類；串流需確認程序收尾 |
| timeout | 讀 stdout 時有 deadline；EOF 後 `child.wait()` 沒有 deadline | SSE deadline 與失敗時 abort | CLI 有疑似等待漏洞，需重現；不能只看既有「仍輸出／pipe 未關」timeout 測試 |
| cancellation | 子程序未設 kill_on_drop | 顯式 stream 失敗呼叫 abort | drop future 的 child 存活需探測；不把 Web 斷線等同取消（Web 執行在獨立 thread） |
| cleanup／compact | delete_session 預設 no-op；list 不支援；compact 預設送 `/compact` prompt | session API 與 summarize | 不在本 issue 重構；CLI compact 是否等價仍需盤點 |
| Docker lifecycle | CLI profile 已可使用本地 process | 三服務預設 URL、depends_on server | 兩份 compose 皆然；共享設定 baseline 每次 entrypoint 覆寫、user 層保留 |

## 目前可執行探測

- 2026-10-03 `opencode --version`：exit 0，`1.18.31`。
- 2026-10-03 `opencode run --help`：exit 0，列出 `--format json`、`--session`、`--auto`（自動允許非 deny）；未列出 compose 使用的 `--dangerously-skip-permissions`。未列出不代表不支援，另以隔離設定實際探測。
- GitNexus 初讀 index 落後 67 commits；`npx gitnexus analyze --skip-agents-md` exit 0，成功重建且未建立 CLAUDE.md。切到主幹後再修改 symbol 前須核對影響分析。

## 問題與未知

1. question／permission 互動不等價，切預設前需 TBD-1 決策。
2. 串流 EOF 後等待 child 的 deadline 與取消清理缺口需重現；屬 issue 已要求的 failure／timeout 範圍。
3. 真實 OpenCode 旗標、stream 形狀與兩回合續接不能以 fixture 代替。
4. 容器 CLI 在 Web／Telegram／Scheduler 本身執行重活，既有 service 資源限制與設定共享需要檢查，不能沿用「重活都在 server」的文件宣稱。

- 2026-10-03 隔離 XDG 目錄實際執行 `opencode run --dangerously-skip-permissions --format json "reply only probe"`（model 設為 `missing/nope`）：exit 1，收到帶 sessionID 的結構化 error。證明該旗標未阻止啟動；因無效模型未執行工具，不構成權限成功證據。
- 2026-10-03 `cargo test -p wukong-gateway -p wukong-runtime -p wukong-scheduler`：exit 0，125／69／27 項測試通過（合計 221）。其中 `cli_backend_cannot_answer_questions` 明確驗證 CLI responder 不支援回覆；不是四入口 CLI 等價驗證。

## 實測結論（2026-10-03）

真實 OpenCode 1.18.31 的 permission 對照：明確 deny 時 bash 不在可用工具中、檔案副作用不存在；ask 加既有部署旗標時工具成功且檔案內容為 `CLI_PERMISSION_PROBE`；ask 不帶旗標時 tool state 為 error、stderr 明確 auto-reject、沒有檔案且正常退出。三者都有 exit 0，故不能只用 exit code 判定工具權限成功。可重跑的探針與四入口證據見 implementation-plan.md。

程式盤點發現的 EOF deadline、drop child 與 CLI 串流入口丟棄 fallback 已依 Task 1.2／1.3 重現並最小修補。CLI question 回覆與 server 工具／session 管理差異保留，沒有藉本 issue 重設 adapter。

## 互動要求與控制通道查核（2026-10-03）

使用者明確要求「Cli 也要支援互動問答才行」，因此非互動降級不是交付方案。以下查核固定 OpenCode `v1.18.31` 官方原始碼，尚未取得新路徑的真實問答證據。

- [run.ts](https://github.com/anomalyco/opencode/blob/v1.18.31/packages/opencode/src/cli/cmd/run.ts)：非互動 session 建立時加入 question deny；stdin 讀至 EOF 後才執行。`--interactive` 宣告未用於 handler 的互動判定；實際 mini 模式要求 TTY 且不允許 JSON 格式。`--port` 不能據此推論 run 暴露 HTTP 回覆端點，local SDK 使用 in-process fetch。
- [ACP event.ts](https://github.com/anomalyco/opencode/blob/v1.18.31/packages/opencode/src/acp/event.ts)：轉送 message 及 permission 事件，沒有 question 事件分支。ACP stdio 的 requestPermission 不能直接替代多問題／多選／自訂文字的 question reply。
- [ACP registry.ts](https://github.com/anomalyco/opencode/blob/v1.18.31/packages/opencode/src/tool/registry.ts)：question 工具的預設啟用 client 清單未包含 acp；另有旗標可啟用，但不能因此宣稱已有 reply 通道。
- [ACP command](https://github.com/anomalyco/opencode/blob/v1.18.31/packages/opencode/src/cli/cmd/acp.ts)：啟動 HTTP listener 並建立 SDK，再處理 stdio ACP；故它也不是完全沒有本機 HTTP 的程序。尚未實測其 HTTP 問答與生命週期，不採用名稱作為等價證據。

2026-10-03 已提出兩種有實質維護差異的方向：每回合管理本機控制程序，或維護純 run 的 OpenCode 修改版。前者避免常駐獨立服務，但仍使用本機控制 API；後者要修改第三方協定與出貨版本。執行路徑決策待 README 的 TBD-2，不先更動 Gateway 或部署。

## 執行路徑決策（2026-10-04）

2026-10-03 使用者指出常駐 server 佔用資源，接受每次執行啟動本機控制程序；TBD-1／2 已解決，核准規格提交為 `335f987`。按需 `serve` 的官方 question API、續接與程序回收已由真實 OpenCode 1.18.31 探針取得，不使用 fork 或 ACP 補造協定。詳細比較與限制見 [technical-analysis.md](technical-analysis.md)，完成證據以 implementation-plan.md 為準；上方「待決策／尚未實作」描述保留為前期查核紀錄。
