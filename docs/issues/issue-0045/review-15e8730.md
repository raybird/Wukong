# 審查報告
- 範圍：完整交付範圍（`f9bc86e..15e8730`），以增量覆審進行
- Reviewed BASE SHA：f9bc86e73e261f3a7d04257b92eaa7e200787fe2
- Reviewed HEAD SHA：15e8730ff203814089887b4755726b356481db6d
- Reviewed patch-id：598f2f85e810ae38cad98d8b5b66416364f70cab（2026-11-27 以 `git diff f9bc86e 15e8730 | git patch-id --stable` 計算）
- 獨立 reviewer：Claude Code subagent（沒有參與實作，也不是 review-65e960a、review-623c436、review-4ccfcec、review-364cebf、review-794daea 的 reviewer）
- 增量覆審：上一份報告是 review-794daea.md（完整交付範圍，PASS，Reviewed HEAD 794daea）。增量範圍 `794daea..15e8730`，共 2 個提交：57d7b5b 保存上一份報告並更新 README 的 `**現行 review**` 與 Timeline，15e8730 是修正。這一輪處理的是上一份 PASS 的建議：使用者在 README TBD-17 選修 SHOULD FIX 1（清單查詢失敗的原因被 `err.to_string()` 丟掉）、併入它的 NICE 1（讀回應的程式與 `send_json` 重複）、NICE 2（TBD-15 影響欄拿掉 SCN-012）；NICE 3 記為已知差距。

增量的三個條件我都核對過：
1. review-794daea 綁定 794daea，記的是完整交付範圍，判定 PASS。提交鏈是 794daea → 57d7b5b → 15e8730，每個提交只有一個父提交；`git log --merges 794daea..15e8730` 沒有輸出，也沒有改寫歷史。
2. 本機 `main` 與 `origin/main` 都是 f9bc86e，和 15e8730 的 merge-base 仍是 f9bc86e，和上一份報告的 Reviewed BASE 相同。
3. 15e8730 上 `docs/AGENTS.md` 的流程契約是 2.7，不低於 2.6；協調者把上一份報告和新範圍一起交給我。

`verify-artifact.py --reviewed 794daea --head 57d7b5b` 回 OK（新增報告、Timeline、現況、現行 review 由 review-364cebf.md 改成 review-794daea.md），所以 57d7b5b 是有效的記帳。`--head 15e8730` 回 INVALID（`opencode_server.rs` 有變更），符合預期，本報告就是那次重審。

`**核准 commit**` 是 f0ce34a，不是 `待提交`，所以本報告不另記核准基線。增量的 README 變更只在記帳區段（`## 現況` 的 `**現行 review**`、`## 待確認事項` 的 TBD-15 影響欄與新增的 TBD-17、`## Timeline` 兩列）；`## Gherkin 驗收劇本` 與核准表都沒有變動，原核准仍然有效。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
無。

### NICE TO HAVE
無。

## 已查核維度

### 增量範圍內每個變動檔案的結論

| 檔案 | 提交 | 結論 |
|---|---|---|
| `crates/wukong-gateway/src/opencode_server.rs`：新的模組層級 `request_json`（`:184-206`） | 15e8730 | 正確。內容是原 `send_json` 授權之後那一段原封不動搬過來，見「`send_json` 的七個呼叫者」 |
| `crates/wukong-gateway/src/opencode_server.rs`：`send_json`（`:770-776`） | 15e8730 | 行為不變：授權後呼叫 `request_json` |
| `crates/wukong-gateway/src/opencode_server.rs`：`ServerHandle::background_jobs`（`:164-175`），連帶 `interrupt_turn`（`:136-162`，log 在 `:147`）與 `has_background_jobs`（`:731-746`，log 在 `:737-739`） | 15e8730 | 正確。回傳改成 `GatewayError`，送出與讀 body 的錯誤經 `http_error` 展開 source 鏈；兩個呼叫者只用 `{err}` 印出，不依賴錯誤型別，行為不變 |
| `crates/wukong-gateway/src/opencode_server.rs`：新測試 `a_failed_job_lookup_says_why`（`:2039-2057`） | 15e8730 | 是真的測試，三種變異都紅燈，見「測試真偽」 |
| `docs/issues/issue-0045/implementation-plan.md`：新增 Task 6.10（`:776-792`） | 15e8730 | 與程式一致，數字我重現了，見「文件」 |
| `docs/issues/issue-0045/README.md` | 57d7b5b、15e8730 | 都在記帳區段，見「流程記帳」；TBD-15 影響欄見「文件」 |
| `docs/issues/issue-0045/review-794daea.md` | 57d7b5b | 只新增報告（`--name-status` 為 A），之後沒有再改（`git diff 57d7b5b 15e8730 -- review-794daea.md` 沒有輸出）；`verify-artifact` 回 OK |

### 上一份報告選修項目的確認

| review-794daea | 處理 | 判定 |
|---|---|---|
| SHOULD FIX 1：查詢逾時或連不上時，log 看不出原因 | `background_jobs` 改回 `Result<Value, GatewayError>`，經 `request_json` 用 `http_error("background_jobs", err)` 包送出與讀 body 的錯誤 | 已修好。實跑 log 現在是 `background_jobs_lookup_failed session_id=ses_fake1 attempt=1/3: agent command failed (code None): opencode server background_jobs failed: error sending request for url (…): operation timed out`，帶出逾時；取消路徑 `turn_cancelled_jobs_lookup_failed` 走同一個函式 |
| NICE 1：回應處理多了一份 | 讀 text、非 2xx 就失敗、解析 JSON 只剩 `request_json` 一份，`send_json` 與 `background_jobs` 共用；`request_json` 不碰 backend，Drop 路徑的 `ServerHandle` 也能用 | 已修好 |
| NICE 2：README TBD-15 的影響欄還寫 SCN-012 | 改成「SCN-002（派工後等 subagent 回報）與 #44 起取消時中斷 session 的既有保障補強，不新增驗收」 | 已修好，與 Task 6.8 `:732` 的寫法一致 |
| NICE 3：body 讀到一半卡住沒有測試 | 使用者決定記為已知差距（TBD-17）；程式沒動 | 決定已記錄，不需再查 |

### `send_json` 的七個呼叫者

- 呼叫者在 `:358`、`:384`（health_check）、`:579`（instruction_entries）、`:975`（list_agents）、`:1069`、`:1106`（session_created_ms）、`:1133`（list_sessions），共七處，數目與 Task 6.10 的說法相同，沒有變成死碼。
- 我把 794daea 的 `send_json` 從 `let response = request.send()…` 到結尾那 16 行，和 15e8730 的 `request_json` 同一段（補回一層縮排）做 `diff`，結果完全相同。授權仍在 `send_json` 裡先做（`self.authorize(request)`），所以錯誤訊息格式（`opencode server {phase} failed: …`、`returned {status}: {text}`、`returned invalid JSON: …; body: …`）、`code`（非 2xx 為狀態碼，其他為 None）與 basic auth 都不變。
- 旁證：把 `request_json` 送出錯誤改成 `err.to_string()` 的變異，除了新測試之外，`backend::tests::server_backend_readiness_reports_connection_failure` 與 `local_process::tests::health_failure_releases_child` 也紅燈，表示既有測試鎖住了 `send_json` 呼叫者的錯誤格式，而原版這兩項照常通過。

### `background_jobs` 改型別後的兩條路徑

- `has_background_jobs`：`Err(err)` 只用來 `eprintln!`，重試次數、間隔與「三次都失敗回 false」都沒變；`Ok` 分支照舊看 `subagent_jobs`。log 前綴 `background_jobs_lookup_failed session_id=… attempt=n/3` 還在，後面接 `GatewayError` 的 Display。
- `interrupt_turn`：`Err(err)` 一樣只印 `turn_cancelled_jobs_lookup_failed session_id={root}: {err}` 再當作沒有清單，不中斷額外的 session。
- 單次逾時：`.timeout(self.jobs_lookup_timeout)` 仍在授權之後、送出之前設在這個請求上，和 794daea 相同；`a_job_lookup_that_hangs_times_out_on_its_own` 照常 1.6 秒通過。
- log 多了 `agent command failed (code None):` 這段前綴，那是 `GatewayError::AgentFailed` 的 Display。794daea 之前 `has_background_jobs` 走 `send_json` 時就是這個格式，不是本增量帶進來的，不列為發現。

### 測試真偽

我在工作樹暫時改動實作，每次都 `git checkout -- .` 還原，最後 `git status --short` 只剩本報告。

| 改動 | 結果 |
|---|---|
| `request_json` 送出錯誤改成不展開 source 鏈（`format!("opencode server {phase} failed: {err}")`） | 新測試失敗在 `:2056`（`timed out`），其餘 194 項通過 |
| `request_json` 送出錯誤改成 `err.to_string()` | 新測試失敗在 `:2055`（`background_jobs`），另有兩項 health 測試失敗（見上節），其餘 192 項通過 |
| `background_jobs` 整個換回 794daea 的版本（`Result<Value, String>`） | 新測試失敗在第一個斷言（`background_jobs`；函式變長，行號移到 `:2064`），其餘 194 項通過。這就是 Task 6.10 說的紅燈 |

- 新測試直接呼叫 `server_handle().background_jobs`，不跑回合，不讀 `WUKONG_AGENT_TIMEOUT_SECS`，所以不拿 `AGENT_TIMEOUT_ENV_LOCK` 合理。假 server 的 `hang` 在 header 之前停住，100ms 後 reqwest 回逾時，錯誤在送出階段，source 鏈是 `operation timed out`。
- `hang` 的連線 task 在測試的 runtime 結束時被取消，上一份報告已查過，不會卡住其他測試；gateway lib 連跑 8 次都在 12.3 秒左右結束。

### 文件

- Task 6.10（`implementation-plan.md:776-792`）：產出、相依、完成判準與證據都和 15e8730 的程式一致。「改之前錯誤只有 `error sending request for url (…)`（紅燈）」我用換回 794daea 版本的變異重現了；195 passed 與 783 passed、0 failed、14 ignored 和我跑的結果相同。
- TBD-15 影響欄（README `:258`）與 Task 6.8 驗收編號（`implementation-plan.md:732`）現在說法一致。
- TBD-17（README `:260`）：選修與已知差距的劃分和 Task 6.10、commit 訊息相同；內容不含 `<`，行首沒有粗體或 `[`。
- 程式註解（`:184-185`、`:2039-2040`）描述的行為與實作相符。根目錄 `AGENTS.md` 與 CHANGELOG 沒寫錯誤訊息格式，不需要改。

### 驗收與證據（相對於 review-794daea）

| 編號 | 本輪 | 結論 |
|---|---|---|
| SCN-002 | 重新查核：等 subagent 回報時查清單的程式改成走 `request_json`。成功與失敗時的回合行為不變；新測試先紅燈後綠燈，兩項重試測試、單次逾時測試與既有派工測試照常通過 | 通過 |
| SCN-001、003～014 | 沿用。增量只改了 gateway 的 `opencode_server.rs` 與文件；`send_json` 的呼叫者行為不變（見上），其他 crate 沒有變動，workspace 783 項全過 | 通過（沿用） |

### 相關失敗面（本輪新增）

| 輸入／狀態 | 預期 | 覆蓋 | 判定 |
|---|---|---|---|
| 清單查詢逾時 | log 帶出 `operation timed out` | 新測試（變異確認）、實跑 `--nocapture` | 已覆蓋 |
| 清單查詢連線被拒、被 reset | log 帶出 source 鏈裡的原因 | 讀程式：同一個 `http_error`；health 路徑的連線失敗測試鎖住同一個函式 | 可接受 |
| 清單回非 2xx 或非 JSON | 失敗並印狀態碼與 body，等待路徑重試、取消路徑不補中斷 | 讀程式：與搬移前逐行相同；既有重試測試照常通過 | 已覆蓋 |
| `send_json` 七個呼叫者的錯誤格式、code、授權 | 不變 | 逐行 diff、gateway lib 全過、`to_string` 變異讓既有 health 測試紅燈 | 已覆蓋 |

### 需求、架構、安全、品質

- **需求**：選修的 SHOULD FIX 1、NICE 1、NICE 2 都處理了，NICE 3 照使用者的決定記為已知差距。沒有新增驗收，也沒有改任何 Scenario 的可觀察結果。
- **架構**：變動只在 `wukong-gateway` 的 server backend 內，沒有新的依賴，`AiBackend` 介面沒變。
- **安全**：授權仍在各自的呼叫端做（`send_json` 用 `self.authorize`，`background_jobs` 用 `ServerHandle::authorize`），`request_json` 只收已授權的請求，文件註解也寫明；log 印 URL、狀態碼與 body，不印密碼。
- **品質（code-simplify）**：程式改動是 49 行新增、30 行刪除（含 20 行新測試），回應處理只有 `request_json` 一份共用實作、兩個呼叫端各一行，沒有過度設計，也沒有改到增量以外的程式。`send_empty`（`:778-797`）與 `open_event_stream` 仍各有一份「非 2xx 就失敗」的處理，那是增量之前就有的、不讀 JSON 的路徑，不在選修範圍，不列為發現。

### 流程記帳

- 57d7b5b 在同一個提交保存 review-794daea.md 並把 `**現行 review**` 改成它，符合 review-evidence.md 的要求。
- 15e8730 的 README 變更都在記帳區段（TBD-15 影響欄、TBD-17、Timeline 一列）；`**狀態**` 仍是「審查中」，`## 現況` 仍在 `## 概述` 之後。目前 `**現行 review**` 是 review-794daea.md，保存本報告的提交記得改成 review-15e8730.md。
- **交付提醒**：`git ls-remote origin issue-0045-opencode-v2-slim` 仍是 cf47307，364cebf 之後的提交都還沒推送。回覆 PR #47、請第三方做第二輪之前，先推送，再確認 PR head 是本報告的 Reviewed HEAD 或它的有效記帳後繼。

### 我執行的驗證（HEAD 15e8730，工作樹乾淨，2026-11-27）

| 命令 | 結果 |
|---|---|
| `git rev-parse 15e8730`；`git diff f9bc86e 15e8730 \| git patch-id --stable` | 15e8730ff203…；patch-id 598f2f85e810ae38cad98d8b5b66416364f70cab |
| `git merge-base main 15e8730`；`git rev-parse main origin/main`；`git log --merges 794daea..15e8730`；`git log --format='%h %p' 794daea..15e8730` | f9bc86e；兩者都是 f9bc86e；沒有輸出；線性的兩個提交 |
| `git show --name-status --format= 57d7b5b` | `M README.md`、`A review-794daea.md` |
| `verify-artifact.py --repo . --reviewed 794daea --head 57d7b5b --artifact docs/issues/issue-0045/review-794daea.md` | OK（新增報告、Timeline、現況、現行 review） |
| `verify-artifact.py --repo . --reviewed 794daea --head 15e8730 --artifact docs/issues/issue-0045/review-794daea.md` | INVALID（`opencode_server.rs` 有變更），符合預期 |
| 794daea `send_json` 與 15e8730 `request_json` 的主體 `diff` | 完全相同（16 行） |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --all-targets --locked -- -D warnings`（TMPDIR=target/tmp） | exit 0（本機 toolchain；W-9：CI 的 stable 可能較新） |
| `cargo test -p wukong-gateway --locked --lib`（連跑 8 次） | 每次都是 195 passed、0 failed |
| `cargo test -p wukong-gateway --locked --lib -- a_job_lookup_that_hangs_times_out_on_its_own a_failed_job_lookup_says_why --nocapture` | 2 passed，1.61 秒；log 帶出 `operation timed out` |
| `cargo test --workspace --locked --no-fail-fast`（跑 6 次） | 第 1 次 781 passed、2 failed、14 ignored，失敗的測試名稱沒有留下；之後 5 次都是 783 passed、0 failed、14 ignored，和 Task 6.10 的數字相同。見「限制」 |
| 三種變異（見「測試真偽」），之後 `git checkout -- .` | 三種都紅燈；`git status --short` 只剩本報告 |
| `git ls-remote origin issue-0045-opencode-v2-slim` | cf47307 |

沒有動正式容器 wukong-telegram、wukong-web、wukong-schedulerd，也沒有啟動真實 V2。

### 沿用的結論

增量沒有動到這些項目的程式與證據，沿用 review-794daea：
- 單次逾時的涵蓋範圍（含 body 卡住，NICE 3 已記為已知差距）、預設值與建構路徑、最壞延遲與副作用；
- 假 server 的 `HANG`、去重測試與其變異結果；
- P2-1 重試的次數與間隔、只在 Idle 分支呼叫；P2-2 取消的順序、`disarm` 之後不送請求、只挑 subagent、孫 session 經 `descendants=true` 列出；
- V2 原始碼查證與 TBD-16 NICE 4 的殘餘風險分析；
- SCN-001、003～014 的驗收結論，以及 review-364cebf 以前沿用下來的結論。

其餘沿用 review-794daea.md。

### 豁免、待確認與限制

- **豁免**：沒有 Gate 豁免紀錄。
- **待確認事項**：TBD-1～TBD-17 都已解決，狀態我沒有改。本報告沒有 `需決策` 的項目。
- **限制**：
  - 沒有啟動真實 V2，錯誤訊息只用假 server 測。連線被拒、被 reset 的 log 只讀程式確認走同一個 `http_error`。
  - workspace 測試第 1 次有 2 項失敗，我只留了彙總數字，沒有留下測試名稱；之後 5 次都全過，gateway lib 連跑 8 次與 3 次變異的未變異項目也都通過。無法確定那兩項在哪個 crate，所以不能排除是 gateway 裡和時間有關的測試；本增量沒有改任何時間或並行相關的程式，我判斷與本增量無關，但這只是推斷。
