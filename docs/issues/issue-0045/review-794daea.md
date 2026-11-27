# 審查報告
- 範圍：完整交付範圍（`f9bc86e..794daea`，42 個提交、136 個檔案），以增量覆審進行
- Reviewed BASE SHA：f9bc86e73e261f3a7d04257b92eaa7e200787fe2
- Reviewed HEAD SHA：794daeadcd41375195afefd34819bc1dfad37e08
- Reviewed patch-id：b66d90b7fc2e85decb7ca7fef94a720ee2325e65（2026-11-27 以 `git diff f9bc86e 794daea | git patch-id --stable` 計算）
- 獨立 reviewer：Claude Code subagent（沒有參與實作，也不是 review-65e960a、review-623c436、review-4ccfcec、review-364cebf 的 reviewer）
- 增量覆審：上一份報告是 review-364cebf.md（完整交付範圍，PASS，Reviewed HEAD 364cebf）。增量範圍 `364cebf..794daea`，共 2 個提交：4026099 只保存上一份報告，794daea 是修正。這一輪處理的是上一份 PASS 的建議。使用者在 README TBD-16 選修 NICE TO HAVE 1（每次查背景工作有自己的短逾時）、2（取消時去重補測試）、3（等待與取消共用一份清單查詢，取消也看 HTTP 狀態）、5（Task 6.8 措辭兩處）；NICE 4 記為已知差距。

增量的三個條件我都核對過：
1. review-364cebf 綁定 364cebf，記的是完整交付範圍，判定 PASS。提交鏈是 364cebf → 4026099 → 794daea，每個提交只有一個父提交；`git log --merges 364cebf..794daea` 沒有輸出，也沒有改寫歷史。
2. 本機 `main` 與 `origin/main` 都是 f9bc86e，和 794daea 的 merge-base 仍是 f9bc86e，和上一份報告的 Reviewed BASE 相同。
3. 794daea 上 `docs/AGENTS.md` 的流程契約是 2.7，不低於 2.6；協調者把上一份報告和新範圍一起交給我。

`verify-artifact.py --reviewed 364cebf --head 4026099` 回 OK（只新增報告），所以 4026099 是有效的記帳。`--head 794daea` 回 INVALID（`opencode_server.rs` 有變更），符合預期，本報告就是那次重審。

`**核准 commit**` 是 f0ce34a，不是 `待提交`，所以本報告不另記核准基線。794daea 的 README 變更只有三處記帳內容：`**現行 review**` 改成 review-364cebf.md、TBD-16 一列、Timeline 兩列。`## Gherkin 驗收劇本` 與核准表都沒有變動，原核准仍然有效。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX

1. **查詢逾時或連不上時，log 看不出原因**：`crates/wukong-gateway/src/opencode_server.rs:176`、`:178`
   - `background_jobs` 用 `err.to_string()` 轉 reqwest 的錯誤。reqwest 0.12 的 Display 只印頂層訊息，真正的原因（`operation timed out`、connection refused）在 source 鏈裡。同一個檔案的 `http_error`（`:1331-1342`）就是為了這件事展開 source 鏈，註解也寫了。
   - 實際輸出（我跑 `a_job_lookup_that_hangs_times_out_on_its_own -- --nocapture`）：
     `background_jobs_lookup_failed session_id=ses_fake1 attempt=1/3: error sending request for url (http://127.0.0.1:38127/api/experimental/job?sessionID=ses_fake1&descendants=true)`
     沒有提到逾時。改之前 `has_background_jobs` 走 `send_json` → `http_error`，這種情況會印出 `…: operation timed out`。
   - 也就是說，這次增量新增的情況（單次逾時）正好是 log 分辨不出來的那一種：逾時、連線被拒、連線被 reset 印出來都一樣。取消路徑的 `turn_cancelled_jobs_lookup_failed`（`:147`）也一樣。
   - 不影響行為，也不影響驗收，所以不阻擋 PASS。修法很小：`background_jobs` 改回 `Result<Value, GatewayError>`，送出與讀 body 的錯誤用 `http_error("background_jobs", err)`；或把 `send_json` 讀回應、檢查狀態、解析 JSON 那一段抽成兩邊共用的函式（同時處理 NICE 1）。

### NICE TO HAVE

1. **回應處理又多了一份**（code-simplify）：`crates/wukong-gateway/src/opencode_server.rs:177-183` 與 `:761-775`
   - NICE 3 把 URL 收成一份（`:168` 是唯一一處組 `experimental/job` 的地方），但 `background_jobs` 的「讀 text → 非 2xx 就失敗 → 解析 JSON」和 `send_json` 幾乎逐行相同，只差錯誤型別與 phase 前綴。
   - 理由成立：Drop 路徑在 backend 借用之外，用不了 `self.send_json`。不過 `send_json` 只用到 `self.authorize`，讀回應那一段不需要 backend，可以抽成接收 `reqwest::Response` 的自由函式給兩邊用。和 SHOULD FIX 1 一起改最省事。
   - `send_json` 還有 7 個呼叫者（`:343`、`:369`、`:564`、`:976`、`:1070`、`:1107`、`:1134`），沒有變成死碼。

2. **README TBD-15 的影響欄還寫 SCN-012**：`docs/issues/issue-0045/README.md:258`
   - NICE 5 把 Task 6.8 的驗收編號改成「取消與失敗時中斷 session 是 #44 起的既有保障，不在 Gherkin 裡」（`implementation-plan.md:732`），但 TBD-15 的影響欄仍寫「SCN-002、SCN-012 的既有保障補強」。這是記帳區段，可以在保存本報告的提交順手改，不改也不影響驗收。

3. **body 讀到一半卡住沒有測試**：`crates/wukong-gateway/src/opencode_server/fake_v2.rs:65-68`
   - 假 server 的 `hang` 在送出任何 header 之前就停住，所以測試只證明「連線開著、header 不回」會逾時。header 回了、body 卡住的情況我讀了 reqwest 0.12.28 的原始碼確認也涵蓋：`RequestBuilder::timeout` 存進 request extensions，`execute_request` 以它當 `total_timeout`（`async_impl/client.rs:2629-2634`），收到 header 後把同一個 `Sleep` 交給 `Response::new`，再由 `body::response(body, total_timeout, …)` 包住 body（`async_impl/response.rs:39-45`），說明文件也寫「until the response body has finished」。所以行為沒問題，只是沒有測試鎖住。不建議為此再加測試。

## 已查核維度

### 增量範圍內每個變動檔案的結論

| 檔案 | 提交 | 結論 |
|---|---|---|
| `crates/wukong-gateway/src/opencode_server.rs`：`JOBS_LOOKUP_TIMEOUT`（`:35-37`）、`jobs_lookup_timeout` 欄位（`:50-51`、`:124`）、`from_env`（`:323`）、`server_handle`（`:697-704`） | 794daea | 正確。見下方「NICE 1：單次逾時」 |
| `crates/wukong-gateway/src/opencode_server.rs`：`ServerHandle::background_jobs`（`:164-184`）、`has_background_jobs`（`:716-733`）、`interrupt_turn`（`:136-162`） | 794daea | 行為正確。log 少了原因見 SHOULD FIX 1，重複的回應處理見 NICE 1 |
| `crates/wukong-gateway/src/opencode_server.rs`：新測試 `a_job_lookup_that_hangs_times_out_on_its_own`（`:2014-2038`）、改過的 `a_cancelled_turn_interrupts_the_subagent_sessions_too`（`:2071-2110`） | 794daea | 都是真的測試，變異會紅燈，見「測試真偽」 |
| `crates/wukong-gateway/src/opencode_server/fake_v2.rs`：`HANG`（`:18`、`:65-68`、`:315-317`）、`script_jobs` 說明（`:109`） | 794daea | 正確，不會卡住其他測試，見「假 server 的 HANG」 |
| `docs/issues/issue-0045/implementation-plan.md` Task 6.8（`:732`、`:739`）、新增 Task 6.9（`:758-774`） | 794daea | 兩處措辭已修正，和程式、V2 原始碼查證一致；Task 6.9 的證據我逐項重現了，見「文件」 |
| `docs/issues/issue-0045/README.md` | 794daea | 見「流程記帳」；TBD-15 的殘留寫法見 NICE 2 |
| `docs/issues/issue-0045/review-364cebf.md` | 4026099 | 只新增報告（`--name-status` 為 A），之後沒有再改；`verify-artifact` 回 OK |

### 上一份報告選修項目的確認

| review-364cebf | 處理 | 判定 |
|---|---|---|
| NICE 1：查背景工作沿用 20 分鐘 client 逾時，重試放大成 3 倍 | 每次查詢 `.timeout(self.jobs_lookup_timeout)`，預設 10 秒；等待與取消兩條路徑都經過 `background_jobs` | 已修好 |
| NICE 2：去重沒有測試鎖住 | 取消測試的清單也列出串流看過的 fixer，斷言 fixer 的 interrupt 只有 1 次 | 已修好，變異 10/10 紅燈 |
| NICE 3：清單查詢兩份、取消路徑不看狀態碼 | URL 只剩 `:168` 一處；非 2xx 與非 JSON 在兩條路徑都算失敗並寫 log | 已修好；log 內容見 SHOULD FIX 1 |
| NICE 4：中斷子 session 後回報可能喚醒 primary（`需決策`） | 使用者選 (a)，TBD-16 記為已知差距；程式沒動 | 決定已記錄，不需再查 |
| NICE 5：Task 6.8 措辭兩處 | `:739` 改成「只有一處呼叫，在 `Session.remove` 裡」；`:732` 拿掉 SCN-012，改寫成「#44 起的既有保障，不在 Gherkin 裡」 | 已修好；「#44 起」我用 `git log -S "fn interrupt_on_drop"` 查到最早是 b65b487（#44），成立 |

### NICE 1：單次逾時

- **涵蓋範圍**：reqwest 0.12.28 的 `RequestBuilder::timeout` 會取代 client 的 timeout，從開始連線算到 body 讀完。`PendingRequest::poll` 先看 `total_timeout`，所以連線開著、header 一直不回時會回 `TimedOut`；收到 header 之後，同一個計時交給 body，`response.text()` 卡住也會逾時（見 NICE 3）。
- **預設值**：`OpencodeServerBackend` 只有 `from_env` 用結構字面值建立（`for_local_process` 也是先呼叫 `from_env`），兩處都拿到 `JOBS_LOOKUP_TIMEOUT`；`ServerHandle` 只由 `server_handle()` 建立，從 backend 複製。我 grep 過 `OpencodeServerBackend {` 與 `ServerHandle {`，沒有其他建構路徑。測試只在新測試裡把它改成 200ms。
- **最壞延遲**：等待路徑最壞 3×10 秒加 2×0.5 秒，約 31 秒，不再是 3×1200 秒；取消路徑的 `send_now`（回合失敗時）最多多等 10 秒。查詢仍在 `select!` 外面，等待期間看不到回合 deadline，但界限已經是幾十秒，可以接受。POST interrupt 仍沿用 client 逾時，上一份報告提過、這次不在選修範圍。
- **副作用**：V2 很忙、清單查詢正常但要超過 10 秒才回時，現在會被當成失敗；三次都失敗就以目前的回覆收尾（TBD-15 的決定）。清單查詢是輕量的 GET，10 秒是很寬的值，我判斷可以接受。

### 改用 `ServerHandle::background_jobs` 後的資訊量

- 等待路徑的 log 從 `opencode server background_jobs returned 503 …`／`… failed: …: operation timed out` 變成 `returned 503 Service Unavailable: {body}`／`error sending request for url (…)`。前綴 `background_jobs_lookup_failed session_id=… attempt=n/3` 還在，狀態碼與 body 也還在，非 2xx 與非 JSON 的情況資訊足夠。少的是 reqwest 錯誤的 source 鏈，見 SHOULD FIX 1。
- 取消路徑從「非 2xx 靜默當空清單」變成寫 `turn_cancelled_jobs_lookup_failed`，比改之前多了線索。行為不變：查不到時不中斷任何額外的 session。
- `has_background_jobs` 每次呼叫建一個 `ServerHandle`（複製 URL 字串、`Client` 是 Arc、密碼字串）。只在 Idle 且事件看不出後續時才呼叫，成本可以忽略。

### 假 server 的 HANG

- `handle` 先記下請求、取出這一項，再回 `HANG`；連線的 task 睡 3600 秒後直接 return，不寫回應。client 逾時後丟掉連線，server 那一頭的 task 只是在睡，不會報錯。
- 這個 task 和 accept 迴圈都是用 `tokio::spawn` 開在該測試的 runtime 上，`#[tokio::test]` 結束時 runtime 被丟掉，task 一起被取消，不會留到下一個測試。我做變異 2 時（拿掉 `.timeout`），新測試在 10 秒外層逾時後失敗，整批測試照常跑完（20.8 秒），沒有卡住。
- `status == HANG` 只在 `handle` 回 `"hang"` 時成立，真實狀態列不會是這個字；`jobs == HANG` 是 `Value` 和 `&str` 比較，只在項目是字串 `"hang"` 時成立，不影響既有的陣列與 `"fail"` 項目。
- 新測試拿 `AGENT_TIMEOUT_ENV_LOCK`：回合 deadline 讀 `WUKONG_AGENT_TIMEOUT_SECS`，別的測試把它設成 1 到 5 秒時可能讓回合先逾時，拿鎖合理。

### 測試真偽

我在工作樹暫時改動實作，每次都 `git checkout -- .` 還原，最後 `git status --short` 只剩本報告。

| 改動 | 結果 |
|---|---|
| 去重判斷 `if !sessions.iter().any(…)` 改成 `if true` | `a_cancelled_turn_interrupts_the_subagent_sessions_too` 失敗在 `:2105`（fixer 被中斷 2 次），其餘 193 項通過；單獨連跑 10 次都紅燈 |
| 拿掉 `.timeout(self.jobs_lookup_timeout)` | `a_job_lookup_that_hangs_times_out_on_its_own` 失敗在 `:2029`（10 秒內等不完），其餘 193 項通過 |

- 去重測試：取消前事件停在子 session 建立後 3 則再 hold，沒有 Idle，所以清單在取消前不會被查掉；測試先等 child 的 interrupt 與清單查詢都出現，再多等 100ms 讓可能的第二次 interrupt 送出。原版連跑 20 次（兩項取消測試）都通過，沒有看到不穩定。
- 新逾時測試：三次 `hang`、每次 200ms，加兩次 500ms 間隔，實測 1.64 秒；斷言回覆是 DONE、查詢 3 次。
- gateway lib 測試連跑 3 次，每次 194 passed、0 failed。

### 文件

- Task 6.8 `:739`：「全檔只有一處呼叫，在 `Session.remove` 裡」和上一份報告在映像裡數到的 `.cancelSession(` 1 處一致。
- Task 6.8 `:732`：SCN-012 的 Gherkin（README `:174-182`）只有空輸出修復、上游錯誤分類與同 scope 拒絕，拿掉 SCN-012 正確。
- Task 6.9：紅燈說法（拿掉逾時 10 秒內等不完、去重變異會紅燈）我都重現了；782／0／14 與 194 passed 和我跑的結果相同。
- 根目錄 `AGENTS.md:305` 與 CHANGELOG 沒改。兩者都沒寫逾時的數字，描述（失敗時重試、三次都失敗才以目前的回覆收尾）仍然成立，不需要改。

### 驗收與證據（相對於 review-364cebf）

| 編號 | 本輪 | 結論 |
|---|---|---|
| SCN-002 | 重新查核：等 subagent 回報的清單查詢多了單次逾時。新測試先紅燈後綠燈；兩項重試測試與既有派工測試照常通過 | 通過 |
| SCN-012 | 增量沒有改到空輸出修復、上游錯誤分類與 scope 拒絕。取消中斷本來就不在它的 Gherkin 裡（NICE 5），相關測試照常通過 | 通過（沿用） |
| SCN-001、003～011、013、014 | 沿用。增量只改了 gateway 的 `opencode_server.rs` 與它的假 server，加上文件；其他 crate 沒有變動，workspace 782 項全過 | 通過（沿用） |

### 相關失敗面（本輪新增）

| 輸入／狀態 | 預期 | 覆蓋 | 判定 |
|---|---|---|---|
| 清單查詢連線開著、header 不回 | 每次 10 秒放棄，三次後以目前的回覆收尾 | 新測試（我做了變異確認） | 已覆蓋 |
| header 回了、body 卡住 | 同一個逾時涵蓋 | 讀 reqwest 原始碼 | 可接受（NICE 3） |
| 取消時清單查詢卡住 | Drop 路徑在背景等最多 10 秒；`send_now` 多等最多 10 秒 | 讀程式 | 可接受 |
| 取消時清單回 401／503 或非 JSON | 不中斷額外 session，寫 `turn_cancelled_jobs_lookup_failed` | 讀程式 | 可接受 |
| 逾時、連線被拒時的 log | 要分得出原因 | 實跑 `--nocapture` | 分不出來（SHOULD FIX 1） |
| 子 session 同時在串流與清單 | 只中斷一次 | 改過的取消測試（變異確認） | 已覆蓋 |
| V2 正常但清單要超過 10 秒才回 | 當成失敗，三次後提早收尾 | 讀程式 | 可接受，10 秒對輕量 GET 很寬 |

### 需求、架構、安全、品質

- **需求**：選修的 1、2、3、5 都處理了，NICE 4 照使用者的決定記為已知差距。沒有新增驗收，也沒有改任何 Scenario 的可觀察結果。
- **架構**：變動只在 `wukong-gateway` 的 server backend 內，沒有新的依賴，`AiBackend` 介面沒變。
- **安全**：清單查詢仍經 `ServerHandle::authorize` 帶 basic auth；log 印 URL、狀態碼與回應 body，不印密碼。
- **品質（code-simplify）**：沒有過度設計，逾時是一個常數加一個只給測試改的欄位，和既有的 `retry_give_up_after` 寫法一致。重複的回應處理見 NICE 1。`:713` 的說明註解超過其他行的寬度，rustfmt 不管註解，不列為發現。

### 流程記帳

- 4026099 只新增 review-364cebf.md。`**現行 review**` 與 Timeline 改在 794daea 才更新；review-evidence.md 要求保存報告的同一個提交更新 `**現行 review**`，這次晚了一個提交，現在的值是對的，不影響判定。保存本報告的提交記得把 `**現行 review**` 改成 review-794daea.md。
- 794daea 的 README 變更都在記帳區段（`## 現況` 的 `**現行 review**`、`## 待確認事項` 的 TBD-16、`## Timeline` 兩列），`**狀態**` 仍是「審查中」，`## 現況` 仍在 `## 概述` 之後（第 23 行）。
- **交付提醒**：`git ls-remote origin issue-0045-opencode-v2-slim` 仍是 cf47307，364cebf、4026099、794daea 都還沒推送。回覆 PR #47、請第三方做第二輪之前，先推送，再確認 PR head 是本報告的 Reviewed HEAD 或它的有效記帳後繼。

### 我執行的驗證（HEAD 794daea，工作樹乾淨，2026-11-27）

| 命令 | 結果 |
|---|---|
| `git rev-parse 794daea`；`git diff f9bc86e 794daea \| git patch-id --stable` | 794daeadcd41…；patch-id b66d90b7fc2e85decb7ca7fef94a720ee2325e65 |
| `git merge-base main 794daea`；`git rev-parse main origin/main`；`git log --merges 364cebf..794daea`；`git log --format='%h %p' 364cebf..794daea` | f9bc86e；兩者都是 f9bc86e；沒有輸出；線性的兩個提交 |
| `git show --name-status --format= 4026099` | `A docs/issues/issue-0045/review-364cebf.md`，只有這一個檔案 |
| `verify-artifact.py --repo . --reviewed 364cebf --head 4026099 --artifact docs/issues/issue-0045/review-364cebf.md` | OK（新增報告） |
| `verify-artifact.py --repo . --reviewed 364cebf --head 794daea --artifact docs/issues/issue-0045/review-364cebf.md` | INVALID（`opencode_server.rs` 有變更），符合預期 |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --all-targets --locked -- -D warnings`（TMPDIR=target/tmp） | exit 0（本機 toolchain；W-9：CI 的 stable 可能較新） |
| `cargo test -p wukong-gateway --locked --lib`（連跑 3 次） | 每次都是 194 passed、0 failed |
| `cargo test -p wukong-gateway --locked --lib a_job_lookup_that_hangs -- --nocapture` | 通過，1.64 秒；log 內容見 SHOULD FIX 1 |
| `cargo test --workspace --locked --no-fail-fast` | 782 passed、0 failed、14 ignored，和 Task 6.9 的數字相同 |
| 兩種變異（見「測試真偽」）、取消測試原版連跑 20 次、去重變異連跑 10 次，之後 `git checkout -- .` | 兩種變異都紅燈；原版 20/20 通過；去重變異 10/10 紅燈；`git status --short` 只剩本報告 |
| 讀 `~/.cargo/registry/src/*/reqwest-0.12.28` 的 `async_impl/request.rs`、`client.rs`、`response.rs`、`error.rs` | 單次 timeout 涵蓋到 body 讀完；Display 不含 source 鏈 |
| `git ls-remote origin issue-0045-opencode-v2-slim` | cf47307 |

沒有動正式容器 wukong-telegram、wukong-web、wukong-schedulerd，也沒有啟動真實 V2。

### 沿用的結論

增量沒有動到這些項目的程式與證據，沿用 review-364cebf：
- P2-1 重試的次數與間隔、只在 Idle 分支呼叫；P2-2 取消的順序、`disarm` 之後不送請求、只挑 subagent、孫 session 經 `descendants=true` 列出、basic auth；
- V2 原始碼查證（`Session.interrupt`、`Job.cancelSession`、`SubagentJob`）與 NICE 4 的殘餘風險分析；
- SCN-001、003～011、013、014 的驗收結論，以及 review-4ccfcec 以前沿用下來的結論（角色顯示、前端跳脫、背景 shell 不算 subagent、派工評估、TBD-10～TBD-15、架構相依圖與 primary 權限）。

其餘沿用 review-364cebf.md。

### 豁免、待確認與限制

- **豁免**：沒有 Gate 豁免紀錄。
- **待確認事項**：TBD-1～TBD-16 都已解決，狀態我沒有改。本報告沒有 `需決策` 的項目。
- **限制**：
  - 沒有啟動真實 V2，逾時行為只用假 server 測。body 讀到一半卡住的情況只讀了 reqwest 原始碼。
  - 去重測試的「多等 100ms」是時間上的假設；在很慢的 CI 上，第二次 interrupt 可能晚於 100ms 才送出，那時去重壞掉也可能被漏看（只會漏看，不會誤判紅燈）。本機 10 次變異都紅燈。

## 流程判定
PASS

PASS 只適用於上列 Reviewed HEAD SHA；之後提交 README 記帳區段與本報告以外的檔案會讓它失效，提交前先告知使用者（review-evidence.md「審查之後的提交」）。
