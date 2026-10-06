# Issue 9 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)。

## 現況（2026-10-06 讀碼與 RunWuKong 資料確認）

- **寫入**：`turn.rs` 以 `captured_session` 或 `stored` 為 `turn_key`，key 為 `runtime:{turn_key}:user|assistant`；`store::insert_memory` 遇到相同 key 直接回傳既有 id，不寫入。原 commit `e9a9e85` 的目的是讓同一次回合的寫入冪等，但 session 長期沿用，結果第二回合起全部被當成重複。
- **召回**：`run_turn` 開頭以 `RecallMode::Hybrid`、`top_k = 5` 召回一次，結果注入每一棒。Hybrid 合併關鍵字 50 筆、同 scope 最新 50 筆與向量前 20 筆，依 `lexical 0.4 / semantic 0.2 / decay 0.25 / importance 0.15` 排序；沒有相關度門檻，最新的記憶靠時間與重要度分數就能進前 5。
- **注入**：`gateway::prompt::compose_prompt` 逐筆輸出全文，沒有長度上限。
- **session**：只有最後一棒帶 session；輔助棒每次都是新的、無狀態。session 預設每 20 回合壓縮一次（`WUKONG_SESSION_COMPACT_EVERY_TURNS`）。
- **合併**：schedulerd 每 900 秒檢查，未合併的 `event/note` 達 40 筆的 scope 會把全部候選每 20 筆交給 `OpencodeSummarizer`（`run_ephemeral`）摘要，寫入 `summary` 後標記並刪除原始資料。`memory_auto_maintenance` 以 `?` 傳遞錯誤，第一個失敗的 scope 會中止整輪；`Memory::consolidate` 不檢查摘要是否為空白。
- **其他 Hybrid 使用者**：Telegram、REPL、Web 的記憶查詢功能與 memoryd 也用 `RecallMode::Hybrid`，所以不能改變 Hybrid 本身的語意。

## 設計

### 寫入 key（SCN-001）

`run_turn` 開始時產生本回合專屬的識別碼，key 改為 `runtime:{scope}:{turn_id}:user|assistant`。同一次 `run_turn` 只寫一次；新的回合（即使輸入相同）一定寫入。識別碼的產生方式由實作時沿用 workspace 既有依賴決定，不新增 crate。

### 召回（SCN-002～004）

```diff
 run_turn
-  recall(Hybrid, top_k=5)            → 注入每一棒
+  relevant = recall(新模式：關鍵字＋向量，不含最近來源；向量需達相似度門檻)
+  last_turn = 本 scope 最近一個回合的 User／Assistant（不含本回合）
+  最後一棒注入 relevant
+  輔助棒注入 relevant ＋ last_turn（去重）
```

- 新增一個 `RecallMode` 變體給 `run_turn` 使用；既有 `Hybrid` 行為不變。（2026-10-06 更正：memoryd 直接反序列化 `RecallQuery`，所以 `"mode":"relevant"` 對外也可用；Web 記憶 API 只接受 `hybrid`。）
- 相關度判斷採結構訊號：命中來自 `keyword`、`cjk_fallback`，或向量相似度達門檻（TBD-2）。只靠 `recent` 入選的不算。
- 最近一個回合用既有的 `recent_candidates` 取得，限本 scope、最多 2 筆。

### 注入長度（SCN-005）

`compose_prompt` 對每筆記憶截斷到固定字元數（初始值 800 字元，以 `char` 計，避免切斷 UTF-8），超過時加上截斷標示。只影響注入內容，不改資料庫。

### 合併（SCN-006～008）

- `memory_auto_maintenance`：每個 scope 的 consolidate／prune 錯誤改為記錄 `memory_consolidate_failed scope=… error=…` 並繼續下一個 scope；報表加上失敗數。
- SCN-007：`Memory::consolidate` 收到空白摘要時，該批不寫摘要、不標記來源，回報錯誤，交給 scope 隔離處理。
- SCN-008：以 schedulerd 的維護路徑搭配按需 OpenCode 真實程序執行一次合併。模型輸出沿用 issue 7 的可控制方式（`docs/issues/issue-0007/probe-managed.py`），不呼叫外部 LLM。

## 實作步驟

1. ✅ **SCN-006：合併的 scope 失敗隔離** — 產出：`maintenance.rs` 的隔離與報表、以兩個 scope 一成一敗的測試。相依：無。完成判準：測試先因第二個 scope 未合併而失敗，修改後通過，且失敗 scope 的原始記憶數不變。
   - 影響分析（2026-10-06）：`gitnexus_impact(memory_auto_maintenance, upstream)` 為 HIGH，直接呼叫者為 schedulerd `run_once` 與既有測試，經 `run_memory_maintenance` 到 `run`；改動只限錯誤處理與報表欄位，成功路徑不變。
   - 紅燈：先只加 `AutoMaintenanceReport::scopes_failed` 欄位、不改行為，`cargo test -p wukong-runtime --lib maintenance::` exit 101；`auto_maintenance_isolates_a_failing_scope` 失敗於 `one failing scope must not abort the whole pass: Memory(Other("summarizer backend failed: … model unavailable"))`，即 project:A 的摘要失敗讓整輪回傳錯誤、project:B 未處理。
   - 綠燈：每個 scope 的處理移入 `maintain_scope`，錯誤時記 `memory_consolidate_failed scope=… error=…` 並計入 `scopes_failed` 後繼續。同命令 exit 0，4 passed。測試以字面值斷言：A 的兩筆原文保留、B 只剩 `summary`、`scopes_failed=1`、`memories_pruned=2`。
   - 單迴圈合併：被測行為就是 `memory_auto_maintenance` 對外的報表與資料結果，整合測試（真實 SQLite＋假 backend）已涵蓋，沒有另一層底層責任。
   - 下游與靜態檢查：`cargo test -p wukong-runtime -p wukong-schedulerd` 全綠（70／22／2／0 passed）；`cargo clippy -p wukong-runtime -p wukong-schedulerd --all-targets -- -D warnings` 通過；`cargo fmt --all -- --check` 通過（rustfmt 只調整換行）。
   - code-simplify：no-op，抽出函式與錯誤隔離已是最小改動，綠燈即最終狀態證據。
2. ✅ **SCN-007：空白摘要不刪原始記憶** — 產出：`Memory::consolidate` 的空白檢查與測試。相依：步驟 1。完成判準：空白摘要的測試先紅後綠，來源記憶未被標記。
   - 影響分析（2026-10-06）：`gitnexus_impact(Memory.consolidate, upstream)` 為 HIGH，直接呼叫者為自動維護、手動 `memory_consolidate`（`wukong memory consolidate`）與兩個既有測試，另經 scheduler executor 的合併 job。只改變「摘要為空白」時的結果：由寫入空摘要改為回傳錯誤，三個呼叫端都已會呈現錯誤。
   - 紅燈：`cargo test -p wukong-memory --lib consolidate_keeps_sources` exit 101，`consolidate_keeps_sources_when_summary_is_blank` 失敗於 `blank summary must be reported as a failure`，即摘要回傳 `" \n"` 時仍成功寫入。
   - 綠燈：摘要 `trim()` 為空時回傳 `MemoryError::Other`，不寫摘要、不標記來源。同命令 exit 0；斷言來源仍是同一批 2 筆候選、沒有 summary 記憶、`prune_consolidated` 刪除 0 筆。
   - 層級說明：SCN-007 的 When 是自動維護；自動維護把 `consolidate` 的錯誤交給步驟 1 的 scope 隔離（已由 `auto_maintenance_isolates_a_failing_scope` 證明會保留來源並繼續），所以只在 `consolidate` 這一層補紅燈，沒有遺漏另一層保障。
   - 下游與靜態檢查：`cargo test -p wukong-memory -p wukong-runtime` 與 `cargo test -p wukong-scheduler -p wukong-schedulerd` 全綠；`cargo clippy -p wukong-memory -p wukong-runtime -p wukong-scheduler -p wukong-schedulerd --all-targets -- -D warnings` 通過；`cargo fmt --all -- --check` 通過。
   - code-simplify：no-op，一個空白檢查已是最小改動。
3. ✅ **SCN-008：按需模式 schedulerd 真實合併** — 產出：真實程序的驗證腳本或 ignored 整合測試，以及執行紀錄。相依：步驟 1。完成判準：紀錄中有非空摘要、被刪除的來源筆數，以及結束後沒有 OpenCode 程序；若揭露其他失敗，回報後再決定處理方式。
   - 探針：[probe-consolidate.py](./probe-consolidate.py)。真正的 `target/debug/wukong-schedulerd --once`（被測提交 `03f9d14`）、`WUKONG_AGENT_CMD=opencode run`、未設 server URL、OpenCode 1.18.31，模型為本機假的 OpenAI 相容 server，不呼叫外部 LLM。隔離 XDG、HOME 與資料庫，以包裝腳本記錄每個 opencode PID。需要有 sqlite3 模組的 Python（pyenv 3.11.11 沒有，使用 `/usr/bin/python3`）。
   - 結果（2026-10-06）：`/usr/bin/python3 docs/issues/issue-0009/probe-consolidate.py` exit 0，約 4 秒。40 筆 `event` 變成 2 筆 `summary`，內容皆為 `PROBE_SUMMARY sources=20`（假模型在 prompt 裡實際數到的來源數）；日誌 `memory_consolidated scope=user:tg-probe summaries=2 pruned=40`；啟動 2 個 OpenCode、結束後殘留 0 個；兩個臨時 session 都有 `session_deleted`。
   - 判準反向自檢：合併沒發生時資料表仍是 40 筆 `event`；摘要不是模型產生的時內容不會是 `sources=20`；程序沒收尾時 `leftover_pids` 非空。三者都會讓探針失敗。
   - 對照組（同時是 SCN-007 的真實程序證據）：`PROBE_BLANK=1 /usr/bin/python3 docs/issues/issue-0009/probe-consolidate.py` exit 0。假模型回空白，第一批後該 scope 停止（1 次摘要呼叫），40 筆 `event` 原封不動，日誌 `memory_consolidate_failed scope=user:tg-probe error=memory error: summarizer returned a blank summary for 20 source memories in user:tg-probe`，殘留程序 0 個。也證明按需 backend 確實會把空白回覆原樣交給摘要器。
   - 未涵蓋：真實外部 LLM 的摘要品質、容器內執行（本機程序，與 issue 7 的 Docker 證據分開）。
4. ✅ **SCN-005：注入長度上限** — 產出：`compose_prompt` 截斷與測試。相依：無。完成判準：超長記憶的測試先紅後綠，斷言用字面期望值，資料庫內容不變。
   - 影響分析（2026-10-06）：`gitnexus_impact(compose_prompt, upstream)` 為 CRITICAL，所有回合的每一棒都經過它（`persona::build_prompt`／`build_prompt_with_skill` → `run_turn`）。已告知使用者；控制方式是只改超過上限的記憶，短記憶輸出逐字不變，並跑 gateway＋runtime 全部既有測試。
   - 紅燈：`cargo test -p wukong-gateway --lib prompt::` exit 101，`long_memory_is_truncated_and_marked` 的 `assert_eq!` 失敗（801 字原文照樣注入）。
   - 綠燈：`MAX_MEMORY_CHARS = 800`，以 `char_indices().nth(800)` 找切點，超過時輸出前 800 字＋`…（已截斷）`。同命令 exit 0，3 passed。期望值為字面組成：恰好 800 字的「記」原樣保留，801 字的切成 800 字加標示，以字元而非位元組計算。
   - 資料庫不變：`compose_prompt` 只接收 `&[RecallHit]` 唯讀切片、不持有記憶庫，結構上無法改寫原始記憶。
   - 單迴圈合併：截斷是 `compose_prompt` 這個純函式的輸出行為，單元測試即是對外可觀察層級。
   - 回歸與靜態檢查：`cargo test -p wukong-gateway -p wukong-runtime` 全綠（131／70 passed，5 ignored 為既有的真實 OpenCode 測試）；`cargo clippy -p wukong-gateway -p wukong-runtime --all-targets -- -D warnings` 通過；`cargo fmt --all` 只調整本檔換行。
   - code-simplify：no-op。
5. ✅ **SCN-002、SCN-003：最後一棒只注入相關記憶** — 產出：新的召回模式、相關度門檻、`run_turn` 最後一棒改用它，以及測試。相依：步驟 4。完成判準：以假 backend 擷取最後一棒的 prompt，「只有不相關的最新記憶」時沒有 `[相關記憶]`，「有相關舊記憶」時含該筆；兩者先紅後綠。斷言檢查命中來源（`source_signals`），不只檢查筆數。
   - 影響分析（2026-10-06）：`sources_for_mode` 與 `run_turn_traced_with_attachments` 皆為 CRITICAL（`Memory::recall` 被 Web 預覽、Telegram、REPL 與所有回合使用；`run_turn_traced_with_attachments` 是四個入口共用的回合主流程）。已告知使用者。控制方式：新增 `RecallMode::Relevant` 只給 `run_turn` 用，Keyword／Tree／Hybrid 行為不變；Web 記憶 API 只接受 `hybrid`。（2026-10-06 更正：memoryd 可以選到 `relevant`，見設計段落。）
   - 紅燈：先寫 `final_step_omits_memories_selected_only_by_recency`、`final_step_keeps_older_relevant_memory`，`cargo test -p wukong-runtime --lib final_step_` exit 101。前者在 `!final_prompt.contains("[相關記憶]")` 失敗（三筆中文、與英文輸入無共同詞的記憶因「最近」來源被注入）；後者含相關的 `deploy port is 8787`，但在 `晚餐吃拉麵 leaked into the final step` 失敗。
   - 綠燈：新增 `RecallMode::Relevant`（關鍵字＋向量、無最近來源；向量命中須 cosine ≥ `MIN_RELEVANT_VECTOR_SIM` 0.4），`run_turn` 改用它。同命令 exit 0。
   - 向量門檻（embedding 開啟時）：`wukong-memory/tests/integration.rs` 的 `relevant_mode_requires_vector_similarity_floor` 以 stub embedder 讓兩筆記憶對查詢的 cosine 為 0.9 與 0.1，期望只回 `["near memory"]`。紅燈取得方式：測試寫在實作之後，因此暫時把 retain 條件改為恆真再跑，`left: ["near memory", "far memory"]`、exit 101；還原後 exit 0（`git diff` 確認還原為原實作）。
   - 斷言以「哪些文字出現在 prompt」判定來源，沒有只檢查筆數；不相關記憶刻意與輸入沒有共同詞，排除了關鍵字命中。
   - 回歸與靜態檢查：`cargo test --workspace` passed=623 failed=0 ignored=9（新增整合測試前）；之後 `cargo test -p wukong-memory` passed=99 failed=0；`cargo clippy --all-targets -- -D warnings` 通過；`cargo fmt --all -- --check` 通過。`cargo check -p wukong-memory --features embed` 因本機缺 `openssl-sys` 建置環境失敗，改動前的基準版本同樣失敗，與本次無關；向量門檻程式不在 feature 條件內，已由預設建置編譯並由 stub 測試執行。
   - 中間狀態：本步驟後輔助棒也只拿到相關記憶，失去最近一回合；由步驟 6 補上。
   - code-simplify：no-op。
6. ✅ **SCN-004：輔助棒取得最近一個回合** — 產出：輔助棒的注入組合與測試。相依：步驟 5。完成判準：多棒回合中輔助棒 prompt 含上一回合、不含更早的不相關回合，先紅後綠。
   - 影響分析：與步驟 5 同為 `run_turn_traced_with_attachments`（CRITICAL，已告知）。本步驟只改輔助棒的注入內容，最後一棒仍用步驟 5 的相關記憶。
   - 紅燈：`helper_steps_receive_only_the_previous_turn` 先寫，`cargo test -p wukong-runtime --lib helper_steps_receive` exit 101，失敗於 `helper.contains("User: 晚餐吃什麼")`（步驟 5 後輔助棒拿不到上一回合）。
   - 綠燈：新增 `with_previous_turn`，以 `Memory::records(scope, Event, 2)`（`created_at DESC, id DESC`）取該 scope 最新兩筆回合記憶，轉成 `source_signals = ["previous_turn"]` 的命中、依時間由舊到新、與相關記憶去重後只給輔助棒；因此也套用步驟 4 的 800 字上限。`cargo test -p wukong-runtime --lib` exit 0，73 passed。測試以字面值斷言：輔助棒含 `User: 晚餐吃什麼`／`Assistant: 吃拉麵`，不含更早的 `舊問題`／`舊回答`；最後一棒不含上一回合。
   - 時序：`run_turn` 在回合結束才寫入記憶，所以取最新兩筆時本回合尚未寫入，取到的是上一回合。
   - 回歸與靜態檢查：`cargo test --workspace` passed=625 failed=0 ignored=9；`cargo clippy --all-targets -- -D warnings` 通過；`cargo fmt --all -- --check` 通過。
   - code-simplify：no-op。
7. ✅ **SCN-001：每個回合都寫入記憶** — 產出：回合識別碼與 key 修改、連續兩回合的測試。相依：步驟 1、3、5、6（先讓召回與合併準備好，再啟動寫入）。完成判準：同 session 連續兩回合（含相同輸入）後有四筆，測試先紅後綠。
   - 影響分析：`run_turn_traced_with_attachments`（CRITICAL，已告知）；只改寫入的 dedupe key。`uuid`（v4）為既有 workspace 依賴，`wukong-runtime/Cargo.toml` 加入引用，`Cargo.lock` 只多一行依賴關係。
   - 紅燈：`every_turn_in_a_reused_session_is_remembered` 先寫，`cargo test -p wukong-runtime --lib every_turn_in_a_reused` exit 101，第二回合後 `left: 2, right: 4`。與 RunWuKong 實測相同：同一 session 的第二回合沒有寫入。
   - 綠燈：`turn_key` 改為 `scope:{scope}:turn:{uuid v4}`，每次 `run_turn` 一個。`cargo test -p wukong-runtime --lib` exit 0，74 passed。斷言以字面值檢查：兩回合輸入相同，第一回合後 2 筆、第二回合後 4 筆，內容為兩筆 `User: same question` 與 `Assistant: first answer`／`Assistant: second answer`，session 都是 `ses_new`。
   - 「同一回合只寫一次」：`run_turn` 每回合只呼叫一次 `remember`，第一回合後恰為 2 筆。
   - 回歸與靜態檢查：`cargo test --workspace` passed=626 failed=0 ignored=9；`cargo clippy --all-targets -- -D warnings` 通過；`cargo fmt --all -- --check` 通過。
   - code-simplify：no-op。
8. ✅ **收尾** — 產出：AGENTS.md「一回合資料流」更新、CHANGELOG `[Unreleased]`、全量 `cargo test`、`cargo clippy --all-targets -- -D warnings`、`gitnexus_detect_changes`。相依：步驟 1～7。完成判準：命令全綠，文件描述與實作一致。
   - 常青文件（2026-10-06）：AGENTS.md「一回合資料流」改寫召回、注入與寫入三點；CHANGELOG `[Unreleased]` 新增 Changed（最後一棒只注入相關記憶、800 字上限）、Fixed（寫入 key、scope 隔離、空白摘要）與已知限制。純文件，以人工逐句對照實作（`RecallMode::Relevant`、`MAX_MEMORY_CHARS`、`with_previous_turn`、`turn_key`、`maintain_scope`）確認一致。
   - 全量檢查：程式碼自步驟 7（`20edf32`）後未再變動，沿用該次結果：`cargo test --workspace` passed=626 failed=0 ignored=9、`cargo clippy --all-targets -- -D warnings`、`cargo fmt --all -- --check` 皆通過；SCN-008 探針結果見步驟 3。
   - `gitnexus_detect_changes`：每次提交前執行（步驟 2 為提交後補跑，已於該步記錄）；本步驟只有文件。

### 審查退回後的修正（2026-10-06，review-cc78101）

- **查詢端排除無鑑別力的詞**：`Relevant` 模式組 FTS 查詢時排除既有 `STOPWORDS` 與回合記憶固定帶的角色標籤 `user`／`assistant`；全部被排除時不做關鍵字召回。`Hybrid` 不變。
- **規則文字不進記憶（SCN-009）**：runtime 定義檔案互動規則區塊的標頭常數，Telegram 改用它組輸入；`run_turn` 以標頭切出使用者原文，召回查詢與 `User:` 記憶只用原文，planner 與各棒 prompt 仍用完整輸入。

## 實作步驟（審查退回後追加）

9. ✅ **SCN-002（退回修正）：同語言輸入不因停用詞或角色標籤命中** — 產出：`Relevant` 查詢排除 `STOPWORDS` 與 `user`／`assistant`，同語言的 runtime 測試。相依：步驟 5。完成判準：英文記憶（含 `User:`／`Assistant:` 與 the、is）配同語言但無實質共同詞的輸入時，最後一棒沒有 `[相關記憶]`，先紅後綠；SCN-003 仍綠。
   - 影響分析（2026-10-06）：`fts_match_string` 為 CRITICAL（唯一呼叫者 `Memory::recall`）；行為不變，只把組字串抽成 `or_match` 共用。新過濾只在 `Relevant` 生效。
   - 紅燈：`final_step_ignores_stopword_and_role_label_matches` 先寫（記憶 `User: what is the weather today`／`Assistant: It is sunny in the city`，輸入 `deploy the server for this user`，同語言、只共用 the 與 user），`cargo test -p wukong-runtime --lib final_step_ignores` exit 101，失敗於 `!final_prompt.contains("[相關記憶]")`，重現審查 MUST FIX。
   - 綠燈：新增 `relevant_match_string`（排除 `STOPWORDS` 與 `TURN_ROLE_LABELS`＝user／assistant，全排除時回 None），`Relevant` 模式改用它。`cargo test -p wukong-runtime --lib final_step_` exit 0，SCN-002 兩個測試與 SCN-003 皆綠。
   - 單元守門：`relevant_match_string_drops_stopwords_and_turn_labels` 以字面值鎖定 `"deploy" OR "server" OR "this"`、全排除時 None、`fts_match_string` 原樣不變。此測試寫在實作之後，未取得紅燈，作為回歸守門；紅燈由上面的 runtime 測試提供。
   - 回歸與靜態檢查：`cargo test --workspace` passed=628 failed=0 ignored=9；`cargo clippy --all-targets -- -D warnings`、`cargo fmt --all -- --check` 通過。
   - code-simplify：抽出 `or_match` 讓兩種模式共用組字串，無其他改動。
10. ✅ **SCN-009：Telegram 規則文字不進入記憶與召回** — 產出：runtime 規則標頭常數與切分函式、`dispatch.rs` 改用常數、測試。相依：步驟 9。完成判準：附規則的輸入跑完回合後，`User:` 記憶只含原文；過去附規則的記憶不因規則文字被召回；最後一棒 prompt 仍含規則；先紅後綠。
   - 影響分析（2026-10-06）：`run_turn_traced_with_attachments`（CRITICAL，已告知）；`prompt_with_artifact_instruction`（CRITICAL，Telegram 所有訊息的必經路徑，已告知），只把字面標頭換成常數，輸出逐字不變。
   - 紅燈：先只加 `persona::FILE_RULES_HEADER` 常數（不改行為），再寫 `file_rules_reach_the_prompt_but_not_memory_or_recall`（過去記憶 `User: 晚餐吃拉麵`＋規則、輸入 `部署伺服器的步驟`＋規則）。`cargo test -p wukong-runtime --lib file_rules_reach` exit 101，失敗於 `!final_prompt.contains("晚餐吃拉麵")`：舊記憶只因共用規則文字被召回。
   - 綠燈：`run_turn` 以 `"\n\n" + FILE_RULES_HEADER` 切出 `user_text`，召回查詢與 `User:` 記憶改用它；planner 與各棒 prompt 仍用完整輸入。`dispatch.rs` 改用同一常數。`cargo test -p wukong-runtime --lib` exit 0，76 passed。字面值斷言：最後一棒含 `[Wukong 檔案互動規則]`、不含 `晚餐吃拉麵`，寫入的 User 記憶恰為 `["User: 部署伺服器的步驟"]`。
   - 層級說明：Telegram 端只負責以常數組字串，標頭一致由同一常數保證；切分與記憶行為在 runtime 整合測試驗證，沒有另寫 Telegram 端對端測試。`cargo test -p wukong-telegram` 全綠（33／3／1 passed）。
   - 既有資料：修正前已寫入、含規則文字的舊記憶仍在，但查詢不再帶規則文字，不會因此被召回。
   - 回歸與靜態檢查：`cargo test --workspace` passed=629 failed=0 ignored=9；`cargo clippy --all-targets -- -D warnings`、`cargo fmt --all -- --check` 通過。
   - code-simplify：no-op。
11. ⏳ **退回項目收尾** — 產出：SCN-004 改為三棒鏈逐棒斷言；`docs/memory.md` 召回模式與防重複說明更新；AGENTS.md「防重複」措辭；CHANGELOG 已知限制補 TBD-3、無 session 後端與 session 輪替那一回合失去近期脈絡；全量檢查；新的獨立審查。相依：步驟 9、10。完成判準：文件與實作一致，全量命令全綠。

## 測試策略

| 驗收 | 層級 | 位置 |
|------|------|------|
| SCN-001 | runtime 整合（假 backend＋暫存 SQLite） | `wukong-runtime` 的 `turn.rs` 測試 |
| SCN-002～004 | runtime 整合，擷取各棒 prompt | 同上 |
| SCN-005 | 單元 | `wukong-gateway/src/prompt.rs` |
| SCN-006、SCN-007 | runtime／memory 整合（假 summarizer） | `maintenance.rs`、`wukong-memory` 測試 |
| SCN-008 | 真實程序（按需 OpenCode、可控制模型輸出） | `wukong-schedulerd` ignored 測試或 issue 目錄的探針，紀錄保存在本計畫 |

SCN-002～006 的外迴圈與內迴圈都在同一個整合層級，可依 verification.md 合併紅燈並記錄理由。

## 使用方式對照

| 情境 | 變更前 | 變更後 |
|------|--------|--------|
| 長期聊天 scope 的第 2 回合起 | 不寫入記憶 | 每回合寫入 |
| 一般追問（「那剛剛那個呢」） | 最後一棒收到最近 5 筆全文 | 最後一棒靠 session；輔助棒收到上一回合 |
| 20 回合前討論過的細節 | 幾乎召回不到（只有第一回合在庫） | 關鍵字或語意相符時召回 |
| 單一 scope 摘要失敗 | 本輪其他 scope 全部跳過 | 只跳過該 scope |

## 檢查清單

- [x] 每個 production 改動前執行 `gitnexus_impact` 並回報影響範圍
- [x] 紅燈原因與目標行為相關，期望值不由實作重算
- [x] 不改 `RecallMode::Hybrid` 的既有行為與 schema
- [x] 提交前 `gitnexus_detect_changes()`（步驟 2 為提交後補跑）
