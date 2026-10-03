# 審查報告

- 審查日期：2026-10-04。
- 範圍：PR #8 完整固定差異，35 個檔案；TARGET `main`。
- Reviewed BASE SHA：`b9864932e4ea40f9c26b248ef6ac6f7ab55b8566`。
- Reviewed HEAD SHA：`31154cb93b0a1de8dcf8153bb42c3da1e47f17ae`
- Reviewed patch-id：`311ac8bf4de54665c6512c7c4fbe41b07bb8e9fe`。
- 獨立 reviewer：Codex 獨立審查 agent `/root/issue7_review`；不是產品實作者，未修改產品、測試或其他文件，未執行 commit。
- Review artifact：`docs/issues/issue-0007/review-31154cb.md`。
- PR：<https://github.com/raybird/Wukong/pull/8>；遠端 source HEAD 一致由協調者提供，本 reviewer 另確認本機 HEAD 與 patch-id。
- 核准規格：`335f98765e17733b83b1f2e4d49358780b106b7b:docs/issues/issue-0007/README.md`。
- 風險：High；涉及權限互動、四入口核心行為與程序回收。GitNexus 的 CRITICAL／24 flows 是影響提示，不作正確性或無風險證據。

## 問題與風險

- MUST FIX：無。沒有發現足以阻擋這個固定版本交付的新增安全、邏輯或範圍錯誤。
- SHOULD FIX：`crates/*/tests/managed_actual.rs` 的 9 項真實契約測試預設 ignored；建議建立明確執行 `python3 docs/issues/issue-0007/probe-managed.py --gateway-tests --all-entrances` 的受控回歸入口，固定 OpenCode 版本。這是證據持久力建議：本次 9 項已另跑全綠，並非把 ignored 當通過，也不構成假綠燈。
- NICE TO HAVE：`crates/wukong-gateway/src/opencode_server.rs:100` 的本機建構覆寫檔案模式與 server workspace，現有附件測試主要直接驗 server adapter。建議新增本機建構的附件路徑對照，確認遠端環境變數不影響本機 file URL，並涵蓋工作區內檔案與工作區外拒絕。目前預設 Docker 的絕對 `/workspace` 可由實作與既有 adapter 測試核對；沒有把未執行的本機真實附件工具回合作為證據。

## 已查核維度

### 規格與範圍

已讀專案 `AGENTS.md`、`docs/AGENTS.md`、`docs/agents/{project,acceptance,verification,review-evidence}.md`、review skill，以及 issue README、需求分析、技術分析、實作計畫與驗證紀錄。完整讀取固定範圍的產品、設定、測試與探針差異；新增 issue 文件另讀全文。Cargo.lock 只新增 gateway 對既有 `libc` 的依賴，與 Unix process group 收尾用途一致，沒有新增套件版本或 provider。

核准版本至交付 HEAD 的 README 差異為實作檔案清單、進度、文件連結與核准 SHA 回填；SCN-001 至 SCN-010 的 Given／When／Then 及核准來源未變。一般 `opencode run` 按次啟動本機官方控制程序，完成後回收，符合使用者對互動問答及避免常駐 server 成本的要求。技術文件明列每次 backend 執行各啟動程序，包括規劃與輔助棒；不冒稱整個 runtime 回合只有一次啟動。

### 驗收與證據

下表證據皆以交付 HEAD 中的 [verification.md](verification.md) 與 [implementation-plan.md](implementation-plan.md) 定位；探針及測試原始碼在同一 HEAD。2026-10-04 reviewer 查核可取得的原始 log，未無理由重跑全量測試。

| 驗收 | 已核對的外部結果與證據 | 結論 |
|---|---|---|
| SCN-001 | requirement-analysis 的六類契約／四入口盤點、固定版本 CLI／ACP／server 比較，原生 run 非互動 deny／ask 對照；互動差異有使用者決策與按需方案 | 通過 |
| SCN-002 | CLI 同 scope 兩回合／不同 scope 對照；真實 Gateway 跨程序 provider 收到歷史，首回合 NO_HISTORY 與續接 HISTORY_PRESENT 不只比較 session ID | 通過 |
| SCN-003 | 真正 CLI binary、Axum router／SSE、Telegram dispatch、Scheduler executor；Web tool 或 question 在 done 前送出，最終答案各有斷言 | 通過所述入口層級 |
| SCN-004 | Scheduler Reject／AllowOnce 各兩回合，marker 存在及內容對照、各回合新的 permission request；兩種策略一般 question 都拒絕；既有 reply 失敗重試及中止 regression | 通過 |
| SCN-005 | CLI sentinel stdout 的真實紅燈與修補、正常答案只出現一次、輸出 directive、Assistant 記憶召回；runtime 既有 fallback／remember／recall 測試 | 通過 |
| SCN-006 | 純 CLI EOF 後 wait deadline／drop 的 PID 紅綠；本機啟動取消、無效 listener、health failure；真實待答 drop 及 Docker 不回答的 deadline，完成後程序與埠皆消失 | 通過已列生命週期 |
| SCN-007 | Compose 展開：預設三入口、空 URL、server profile、init；Docker 新 musl binary 的 5 CLI／2 REPL／timeout，19 程序及 1 埠回收；installer 停止已停用 server 的紅綠與 profile 對照 | 通過；完整 release 部署不在證據內 |
| SCN-008 | 非空 URL 選 Server 的單元對照；Docker 以不可執行 agent command 配合顯式 URL 完成兩回合，獨立共用 server 在回合間持續存活 | 通過既有顯式路徑 |
| SCN-009 | cargo test 618 passed／0 failed／9 ignored；9 項真實 fixture 另跑成功；clippy 0 warning、fmt、shell、Docker static、installer all | 通過 |
| SCN-010 | 真實 CLI 單選／自訂／多問題多選／空白重問／no-stream／cancel／EOF；Web HTTP reply、Telegram callback；並行 session 與交叉 request 對照、drop 後舊 reply 拒絕 | 通過 |

### 重要失敗面

| 輸入或狀態 | 預期、覆蓋與判定 |
|---|---|
| 明確 URL、額外 run 旗標、任意自訂命令 | 非空 URL 優先 Server；一般 run 才 Local；自訂旗標維持 Cli。backend 選擇測試與 Docker 不可執行命令對照吻合。模型旗標沿用既有 Wukong 請求契約，未另造 provider 邏輯。 |
| 問題正在等待，使用者取消、EOF 或不回答 | cancel／EOF 回傳官方 reject；不回答由 turn deadline 結束，stdin reader 不阻止退出。真實 binary 與 Docker 有工具結果／非零逾時退出斷言。 |
| 兩個回合同時等待、session 或 request 不匹配 | pending key 為 session＋request，client 綁定該次 server；錯 session／交叉 request 被拒絕，A／B 各回正確工具結果。PendingTurn drop 移除所屬 client 的 routes。 |
| 啟動尚未回報位址、非法監聽位址、health 失敗 | deadline 包含啟動；只接受 HTTP 的 127.0.0.1 明確埠；錯誤、timeout 與 future drop 都由 Process guard 收尾。三項 Linux PID 測試與探針程序觀察覆蓋。 |
| 正常退出、EOF 後仍活著、取消 future | 純 CLI kill_on_drop 與 child.wait deadline；Local SIGINT、bounded wait、必要強殺及 process group。主機真實 44 tracked processes／2 ports、Docker 19／1 全回收；不是只讀配置或固定 4096。 |
| 無 callback 的規劃呼叫、排程詢問 | plain run 禁用 question，仍收到 permission 時主動 reject；排程沿用既有 permission responder，AllowOnce 沒變全域允許，一般 question 仍拒絕。檔案副作用與兩回合 request 對照驗證。 |
| session 刪除、compact、輔助回合 | Local 重用原生 summarize／delete；已刪 session 不再續接；ephemeral 刪除後不暴露可續接 ID，plain／streaming 紅綠均有證據。runtime session lease／rotation 未改。 |
| 附件與工作區安全 | 本機強制共享模式及入口 workspace；重用既有 canonicalize、工作區邊界與 file URL 建構。全量包含 shared／inline／工作區外拒絕及 runtime forwarding；未修改 upload root 防護。直接 Local 建構覆蓋可依上述非阻擋建議補強。 |
| Docker 升級、停用 profile、Memoria | 停用 profile 不會自動停舊容器有真實對照；installer 僅操作同 project 未啟用 server，保留 .env，active profile 不 stop，rollback 使用同 helper。Memoria anchor 掛到真正執行 agent 的入口，Compose 合併保留原 volumes／environment 並加入 runtime 完成相依。 |
| 保留期 cleanup 與 vacuum 所有權 | Local 沒有 list_sessions 實作，沿用 Unsupported，沒有啟動常駐清理；原顯式專用 Server 路徑保留。沒有擴張 session retention 架構或修改 memory schema。 |

### 安全、架構與跨 Task 品質

沒有新增自動許可政策或變更既有 deny baseline。Local 只監聽 loopback，Compose 不發布其控制埠；標準 OpenCode server credential 先移除，再依 Wukong credential 設定子程序，adapter 使用相同設定來源。真實 fixture 驗證了標準 password 不意外傳入本機程序；自訂 Wukong 認證組合未另測，保留為限制。HTTP loopback 並不新增 OS 使用者隔離，容器與工作區仍是既有安全邊界。

Local adapter 集中 run／compact／delete 的生命週期，重用 server 事件及權限路由；CLI stream／no-stream 共用問答處理。Process、Drain、PendingTurn 各自負責不同資源的 drop，並非同義重複 guard。Memoria 共用 anchor、四入口 fixture 共用，沒有跨 Task 重複已分歧而導致不一致的 MUST FIX，也沒有新增框架、memory schema 或大規模無關重構。依 code-simplify 標準核對，維持公開 AiBackend 契約；沒有足以合理要求額外重構的過度設計。

### 測試真偽與版本

2026-10-04 reviewer 實際執行 `git rev-parse HEAD`、固定 BASE／HEAD 的 `git patch-id --stable`、`git diff --check BASE HEAD`，皆 exit 0，HEAD／patch-id 與本報告一致；開始審查時工作區乾淨。讀取 `/tmp/issue7-cargo-test.log`、`/tmp/issue7-managed-final.log`、`/tmp/issue7-docker-final.log`、`/tmp/issue7-ephemeral-red.log`，確認全量輸出、9 個 ignored 的實際執行、程序觀察摘錄與 ephemeral 的真實紅燈。可持久引用的命令及結果保存在交付 HEAD 的 verification／implementation-plan；`/tmp` 僅作本次交叉查核，不能當永久 artifact。

2026-10-04 協調者另提供相同 source HEAD 的 CI 補充證據：[fmt · clippy · test，run 37149047864](https://github.com/raybird/Wukong/actions/runs/37149047864) 成功。本 reviewer 沒有另取遠端 CI 輸出；此項只補充固定版本的本機證據，不取代真實 OpenCode 問答與程序觀察。

provider fixture 控制模型輸出，實際 OpenCode 仍執行 question／permission 工具、保存 session、接收 reply。期望工具答案、marker 字面值、跨回合 provider user 歷史、PID 與 socket 來自獨立可觀察來源，沒有以重算產品邏輯製造同義反覆。Web live-tool 判準另查 child 完成 marker，不能被 HTTP 200 或收尾文字代替。修正 Unicode／recall fixture 誤測沒有冒作產品紅燈。

### 豁免、待確認與限制

沒有 gate 豁免；TBD-1／2 已有使用者來源並解決。所述限制揭露充分，不算假綠燈：主機 OpenCode 1.18.31、Docker OpenCode 1.18.29；Docker 掛入新 musl binary，hash 已記錄，並非測舊 Wukong；未做外部真實 LLM、外部 Telegram 網路、瀏覽器 UI、真實 cron 等候、完整 release image 重建／全套常駐入口部署或長期壓力。這些未執行項目沒有被用來宣稱通過。

顯式遠端 server 一般 question 的既有 session scoped 路由未改；本機官方 endpoint 證據不延伸為最新 OpenCode 遠端一般 question 相容保證。額外 run 旗標保持純 CLI 且不支援控制問答，既有 .env 必須依 docker.md 遷移。Linux／Docker 的 process group 證據不延伸到其他 OS 或自行 daemonize 至不同 group 的工具。這些邊界與核准範圍一致。

## 流程判定

**PASS**。獨立完整審查已完成，報告已保存，核准 SCN-001 至 SCN-010 與重要失敗面均有相符證據，沒有 MUST FIX。以上 SHOULD FIX／NICE TO HAVE 不阻擋本次交付。

本判定只綁定 Reviewed HEAD。協調者若新增唯一的 report-only 直接後繼提交，須依 `docs/agents/review-evidence.md` 及 review skill 的 `verify-artifact.py` 查核後才能沿用；其他產品、測試、設定或文件變更必須重新固定範圍並審查。本報告不代表已合併。
