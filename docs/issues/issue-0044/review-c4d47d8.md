# 審查報告
- 範圍：完整交付範圍（TARGET main e6a0bf9；增量覆審）
- Reviewed BASE SHA：e6a0bf921fd12d65e5f95b2418eb2916b847b4d8
- Reviewed HEAD SHA：c4d47d8e892af7ca5bf652bd083c29b7ea03f0df
- Reviewed patch-id：5bfa9cb4ae929180f50c18db9837f3fe74a49b4c
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，claude-opus-5-5）。不是實作者，也沒有參與修正
- Review artifact：docs/issues/issue-0044/review-c4d47d8.md
- 增量覆審：上一份報告 review-0405603.md，增量範圍 0405603..c4d47d8
- 審查日期：2026-11-25
- 規格基線：核准 commit 94aaa40。`git diff 94aaa40 c4d47d8 -- docs/issues/issue-0044/README.md` 的差異只有以下幾項，Gherkin 劇本（SCN-001～SCN-017）沒有改動：
  - 核准 commit 欄位、Phase 狀態符號與 `**狀態**`
  - 新增 `## 現況`（位置在 `## 概述` 之後）
  - 「涉及檔案」補上 `wukong-cli` 與 `opencode-idle-restart.sh`，這兩項是使用者決定修 M1、M2 後的必要範圍
  - `## 待確認事項` 新增 TBD-4、TBD-5
- 風險等級：High（沿用上一份報告：不可逆的 opencode.db 遷移，以及三個入口的核心外部契約）

## 增量覆審的前提

| 條件 | 核對 | 結果 |
|---|---|---|
| 上一份報告綁定 H1、記完整範圍 | review-0405603.md 綁定 0405603，範圍是完整交付，判定 RETURN TO execute-task | 成立 |
| H1 是新 HEAD 的祖先，中間沒有改寫歷史 | `git log 0405603..c4d47d8` 只有 51fac87、c4d47d8，兩個都是一般提交 | 成立 |
| merge-base 沒有變 | `git merge-base main HEAD` = e6a0bf9 = 上一份報告的 BASE | 成立 |
| patch-id | `git diff e6a0bf9 c4d47d8 \| git patch-id --stable` = 5bfa9cb4…，與指定值相同 | 成立 |

## 問題與風險

### MUST FIX
無

### SHOULD FIX
無

### NICE TO HAVE

**N-A. 從 V1 升級後的第一次啟動，兩個 `--standalone` 命令同時執行時，其中一個會失敗**
- 位置：`crates/wukong-web/src/lib.rs:585`。系統診斷用 `tokio::join!` 同時執行 `auth list --standalone` 與 `models --standalone`
- 實測（隔離容器，`--network none`，用 v0.25.1 的 V1 1.19.6 建出含 5 個 session 的 opencode.db，每次用一份新的副本）：
  - 在還沒遷移的 V1 DB 上同時執行兩個命令，3 次都有一個 rc=1，錯誤為 `Standalone server exited before reporting readiness`
  - 已遷移的 DB 同時執行 10 次、全新 HOME 3 次、`serve` 與 `models --standalone` 同時啟動 3 次，全部成功
  - 失敗之後再啟動 V2 serve：5 個 session 都在，`/api/experimental/migration/v1` 為 `completed`，`integrity_check` 為 ok
- 影響：Docker 升級後，如果第一個動作是打開 Web 的系統診斷，Providers 或 Models 其中一項會顯示一次錯誤，重新整理就正常，資料不受影響
  - 修正前 `providers list` 本來就會失敗，所以這不算退步
  - 同樣的競爭也可能發生在 CLI 的 `run --standalone` 與 Web 的診斷之間，條件同樣很窄
- 建議：不必改程式。在 rehearsal.md「觀察」或 README `## 待確認事項` 記一行就好。想消除的話，把系統診斷改成依序執行（每個命令約 1 秒，在 5 秒逾時內）

**N-B. CHANGELOG 兩條新條目寫的是沒有發布過的中間狀態**
- 位置：`CHANGELOG.md:33`、`CHANGELOG.md:37`
- 內容：「原本的命令會直接失敗」「每天都被當成有活動而跳過重啟」，描述的是這個 issue 分支上 V2 的中間狀態。從 v0.25.x（V1）升級的使用者不曾遇到這兩件事
- 建議：改寫成跟上一個 release 比較的差異，例如：
  - `/providers` 改為列出 `auth list` 的輸出，格式不同
  - 閒置重啟問不到 session 列表時，log 會寫出原因

**N-C. 新建 `.env` 的 0600 沒有獨立的斷言**
- 位置：`scripts/install.sh:771`
- 現況：`docker-env-server-password` 只涵蓋改寫 `.env` 的路徑（`os.chmod`）。從 `.env.example` 新建、而且之後沒有被遷移改寫的路徑，沒有權限斷言
- 本次以等價命令實測：umask 0002 下 `(umask 077 && cp .env.example .env)` 產生 600，邏輯正確
- 建議：在新裝的案例補一行 `stat` 斷言

## 已查核維度

### 上一份報告各項的修正確認

| 項目 | 修正 | 本次查核 | 結論 |
|---|---|---|---|
| M1 `OpencodeUtility` 用 V1 子命令、留下常駐 service | `backend.rs` 新增 `PROVIDERS_ARGS`（`auth list`）、`MODELS_ARGS`（`models`）；`run_fixed` 遇到 opencode 時補 `--standalone`。CLI 與 Web 的 4 個呼叫點都改用這兩個常數；`docs/entrypoints.md:55,77-78` 已更新 | 見下方四點 | 已修好 |
| M2 閒置重啟在 V2 永遠不觸發 | `sessions_idle` 改查 `/api/session?limit=1`，經 stdin 帶 basic auth，讀 `data[].time.updated`。看不懂回應時判「忙」，原因寫進 `SESSIONS_BUSY_REASON`。測試的假 server 改成 V2 形狀，並新增兩個反向案例 | 見下方三點 | 已修好 |
| S1 壓縮失敗或被丟掉時不中止 | 新增 `consume_or_interrupt`，`run_streaming` 與 `compact_session` 共用 | 消融：`compact_session` 換回 `consume_event_stream` 後，`a_compaction_retrying_too_long_is_interrupted`（`opencode_server.rs:1979`）與 `dropping_a_compaction_interrupts_the_session`（`:2006`）都失敗。c4d47d8 上兩支都通過。`hold_session_lease`（`wukong-runtime/src/session.rs:220`）不會主動丟掉 work，所以實際上會丟掉壓縮的是外層的取消，行為符合預期 | 已修好 |
| S2 備份與 `.env` 的權限 | 新增 `make_backup_dir`（0700，舊的 0755 也收緊）。`backup_docker_state` 改成 subshell 並設 umask 077，tar 為 0600。新建與改寫 `.env` 時設為 0600 | 見下方兩點 | 已修好（新建 `.env` 的覆蓋見 N-C） |
| S3 free-limit 429 的 161 秒 | 擷取真實樣本，存為 fixture `server-free-limit.ndjson`；新增特徵化測試 `a_real_free_limit_fails_at_once_as_rate_limited`；結論記在 README TBD-5 | 見下方三點 | 已處理。161 秒當時的原因已無法對證，README 有誠實寫明 |
| N1 doc comment 錯置 | 舊說明移回 `abort_session` 上方（`opencode_server.rs:529-530`） | 讀過 diff | 已修好 |
| N2 `declined` 之後任何 interrupted 都放行 | 改成只在 `declined` 而且 `reason == "shutdown"` 時才判 Idle（`event_map.rs:132-137`） | 見下方三點 | 已修好 |
| N3 install.sh 重複的 inline python | 新增 `state_backup_entries`，取代四處 inline python 與兩處串接路徑的寫法 | `grep "python3 -c 'import json" scripts/install.sh`：與 stateBackups 有關的只剩 `state_backup_entries` 本身（:710），以及 `backup_docker_state` 組 JSON 的那一處（:702，用途不同） | 已修好 |
| N4 `forms` 的 `lock().unwrap()` | 三處都改用 `unwrap_or_else(\|p\| p.into_inner())` | `opencode_server.rs` 的 6 個非測試 `lock()`（:144、:367、:378、:432、:517、:815）都已一致 | 已修好 |
| N5 `backupRequired` 的發版提醒 | `AGENTS.md:298` 補上「之後不再遷移 opencode.db 的版本要把 `backupRequired` 改回 false、`affectedState` 清空」 | 讀過 diff | 已修好 |
| SCN-005 `需決策` | 使用者選「規格不動、記入待確認」 | README TBD-4 寫「已解決（2026-11-25 使用者選「記入待確認」）：規格不動，以 gateway 層證據驗收」，並說明入口日後新增取消操作時要另外驗證端到端。與使用者的決定一致；Gherkin 沒有改動 | 一致 |

M1 的查核：
- 消融：拿掉 `--standalone` 之後，`utility_commands_use_v2_subcommands_with_a_private_server` 失敗（`left: "auth list"`、`right: "auth list --standalone"`），c4d47d8 上通過
- 隔離容器實測 `opencode auth list --standalone`：rc=0，1.91 秒，只印出 `Nvidia  API key  stored`
  - 測試用的假憑證放了三種：`api`、`oauth`、`wellknown`，另外加一個環境變數 `OPENROUTER_API_KEY`
  - 假憑證外洩 0 次；環境變數那一筆只顯示變數名稱
- `opencode models --standalone`：rc=0，1.07 秒，輸出仍是 `provider/model` 格式
- 兩個命令結束 3 秒後掃 `/proc`，沒有任何 opencode 程序；兩者同時執行時也一樣

M2 的查核：
- 紅燈：把 0405603 的舊 `opencode-idle-restart.sh` 放在 c4d47d8 的測試底下執行，rc=1。log 一直是「a session was updated within 1s; skipping」，正是上一份報告描述的無聲失效
- 實作計畫 `implementation-plan.md:540` 寫的是「案例二」一直印這行，但本次重跑最先失敗的是案例一（等不到 blocking connection 的 log）。紅燈是真的，只是案例編號寫得不精確
- 真實 V2 實測（隔離容器，`OPENCODE_SERVER_PASSWORD=realpw`，用真的 supervisor 腳本，`TARGET_PID` 指向 serve）：
  - 密碼錯誤：每秒印「the server rejected the password when listing sessions (401); skipping」
  - 沒有密碼：印「OPENCODE_SERVER_PASSWORD is not set…」
  - 密碼正確：先印「opencode.db was written within 1s」，下一輪印「idle for 1s … sending SIGINT」，serve 以 rc=130 結束
  - `/api/session?limit=1` 的回應形狀是 `{"data":[{…,"time":{"created":…,"updated":…}}],"cursor":…}`，與腳本的解析一致
- gateway 自己的 `list_sessions`（`opencode_server.rs:843`）同樣不帶 directory 查 `/api/session`，兩者看到的範圍相同
- compose 會把 `OPENCODE_SERVER_PASSWORD` 交給 opencode-server（`docker-compose.yml:66`、`docker-compose.release.yml:49`）；entrypoint 以 `gosu wukong` 啟動 supervisor，環境變數會繼承下去（`scripts/docker-entrypoint.sh:309`）

S2 的查核：
- 紅燈：把 0405603 的舊 install.sh 放在 c4d47d8 的測試底下執行，三個案例都失敗：
  - `docker-env-server-password`：「.env with the generated password is not 0600」
  - `docker-state-backup`：「the opencode-state archive is not 0600」
  - `docker-state-rollback`：「the set-aside opencode-state archive is not 0600」
- c4d47d8 上 `test-installer-upgrade.sh all` 通過
- 真實 docker：`state-backup-real-docker.sh` 重跑結果：
  - `archive mode=600 volumes dir=700 root=700`，呼叫端的 umask 仍是 0002
  - 還原後 `find -printf` 清單與 sha256 都相同，`integrity ok rows 5000 v2 table 0`
  - 測試 volume 已自動刪除（`docker volume ls --filter name=wk44probe` 沒有輸出）

S3 的查核：
- fixture 的事件序列：`step.started → step.failed（provider.free-limit，status 429，retryAfterMs 56537000）→ execution.failed`，沒有 retry 事件
- `server-retry.ndjson` 兩次 `retry.scheduled` 之間只有 `step.started`，沒有會讓 `RetryWatch.recovered()` 重新起算的事件（`event_map.rs:95-104`，只認 `step.ended` 與 text、reasoning、tool 的輸出）。README 的說法成立
- `a_real_free_limit_fails_at_once_as_rate_limited` 是特徵化測試，修正前也會通過，實作計畫照實寫成「特徵化」，沒有宣稱紅燈
- 擷取腳本會送一則合成提示詞給預設的免費模型，符合使用者 2026-11-25「只送合成提示詞」的決定。本次 reviewer 沒有執行它

N2 的查核：
- 消融：拿掉 `reason == "shutdown"` 之後，`only_the_shutdown_after_a_decline_ends_the_turn_normally` 失敗（`event_map.rs:554`）
- 真實樣本裡，取消提問與拒絕權限之後的 reason 都是 `shutdown`（`server-form-cancel`、`server-permission-reject`），使用者按停止與 Wukong 自己送的 interrupt 是 `user`（`server-interrupt`、`server-retry`）
- `a_cancelled_question_ends_the_turn_like_v1`、`a_rejected_permission_ends_the_turn_like_v1` 在 c4d47d8 與消融版本上都通過

### 增量範圍內每個變動檔案的結論

| 檔案 | 結論 |
|---|---|
| `AGENTS.md` | N5 的提醒正確，與 `release/data-compatibility.json` 的欄位名稱一致 |
| `CHANGELOG.md` | 內容正確，措辭見 N-B |
| `crates/wukong-cli/src/command.rs` | 改用共用常數，沒有其他行為變更 |
| `crates/wukong-gateway/src/backend.rs` | 常數與 `--standalone` 正確。不是 opencode 的執行檔不會補旗標；`run_fixed_kills_the_child_when_the_caller_gives_up` 用 `sh`，不受影響 |
| `crates/wukong-gateway/src/opencode_server.rs` | `consume_or_interrupt` 正確。錯誤路徑的順序跟以前一樣：先 interrupt，再清理自己建立的 session。N1、N4 已處理 |
| `crates/wukong-gateway/src/opencode_server/event_map.rs` | N2 正確，與真實樣本一致 |
| `crates/wukong-gateway/tests/fixtures/opencode-v2/README.md` | 樣本來源記錄完整 |
| `crates/wukong-gateway/tests/fixtures/opencode-v2/server-free-limit.ndjson` | 真實樣本。只有合成提示詞、`/p/ws` 路徑與 opencode 的錯誤訊息，沒有個人或憑證資料 |
| `crates/wukong-web/src/lib.rs` | 改用共用常數。系統診斷平行執行的競爭見 N-A |
| `docs/docker.md` | 與 install.sh 的行為一致，包括舊目錄也會收緊 |
| `docs/entrypoints.md` | 與程式一致 |
| `docs/issues/issue-0044/README.md` | `## 現況` 位置正確；TBD-4 與使用者的決定一致；TBD-5 與證據一致；Gherkin 沒有改動 |
| `docs/issues/issue-0044/implementation-plan.md` | 證據都經本次重跑或消融確認，只有 R2 的案例編號寫得不精確（見 M2 的查核） |
| `docs/issues/issue-0044/rehearsal.md` | 觀察的補充與 TBD-5 一致 |
| `docs/issues/issue-0044/review-0405603.md` | 上一份報告的保存，只新增、沒有改動 |
| `docs/issues/issue-0044/scripts/capture-free-limit.py` | 探測工具，送出的只有合成提示詞；本次沒有執行 |
| `docs/issues/issue-0044/scripts/state-backup-real-docker.sh` | 本次重跑通過，只動自己建立的 volume。檔案模式是 644，要用 `bash` 執行，與用法說明相容 |
| `scripts/install.sh` | 正確，影響見下一節 |
| `scripts/opencode-idle-restart.sh` | 正確，已用真實 V2 驗證 |
| `scripts/test-idle-restart-decision.sh` | 假 server 改成 V2 形狀，反向案例有效（舊腳本會失敗） |
| `scripts/test-installer-upgrade.sh` | 權限斷言有效（舊 install.sh 會失敗）；新建 `.env` 的覆蓋見 N-C |

### 重新查核的項目與理由

- **`OpencodeUtility::run_fixed` 的其他呼叫者**
  - 理由：所有呼叫都會多帶 `--standalone`
  - `grep run_fixed`：非測試的呼叫只有 CLI 的 `/providers`、`/models`，以及 Web 的 `run_opencode_diagnostic`（模型清單、系統診斷），全部只用這兩組參數
  - 參數表只有 `auth list` 與 `models`，沒有 `--version` 這類不吃 `--standalone` 的子命令
  - 使用者的 `WUKONG_AGENT_CMD` 帶 `--server` 時，工具命令改走本機的私有 server。V1 的 `opencode models` 本來就不走 server，行為相同
- **Web 的 5 秒診斷逾時（`lib.rs:540`）與第一次遷移**
  - 理由：修正前 `opencode models` 會起常駐 service，被逾時砍掉的只有 client；現在私有 server 會跟著被砍
  - 實測一：在 V1 DB 上，第一次執行 V2 `models --standalone` 只要 0.94 秒（含 24 個 migration），20 張表變成 29 張
  - 實測二：先執行 `models --standalone`，或在 0.4 秒時對它送 SIGKILL，之後啟動 serve，5 個 session 都在，migration 為 `completed`，integrity ok
  - 大型 DB 的遷移時間沒有量，列為限制
- **`consume_or_interrupt` 改變了 `run_streaming` 的錯誤路徑**
  - 理由：原本內嵌的邏輯被抽出
  - 比對 diff：disarm、abort、清理的順序不變
  - SCN-002、005、007、016 相關測試在 c4d47d8 全部通過：`dropping_the_turn_interrupts_the_session`、`retries_that_last_too_long_give_up_as_unreachable`、`a_retry_that_recovers_in_time_lets_the_turn_finish`、`an_interrupted_execution_reports_the_turn_as_aborted`
  - 本機模式的 `compact_session`（`local_process.rs:168`）失敗時，會在回收 serve 前多送一次 interrupt，屬 best-effort，無害
- **`backup_docker_state` 改成 subshell 函式**
  - 理由：在 subshell 裡設的全域變數不會傳回呼叫端
  - 函式內沒有 `abort`，也不設呼叫端要用的全域變數，只透過 stdout 輸出 JSON
  - 兩個呼叫端：升級（:765）原本就用 `$(…)` 擷取；rollback（:852）只看 rc，不受影響
  - `state_backup_entries` 的輸出依名稱排序；錯誤訊息的路徑原本照插入順序，現在照名稱順序，只影響顯示
  - SCN-013、014、015、017 的 installer 案例與真實 docker 腳本在 c4d47d8 重跑通過
- **event_map 的 declined 判斷對 Telegram、Web、排程取消提問流程的影響**
  - 這三個入口都經 `AgentBackend` 呼叫 server 或本機模式，同樣走 `ServerEventMapper`
  - 重播真實樣本的取消與拒絕測試仍判為正常結束
  - 入口自己的測試（`dispatch::tests::cancel_callback_rejects_and_clears_pending`、`executor::tests::unattended_turn_rejects_permission_by_default` 等）在全套測試中通過
  - CLI backend 用另一套事件對應，不受影響
- **`.env` 改成 0600 對 compose 的影響**
  - 三份 compose 都沒有 `env_file`，也沒有把 `.env` 掛進容器，`.env` 只由執行 `docker compose` 的使用者在 client 端讀
  - installer 以部署擁有者的身分建立或改寫 `.env`，擁有者照常讀得到
  - `test-docker-runtime.sh` 通過（38 個 `WUKONG_*` 變數都接到容器）
- **閒置重啟改動對 compose、entrypoint 的前提**
  - 增量沒有改 compose、entrypoint 與 Dockerfile（`git diff --stat 0405603 c4d47d8`）
  - 真實 V2 收到 SIGINT 後以 130 結束，與 V1 時「PID 1 只收得到有 handler 的 SIGINT」的前提一致；實作者另外以 entrypoint 啟動做了端到端驗證

### 沿用的結論

以下項目的程式與證據在增量中沒有改動，或已確認不受影響，沿用 review-0405603.md 的結論：
- SCN-001、003、004、008、009、010、011、012 的證據與判定
- 演練映像建自 eef1bc5 的演練證據
- 架構邊界：`wukong-runtime`、`wukong-orchestrator`、`wukong-skills` 在 `e6a0bf9..c4d47d8` 仍然沒有改動
- 安全面的其他項目：本機模式的密碼產生方式、healthcheck 經 stdin 傳 header、installer 不印出密碼

這次重新確認的項目：
- SCN-002：M1 的缺口已補上
- SCN-005、006、007、016：受 `consume_or_interrupt` 與 N2 影響，全套測試已重跑
- SCN-013、014、015、017：受 install.sh 影響，已重跑

### 相關失敗面（本輪新增或改變的）

| 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|
| 主機或容器呼叫 `/providers`、`/models`、模型清單、系統診斷 | 命令有效，不連也不啟動 V2 背景 service，不印出密鑰 | argv 單元測試（有消融紅燈）；本次在隔離容器實測 rc、輸出、殘留程序與外洩檢查 | 通過（原本是 M1） |
| V1 DB 第一次被 V2 工具命令開啟，或命令被砍 | 遷移完成，資料完整 | 本次實測：小型 DB，SIGKILL 在 0.4 秒 | 通過；大型 DB 沒有量 |
| 兩個 `--standalone` 同時第一次開啟 V1 DB | 兩者都成功 | 本次實測：其中一個失敗，資料完整 | N-A |
| server 模式在窗口內真的閒置 | 送 SIGINT 讓 compose 重啟 | 假 V2 案例二；本次以真實 V2 實測 | 通過（原本是 M2） |
| session 列表回網頁、401、沒有密碼、非 200 | 判「忙」，log 寫出原因，不印出密碼 | 假 server 的案例四、五（舊腳本會失敗）；本次以真實 V2 實測 401 與沒有密碼 | 通過 |
| 壓縮重試太久、壓縮的 future 被丟掉 | 送 interrupt | 兩支新測試（有消融紅燈） | 通過（原本是 S1） |
| 備份目錄、tar、rollback 另存的 tar、改寫後的 `.env` | 0700、0600 | installer 斷言（舊版會失敗）、真實 docker 腳本 | 通過（原本是 S2） |
| 新建 `.env` | 0600 | 只有本次的等價命令實測 | 通過；覆蓋見 N-C |
| 舊版留下 0755 的 `.wukong-backups` | 收緊成 0700 | installer 測試與真實 docker 腳本都先設成 755 | 通過 |
| free-limit 429 | 立即失敗，分類為上游限流，不等 90 秒 | 真實樣本特徵化測試 | 通過（原本是 S3） |
| 取消提問之後收到 reason 為 `user` 的 interrupted | 回合中止 | 單元測試（有消融紅燈） | 通過（原本是 N2） |
| forms 的 mutex poison（包括 Drop 時） | 不 panic | 讀碼確認一致；沒有專門的測試 | 通過，屬防禦性修正 |

### 需求、架構、安全、品質

- **需求**：M1、M2 修正後，「行為與 V1 相同」在工具命令與閒置重啟上都成立。核准後的 Gherkin 沒有改動
- **架構**：
  - 工具命令的參數集中在 `wukong_gateway::backend` 的兩個常數，CLI 與 Web 依賴 gateway，方向不變
  - `wukong-runtime` 沒有改動
- **安全**：
  - 備份與 `.env` 的權限收緊
  - supervisor 的密碼經 stdin 交給 curl，不進程序參數；測試另外斷言 log 不含密碼
  - `auth list` 不印密鑰，本次已用三種憑證類型與一個環境變數驗證
- **測試真偽**：
  - 新增的行為測試，本次都以消融或換回舊腳本重跑，確認會失敗：M1、M2、S1、S2、N2
  - 唯一修正前也會通過的是 free-limit 的特徵化測試，實作者已照實標示
  - 期望值都取自真實樣本或與實作無關的字面值
- **重複與過度設計（code-simplify 標準）**：
  - `consume_or_interrupt`、`state_backup_entries`、`make_backup_dir` 都是把既有的重複收斂成一處，各有 2 個以上的呼叫者，不是只用一次的抽象
  - `backup_docker_state` 內的 `umask 077`，與 `make_backup_dir` 自己的 umask 有重疊，但 tar 需要前者，可以接受
  - install.sh:774 另外 `chmod 700 .wukong-backups`，是因為 `.env` 的備份目錄由 python 視需要建立，無法改用 `make_backup_dir`，理由成立
  - 沒有看到推測性的設定項
- **CI 三道**：fmt、clippy、test 在 c4d47d8 實跑通過（見下方命令）

### 其餘沿用 review-0405603.md

除了上面重新查核的項目，其餘都沿用 review-0405603.md：驗收對照表中沒有受影響的列、失敗面表中沒有受影響的列，以及已揭露的限制。

### 豁免、待確認與限制（本輪新增）

- 沒有 Gate 豁免紀錄
- 待確認：沒有新的 `需決策`。N-A 可以視需要記入 README `## 待確認事項` 或 rehearsal.md「觀察」
- 限制：
  - 遷移時間與「遷移中被砍」只用小型合成 DB 驗證（5 個 session，0.94 秒），沒有用正式規模（約 42 MB）的副本量測
  - SIGKILL 在 0.4 秒時，是否剛好落在遷移進行中，無法確認
  - reviewer 沒有重建 musl 的 `wukong-web`、`wukong` 做入口端到端驗證，改以 argv 單元測試加上 opencode 命令層的容器實測代替。實作者的入口端到端證據記在 implementation-plan.md R1
  - 沒有重建 release 映像；增量沒有改 Dockerfile 與打包流程
- 本次 reviewer 遵守的界線：
  - 沒有執行任何 `#[ignore]` 的真實模型測試，沒有執行 `capture-free-limit.py`，沒有送任何資料給外部模型
  - 沒有碰正式部署的容器與主機上的 opencode service，也沒有跑會動到主機 docker 部署的 installer 升級或 rollback
  - 容器探測都用 `--network none`、uid 1000、scratchpad 底下的隔離 HOME，跑完自動移除（`--rm`）

### 本次實際執行的命令與結果

- `git rev-parse HEAD` = c4d47d8；`git merge-base main HEAD` = `git rev-parse main` = e6a0bf9
- `git diff e6a0bf9 c4d47d8 | git patch-id --stable` → `5bfa9cb4ae929180f50c18db9837f3fe74a49b4c`，相符
- `cargo test --workspace --locked`：exit 0，792 passed、0 failed、14 ignored
- `cargo fmt --all -- --check`：exit 0
- `cargo clippy --all-targets --locked -- -D warnings`：exit 0
- `bash scripts/test-idle-restart-decision.sh`：exit 0，5 個 `ok:` 加上「idle-restart decision checks passed」
- `bash scripts/test-docker-runtime.sh`：exit 0，「all 38 WUKONG_* variables … reach a container」
- `bash scripts/test-installer-upgrade.sh all`：exit 0，「installer upgrade checks passed (all)」
- 紅燈重跑：把 c4d47d8 的檔案樹取出到 scratchpad，換回 0405603 的兩支腳本
  - `test-idle-restart-decision.sh`：rc=1
  - `test-installer-upgrade.sh docker-env-server-password`、`docker-state-backup`、`docker-state-rollback`：三個都是 rc=1，訊息見上方 S2 的查核
- Rust 消融：在 scratchpad 的副本拿掉三處修正，執行 `cargo test -p wukong-gateway --lib`，4 支失敗：
  - `utility_commands_use_v2_subcommands_with_a_private_server`
  - `only_the_shutdown_after_a_decline_ends_the_turn_normally`
  - `a_compaction_retrying_too_long_is_interrupted`
  - `dropping_a_compaction_interrupts_the_session`
  - 同一輪的對照組 9 支照常通過
- `bash docs/issues/issue-0044/scripts/state-backup-real-docker.sh wukong-issue44:v2-local <scratch 空目錄>`：rc=0，`archive mode=600 volumes dir=700 root=700`，「ok: backup is private, restore matches the state before the backup」；之後沒有殘留的測試 volume
- 隔離容器探測（`docker run --rm --network none --user 1000:1000 -e HOME=/p/home`，映像 `wukong-issue44:v2-local`；V1 狀態用 `ghcr.io/raybird/wukong:v0.25.1` 建立）：
  - `opencode auth list --standalone`：rc=0、1.91 秒、假憑證外洩 0 次；`opencode models --standalone`：rc=0、1.07 秒；之後沒有殘留的 opencode 程序
  - 在 V1 DB 上第一次執行 `models --standalone`：0.94 秒，20 張表變成 29 張；直接啟動 serve、先跑 `models`、0.4 秒時 SIGKILL 三種情況，之後 serve 都看得到 5 個 session、migration 為 `completed`、integrity ok
  - V1 DB 上同時執行兩個 `--standalone` 命令：3 次都有一個失敗（N-A）；已遷移的 DB 10 次、全新 HOME 3 次、與 serve 同時啟動 3 次，全部成功
  - 真實 supervisor 對真實 V2：401、沒有密碼都判「忙」並寫出原因；密碼正確時送 SIGINT，serve 以 rc=130 結束

## 流程判定
PASS

PASS 只適用於上列 Reviewed HEAD SHA；之後提交 README 記帳區段與本報告以外的檔案會讓它失效，提交前先告知使用者（review-evidence.md「審查之後的提交」）。
