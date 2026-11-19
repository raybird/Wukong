# 審查報告
- 範圍：完整交付範圍（main..issue-0039-progress-text-limit，#39 AC-5 後續修正）
- Reviewed BASE SHA：b3004d4accf5186b42d7d444a3a9764c8bf98c72
- Reviewed HEAD SHA：a8efb1c393ade3b9bf626a969e4a2a58428ee48e
- Reviewed patch-id：36ccdda8c40f7e3cd798a1430bf7b7262c08c0c5
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，未參與實作）
- Review artifact：docs/issues/issue-0039/review-a8efb1c.md

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
1. **新測試沒有斷言「放得下的部分」要盡量填滿**（`crates/wukong-telegram/src/dispatch.rs:3185-3194`、`:3198-3227`）
   - 影響：AC-5 寫的是「只留思考內容最後面放得下的部分（整則約 4000 字）」。現有斷言只檢查不超過 4096、開頭有「💭　…」、結尾是「長結尾」、不含「開頭」。如果有變異只留最後幾個字（例如把 `room` 改成 10），兩個測試都會照樣綠燈。依 verification.md，這屬於證據持久力不足，不是假綠燈。理由有兩個：程式本身確實填到上限（照實作推導，head 22 單位加 tail 3978 單位，剛好 4000）；README 步驟 7 記錄的變異檢查（上限改成 4200 時紅在 `4200 units`）也間接證明輸出會頂到上限。所以本次驗收有效、不阻塞。
   - 建議：在 `bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit` 加一個下限斷言，例如 `telegram_len(&text)` 不少於 3990（期望值由人工推導，跟實作無關）。順便補一個邊界例：全文剛好 4000 單位時不截斷、不出現「…」，就能抓到把比較改成「小於」的變異。

### NICE TO HAVE
1. **長思考的草稿測試沒有實際斷言閒置重送有發生**（`dispatch.rs:3222`）：照目前的節流行為實際會送 6 則草稿（Role、第 1 段思考、3 次工具、20 秒重送），但斷言只要求 `drafts.len() >= 5`，少了重送也會綠燈。重送用的是同一個 `status` 字串，長度已由前面的斷言涵蓋，所以風險低。不過 README 步驟 6 說「再閒置 25 秒讓重送也發生」，這句目前沒有斷言撐著。建議改成 `>= 6`，或斷言最後兩則內容相同。
2. **截斷點可能落在字形叢集或工具行中間**（`dispatch.rs:726-733`）：反向掃描以 `char` 為單位，切點保證在 UTF-8 字元邊界上（`start` 取自 `char_indices`，初值是 `r.len()`），不會 panic。但 tail 開頭可能是 ZWJ 序列或旗幟 emoji 的後半，也可能是「具 bash」這種被切開的工具行，甚至是換行。這只影響外觀，Telegram 不會拒收。要改的話可以在切點後跳過開頭的空白或換行，不改也可以接受。
3. **步驟 7 沒寫重構階段（code-simplify）的 no-op 理由，也沒寫被測提交 SHA**：verification.md 的「紅綠重構證據」要求明列重構結果。我在 a8efb1c 上獨立重跑了套件與 workspace 測試，都是全綠（見下方），所以最終狀態有證據。「實作與驗證步驟」是凍結章節，現在補會讓 PASS 失效。建議下一個 issue 照做就好，本次不補。
4. **README 小幅不同步**：`docs/issues/issue-0039/README.md:43`「涉及檔案」的 entrypoints 那行沒提到 AC-5 的長度上限；`:116` 的 `**狀態**` 還寫「待提交與審查」。`**狀態**` 屬於記帳欄位，可以在報告提交時一起更新；第 43 行是凍結內容，建議不要動。
5. **範圍外的觀察**：問題訊息（`render_pending_question`，`dispatch.rs:369`）的 header、question 由 agent 提供，長度沒有上限，理論上也可能超過 4096 被拒收。這不是進度文字，不在 AC-5 範圍內，而且在本次變更前就存在，只順口一提。如果實際遇到再另開 issue。

## 已查核維度

### 驗收與證據
- **AC-5 各子句**
  - 「放得下就全文」：既有 `bubble_text_keeps_full_reasoning`（`dispatch.rs:2649`）沒有改動，仍然綠燈。
  - 「超過上限只留結尾、前面加『…』、角色那一行照常顯示」：由 `bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit` 的 `starts_with("🐵　悟空·oracle 思考中…\n💭　…")`、`ends_with("長結尾")`、`!contains("開頭")` 涵蓋。
  - 「草稿模式累積超過上限的每一則都不超過上限」：由 `long_reasoning_progress_drafts_stay_within_the_telegram_limit` 逐則斷言。
  - 「整則約 4000 字」：沒有直接斷言（見 SHOULD FIX 1），但程式推導與記錄的變異結果都支持。
  - 「實送達上限長度回 ok」：只有 README 記錄（`probe39e.py`：4000 與 4096 單位都回 ok；`probe39d.py`：全中文 4097 字回 `MESSAGE_TOO_LONG`），輸出不在 repo 裡。我把它當作已記錄的證據採信，沒有重送，依指示不對外送出。
- **斷言的獨立性**：上限用字面值 4096（Telegram 的規格值），沒有用實作常數 `PROGRESS_TEXT_LIMIT`，也沒有重算實作邏輯，不是同義反覆。
- **紅燈數字核對**：我手算了改動前的長度。測試 1：基底 17 加「\n💭　」4 加 6004 = 6025。測試 2：第 2 段思考因 1.5 秒節流不顯示，第一則超限的是第 2 次工具事件後的草稿：21 + 3000 + 13 + 3000 + 12 = 6046。兩個數字都跟 README 記錄的 `6025 units`、`6046 units` 相符，紅燈確實來自目標行為。
- **獨立重跑（2026-11-19，HEAD a8efb1c，工作區乾淨）**：`cargo test -p wukong-telegram --locked` lib 44 passed、0 failed，兩個新測試和 `bubble_text_keeps_full_reasoning` 都通過；`cargo test --workspace --locked` 合計 759 passed、0 failed；`cargo fmt --all -- --check` 無差異；`cargo clippy --all-targets --locked -- -D warnings` 無警告。
- **AC-4**：既有測試的期望值沒有改動（diff 只新增測試）。

### 相關失敗面
| 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|
| 思考內容空白或只有空白 | 只顯示角色那一行 | `trim()` 後提早回傳，邏輯不變 | 沒問題 |
| 全文剛好 4000 單位 | 全文顯示、不加「…」 | 只有程式推導（比較用的是「不超過」），沒有測試 | 正確，覆蓋見 SHOULD FIX 1 |
| 全文 4001 單位以上 | 角色行加「💭　…」加結尾，總長不超過 4000 | 測試 1、2 | 正確。推導：full 超限時 r 至少要丟一個字，不會出現「…」加完整內容 |
| 結尾含 2 單位字元（emoji、surrogate pair） | 不超限、不切壞 | 以 `len_utf16()` 計數，以 `char_indices` 取切點 | 正確，切點一定在字元邊界上 |
| 很長的角色名稱 | 不會讓 head 超過上限 | 角色只有固定的 5 個名稱（`role.rs:24-30`），最長 9 字；`saturating_sub` 防止溢位 | 不可能發生 |
| 群組狀態訊息模式（`edit_message_text`） | 用同一份文字 | `status` 只由 `bubble_text`／`resumed_text` 產生，兩種模式共用 `update_progress_display` | 正確，沒有專屬測試，但路徑相同 |
| 草稿失敗後退回 `send_message` | 同一份文字、不超限 | `update_progress_display` 用同一個 `text` | 正確 |
| 閒置 20 秒重送 | 重送目前的 `status` | 同一個字串；測試沒有斷言重送有發生 | 正確，見 NICE TO HAVE 1 |
| 其他進度文字：初始狀態、`resumed_text` | 不超限 | 都是固定短字串加角色名 | 不可能超限 |
| Telegram 額外加長度（parse mode、entities） | 不增加計數 | `send_message_draft`、`send_message`、`edit_message_text` 都送純文字，沒有 `parse_mode`（`wukong-tg-client/src/client.rs:262-345`） | HTML 實體或標籤不會造成長度差，沒問題 |

### 需求、架構、安全、品質
- **需求**：只改 `wukong-telegram` 的 `bubble_text`，範圍跟 AC-5 一致，沒有跨 crate。
- **架構**：沒有新增抽象，只加一個常數。
- **安全**：沒有新增外部輸入面；切片不會 panic。
- **簡單優先**：實作約 15 行，用一次反向掃描算出 tail，不算過度設計。
- **效能**：每個進度事件會複製一次完整思考文字並計數 UTF-16，複雜度是 O(n)。改動前的 `format!` 本來就是 O(n)，新增的計數只是同一量級的常數倍。實際長度數十 KB、事件間隔數秒，負擔可以忽略，不需要處理。`reasoning` 在回合內無上限成長，回合結束就釋放；這在本次變更前就存在，不在 AC-5 範圍內，不列為問題。
- **文件**：
  - CHANGELOG `[Unreleased]` 的 Fixed 描述（全文放得下照常顯示，超過留最後面加「…」，群組也一樣）跟實作相符。
  - `docs/entrypoints.md`「只留最後面約 4000 字、前面加『…』」也相符。精確來說是整則約 4000 字，可以接受。
  - README 新增的「部署後觀察」、AC-5、步驟 6 至 7、TBD-2 狀態與 Timeline 彼此一致。

### 豁免、待確認與限制
- **核准來源**：AC-5 的核准來源已記在 `**核准來源**` 最後一句，有日期、題目原文與選項「放得下就全文，超過留最後面 (Recommended)」、對應 AC-5，另有「繼續在 #39 修」。可以追溯。Small 加 Low 不需要核准 commit。「整則約 4000 字」與「…」這些呈現細節，我沒辦法從紀錄確認是不是寫在選項說明裡，但它們沒有超出使用者所選做法的語意，不視為缺漏。
- **TBD-2**：仍是「待確認」。已寫明修正發版、部署後要重測三項，不阻塞合併，跟原本核准的「TBD-2 端到端觀察不擋合併」一致。
- **沒有 Gate 豁免。**
- **審查限制**：實送證據只審了 README 的記錄，我沒有重送，依指示不對外發送任何東西。變異檢查沒有重做，因為不修改檔案；改用手算紅燈數字與推導作為交叉核對。

## 流程判定
PASS。沒有 MUST FIX。AC-5 每個子句都有測試或記錄的證據。紅燈數字跟手算吻合，在 HEAD 上獨立重跑套件、workspace 測試、fmt 與 clippy 都全綠。SHOULD FIX 與 NICE TO HAVE 都屬於證據持久力或外觀，不阻塞。

PASS 只適用於上列 Reviewed HEAD SHA。之後在這個分支提交 issue README 記帳區段與本報告以外的檔案（例如交接文件），PASS 會失效、需要重新審查（流程契約 2.3 以下只有單獨新增本報告的提交可沿用）；提交前先告知使用者，由使用者決定做法。
