# 審查報告
- 範圍：完整 PR，第二輪（TARGET main，`f0b1a28..c408367`，16 個提交）。增量為 e17ecc6（第一輪報告）、8cef2a2／e85d3ba（規格修訂與基線）、713331d、dbd8cb7、c408367
- Reviewed BASE SHA：f0b1a28f01aebcb7faebe94aad4824d69cf03b3d
- Reviewed HEAD SHA：c408367754f72bd597cd7701bcecc7813dfecf6e
- Reviewed patch-id：fbbad09c1eda78b0d84b444dafa0e32d785e9b9c（`git diff f0b1a28 c408367 | git patch-id --stable` 重算一致）
- 獨立 reviewer：Claude subagent（general-purpose，獨立於實作 session；與第一輪 review-cc78101 為同一 reviewer）
- Review artifact：docs/issues/issue-0009/review-c408367.md
- 審查日期：2026-10-06

範圍確認：本地分支 `issue-0009-memory-turn-key` 的 HEAD、`gh pr view 10` 的 headRefOid，以及 c408367 三者相同，工作區乾淨。第一輪提出、在 cc78101 已經查核過的部分，本輪以完整 diff 重新檢查有沒有被後續提交改動，並重跑全量驗證；結論沿用第一輪報告的地方會註明。

## 問題與風險

### MUST FIX

無。

### SHOULD FIX

1. **「舊記憶不會再因規則文字被召回」的說法不成立。**
   位置：`CHANGELOG.md:40-41`、`docs/issues/issue-0009/implementation-plan.md:122`。
   修正前寫入的 User 記憶仍帶著完整規則文字，裡面有「檔案」「修改」「轉換」「使用者」「目錄」等詞。查詢本身不再帶規則，但只要使用者原文碰到這些詞，舊記憶仍會靠規則文字命中。實測（本 HEAD 的 memoryd，`relevant` 模式）：舊記憶「User: 晚餐吃什麼＋規則」在查詢「幫我把這個檔案轉成 PDF」時被回傳，查詢「部署伺服器的步驟」時則不會。受影響的資料量很小：修正前每個 session 只寫入第一回合，每個 scope 大約只有一兩筆這種記憶。但這是發布說明裡可以被證偽的敘述，建議改成「與檔案相關的查詢仍可能召回修正前寫入、帶規則文字的舊記憶」，或另外清理這批舊資料。不阻塞。

### NICE TO HAVE

1. `crates/wukong-runtime/src/turn.rs:168`：`split(...).next()` 切在第一次出現標頭的位置。若使用者原文本身含有 `\n\n[Wukong 檔案互動規則]`（例如把上一則 prompt 貼回來），召回與記憶會在使用者自己那一段就被截斷。Telegram 的規則一定附在最後，改用 `rsplit_once`／`rfind` 切在最後一次出現的位置會比較穩。實務上機率極低。
2. 同一行：輸入若剛好以標頭開頭（使用者原文為空），`user_text` 會是空字串，`Memory::recall` 會回 `InvalidQuery`，整個回合失敗。Telegram 對只有附件、沒有說明文字的訊息會補上 fallback prompt（`wukong-tg-client/src/parse.rs:85-93`），Telegram 本身也不送空白訊息，所以目前碰不到；如果之後有其他入口重用這個標頭，要記得這一點。
3. 第一輪 NICE 1（schedulerd 的彙總日誌不含 `scopes_failed`，而且只有失敗時不會印出彙總行）沒有處理。**判定可以接受**：每個失敗的 scope 都有一行 `memory_consolidate_failed scope=… error=…`，診斷資訊沒有遺失；schedulerd 對維護錯誤原本也只記 warning，不做其他處置。
4. `TURN_ROLE_LABELS` 會讓內容真的與「user」有關的記憶，不能只靠 user 這個詞命中（例如「user table」仍會靠 table 命中）。這是刻意的取捨，`docs/memory.md` 已寫明，列出來供日後調整停用詞清單（TBD-3）時一併考慮。

## 已查核維度

### 驗收與證據

重跑結果（2026-10-06，HEAD c408367）：
- `cargo test --workspace`：passed=629 failed=0 ignored=9，exit 0。
- `cargo clippy --all-targets -- -D warnings`：exit 0。`cargo fmt --all -- --check`：exit 0。
- `cargo build -p wukong-schedulerd` 後，`/usr/bin/python3 docs/issues/issue-0009/probe-consolidate.py` 印出 `SCN-008 probe passed`；`PROBE_BLANK=1` 對照組印出 `SCN-007 control passed`。

紅燈獨立驗證：我把 c408367 的兩個新測試移植到 cc78101 原始碼（放在 scratchpad 的副本，常數以字面值代替），兩個都失敗，原因與計畫記錄一致：
- `final_step_ignores_stopword_and_role_label_matches` 失敗於 `!final_prompt.contains("[相關記憶]")`。
- `file_rules_reach_the_prompt_but_not_memory_or_recall` 失敗於 `!final_prompt.contains("晚餐吃拉麵")`。

| SCN | 證據 | 結論 |
|-----|------|------|
| SCN-001 | `turn.rs` `every_turn_in_a_reused_session_is_remembered`（第一輪已查核，後續提交未改動） | 成立 |
| SCN-002 | 原有測試，加上 `turn.rs:1665` 同語言測試（只共用 the 與 user）、`recall/mod.rs:539` 字面值單元守門、`tests/integration.rs` 向量門檻。另以真實程序重現第一輪的三種情境，見下一節 | 成立。中文高頻二字詞列為 TBD-3，已揭露 |
| SCN-003 | `final_step_keeps_older_relevant_memory` 仍綠 | 成立 |
| SCN-004 | `turn.rs:1588` 改為 explorer→oracle→fixer 三棒，兩個輔助棒逐一斷言，最後一棒不含上一回合 | 成立。屬守門加強，沒有改產品程式，不需另取紅燈 |
| SCN-005 | `prompt.rs` `long_memory_is_truncated_and_marked`（未改動） | 成立 |
| SCN-006 | `maintenance.rs` `auto_maintenance_isolates_a_failing_scope`（未改動），日誌由探針證明 | 成立 |
| SCN-007 | 單元測試加上 `PROBE_BLANK=1` 真實程序（已重跑） | 成立 |
| SCN-008 | `probe-consolidate.py`（已重跑） | 成立 |
| SCN-009 | `turn.rs:1696`：最後一棒含規則、不含因規則而命中的舊記憶，寫入的 User 記憶恰為 `["User: 部署伺服器的步驟"]`。另以真實 `wukong` CLI 端對端重現，見下一節 | 成立。「召回只以原文查詢」是由行為結果間接證明（舊記憶不再被召回）；真實程序對照組可以區分修正前後 |

驗收集合與 Proof 一致：SCN-001～009 共 9 項都有證據。待確認事項有 TBD-1（已解決）、TBD-2 與 TBD-3（不影響本次交付）。沒有發現刪除或弱化測試、同義反覆或 mock 掉核心行為的情況。

### 第一輪三種失敗情境的重現（同一組資料，本 HEAD）

- **停用詞與角色標籤**（memoryd `/v1/recall`，`relevant`）：「deploy the server」與「user table schema」都回傳 `[]`，第一輪分別回傳 2 筆。同一份資料用 `hybrid` 查「deploy the server」仍回傳 the 命中加上近期來源，證明 Hybrid 沒變。
- **Telegram 預設部署**：用真正的 `wukong` CLI（`--agent-cmd` 指向一支會記錄 prompt 的假 agent，暫存 DB，scope `user:tg-1`），以完整的真實規則文字跑三個回合：「晚餐吃什麼」「推薦一本小說」「部署伺服器的步驟」，三則都附規則。
  - **對照組**（從 cc78101 原始碼另外建置的 binary）：第三回合的 prompt 出現 `[相關記憶]` 2 次，含「晚餐吃什麼」。
  - **本 HEAD**：`[相關記憶]` 0 次，規則仍在 prompt 中，DB 裡的 User 記憶都只有原文（`User: 晚餐吃什麼`、`User: 推薦一本小說`、`User: 部署伺服器的步驟`）。
  - 判準的反向自檢：沒修好時，對照組在同一組輸入下確實呈現不同結果，所以這個判準抓得到失敗。
- **中文高頻二字詞**：「我們明天要部署伺服器」仍會靠「明天」命中「User: 明天天氣如何」。這屬於 TBD-3 已揭露的範圍。

### 相關失敗面

| 輸入／狀態 | 預期 | 現況與覆蓋 | 判定 |
|-----------|------|-----------|------|
| 沒有規則文字（Web、CLI、REPL、排程） | `user_text == input` | `split` 找不到分隔字串時回傳整段，行為與修正前相同；既有 runtime 測試全綠 | 成立 |
| 使用者原文本身含標頭 | 盡力而為 | 會切在第一次出現的位置（NICE 1） | 低風險 |
| 使用者原文為空、只有規則 | 不讓回合失敗 | 會回 `InvalidQuery`；Telegram 目前碰不到（NICE 2） | 低風險 |
| 原文以換行結尾 | 正確切分 | 分隔字串是 `\n\n` 加標頭，多出來的換行留在 `user_text` 尾端，無害 | 成立 |
| Telegram 組出的字串逐字不變 | 輸出與修正前相同 | 原本寫死的標頭換成常數，位置參數依序對應（標頭、目錄）；`cargo test -p wukong-telegram` 全綠 | 成立 |
| planner 與各棒仍拿到規則 | 規則只是不進記憶 | `plan_skill_chain_with_preferences` 與 `augmented` 仍用完整的 `input`；測試斷言最後一棒含規則，真實 CLI 的 prompt 紀錄也有規則 | 成立 |
| `Relevant` 的詞全部被排除 | 不做關鍵字召回 | `relevant_match_string` 回 None，keyword 為空，不會退回到 cjk_fallback（只有 Some 分支才會進 fallback）；有單元測試 | 成立 |
| Hybrid／Keyword 不受影響 | `fts_match_string` 輸出不變 | 抽出 `or_match` 共用，單元測試以字面值鎖定 `"the" OR "user"`；memoryd 的 hybrid 實測照舊 | 成立 |
| 修正前帶規則的舊記憶 | 說明文字與實際一致 | 與檔案相關的查詢仍會召回（SHOULD 1） | 揭露不準確，不阻塞 |
| 第一輪其他失敗面（turn_key、`with_previous_turn`、向量門檻、部分合併、scope 隔離、UTF-8） | 沿用第一輪判定 | 相關程式碼在 cc78101 之後只有 `user_text` 這一處改動 | 成立 |

### 需求、架構、安全、品質

- **規格修訂與核准來源**：8cef2a2 新增 SCN-009、TBD-3，並把 `dispatch.rs` 的範圍由「不可觸及」改成「只改組字串」；核准來源寫明使用者 2026-10-06 在對話中選擇「擴大範圍修掉」與「先揭露為限制」。e85d3ba 回填核准基線，兩個提交都早於實作提交 713331d／dbd8cb7，順序正確。`git diff a51009c c408367` 顯示 SCN-001～008 的 Gherkin 原文沒有變動，SCN-009 只有新增。限制：reviewer 看不到使用者對話本身，核准的真實性依 README 紀錄與協調者轉述判斷；紀錄內容具體，且與第一輪報告提出的選項一致。
- **架構**：`wukong-telegram` 原本就依賴 `wukong-runtime`，這次只是引用 `persona::FILE_RULES_HEADER`，沒有新增依賴邊。標頭常數由 runtime 持有，因為切分的契約在 runtime，這個位置合理；兩邊共用同一個常數，避免字串各自漂移。
- **重複邏輯與過度設計**：`or_match` 抽出後兩種模式共用，沒有重複。`relevant_match_string` 只過濾既有清單，沒有新增設定項。`user_text` 一行切分，不另建抽象層。沒有發現推測性設計。
- **安全**：沒有新的輸入面；規則文字仍會送到 agent，權限行為不變。

### 豁免、待確認與限制

- 沒有豁免項目。
- TBD-3（不影響本次交付）：已實測確認中文高頻或共用二字詞仍會命中。CHANGELOG 已知限制有揭露，有使用者決策與日期，判定合理。
- TBD-2：第一輪已查證，判定不變。
- 已知限制的揭露：第一輪 SHOULD 2（無 session 後端）、NICE 2（合併後輔助棒拿不到上一回合）已寫進 CHANGELOG；SHOULD 3（memoryd 可選 `relevant`、`docs/memory.md` 過時）已修正，計畫裡的錯誤說法也已以「2026-10-06 更正」標註。新發現的 SHOULD 1 敘述不準確。
- 常青文件：AGENTS.md「一回合資料流」第 4 點與實作一致。`docs/memory.md`、根目錄 README 的模式清單與實作一致。CHANGELOG 除了 SHOULD 1 那句之外，都與實作一致。

## 流程判定

PASS

理由：沒有 MUST FIX。第一輪的 MUST FIX 已用同語言紅燈測試與 SCN-009 測試修正，我也在 cc78101 原始碼上獨立確認這兩個測試會紅；再以真實 memoryd 與真實 `wukong` CLI 加上修正前 binary 對照組，重現並確認 SCN-002 在 Telegram 預設部署與同語言輸入下成立。SCN-001～009 都有有效證據，全量測試、clippy、fmt 與兩種探針全部通過。SHOULD 1 只是發布說明的措辭準確度，不影響本次驗收。
