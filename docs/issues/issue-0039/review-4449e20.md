# 審查報告
- 範圍：完整交付範圍（main..issue-0039-progress-text-limit，#39 AC-5 後續修正）
- Reviewed BASE SHA：b3004d4accf5186b42d7d444a3a9764c8bf98c72
- Reviewed HEAD SHA：4449e20aec577c562722b8fde3ca6e26310029a5
- Reviewed patch-id：12e56c64da4737dbd016ac188665eb96fabd19a6
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，未參與實作）
- Review artifact：docs/issues/issue-0039/review-4449e20.md
- 增量覆審：上一份報告 review-ba4cf59.md 審查過 ba4cf59；流程契約 2.5 以下完整審查，本次審查完整範圍

## 問題與風險

- MUST FIX：無。
- SHOULD FIX：無。
- NICE TO HAVE 1：`docs/issues/issue-0039/README.md` 步驟 7 證據寫「上限以 UTF-16 單位計」，`crates/wukong-telegram/src/dispatch.rs` 的 `PROGRESS_TEXT_LIMIT` 註解（約 737–739 行）與測試輔助函式 `telegram_len` 的註解（約 3178–3179 行）也寫成實送得到的結論。但紀錄裡的實送是：4096 單位（3840 字）回 `ok`、全中文 4097 字回 `MESSAGE_TOO_LONG`。如果 Telegram 改用字元（code point）計算，這兩個結果也一樣成立，所以這組實送分不出是用 UTF-16 單位還是字元計算。影響：不影響功能。同一段文字的 UTF-16 單位數一定不少於字元數，以 UTF-16 計算本來就是比較保守的做法，不管 Telegram 實際怎麼算都不會超過上限。只是文件上的這句說法沒有證據支持，違反根目錄 `AGENTS.md`「要驗證一句宣稱，去問產物」。建議：改寫成「以 UTF-16 單位計算是保守做法（不少於字元數）」；要不然就補一則實送，例如 2049 個 emoji（4098 單位、2049 字），確認會被拒收。
- NICE TO HAVE 2：`bubble_text` 從結尾往前掃時，有一個分支是「剩下 1 單位、下一個字元要 2 單位」就停下來（`c.len_utf16() > room` 的 break），這個分支沒有測試直接覆蓋。emoji 那組測試中，前綴佔 22 單位、剩下 3978 單位，「結尾」兩字放完後剩下的是偶數，一路剛好用到 0。我推估：如果把條件換成只在 `room == 0` 時停止，現有測試會通過，但結果最多超出 1 個單位，例如 4001，仍然遠低於 4096，所以只是測試精確度的問題。建議（可不做）：加一組讓剩餘空間為奇數的 emoji 案例，例如思考內容前面多放一個中文字。

## 已查核維度

### 驗收與證據

- AC-5 逐項核對：
  - 短思考照全文顯示：`bubble_text_keeps_full_reasoning` 在這個範圍內沒有改動（看 diff 確認）。
  - 長思考不超過上限、保留結尾與「…」、角色那一行照常顯示：`bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit` 檢查開頭為「🐵　悟空·oracle 思考中…\n💭　…」、結尾正確、不含開頭內容、長度在 3990 到 4096 單位之間，中文與 emoji 各一組。
  - 剛好 4000 單位時不截斷、多 1 字就截斷：`bubble_text_is_cut_only_past_4000_units`。
  - 草稿模式累積超過上限：`long_reasoning_progress_drafts_stay_within_the_telegram_limit` 檢查每則草稿都不超過 4096、最後一則是長文字的閒置重送。
  - 實送：照委託說明，採用紀錄中 4000 與 4096 單位回 `ok`、4097 字回 `MESSAGE_TOO_LONG` 的結果。這些實送不在 repo 裡，本次沒有重做。
- 我重跑的命令（2026-11-19，主工作樹 HEAD 為 4449e20、工作樹乾淨）：
  - `cargo fmt --all -- --check`：沒有差異。
  - `cargo test -p wukong-telegram --locked`：lib 45 passed；tests 3、1 passed；1 ignored；0 failed。
  - `cargo clippy --all-targets --locked -- -D warnings`：沒有警告。
  - `cargo test --workspace --locked`：760 passed、0 failed、12 ignored，和 README 步驟 9 記錄的 760 一致。
- 變異重現：在 `/tmp/claude-1000/` 下用 `git worktree add --detach` 開一份副本來做，目標目錄也獨立，做完已移除 worktree 與目標目錄，主工作樹沒有動到。
  - README 記錄的四個變異都照紀錄紅燈：
    - (a) `room` 改成 10：紅在下限斷言，`32 units`；
    - (b) 不截斷的比較由「不超過」改成「小於」：紅在 `bubble_text_is_cut_only_past_4000_units` 的 `assert_eq!`；
    - (c) 兩處 `c.len_utf16()` 改成 1：紅在 emoji 的上限斷言，`6026 units`；
    - (d) `keep_alive` 恆為 false：紅在 `the long text was not resent when idle`，既有 AC-3 測試也一起紅。
  - 我另外做的四個變異都紅燈：
    - 拿掉「…」：兩個 `bubble_text` 測試紅；
    - 截斷時拿掉角色那一行：兩個 `bubble_text` 測試紅；
    - 上限改成 4096：`bubble_text_is_cut_only_past_4000_units` 紅；
    - 完全不截斷：三個新測試都紅。
- 測試和實作是分開的：期望長度用測試裡的前綴字面值與 `telegram_len` 算出來，沒有引用 `PROGRESS_TEXT_LIMIT`。上限以 4096 斷言，下限以 3990 斷言，都是從規格來的數字。

### 相關失敗面

| 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|
| 思考內容是空字串或只有空白 | 只顯示角色那一行 | 程式在截斷之前就提早回傳（沒有改動）；`trim` 之後才判斷 | 通過 |
| 全文剛好 4000 單位 | 原樣顯示、不加「…」 | `bubble_text_is_cut_only_past_4000_units`、變異 (b) | 通過 |
| 4001 單位以上 | 保留角色那一行與「💭　…」，從結尾往前放到 4000 單位為止 | 兩個 `bubble_text` 測試、拿掉角色那一行與拿掉「…」的變異 | 通過 |
| emoji（UTF-16 代理對） | 以 UTF-16 單位計算、不切斷字元 | emoji 那組測試、變異 (c)；切點用 `char_indices`，一定落在字元邊界 | 通過；剩 1 單位遇到 2 單位字元的分支沒有直接覆蓋，見 NICE TO HAVE 2 |
| 還沒有角色（`role` 為 None） | 前綴比較短，套用同一段邏輯 | 沒有專門測試；`base` 只影響 `head` 的長度，算法相同 | 通過（靠程式結構推得） |
| 草稿閒置重送長文字 | 重送的是截斷後的 `status`，不超過上限 | `long_reasoning_progress_drafts_stay_within_the_telegram_limit`、變異 (d) | 通過 |
| 群組狀態訊息（`edit_message_text`）、草稿失敗後改送一般訊息 | 用同一個 `status`，長度同樣受限 | 沒有長文字的訊息模式測試；`update_progress_display` 兩條路徑都直接送 `status`，而 `status` 只來自 `bubble_text`、`resumed_text` 或起始文字 | 通過（靠程式結構推得）；AC-5 的檢查方式只要求草稿模式 |
| 其他進度文字（起始文字「🐵 收到，思考中…」、`resumed_text`） | 都很短，碰不到上限 | 固定字串加角色名稱 | 不適用 |
| 送出格式 | 草稿、訊息與編輯都是純文字，沒有 `parse_mode`，長度不受 HTML 實體影響 | 讀 `crates/wukong-tg-client/src/client.rs` 確認 | 通過 |

### 需求、架構、安全、品質

- 需求：實作與 AC-5 文字一致：放得下就顯示全文，超過時只留結尾、前面加「…」、角色那一行照常顯示，整則不超過 4000 單位。行為只改超過上限的長文字。
- 架構：只改 `wukong-telegram` 的 `bubble_text`，並新增一個常數與測試，沒有跨 crate 的改動，符合 `AGENTS.md` 第八節。
- 安全：沒有新的輸入面；切字串用 `char_indices`，不會在非字元邊界切開而 panic；`saturating_sub` 避免減法溢位。
- 品質與簡潔：截斷邏輯約 15 行，沒有多餘的抽象。每個事件都會對累積的全文重新計算 UTF-16 長度。實際回合最多約五萬字，這個成本可以忽略。
- 文件：
  - CHANGELOG `[Unreleased]` 新增 `### Fixed` 一條，從使用者看得到的角度描述，含群組與 #39。
  - `docs/entrypoints.md`「即時狀態回饋」補了一句，用「約 4000 字（Telegram 上限 4096）」，和實作一致。
  - README 部署後觀察、AC-5、核准來源（使用者選「放得下就全文，超過留最後面」與「繼續在 #39 修」）、步驟 6–9、TBD-2 窗口改指向後續修正，都可以互相對照。
  - README 的 `**現行 review**` 目前還是 review-ba4cf59.md，屬於保存本報告時要一起更新的記帳區段。

### 豁免、待確認與限制

- 沒有 gate 豁免。
- TBD-2（部署後端到端重測）不阻塞合併，已在核准中記錄，窗口與判準都寫清楚了。TBD-1、TBD-3 不受本範圍影響。
- 限制：實送證據不在 repo 裡，本次以紀錄為準，沒有重新送出；本次沒有連到 Telegram 或任何外部服務。

## 流程判定

PASS。沒有 MUST FIX 或 SHOULD FIX。AC-5 的每一項檢查方式都有測試或紀錄中的實送證據。四道命令都由我重跑並通過。README 記錄的變異證據我都重現了，另外做的變異也都會紅燈。兩項 NICE TO HAVE 只牽涉文件說法與測試精確度，不影響行為。

PASS 只適用於上列 Reviewed HEAD SHA。之後在這個分支提交 issue README 記帳區段與本報告以外的檔案（例如交接文件），PASS 會失效、需要重新審查（流程契約 2.3 以下只有單獨新增本報告的提交可沿用）；提交前先告知使用者，由使用者決定做法。
