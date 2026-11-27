# 審查報告
- 範圍：完整交付範圍（`f9bc86e..364cebf`，40 個提交、135 個檔案），以增量覆審進行
- Reviewed BASE SHA：f9bc86e73e261f3a7d04257b92eaa7e200787fe2
- Reviewed HEAD SHA：364cebff2c272575e0e1e4d13a37b86a38d0861e
- Reviewed patch-id：e16add6515d8244c3c2657ebddd3376c993dd9cc（2026-11-27 以 `git diff f9bc86e 364cebf | git patch-id --stable` 計算）
- 獨立 reviewer：Claude Code subagent（沒有參與實作，也不是 review-65e960a、review-623c436、review-4ccfcec 的 reviewer）
- 增量覆審：上一份報告是 review-4ccfcec.md（完整交付範圍，PASS，Reviewed HEAD 4ccfcec）；增量範圍 `4ccfcec..364cebf`，共 3 個提交：2a49f82、cf47307 只是記帳，364cebf 是修正。這次增量處理的是 PR #47 第三方審查（[留言](https://github.com/raybird/Wukong/pull/47#issuecomment-7214581086)，Reviewed HEAD cf47307，CONDITIONAL PASS），不是上一份 PASS 的建議。使用者在 README TBD-15 選了：P2-1 有限重試，都失敗才以目前的回覆收尾；P2-2 取消時從背景工作清單補中斷子 session；P3 維持已知差距（TBD-12 第 11 項）。

增量的三個條件我都核對過：
1. review-4ccfcec 綁定 4ccfcec，記的是完整交付範圍，判定 PASS。提交鏈是 4ccfcec → 2a49f82 → cf47307 → 364cebf，每個提交只有一個父提交；`git log --merges 4ccfcec..364cebf` 沒有輸出，也沒有改寫歷史。
2. 本機 `main` 與 `origin/main` 都是 f9bc86e，和 364cebf 的 merge-base 仍是 f9bc86e，和上一份報告的 Reviewed BASE 相同。PR #47 的 base 也是 f9bc86e。
3. 364cebf 上 `docs/AGENTS.md` 的流程契約是 2.7，不低於 2.6；協調者把上一份報告和新範圍一起交給我。

`verify-artifact.py --reviewed 4ccfcec --head cf47307` 回 OK（新增報告、狀態、Timeline、待確認事項、現況、現行 review），所以 2a49f82、cf47307 是有效的記帳。`--head 364cebf` 回 INVALID（AGENTS.md 有變更），符合預期，本報告就是那次重審。

`**核准 commit**` 是 f0ce34a，不是 `待提交`，所以本報告不另記核准基線。`git diff cf47307 364cebf -- docs/issues/issue-0045/README.md` 只動了三處記帳內容：TBD-15 一列、Timeline 一列、`**狀態**` 改成「審查中」。`## Gherkin 驗收劇本` 與核准表都沒有變動，原核准仍然有效。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
無。

### NICE TO HAVE

1. **背景工作查詢沿用 20 分鐘的 client 逾時，等待期間不看回合 deadline；重試讓 V2 掛住時的最壞延遲變成 3 倍**：`crates/wukong-gateway/src/opencode_server.rs:284`、`:686-706`、`:918`、`:142`、`:793`
   - `self.client` 的 `.timeout(agent_timeout())` 預設 1200 秒（`:284`）。`has_background_jobs` 在 `select!` 外面 await（`:918`），所以等待期間 `deadline`（`:823`）和 `mapper.retry.expired()` 都不會觸發。
   - 錯誤立即回來時（503、連線被拒）沒問題，最多只多 1 秒。但如果 V2 的 HTTP 卡住、SSE 還活著，原本最壞要等 1×1200 秒，現在是 3×1200 秒再加 1 秒。
   - 取消與失敗路徑也一樣：`interrupt_turn` 多了一個 GET（`:142`）。`send_now` 會在錯誤回傳前 await 它（`:793`），所以回合逾時、V2 已經不正常時，使用者可能要再多等一個 client 逾時才看到錯誤。POST interrupt 本來就有這個問題。
   - 這是既有的界限被放大，需要 V2 卡住但不斷線才會遇到，所以不列為阻擋。建議在兩處的 job 查詢上加短的單次逾時（`RequestBuilder::timeout`，例如 10 秒），或把查詢放進 `select!` 跟 deadline 一起等。

2. **去重沒有測試鎖住**：`crates/wukong-gateway/src/opencode_server.rs:154`
   - 我把 `if !sessions.iter().any(|s| s == child)` 換成 `if true` 後，gateway 測試全部通過。
   - 現有的取消測試裡，串流看過的子 session 不在 scripted 清單裡；新測試裡，子 session 不在串流裡。兩邊沒有重疊，所以「串流看過、清單也有」這種情況沒有被測到。
   - 重複送 interrupt 應該無害，所以不影響行為。如果要補，可以讓 `a_cancelled_turn_interrupts_the_subagent_sessions_too` 的 `script_jobs` 也列出同一個 child，並斷言 interrupt 的次數。

3. **job 清單的查詢寫了兩份，而且行為不一致**（code-simplify）：`crates/wukong-gateway/src/opencode_server.rs:138-147`、`:687-690`
   - URL `…/api/experimental/job?sessionID={…}&descendants=true` 在兩處各組一次。
   - `interrupt_turn` 用 `response.json()` 解析，不看 HTTP 狀態（`:143`），回非 JSON 時 `.ok()` 會靜默丟掉。所以 401、503 都當成空清單，也不寫 log；只有送出失敗時才記 `turn_cancelled_jobs_lookup_failed`。`has_background_jobs` 走 `send_json`，會記狀態碼。
   - 兩種寫法結果都安全（不會中斷錯的 session），只是查不到時，取消路徑沒留下線索。
   - 建議讓 `ServerHandle` 提供一個 `list_jobs(session) -> Result<Value, …>`，檢查狀態並記 log，`has_background_jobs` 也改用它。這樣 URL 和錯誤處理只剩一份。

4. **中斷子 session 之後，V2 可能把回報送進 primary 的 inbox，並喚醒 primary**（`需決策`）：取消路徑整體（`crates/wukong-gateway/src/opencode_server.rs:130-160`）
   - 證據：在 2.3.2 的打包原始碼裡，子 session 的執行結束後，回報路徑會 `inbox.admit({… delivery:"steer"})`，接著 `wake(parentSessionID)`。`Job.prepareSubagentReport` 對狀態還是 running、沒被呼叫端取消的工作，會回 `mode:"background"`。所以由 Wukong 中斷的子 session（不是 `Job.cancel`），可能讓 primary 在回合取消之後再跑一次，佔住 session。`interrupt_on_drop` 的說明要避免的正是這件事。
   - 原始碼也有 `if(!y)return`（依子 session 的執行結果決定送不送），我判斷不出被中斷的執行一定會送回報。V2 也沒有取消 job 的 HTTP 端點，`/api/experimental/job` 只有 list。
   - 串流看過的子 session 從 #45 起就是這樣處理，不是本增量新增的；本增量把同一個做法套到沒看到的子 session，相對於原本讓 fixer 繼續跑，仍然是改善。
   - 待決問題：要不要接受這個殘餘風險？
     - (a) 記為已知差距；
     - (b) 用真實 V2 探測一次「取消派工中的回合後，primary 會不會自己再執行」，再決定要不要處理。
   - 建議：選 (a)，下次對真實 V2 做 probe 時一併觀察。不阻擋本次 PASS。

5. **文件措辭兩處**：`docs/issues/issue-0045/implementation-plan.md:739`、`:732`
   - `:739` 寫「`Job.cancelSession` 全檔只出現一次」。我在映像裡數，`cancelSession` 這個字串出現 5 次（服務名稱、定義、匯出等），呼叫處 `.cancelSession(` 才是只有 1 次（在 `Session.remove` 裡）。結論不受影響，建議改成「呼叫處只有一處」。
   - `:732` 把取消中斷歸到 SCN-012「回合取消與失敗時中斷 session 的既有保障」，但 SCN-012 的 Gherkin（README `@SCN-012`）只列了空輸出修復、上游錯誤分類與同 scope 拒絕，沒有提到取消中斷。這次沒有新增驗收，也沒有改可觀察結果，所以不算驗收缺口，只是對應的寫法比 Scenario 寬。

## 已查核維度

### 增量範圍內每個變動檔案的結論

| 檔案 | 提交 | 結論 |
|---|---|---|
| `crates/wukong-gateway/src/opencode_server.rs`：`has_background_jobs`（`:686-706`） | 364cebf | 正確。見下方「P2-1 重試」 |
| `crates/wukong-gateway/src/opencode_server.rs`：`InterruptOnDrop`、`ServerHandle::interrupt_turn`（`:66-160`） | 364cebf | 正確。見下方「P2-2 取消」；逾時見 NICE 1，去重測試見 NICE 2 |
| `crates/wukong-gateway/src/opencode_server.rs`：`authorize`、`subagent_jobs`（`:169-185`、`:776-778`） | 364cebf | 是行為不變的抽取：`authorize` 和原本 backend 內的 match 逐行相同；`subagent_jobs` 和原本的 `kind == "subagent"` 過濾相同。重複的 URL 見 NICE 3 |
| `crates/wukong-gateway/src/opencode_server.rs`：三項新測試（`:1935`、`:1971`、`:2054`），既有測試改用 `InterruptOnDrop::default()` | 364cebf | 都是真的測試，見下方「測試真偽」 |
| `crates/wukong-gateway/src/opencode_server/fake_v2.rs:103`、`:309-315` | 364cebf | `"fail"` 項目回 503 JSON，其他照舊。`Value == &str` 只在項目是字串 `"fail"` 時成立，不影響既有的陣列項目 |
| `AGENTS.md:305` | 364cebf | 和程式一致：「三次都失敗」等於 `JOBS_LOOKUP_ATTEMPTS = 3`（總共查 3 次）；取消的順序與「中斷父 session 不會取消背景 subagent」都有對應 |
| `CHANGELOG.md:24` | 364cebf | 放在 #45 的條目下，描述整體行為，正確。「都回報之後回合才結束」有一個例外沒寫：查詢三次都失敗時會提早收尾，這是使用者的決定，AGENTS.md 有寫，CHANGELOG 省略可以接受 |
| `docs/issues/issue-0045/README.md` | 2a49f82、cf47307、364cebf | 見下方「流程記帳」 |
| `docs/issues/issue-0045/implementation-plan.md` Task 6.8 | 364cebf | 證據和我重跑、讀程式的結果一致：workspace 781／0／14；紅燈的描述有兩項我實際重現了。措辭見 NICE 5 |
| `docs/issues/issue-0045/review-4ccfcec.md` | 2a49f82 | 新增後沒有再改過；`verify-artifact` 回 OK |

### P2-1 重試（`has_background_jobs`）

- **只在錯誤時重試、成功就返回**：`Ok(value) => return …`（`:696`）。`send_json` 把送出失敗、非 2xx 狀態和非 JSON 內容都當成 `Err`，所以這三種都會重試。回 200 且是 JSON、但沒有 `data` 時，視為成功、沒有工作，不重試，和原本一樣。
- **間隔**：只在還有下一次時才 sleep（`:701-703`），三次都失敗時總共多等 1 秒，並記 `attempt=n/3`。
- **對回合逾時與 `retry` watch 的影響**：查詢和 sleep 都在 `select!` 外面，等待期間兩者都不會觸發，回到迴圈後才生效。錯誤立即回來時最多晚 1 秒，可以接受；V2 卡住時的界限見 NICE 1。
- **呼叫點**：只有 Idle 分支（`:918`），而且只在事件看不出後續時才查。一個回合可能有好幾次 Idle，每次查詢各自重試，互不影響。
- **使用者的決定**：第三方建議「仍失敗則明確回報錯誤或降級狀態」，使用者選「以目前的回覆收尾」（TBD-15），程式照這個決定實作。

### P2-2 取消（`InterruptOnDrop`／`ServerHandle::interrupt_turn`）

- **兩條路徑都走到**：
  - `send_now`（`:86-92`）：`server.take()` 之後 await `interrupt_turn`。之後的 `disarm` 和 Drop 都看到 `None`，不會再送一次。
  - Drop（`:95-111`）：有 runtime 時 spawn `interrupt_turn`，結果逐筆記 log；沒有 runtime 時什麼都不送，和改之前一樣（原本也是 `try_current` 失敗就放棄）。
  - 兩條路徑都先 `take()` 掉 server，所以不會重複送。
- **`disarm` 之後不再送任何請求**：`server = None`、`sessions.clear()`，Drop 的 `let Some(server) … else { return }` 直接返回。正常完成的回合（`:795`）不會查清單，也不會中斷。
- **順序**：先依序中斷 `sessions`，第一個是 primary（`interrupt_on_drop` 建立時放入，`:665-674`），接著是串流中看到的子 session，最後才查清單。新測試斷言 primary 的中斷排在清單查詢之前。
  - 殘餘時間窗：primary 被中斷時，派工的工具呼叫如果正好建立了子 session，而且在查清單之後才出現在清單裡，那一個就漏掉了。這個窗口很窄，第三方也沒有要求做到這一點，可以接受。
- **清單查詢失敗或回非 JSON**：送出失敗時記 log、當成沒有；非 JSON 時 `.ok()` 回 `None`；非 2xx 但是 JSON（例如 401、503 的錯誤物件）時沒有 `data`，`subagent_jobs` 是空的。三種都安全，不會中斷錯的 session。查詢不重試，也不看狀態碼，見 NICE 3。
- **去重**：`sessions` 在迴圈裡會一直累加，所以清單裡重複的 child 和串流看過的 child 都只送一次。這一段沒有測試鎖住，見 NICE 2。
- **只挑 subagent**：用 `subagent_jobs` 過濾，背景 shell（新測試裡的 `sh_1`）不會被當成 session 中斷；沒有 `childSessionID` 的項目會略過。
- **孫 session**：查詢帶 `descendants=true`。V2 2.3.2 的 openapi 說明是「also include background jobs owned by child and nested subagent sessions」（我在映像裡查到），所以孫 session 的工作也會列出，並以它的 `childSessionID` 中斷。這一點沒有專屬測試，處理方式和子 session 相同。
- **中斷已結束的子 session**：清單也會列出「unconsumed」的工作，也就是已經完成、回報還沒被讀取的。中斷這種閒置的 session，在 V2 的執行層應該是 no-op（`interrupt` 找不到執行中的項目就返回）。這是我讀程式推論的，沒有對真實 V2 實測。假 server 一律回 200。
- **basic auth**：`ServerHandle` 從 backend 複製 `password`（`:670`），清單查詢（`:142`）和每一次 interrupt（`:163`）都經過 `ServerHandle::authorize`；`has_background_jobs` 走 `send_json`，裡面也有 authorize。假 server 對所有 `/api/` 路徑都檢查認證，沒有認證會回 401。新取消測試能找到 child，就表示清單查詢帶了正確的認證；如果沒帶，會回 401 JSON、清單是空的，測試就會失敗。interrupt 的認證我是讀程式確認的，假 server 雖然會回 401，但測試沒有檢查回應狀態。
- **壓縮也受影響**：壓縮也走 `consume_or_interrupt`（`:1051`），失敗時同樣會查清單、補中斷。壓縮在兩個回合之間執行，通常沒有 subagent 工作；即使有殘留，中斷它也合理。`dropping_a_compaction_interrupts_the_session` 仍然通過。

### V2 原始碼查證（`wukong-issue44:v2-local`，image af56722acade，唯讀 `docker run --rm`）

| Task 6.8 的說法 | 我查到的 | 判定 |
|---|---|---|
| `Session.interrupt` 只呼叫執行層的 `interrupt(sessionID)` | `u("Session.interrupt")((oH,lH)=>Mi(a.interrupt(oH,lH)))`。執行層的 interrupt 只有在帶 `resume` 時才 wake 該 session，沒有處理子 session 或 job | 成立 |
| `Job.cancelSession` 只有 `Session.remove` 呼叫 | 呼叫處 `.cancelSession(` 只有 1 處：`Session.remove` 依序呼叫 `interrupt` → `awaitIdle` → `cancelSession` → 刪除子 session。字串本身出現 5 次（NICE 5） | 成立（措辭待修） |
| `SubagentJob` 以 `resume(childSessionID)` 執行，只有工作被取消時才中斷子 session | `run:a.resume(l).pipe(Cs(()=>e.isCancelled(l).pipe(d((m)=>m?a.interrupt(l):h))))` | 成立 |
| 結論：中斷父 session 不會停掉背景 subagent | 從上面三點推得出來。job 有自己的 fiber 與 scope，不掛在父 session 的那次執行底下；fiber scope 的細節我沒有逐行追。文件寫的是「以原始碼查證」，沒有說是實測 | 沒有過度推論 |

另外查到的：V2 沒有取消 job 的 HTTP 端點（`/api/experimental/job` 只有 list），所以只能中斷子 session，不能用 `Job.cancel`。回報可能喚醒 primary 的問題見 NICE 4。

### 測試真偽

我在工作樹暫時改動實作，確認新測試會紅燈；每次都 `git checkout -- .` 還原，最後 `git status --short` 是空的。

| 改動 | 結果 |
|---|---|
| `JOBS_LOOKUP_ATTEMPTS` 改成 1（等於改之前的「失敗就當作沒有」） | `a_failed_job_lookup_is_retried_before_the_turn_ends` 失敗在 `:1958`，回覆開頭不是「fixer 已完成」；`a_job_lookup_that_keeps_failing_ends_the_turn_with_the_current_reply` 失敗，查詢次數 left 1、right 3 |
| `interrupt_turn` 中斷完 `sessions` 就返回（等於改之前沒有查清單） | `a_cancelled_turn_interrupts_subagents_the_stream_never_announced` 失敗，其餘 5 項中斷測試通過 |
| 拿掉去重判斷 | 全部通過，見 NICE 2 |

- 新測試的劇本是用真實樣本 `server-dispatch-subagent.ndjson` 改出來的：
  - 第一項拿掉子 session 的 `session.execution.started`，讓第一次 Idle 只能靠清單判斷；查 4 次的算法是兩次失敗、一次看到 fixer，加上最後一次 Idle 的空清單。
  - 第三項拿掉 `session.created` 之後 hold，500ms 時丟掉 future，Drop 路徑觸發。斷言包括：child 有被中斷、primary 的中斷先於查清單、總共 2 次 interrupt（shell 沒有被當成 session）。
- **`AGENT_TIMEOUT_ENV_LOCK`**：會把 `WUKONG_AGENT_TIMEOUT_SECS` 設成 1 到 5 秒的測試，`backend.rs`、`local_process.rs`、`opencode_server.rs` 一共 9 處，全都拿這把鎖（我逐一查了 `set_var`）。兩項重試測試本身跑約 1 秒，backend 建立時讀的 client 逾時和 deadline 都會受這個環境變數影響，所以拿同一把鎖合理。它是 `tokio::sync::Mutex`，可以跨不同測試的 runtime 使用。取消測試不受這個環境變數影響，沒拿鎖也合理。
- gateway lib 測試我連跑 3 次，193 項都通過，沒有看到不穩定。

### 驗收與證據（相對於 review-4ccfcec）

| 編號 | 本輪 | 結論 |
|---|---|---|
| SCN-002 | 重新查核：派工後等 subagent 回報的判定，查詢失敗時改成重試。兩項新測試先紅燈後綠燈，既有的派工測試（`run_streaming_replays_a_real_v2_turn` 等）照常通過 | 通過 |
| SCN-012 | 重新查核：回合失敗與取消時的中斷（Scenario 本身沒寫這項，見 NICE 5），新增「串流沒看到的子 session」。新測試先紅燈後綠燈，既有的 `a_failed_turn_interrupts_the_subagent_sessions_too`、`a_cancelled_turn_interrupts_the_subagent_sessions_too`、`dropping_the_turn_interrupts_the_session`、`dropping_a_compaction_interrupts_the_session` 通過 | 通過 |
| SCN-001、003～011、013、014 | 沿用。增量只改了 gateway 的 `opencode_server.rs` 與它的假 server，加上文件；其他 crate 沒有變動，workspace 781 項全過 | 通過（沿用） |

### 相關失敗面（本輪新增）

| 輸入／狀態 | 預期 | 覆蓋 | 判定 |
|---|---|---|---|
| 清單查詢暫時失敗（503 兩次），fixer 還在跑、事件看不出來 | 重試後看到 fixer，等回報才收尾 | 新測試（我做了變異確認） | 已覆蓋 |
| 清單查詢一直失敗 | 查 3 次、多 1 秒，以目前的回覆收尾 | 新測試（我做了變異確認） | 已覆蓋 |
| 清單回 200 但不是 JSON | `send_json` 回 Err，會重試 | 讀程式 | 可接受 |
| V2 的 HTTP 卡住（不回應、也不斷線） | 每次查詢等到 client 逾時，最壞 3 倍 | 讀程式 | 見 NICE 1 |
| 取消時串流沒看到子 session 的建立事件 | 從清單補中斷，不碰背景 shell | 新測試（我做了變異確認） | 已覆蓋 |
| 取消時清單查詢失敗、回 401／503 或非 JSON | 不中斷任何額外的 session，不 panic | 讀程式 | 可接受（沒有 log，見 NICE 3） |
| 子 session 同時出現在串流與清單 | 只中斷一次 | 讀程式 | 沒有測試（NICE 2） |
| 孫 session | 經 `descendants=true` 列出並中斷 | 讀程式與 openapi 說明 | 可接受，沒有專屬測試 |
| 清單裡已完成、未讀取的 subagent | 中斷閒置 session 應無害 | 讀程式推論 | 可接受，沒有實測 |
| 中斷子 session 後，回報喚醒 primary | 取消後 primary 可能再執行一次 | 原始碼推論，沒有結論 | NICE 4，`需決策` |
| 沒有 runtime 時 Drop | 不送任何請求，和改之前相同 | 讀程式 | 可接受 |
| `disarm` 之後 | 不送任何請求 | 讀程式；正常回合的既有測試會計算請求數 | 已覆蓋 |
| 壓縮失敗 | 一樣查清單、補中斷 | 既有壓縮中斷測試 | 可接受 |

### 需求、架構、安全、品質

- **需求**：P2-1、P2-2 的實作符合 TBD-15 記錄的使用者決定；P3 沒有動。沒有新增驗收，也沒有改任何 Scenario 的可觀察結果。
- **架構**：變動只在 `wukong-gateway` 的 server backend 內，沒有新的依賴，`AiBackend` 介面也沒有變。`ServerHandle` 是為了讓 Drop 在 backend 借用之外也能送請求而複製一份連線資訊，必要而且最小。
- **安全**：每個新請求都帶 basic auth；密碼只存在記憶體的 struct 裡，log 只印 session ID 和錯誤，不印密碼。
- **品質（code-simplify）**：`authorize`、`subagent_jobs` 的抽取確實去掉了重複。剩下的是 job 清單查詢的兩份寫法（NICE 3）。沒有過度設計：重試是固定常數加迴圈，沒有引入通用的 retry 抽象。

### 流程記帳

- 2a49f82：新增 review-4ccfcec.md，`**現行 review**` 改成 review-4ccfcec.md，Timeline 多一列。
- cf47307：TBD-14、Timeline、`**狀態**` 改成已完成。`verify-artifact` 對 4ccfcec → cf47307 回 OK。
- 364cebf 的 README 變更：TBD-15、Timeline 一列、`**狀態**` 改回「審查中」（docs/AGENTS.md 列舉的階段值）。`## 現況` 仍在 `## 概述` 之後（第 23 行）；`**現行 review**` 仍是 review-4ccfcec.md，由保存本報告的提交更新。
- **交付提醒**：
  - `origin/main` 已經是 f9bc86e，和 BASE 一致。
  - 遠端分支 `issue-0045-opencode-v2-slim` 與 PR #47 的 head 還是 cf47307，364cebf 還沒推送。
  - 回覆 PR、請第三方做第二輪之前，先推送，再確認 PR head 就是本報告的 Reviewed HEAD 或它的有效記帳後繼。

### 我執行的驗證（HEAD 364cebf，工作樹乾淨，2026-11-27）

| 命令 | 結果 |
|---|---|
| `git rev-parse 364cebf`；`git diff f9bc86e 364cebf \| git patch-id --stable` | 364cebff2c27…；patch-id e16add6515d8244c3c2657ebddd3376c993dd9cc |
| `git merge-base main 364cebf`；`git log --merges 4ccfcec..364cebf`；`git log --format='%h %p' 4ccfcec..364cebf` | f9bc86e；沒有輸出；線性的三個提交 |
| `verify-artifact.py --repo . --reviewed 4ccfcec --head cf47307 --artifact docs/issues/issue-0045/review-4ccfcec.md` | OK |
| `verify-artifact.py --repo . --reviewed 4ccfcec --head 364cebf --artifact docs/issues/issue-0045/review-4ccfcec.md` | INVALID（AGENTS.md 有變更），符合預期 |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --all-targets --locked -- -D warnings`（TMPDIR=target/tmp） | exit 0（本機 toolchain；W-9：CI 的 stable 可能較新） |
| `cargo test -p wukong-gateway --locked --lib`（連跑 3 次） | 每次都是 193 passed、0 failed |
| `cargo test --workspace --locked --no-fail-fast` | 781 passed、0 failed、14 ignored，和 Task 6.8 的數字相同 |
| 三種變異（見「測試真偽」）＋ `git checkout -- .` | 兩種紅燈如預期，一種全綠（NICE 2）；之後 `git status --short` 是空的 |
| `gh api repos/raybird/Wukong/issues/comments/7214581086 --jq .body`；`gh pr view 47 --json baseRefOid,headRefOid`；`git ls-remote origin` | 第三方審查原文；PR base f9bc86e、head cf47307 |
| `docker run --rm --entrypoint sh wukong-issue44:v2-local -c 'grep -a -o … opencode.exe'`（`Job.cancelSession`、`.cancelSession(` 計數、`Session.interrupt`、`isCancelled`、`prepareSubagentReport`、`experimental/job`） | 見「V2 原始碼查證」；沒有動任何正式容器 |

### 沿用的結論

增量沒有動到這些項目的程式與證據，沿用 review-4ccfcec：
- SCN-001、003～011、013、014 的驗收結論與測試真偽的抽查；
- Telegram、Web、排程的角色顯示（`ActiveSubagents`）與前端跳脫；
- 背景 shell 不算 subagent（本輪只把過濾抽成 `subagent_jobs`，語意相同）；
- 派工評估、TBD-10～TBD-14 的結論，以及架構相依圖與 primary 權限。

其餘沿用 review-4ccfcec.md。

### 豁免、待確認與限制

- **豁免**：沒有 Gate 豁免紀錄。
- **待確認事項**：TBD-1～TBD-15 都已解決，狀態我沒有改。NICE 4 標了 `需決策`。
- **限制**：
  - 我沒有啟動真實 V2 server，也沒有呼叫真實模型。中斷閒置 session 是否無害、被中斷的子 session 會不會喚醒 primary，都是讀打包原始碼推論的。
  - V2 原始碼是用 `grep -a` 從打包後的 minified 檔取出片段來讀的，沒有逐行追 fiber 與 scope 的關係。
  - 孫 session 和 Drop 在沒有 runtime 時的行為，只讀了程式。

## 流程判定
PASS

PASS 只適用於上列 Reviewed HEAD SHA；之後提交 README 記帳區段與本報告以外的檔案會讓它失效，提交前先告知使用者（review-evidence.md「審查之後的提交」）。
