# 審查報告
- 範圍：完整交付範圍（main..issue-0039-progress-text-limit，#39 AC-5 後續修正）
- Reviewed BASE SHA：b3004d4accf5186b42d7d444a3a9764c8bf98c72
- Reviewed HEAD SHA：489a7f3b990cfb24b3a6e51de58a43df75b17b1c
- Reviewed patch-id：424b6189283eb67feaf66efb5796c15fee499ba3
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，未參與實作）
- Review artifact：docs/issues/issue-0039/review-489a7f3.md
- 增量覆審：上一份報告 review-4449e20.md 審查過 4449e20；流程契約 2.5 以下完整審查，本次審查完整範圍

## 問題與風險

- **MUST FIX**：無
- **SHOULD FIX**：無
- **NICE TO HAVE 1**：`crates/wukong-telegram/src/dispatch.rs:736-740` 的 `PROGRESS_TEXT_LIMIT` 註解寫「Telegram rejects a draft or message over 4096 characters ... (real send, 2026-11-19, #39)」。記錄裡的實送只有 `sendMessageDraft`（全中文 4097 字回 `MESSAGE_TOO_LONG`）；一般訊息與 `editMessageText` 的 4096 字上限出自 Bot API 文件，沒有實送過。README 概述「退回的一般訊息也一樣太長」「群組狀態訊息的編輯有同樣的上限」也是同一種推論。另外「however Telegram counts」比證據說得滿：證據只支持「Telegram 的算法不超過 UTF-16 單位數」（4096 單位的草稿 ok、2049 個 emoji 的草稿 ok、bytes 算法已被排除），沒辦法涵蓋所有可能的算法。行為沒有影響，因為 4000 單位、留 96 單位餘裕的做法在每一種觀察到的算法下都成立。建議：有機會時把出處寫成「draft 為實送，message／edit 依 Bot API 文件」，並把「however Telegram counts」改成較保守的說法。不擋交付。
- **NICE TO HAVE 2**（限制，非缺陷）：`dispatch.rs:727-733` 的反向掃描以 `char` 為單位，可能切在字形叢集（grapheme cluster）中間，例如 ZWJ 組合 emoji、膚色修飾符或國旗的一半，「…」後面的第一個字形可能顯示不完整。長度上限與 UTF-8 邊界都正確，只影響外觀，AC-5 沒有要求。不需處理。

## 已查核維度

### 驗收與證據

- AC-5 各檢查方式對照：
  - 短思考照全文顯示：既有 `bubble_text_keeps_full_reasoning` 在 range 內沒有改動（diff 只新增程式碼）。
  - 長思考不超過上限、保留結尾與「…」：`bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit` 斷言 `starts_with("…\n💭　…")` 前綴、`ends_with`、`!contains("開頭")`、上限與下限。
  - 草稿模式累積超過上限，每一則都不超過：`long_reasoning_progress_drafts_stay_within_the_telegram_limit`，含閒置重送。
  - 實送：步驟 7 記錄 4000 單位與 4096 單位的草稿都回 `ok`，與交付時提供的證據相符。
- 本次重跑（2026-11-19，於 HEAD 489a7f3，工作樹乾淨）：
  - `cargo test -p wukong-telegram --locked`：lib 45 passed；tests 3、1 passed；1 ignored。
  - `cargo test --workspace --locked`：760 passed、0 failed、12 ignored，與 README 步驟 10 相符。
  - `cargo fmt --all -- --check`：無差異。
  - `cargo clippy --all-targets --locked -- -D warnings`：無警告。
- 變異重驗：在 `git worktree add --detach` 的 /tmp/claude-1000 副本與獨立 target 目錄執行，結束後已移除，主工作樹未動。
  - README 記錄的變異：
    - (a) `room` 改為 10：紅在 3197 行 `32 units`，與步驟 8 相符。
    - (b) 不截斷的比較由「不超過」改成「小於」：紅在 `bubble_text_is_cut_only_past_4000_units` 的 `assert_eq!`，相符。
    - (c) 兩處 `c.len_utf16()` 改成 1：紅在 3203 行 `6027 units`，與步驟 10 相符。
    - (d) `keep_alive` 恆為 false：紅在 `the long text was not resent when idle`，相符。
    - (e) 停止條件改成剩餘空間為 0 才停：在 731 行減法溢位 panic，相符。
  - 我另外加的變異，都被抓到：
    - (f) `break` 改 `continue`：紅在 `6026 units`。
    - (g) 全文長度改以 bytes 判斷：紅在剛好 4000 單位那個案例。
    - (h) 上限改 4200：三個測試紅。
    - (i) 上限改 4096：剛好 4000 單位與 emoji 的 4000 上限斷言紅，`4095 units`。
- 手算 emoji 案例：`head` 為 22 單位，剩餘空間 3978；扣掉「結尾。」3 單位後剩 3975（奇數），放 1987 個 emoji（3974 單位）後剩 1，下一個 emoji 要 2 單位所以停，整則 3999 單位。確實走到 489a7f3 要補的分支。

### 相關失敗面

| 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|
| 思考內容為空或只有空白 | 只回角色那一行 | `trim()` 後提早 return，邏輯沒有改動；既有測試涵蓋 | 通過 |
| 全文剛好 4000 單位 | 原樣、不加「…」 | `bubble_text_is_cut_only_past_4000_units`；變異 (b)(g) 會紅 | 通過 |
| 全文 4001 單位 | 以「…」開頭截斷 | 同上測試的第二段 | 通過 |
| 全中文長文字 | 盡量填滿、不超過上限 | 6000 字案例的上下限斷言；變異 (a) 會紅 | 通過 |
| emoji（2 單位）、剩餘空間為奇數 | 停在剩 1，不溢位 | emoji 案例；變異 (c)(e)(f) 會紅 | 通過 |
| 截斷點的 UTF-8 邊界 | 切片不 panic | `char_indices` 給的是字元起點，`start` 只會落在字元邊界 | 通過 |
| 角色那一行 | 截斷時保留 | 斷言 `starts_with` 角色那一行加「💭　…」 | 通過 |
| 角色為 None | 同樣套用上限 | `head` 由 `base` 組出，兩種情況同一條路徑；沒有專門測試，但邏輯沒有分支 | 通過 |
| `resumed_text` 與起始文字 | 本來就很短 | 固定短字串加角色名稱，不會接近上限 | 通過 |
| 草稿失敗退回 `send_message` | 同樣不超過上限 | `update_progress_display` 用同一個 `status`；`send_message` 為純文字（無 parse_mode），不會因 HTML 展開而變長 | 通過 |
| 群組狀態訊息的 `edit_message_text` | 同樣不超過上限 | 同一個 `status`；沒有專門的長文字測試，但路徑共用 `bubble_text` | 通過 |
| 閒置 20 秒重送長文字 | 重送的是截斷後的文字 | 草稿測試斷言最後一則等於前一則；變異 (d) 會紅 | 通過 |
| 回答後下一個思考事件 | 帶全部累積內容並截斷 | `Reasoning` 與 `ToolUse` 都經過 `bubble_text` | 通過 |

### 需求、架構、安全、品質

- **需求**：符合 AC-5，核准來源為 2026-11-19 使用者選「放得下就全文，超過留最後面 (Recommended)」。改動只在 `wukong-telegram`，沒有跨 crate。
- **架構**：截斷集中在 `bubble_text`，草稿、退回的訊息、編輯與重送四條路徑都自動涵蓋，沒有新的抽象層。
- **安全**：只送純文字，沒有新的外部輸入面。
- **品質**：
  - 實作約 15 行，夠簡潔。測試不依賴實作常數：前綴用字面值，長度用測試自己的 `utf16_len`。上限斷言用 4096（Telegram 上限）；只有 emoji 案例用 4000，這是 AC-5「約 4000，留餘裕」的直接反映。
  - `reasoning` 在回合內會一直增長，每次事件做一次 O(n) 的格式化與計數；五萬字的規模可以忽略。
- **Telegram 相關說法**：
  - 程式註解、測試輔助函式註解、CHANGELOG `[Unreleased]`、`docs/entrypoints.md` 都已不再說 Telegram 以 UTF-16 單位計算。
  - README 步驟 7 已加更正說明，步驟 6、9 提到的「UTF-16 單位」是在描述測試的量法，不是在描述 Telegram。
  - 「UTF-16 單位數不少於字元數」為真，因為每個 code point 至少佔 1 個 UTF-16 單位。
  - CHANGELOG 寫的「4096 字上限」「約 60 秒後消失」與部署後觀察、實送記錄相符。
  - 剩下一點出處寫法較寬鬆，見 NICE TO HAVE 1。

### 豁免、待確認與限制

- 沒有 gate 豁免。
- TBD-2（部署後端到端觀察）照已核准的方式不阻塞合併，窗口已改指向包含 AC-5 修正的版本。
- TBD-3 維持不修，理由已記錄。
- 已知限制：截斷以 `char` 為單位，見 NICE TO HAVE 2。
- 本審查沒有實送 Telegram，實送證據引用 README 與交付時提供的記錄。
- README「現行 review」欄位仍指向 review-4449e20.md，屬記帳區段，由提交本報告的流程更新。

## 流程判定

PASS。AC-5 的每一項檢查方式都有對應證據；本次重跑測試、fmt、clippy 全綠；README 記錄的變異 (a)～(e) 與另外 4 個變異都被測試抓到；不再有「Telegram 以 UTF-16 單位計算」的說法。剩下兩點 NICE TO HAVE 不影響行為與驗收。

PASS 只適用於上列 Reviewed HEAD SHA。之後在這個分支提交 issue README 記帳區段與本報告以外的檔案（例如交接文件），PASS 會失效、需要重新審查（流程契約 2.3 以下只有單獨新增本報告的提交可沿用）；提交前先告知使用者，由使用者決定做法。
