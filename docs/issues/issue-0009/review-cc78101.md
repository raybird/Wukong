# 審查報告
- 範圍：完整 PR（TARGET main，`f0b1a28..cc78101`，10 個提交）
- Reviewed BASE SHA：f0b1a28f01aebcb7faebe94aad4824d69cf03b3d
- Reviewed HEAD SHA：cc781014916aef6b3df6467c933b02abbb333ac9
- Reviewed patch-id：46feec402ef22c534d2bbdadfcb88220dab3eaf3（`git diff f0b1a28 cc78101 | git patch-id --stable` 重算一致）
- 獨立 reviewer：Claude subagent（general-purpose，獨立於實作 session）
- Review artifact：docs/issues/issue-0009/review-cc78101.md
- 審查日期：2026-10-06

範圍確認：分支 `issue-0009-memory-turn-key` 的 HEAD 等於 cc78101，工作區乾淨。`git diff a51009c cc78101 -- docs/issues/issue-0009/README.md` 只改了核准 commit、TBD-2 狀態與文件狀態列，SCN-001～SCN-008 的 Gherkin 原文沒有變動。

## 問題與風險

### MUST FIX

1. **SCN-002 在真實資料下不成立，而現有測試因為用了跨語言資料而看不到。**
   位置：`crates/wukong-memory/src/lib.rs:343`（`RecallMode::Relevant => keyword`），`crates/wukong-memory/src/recall/mod.rs:131`（`fts_match_string` 把所有 token 以 OR 串接，不排除停用詞），`crates/wukong-runtime/src/turn.rs:1503`（SCN-002 測試）。
   `Relevant` 把「只要有任一個 FTS token 命中」視為相關。命中後在 `rank` 裡依 lexical／decay／importance 排序，所以弱命中的記憶之間又變成由時間決定，也就是 SCN-002 要排除的「只因時間接近而入選」。下列三種情況都由我用本 HEAD 的 `wukong-memoryd`（暫存 DB，`/v1/recall` 帶 `"mode":"relevant"`）實際重現：
   - **Telegram 預設部署（主要情境）**：`artifact_return_enabled()` 在按需模式與 server `shared` 模式下都預設為 true（`crates/wukong-telegram/src/dispatch.rs:716-734`），每則輸入都會附加約 100 字的 `[Wukong 檔案互動規則]`（`dispatch.rs:774`）。這段文字同時是召回查詢與寫入的 `User:` 記憶，因此每一筆過去的 Telegram User 記憶都和任何新輸入共用大量二元組。實測：三筆記憶「晚餐吃什麼／明天天氣如何／推薦一本小說」（各附規則文字），查詢「部署伺服器的步驟」＋規則文字，三筆全部以 `['keyword']` 回傳。觸發本 issue 的 RunWuKong Telegram 情境裡，最後一棒實際上仍會注入最近的回合。
   - **run_turn 自己寫入的角色標籤**：每筆回合記憶的 `search_text` 都含 `User`／`Assistant`。查詢「user table schema」時，「User: 我們晚餐吃什麼」與「User: what is the weather today」都以 `keyword` 命中。
   - **程式碼自己定義的停用詞**：`STOPWORDS`（`recall/mod.rs:7`，含 the/is/a…）只用在 trivial gate，沒有從 MATCH 排除。查詢「deploy the server」時，「User: what is the weather today」與「Assistant: It is sunny in the city」都只靠 `the` 就以 `keyword` 命中。

   SCN-002 與 SCN-003 的測試把中文記憶和英文輸入配在一起，在結構上不可能共用 token，所以分辨不出上述失敗。CHANGELOG 已知限制寫了「Telegram 的使用者記憶仍包含附加的規則文字」，但沒有揭露這段文字會讓 Telegram 的相關度過濾完全失效，揭露內容低估了影響。
   建議：補一個同語言的 SCN-002 紅燈測試（至少涵蓋停用詞與角色標籤），並在 `Relevant` 的查詢端排除 `STOPWORDS` 與 `user`／`assistant`。這樣只改 `recall` 模組，不動 store 或 Hybrid。Telegram 規則文字的問題落在 README 標為「不可觸及」的 `dispatch.rs`，需要使用者決定：擴大範圍（召回用原始輸入，或者寫入、召回時去掉規則文字），或者在 README 待確認事項與 CHANGELOG 明確記錄「Telegram 啟用檔案回傳時 SCN-002 不成立」並取得同意。這不是退步（改動前 Hybrid 一樣會注入最近 5 筆），但目前 SCN-002 被回報為完成，與實際行為不符。

### SHOULD FIX

1. **中文高頻二元組同樣算關鍵字關聯。** 查詢「我們明天要部署伺服器」只靠「我們」就命中「User: 我們晚餐吃什麼」（`keyword`）。中文停用詞要列到什麼程度屬於規格決策，建議記成待確認事項，並在 CHANGELOG 已知限制說明。
2. **不回傳 session 的後端，最後一棒失去近期脈絡，但沒有揭露。** `docs/docker.md:193` 說明「額外 run 旗標與任意命令走純 CLI」。這類命令不會輸出 `sessionID`。改動前的 fallback key `scope:{scope}:input:{input}` 會寫入每個不同的輸入，Hybrid 也會帶入最近的記憶；改動後最後一棒只拿相關記憶，又沒有 session 可以續接。session 因 compaction 回 404／405 而輪替的那一回合（`crates/wukong-runtime/src/session.rs:127-137`）也一樣。這是 SCN-002 的設計結果，但建議寫進 CHANGELOG 已知限制。
3. **新召回模式在 memoryd 對外可用，文件與計畫的說法不準確。** `crates/wukong-memoryd/src/lib.rs:97` 直接反序列化 `RecallQuery`，所以 `"mode":"relevant"` 從原本被拒絕變成可以使用（已實測）。implementation-plan 步驟 5 寫「外部無法選到新模式」，與事實不符。`docs/memory.md:8` 的模式清單沒有更新；`docs/memory.md:7`「turn 記憶帶 dedupe_key，重試時以同一列 id 回傳」在 key 改成每回合 uuid 後已經過時。建議修正這些敘述，或在 memoryd 拒絕 `relevant`。

### NICE TO HAVE

1. `crates/wukong-schedulerd/src/main.rs:156-165`：完成日誌沒有輸出 `scopes_failed`，而且只有失敗時不會印出整輪摘要。某個 scope 的第一批持續空白或失敗時，每 900 秒都會重新呼叫一次摘要模型，後面的批次也永遠輪不到。資料是安全的，但建議在日誌顯示失敗數，並在 CHANGELOG 提一句。
2. `crates/wukong-runtime/src/turn.rs:457`：自動維護會合併（並刪除）該 scope 全部的 event，包含最新一個回合，所以合併後下一個回合的輔助棒拿不到上一回合。另外 `records()` 不過濾 `consolidated_into`，會讀到已標記但尚未刪除的來源（無害）。建議在計畫的限制欄記錄。
3. 測試強度：步驟 5 的完成判準寫「斷言檢查命中來源（source_signals）」，實際斷言的是 prompt 文字。在跨語言資料下兩者等價，但正因為如此才沒抓到 MUST FIX 1。SCN-004 的測試只有一個輔助棒，Gherkin 寫的是「每個輔助棒」，可以改成三棒鏈各別斷言。
4. 回合記憶的 `dedupe_key` 現在每回合唯一，而 `run_turn` 只呼叫一次 `remember`，所以 key 實際上不再擋任何重複。AGENTS.md「每回合一個識別碼防重複」的說法偏強，可以改成「每回合一個識別碼，不再以 session 去重」。
5. README Timeline 沒有記錄 TBD-2 在 2026-10-06 定案。
6. 與本 PR 無關：`crates/wukong-gateway/src/local_process.rs:357` 的 `invalid_listener_releases_child` 在我第二次跑全量測試時失敗一次（`assertion failed: error.to_string().contains("loopback")`）。單獨重跑 5 次全過，全量重跑也全過，這個檔案不在 diff 內，判定為既有的不穩定測試。

## 已查核維度

### 驗收與證據

重跑結果（2026-10-06，HEAD cc78101）：`cargo test --workspace` passed=626 failed=0 ignored=9（另有一次因上述無關的不穩定測試失敗）；`cargo clippy --all-targets -- -D warnings` exit 0；`cargo fmt --all -- --check` exit 0；`cargo build -p wukong-schedulerd` 後執行 `/usr/bin/python3 docs/issues/issue-0009/probe-consolidate.py`，exit 0，`memory_consolidated scope=user:tg-probe summaries=2 pruned=40`，2 筆 `PROBE_SUMMARY sources=20`，啟動 2 個 opencode，殘留 0 個；`PROBE_BLANK=1` 對照組 exit 0，1 次摘要呼叫，40 筆 event 原封不動，日誌有 `memory_consolidate_failed … blank summary for 20 source memories`，殘留 0 個。

| SCN | 證據 | 結論 |
|-----|------|------|
| SCN-001 | `turn.rs:1613` `every_turn_in_a_reused_session_is_remembered`；紅燈 `left: 2, right: 4`。期望值是字面值（2→4 筆、文字清單、`ses_new`） | 成立。紅燈原因就是目標行為。「同一回合只寫兩筆」由第一回合後恰為 2 筆證明 |
| SCN-002 | `turn.rs:1503`、`tests/integration.rs:520`（向量門檻，stub cosine 0.9／0.1，紅燈靠暫時改成恆真取得並已記錄） | **測試本身有效，但覆蓋不足**：只用跨語言資料，真實的同語言或 Telegram 資料下行為不成立（MUST FIX 1） |
| SCN-003 | `turn.rs:1533`；紅燈由「不相關記憶外洩」驅動，「保留相關舊記憶」的斷言在改動前本來就成立 | 以回歸守門而言成立。在 `Relevant` 下不相關記憶不會進入候選，結構上擠不掉相關記憶 |
| SCN-004 | `turn.rs:1582`；紅燈 `helper.contains("User: 晚餐吃什麼")` 失敗；字面值斷言含上一回合、不含更早回合，最後一棒也不含上一回合 | 成立（「每個輔助棒」只驗了一棒，見 NICE 3） |
| SCN-005 | `prompt.rs:70`；800 與 801 個「記」的完整字串以 `assert_eq!` 比對 | 成立。字元而非位元組，期望值獨立。`compose_prompt` 只拿唯讀切片，不會改資料庫 |
| SCN-006 | `maintenance.rs:267`；紅燈為整輪回傳 Err。A／B 字面值斷言，`scopes_failed=1`。不論 scope 的排序先後，沒有隔離時測試都會紅 | 成立。「記錄在日誌」由探針對照組的真實 stderr 證明（同一個 `memory_consolidate_failed` 分支） |
| SCN-007 | `lib.rs:778` 單元測試（紅燈 `blank summary must be reported as a failure`）＋探針 `PROBE_BLANK=1` 真實程序 | 成立。未寫摘要、未標記、prune 0 筆 |
| SCN-008 | `probe-consolidate.py`，由我獨立重跑 | 成立。判準的反向自檢合理：沒合併仍是 40 筆 event；摘要不是模型產生的就不會出現 `sources=20`；程序沒收尾時有殘留 PID |

假綠燈檢查：沒有發現刪除或弱化既有測試、mock 掉核心行為，或用實作重算期望值的情況。SCN-002 的問題屬於覆蓋缺口（資料選擇避開了主要失敗面），不是同義反覆。

### 相關失敗面

| 輸入／狀態 | 預期 | 現況與覆蓋 | 判定 |
|-----------|------|-----------|------|
| turn_key 改成 uuid 後，重試或 final repair | 不重複寫入 | final repair 在同一次 `run_turn` 內、寫入之前完成；Web（`chat_api.rs:313`）、Telegram、REPL、Scheduler executor 都沒有回合層級的重試。寫入後若 lease 失敗，回合回傳 Err，沒有人重送 | 無問題 |
| `with_previous_turn` 只取精確 scope | 上一回合只存在本 scope | `run_turn` 只寫 `cfg.scope`；若改用 ancestry 會混入 global／project 的 event，用精確 scope 正確 | 合理 |
| 上一回合只寫入一筆（`remember` 不是交易） | 盡力而為 | 會取到「前前回合的 Assistant＋上回合的 User」，內容錯置但無害，機率低 | 可接受 |
| 合併後 event 被刪 | 盡力而為 | 下一回合輔助棒沒有上一回合（NICE 2） | 可接受，建議揭露 |
| 上一回合已在相關記憶中 | 不重複注入 | 以 id 去重，順序合理 | 成立 |
| `RecallMode::Relevant` 對外暴露 | Hybrid 不變 | Hybrid 的來源、合併與向量路徑逐行比對都沒有變（門檻只在 `mode == Relevant` 時套用）。serde 新增變體向後相容；telemetry 只存字串，沒有反解析。memoryd 可以選到新模式（SHOULD 3） | Hybrid 不變；外部暴露未揭露 |
| 向量門檻在 `apply_vector_sims` 之前過濾 | 關鍵字候選不受影響 | 低於門檻的列從 vector_cands 移除，已在關鍵字候選中的列保留，只是沒有 semantic 分數；門檻以上的列照常合併訊號。在 top-N 之後才過濾，順序正確 | 成立 |
| 合併做到一半時遇到空白摘要 | 不遺失資料 | 前面成功的批次已有摘要並已標記，下一輪開頭的 `prune_consolidated` 才刪除，安全。手動 `wukong memory consolidate` 回傳錯誤字串，不會報告已完成的批次（與既有的摘要失敗行為一致）；排程 job 以錯誤結束 | 安全 |
| 維護迴圈的 scope 隔離吞掉 DB 錯誤 | schedulerd 仍然看得到 | `memory.scopes()` 的錯誤仍會往上傳；個別 scope 的錯誤逐一記錄 `memory_consolidate_failed`。schedulerd 原本對 Err 也只記一行 warning，沒有其他處置，所以沒有失去任何訊號，只是彙總日誌不含失敗數（NICE 1） | 可接受 |
| `compose_prompt` 的 UTF-8 邊界 | 不切在字元中間 | `char_indices().nth(800)` 取第 801 個字元的起點位元組，`&text[..cut]` 一定落在字元邊界；組合字元或 emoji ZWJ 序列可能在視覺上被切開，但不會產生無效 UTF-8 | 成立 |
| 同語言、停用詞、角色標籤、Telegram 規則文字 | 不相關的記憶不注入 | 未覆蓋，實測失敗 | **MUST FIX 1** |

### 需求、架構、安全、品質

- 需求：寫入、合併隔離、空白摘要、長度上限與輔助棒上一回合都符合核准規格；最後一棒的相關度過濾在真實資料下不符合 SCN-002（MUST FIX 1）。
- 架構：沒有新增 crate 之間的依賴邊；`wukong-runtime` 新增引用的 `uuid` 是既有 workspace 依賴，`Cargo.lock` 只多一行。四柱依賴方向不變；README 標為不可觸及的 `dispatch.rs` 與 `store/` 都沒有改動。
- 安全：沒有新的輸入面；memoryd 多接受一個召回模式，屬於唯讀查詢，風險低。
- 重複邏輯與過度設計（code-simplify 標準）：`maintain_scope` 只是抽出既有邏輯，不算過度設計。`with_previous_turn` 需要把 `MemoryRecord` 轉成 `RecallHit`，欄位清零的寫法冗長但必要。測試輔助函式 `remember_note`／`remember_turn` 與其他測試的寫入樣板相似，屬於可接受的測試重複。沒有發現推測性的抽象或多餘的設定項。

### 豁免、待確認與限制

- 沒有豁免項目。
- TBD-1：已解決，有使用者決策與日期。
- TBD-2（不影響本次交付）：已查證 `docker-compose.yml:25,236` 預設 `WUKONG_EMBED=0`，`release.yml:78` 建置時不帶 `--features embed`，Dockerfile 直接下載 release binary。預設部署不走向量路徑，判定合理。本機 `--features embed` 因 openssl-sys 無法建置，這點已揭露；門檻程式碼不在 feature 條件內，預設建置會編譯並由 stub 測試執行。
- 已揭露的限制大致誠實（未校準的門檻、沒有 schedulerd 時不合併、Telegram 規則文字），但 Telegram 規則文字對 SCN-002 的影響被低估（MUST FIX 1），也沒有揭露無 session 後端的影響（SHOULD 2）。
- 常青文件：AGENTS.md「一回合資料流」與實作一致（`relevant` 模式、800 字、輔助棒帶上一回合、每回合識別碼），只是「防重複」的說法偏強（NICE 4）。CHANGELOG `[Unreleased]` 的 Changed／Fixed 與實作一致，「Web 與 memoryd 的 hybrid 行為不變」屬實。`docs/memory.md` 沒有同步更新（SHOULD 3）。

## 流程判定

RETURN TO execute-task

理由：SCN-002 是核准的必要驗收，在預設的 Telegram 部署與一般同語言輸入下可以實際重現不成立。現有測試的資料選擇看不到這個失敗面，已揭露的限制也低估了影響。其他 SCN 的證據有效。修正 MUST FIX 1 時，Telegram 規則文字的部分落在 README 標為不可觸及的範圍，需要先取得使用者決策（擴大範圍，或記成已同意的限制）。
