# 審查報告
- 範圍：完整交付範圍（main..issue-0039-progress-text-limit，#39 AC-5 後續修正）
- Reviewed BASE SHA：b3004d4accf5186b42d7d444a3a9764c8bf98c72
- Reviewed HEAD SHA：ba4cf599296b3694ebd09a09e978ea92cfbe2048
- Reviewed patch-id：54a32f8946d943b86a6dd7b98b16bf69a691c21c
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，未參與實作）
- Review artifact：docs/issues/issue-0039/review-ba4cf59.md
- 增量覆審：上一份報告 review-a8efb1c.md 審查過 a8efb1c；流程契約 2.5 以下完整審查，本次審查完整範圍

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
1. **沒有測試用到需要 2 個 UTF-16 單位的字元，代理對（surrogate pair）的計數沒有守門**（`crates/wukong-telegram/src/dispatch.rs:725-733`、測試 `:3185-3241`）
   - 影響：三個長度測試的思考內容都是「長」「想」這類 BMP 中文字，1 個字元剛好等於 1 個 UTF-16 單位，字元數和單位數算出來一樣。我在 /tmp 的 worktree 複本做了變異：把反向掃描的 `c.len_utf16()` 兩處都換成 1（等於改數字元）。結果 `bubble_text`、`long_reasoning`、`idle_draft` 共 5 個測試全部綠燈。但同一個變異遇到 3000 個「😀」的思考內容時，輸出是 6025 單位，送到 Telegram 會被 `MESSAGE_TOO_LONG` 拒收，正是本 issue 要修的失敗。
   - 判定：現行實作是對的（見下方探針），紅燈也確實由目標行為驅動，所以這是證據持久力不足，不是假綠燈，不阻塞。
   - 建議：在 `bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit` 再加一組 emoji 為主的思考內容（例如開頭加「😀」重複 3000 次再接「結尾」），用同樣的字面值 4096 和 3990 斷言上下限。

### NICE TO HAVE
1. **長思考的草稿測試仍然沒有斷言閒置重送真的發生**（`dispatch.rs:3236`）：這是上一份報告 NICE TO HAVE 1，這次沒有處理。變異把 `keep_alive` 改成恆為 false 之後，`long_reasoning_progress_drafts_stay_within_the_telegram_limit` 照樣綠燈，只有 AC-3 的 `idle_draft_progress_is_resent_every_20_seconds_except_while_a_question_waits` 紅。重送用的是同一個 `status`，長度已經被逐則斷言涵蓋，風險低。建議把門檻改成 `drafts.len() >= 6`，或斷言最後兩則草稿內容相同，讓步驟 6 說的「讓重送也發生」有斷言支撐。
2. **文件說的截斷門檻是 4096，實際是 4000**：`docs/entrypoints.md:32` 寫「思考內容放得下就全文顯示，超過 Telegram 4096 字上限時只留最後面約 4000 字」。照這句讀，整則 4001～4096 單位的進度會全文顯示；實際上整則超過 4000 就截斷，`bubble_text_is_cut_only_past_4000_units` 也釘的是 4000。AC-5 的原文是「整則約 4000 字，留餘裕」，所以實作與核准語意相符，只是這句文件的門檻寫得不精確。建議改成「整則超過約 4000 字（Telegram 上限 4096）時只留最後面放得下的部分」。CHANGELOG 的寫法比較中性，可以不改。
3. **TBD-2 的窗口還寫「包含 3cfe9ab 的版本」**（`docs/issues/issue-0039/README.md:118`）：狀態欄已經說明修正發版、部署後要重測三項，但窗口與判準沒有指向包含本次修正的版本。`## 待確認事項` 屬於記帳區段，可以在保存本報告的提交一起改，例如改成「包含 AC-5 修正（本分支合併後的提交）的版本」，不影響 PASS。
4. **截斷點可能落在字形叢集中間，也可能讓 tail 從換行開始**（`dispatch.rs:726-733`）：探針用「e」加上 U+0301 重複 3000 次，截斷後的 tail 以單獨的結合符號開頭。切點一定在 `char` 邊界上，不會 panic，Telegram 也不會因此拒收，只影響外觀。這是上一份報告 NICE TO HAVE 2，可以不改。
5. **README「涉及檔案」的 entrypoints 那行沒提到長度上限**（`README.md:47` 附近）：這屬於凍結章節，本次不建議改。上一份報告 NICE TO HAVE 4 已提過。

## 已查核維度

### 驗收與證據
- **AC-5 各子句與證據**
  - 「放得下就照現在顯示全部思考內容」：既有 `bubble_text_keeps_full_reasoning` 沒有改動、綠燈；`bubble_text_is_cut_only_past_4000_units` 斷言整則剛好 4000 單位時原樣輸出、沒有「…」。
  - 「超過時只留結尾、前面加『…』、角色那一行照常顯示」：由 `bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit` 的 `starts_with`（角色行加「💭　…」）、`ends_with("長結尾")`、`!contains("開頭")` 涵蓋；多 1 字的情況另由 `bubble_text_is_cut_only_past_4000_units` 斷言以「…」開頭。
  - 「整則約 4000 字」：由下限斷言（不少於 3990）和上限斷言（不超過 4096）夾住。
  - 「草稿模式累積超過上限的每一則都不超過上限」：由 `long_reasoning_progress_drafts_stay_within_the_telegram_limit` 逐則斷言。
  - 「實送達上限長度回 ok」：README 步驟 7 記錄 `probe39e.py` 的 4000 與 4096 單位都回 ok，對照組 `probe39d.py` 的全中文 4097 字回 `MESSAGE_TOO_LONG`。腳本與輸出不在 repo 裡，我當作已記錄的證據採信，依指示沒有重送。
- **斷言獨立性**：上限 4096、下限 3990、門檻 4000 都是寫死的字面值；前綴 `"🐵　悟空·oracle 思考中…\n💭　"` 也是字面值，我手算是 21 個單位（🐵 2、全形空格 1、悟空 2、· 1、oracle 6、空格 1、思考中… 4、換行 1、💭 2、全形空格 1）。測試沒有引用 `PROGRESS_TEXT_LIMIT`，也沒有重算實作邏輯，所以不是同義反覆。
- **步驟 8 的變異宣稱**：我在 worktree 複本（`git worktree add --detach`，放在 scratchpad，用完已移除，主工作樹保持乾淨）重做兩個變異，結果與 README 相符。
  - (a) `room` 改成 10：紅在 `>= 3990` 斷言，輸出 `32 units`（22 加 10）。
  - (b) 把「不超過」改成「小於」：紅在 `bubble_text_is_cut_only_past_4000_units` 的 `assert_eq!`。
- **其他變異**
  - 上限改成 4096：紅在 `bubble_text_is_cut_only_past_4000_units`。
  - 代理對改數字元：全綠，見 SHOULD FIX 1。
  - 拿掉閒置重送：只有 AC-3 的測試紅，見 NICE TO HAVE 1。
- **獨立重跑（2026-11-19，HEAD ba4cf59，工作區乾淨）**
  - `cargo test -p wukong-telegram --locked`：lib 45 passed、0 failed；`tests/` 為 3 passed 與 1 passed；1 ignored。
  - `cargo test --workspace --locked`：合計 760 passed、0 failed、12 ignored，與 README 步驟 8 的 760 相符。
  - `cargo fmt --all -- --check`：無差異。
  - `cargo clippy --all-targets --locked -- -D warnings`：無警告。
- **AC-4**：diff 只新增測試，既有測試的期望值沒有改動。
- **核准基線**：Small 加 Low，沒有 `**核准 commit**`。照 acceptance.md 的 pattern，最後修改 `**核准來源**` 行的提交是 c00de6d24a80a1d093bb9f07f40de35210e8168c。從 c00de6d 到 HEAD，AC-5 和核准來源的文字都沒有再改，實作依據的就是核准當時的版本。

### 相關失敗面
| 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|
| 思考內容空白或只有空白 | 只顯示角色那一行 | `trim()` 後提早回傳；探針用「空格加換行」重複 3 次，輸出就是角色那一行 | 正確 |
| 整則剛好 4000 單位 | 全文、沒有「…」 | `bubble_text_is_cut_only_past_4000_units` | 正確，有測試 |
| 整則 4001 單位以上 | 角色行加「💭　…」加結尾，總長不超過 4000 | 測試 1 至 3 | 正確。整則超限時至少要丟掉一個字，不會出現「…」加完整內容 |
| 結尾是代理對（emoji） | 以 UTF-16 計數，不超限 | 探針：3000 個 😀，有角色時 3999 單位、沒有角色時 4000；頭尾各差一個 BMP 字也不超過 4000 | 實作正確，測試沒有涵蓋，見 SHOULD FIX 1 |
| 截斷點落在 UTF-8 多位元組字元中間 | 不 panic | `start` 取自 `char_indices`，初值是 `r.len()` | 正確 |
| 結合符號或 ZWJ 序列 | 不被拒收 | 探針：tail 以 U+0301 開頭，仍是 4000 單位 | 不會被拒收，只影響外觀，見 NICE TO HAVE 4 |
| 角色名稱很長 | head 不超限 | 角色只有固定的 5 個名稱；`saturating_sub` 防止溢位 | 不可能發生 |
| 工具名稱很長 | 不超限 | 工具名稱併入 `reasoning`，跟思考內容一起從結尾截斷；ToolUse 後面的換行會被 `trim()` 去掉 | 正確；測試 3 斷言以「▸ 使用工具 bash」結尾 |
| 其他進度文字：起始文字、`resumed_text` | 不超限 | 都是固定短字串加角色名稱 | 不可能超限 |
| 群組狀態訊息（`edit_message_text`） | 用同一份文字 | `status` 只由 `bubble_text`／`resumed_text` 產生，兩種模式都經過 `update_progress_display` | 正確；AC-5 的檢查方式只要求草稿模式的測試 |
| 草稿失敗後退回 `send_message` | 同一份文字、不超限 | `update_progress_display` 用同一個 `text` | 正確 |
| 閒置 20 秒重送 | 重送目前的 `status` | 同一個字串；有沒有發生只由 AC-3 的測試斷言 | 正確，見 NICE TO HAVE 1 |
| Telegram 另外加長度（parse mode、實體） | 不增加計數 | 草稿、`send_message`、`edit_message_text` 都送純文字，沒有 `parse_mode`（`wukong-tg-client/src/client.rs:262-346`） | 正確 |

### 需求、架構、安全、品質
- **需求**：只改 `wukong-telegram` 的 `bubble_text`，範圍與 AC-5 一致，沒有跨 crate。CHANGELOG 和 entrypoints 跟著更新。
- **架構**：沒有新增抽象，只多一個常數，註解寫明實送依據與日期。
- **安全**：沒有新增外部輸入面，切片不會 panic。
- **簡單優先**：實作約 15 行，一次反向掃描算出 tail，沒有過度設計。
- **效能**：每個事件 O(n)（思考內容數十 KB），跟改動前同一量級，可以忽略。
- **文件**：
  - CHANGELOG `[Unreleased]` 的 Fixed 跟實作相符。
  - entrypoints 有門檻用語不精確的問題，見 NICE TO HAVE 2。
  - README 的 `## 現況` 位在 `## 概述`（含其下兩個子節）之後、`## 涉及檔案` 之前，位置正確。
  - 「部署後觀察」、AC-5、步驟 6 至 8、TBD-2 狀態與 Timeline 彼此一致。

### 豁免、待確認與限制
- **核准可追溯**：AC-5 的核准記在 `**核准來源**` 的最後一句，有日期、題目原文、選項「放得下就全文，超過留最後面 (Recommended)」和「繼續在 #39 修」。
- **TBD-2**：維持「待確認」、不阻塞合併，與原本核准的「端到端觀察不擋合併」一致；窗口文字可以更新，見 NICE TO HAVE 3。
- **沒有 Gate 豁免。**
- **步驟 8 的流程**：這次只補強測試、不改行為，所以用變異取得紅燈，符合 verification.md 對補強證據的處理。我重做變異，結果相符。
- **限制**：
  - 實送證據只審了 README 的記錄，沒有對 Telegram 或任何外部服務送出東西。
  - 變異與探針都在 worktree 複本中進行，沒有修改主工作樹，也沒有提交。

## 流程判定
PASS。沒有 MUST FIX。AC-5 每個子句都有測試或記錄的證據，斷言的期望值來自字面值與規格，不是同義反覆。步驟 8 的兩個變異經獨立重做，結果與記錄相符。在 HEAD 上獨立重跑套件測試、workspace 測試、fmt 與 clippy，全部綠燈。SHOULD FIX 1（代理對計數沒有守門）屬於證據持久力，探針已證實現行實作正確，不阻塞；NICE TO HAVE 都屬於證據持久力、文件精確度或外觀。

PASS 只適用於上列 Reviewed HEAD SHA。之後在這個分支提交 issue README 記帳區段與本報告以外的檔案（例如交接文件），PASS 會失效、需要重新審查（流程契約 2.3 以下只有單獨新增本報告的提交可沿用）；提交前先告知使用者，由使用者決定做法。
