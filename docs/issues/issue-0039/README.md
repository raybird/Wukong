# Issue 39 - Telegram 回答 agent 提問後看不到處理進度，只顯示「已送出回答。」 (Small)

## 概述

GitHub #39。2026-11-18 使用者回報：agent 在回合中提問（`QuestionRequest`），按下選項送出回答後，畫面只剩「已送出回答。」，要等最終回覆出現才知道回合還在跑。使用者同日確認發生在 **Telegram**；Web 未確認，不在本 issue 範圍（見待確認事項）。

### 現況（2026-11-19 讀 `crates/wukong-telegram/src/dispatch.rs` 於 main 662bce6）

- 私人聊天以 `sendMessageDraft` 顯示進度（`start_progress_display`），群組等不支援草稿的聊天退回單一狀態訊息。
- 收到 `Progress::QuestionRequest` 時呼叫 `clear_progress_display`：狀態訊息模式刪掉那則訊息；草稿模式不做事，但送出問題訊息本身就會讓草稿消失（見下方實送）。
- 回答送出成功後，`advance_or_reply` 只把問題訊息改成「已送出回答。」；取消與逾時（`reject_and_clear_question`）只把問題訊息改成「已取消問題。」／「問題已逾時，已取消。」。三條路徑都不通知進度那一端。
- 進度只在下一個 `Role`／`Reasoning`／`ToolUse` 事件才重新送出。agent 收到回答後若沒有這類事件（例如模型不輸出明文推理），最終回覆前畫面上只有「已送出回答。」。

### 實送探測（2026-11-19，RunWuKong `wukong-telegram` 容器的 bot，只送到 allowed chat，私人聊天）

token 由 `docker exec wukong-telegram cat /data/settings.json` 經 stdin 交給 scratchpad 的 Python（`probe39.py`、`probe39b.py`，未進 repo），未出現在命令列、檔案或輸出；未重啟、未改設定。草稿是否顯示由使用者在手機上觀察（Bot API 只回 `ok`）。

| 時間 | 送出 | API | 使用者觀察 |
|---|---|---|---|
| 09:54:44 | 一般訊息 0 後，草稿 D1（390011），每 3 秒刷新 15 秒 | `ok` | 出現 |
| 09:55:00 | 一般訊息 2（模擬問題訊息） | `ok` | 出現；草稿 1 被取代 |
| 09:55:05 | 同一編號 D1 再送，每 3 秒刷新 15 秒 | `ok` | 出現 |
| 09:55:20 | 新編號 D2（390012），每 3 秒刷新 15 秒 | `ok` | 出現 |
| 09:55:36 | 一般訊息 5 | `ok` | 最後只留 0、2、5 三則訊息 |
| 09:59:08 | 草稿 390021 只送一次、不刷新 | `ok` | 出現，中途自己消失；使用者估計消失後約 30 秒才出第 8 則 |
| 10:00:39 | 一般訊息 8（草稿送出後約 90 秒） | `ok` | 出現 |

結論：一般訊息會取代草稿；之後用同一個 `draft_id` 再送，草稿會重新顯示，所以不必換編號。草稿不刷新約 60 秒會消失，回合中任何超過約 60 秒沒有事件的空檔（回答前也一樣）都會讓進度不見。同日 09:06 第一次探測在第 3 步刷新時遇到 `Connection reset by peer` 中斷，改為網路錯誤重試後重跑即上表。

### 部署後觀察（2026-11-19，RunWuKong 升級到 v0.25.0 後，TBD-2）

使用者在私人聊天實測：回答提問後出現「已收到回答，繼續處理…」；取消沒測到；回合處理中、文字沒變時，草稿約 60 秒後消失。

- 補探測（17:24:34–17:28:14，`probe39c.py`）：A 段同一段文字每 20 秒重送（390031），草稿撐到 105 秒後的結束訊息；B 段每 20 秒換文字（390032），草稿照常更新。所以同文字重送確實能保活，問題不在重送本身。
- 讀容器的 `chat_live_events`（唯讀）：16:41:51 回答後 opencode 每 5～27 秒就有一次思考或工具事件，不是沒有事件。照 `run_progress` 的方式把思考與工具累加，第一段思考後進度文字已 8220 字，回答後一萬到五萬字。
- 長度探測（`probe39d.py`）：超過 4096 字的 `sendMessageDraft` 回 `Bad Request: MESSAGE_TOO_LONG`。
- 結論：`bubble_text` 自 2026-06-21 `7b46467`（原本只留最後 200 字）起把整個回合的思考全文放進進度，超過 4096 字後每次更新與重送都被拒收，退回的一般訊息也一樣太長；畫面停在最後一次成功的短文字（「已收到回答，繼續處理…」），約 60 秒後被收掉。群組狀態訊息的編輯有同樣的上限。

## 現況

**現行 review**: review-4449e20.md

## 涉及檔案

- `crates/wukong-telegram/src/dispatch.rs` - 進度顯示工作（回答／取消／逾時後重新顯示、草稿定期重送）、進度文字長度上限（`bubble_text`）、問題回覆路徑與單元測試
- `CHANGELOG.md` - `[Unreleased]` 記錄使用者看得到的修正
- `docs/entrypoints.md` - Telegram「即時狀態回饋」補上問答後的進度與草稿重送

## 驗收條件

1. **AC-1**：回答送出成功後，不等 agent 的新事件，馬上在問題訊息下方顯示「🐵　悟空·{角色} 已收到回答，繼續處理…」（還沒有角色時為「🐵　已收到回答，繼續處理…」）；私人聊天用原本的草稿編號，群組用一則新的狀態訊息。問題訊息照原本改成「已送出回答。」。之後角色、思考、工具事件照原本方式更新同一個進度，回合結束照原本方式收掉（草稿由最終回答取代，狀態訊息刪除）。回答送出失敗時照原本只送「⚠️ 回答送出失敗：…」，不重新顯示進度 — 檢查方式：單元測試以會等待回答的假 backend 跑一回合、同時送出回答 callback，草稿與狀態訊息兩種模式各斷言：回答後、回合結束前出現上述文字，順序在問題訊息之後；回合結束後狀態訊息已刪除；送出失敗的情況不出現該文字
2. **AC-2**：按「取消」或問題逾時被自動拒絕後，問題訊息照原本改成「已取消問題。」／「問題已逾時，已取消。」，並馬上顯示「🐵　悟空·{角色} 繼續處理…」（還沒有角色時為「🐵　繼續處理…」），其餘同 AC-1 — 檢查方式：單元測試取消與逾時各一例，斷言問題訊息文字與重新顯示的進度
3. **AC-3**：私人聊天（草稿模式）在回合執行期間，進度超過 20 秒沒有送出，就以目前的內容重送一次草稿；等待回答問題期間不重送，讓問題訊息留在最下方；回合結束後不再送。群組的狀態訊息不會自己消失，不重送。等待回答期間收到的角色、思考、工具事件（例如同一步的其他工具呼叫）兩種模式都不更新顯示，只累積內容，回答、取消或逾時後才顯示 — 檢查方式：單元測試以暫停的 tokio 時間推進，斷言無事件 20 秒後草稿以相同編號、相同內容重送，等待問題期間與回合結束後沒有新的草稿，狀態訊息模式不因時間經過送出或編輯；另以草稿、狀態訊息兩種模式各斷言等待期間注入角色、思考、工具事件不產生草稿、訊息或編輯，回答後的文字帶上等待期間得到的角色
4. **AC-4**：既有行為不變：草稿漸進更新、群組狀態訊息、問題鍵盤、多題、自訂回答、取消、逾時與最終回答的既有測試全過 — 檢查方式：`cargo test -p wukong-telegram --locked` 全綠，既有測試不改期望值
5. **AC-5**：進度文字（草稿與群組狀態訊息）放得下就照現在顯示全部思考內容；超過 Telegram 4096 字上限時，只留思考內容最後面放得下的部分（整則約 4000 字，留餘裕），前面加「…」，角色那一行照常顯示，讓更新與 AC-3 的重送不被拒收 — 檢查方式：單元測試斷言短思考照全文顯示（既有 `bubble_text_keeps_full_reasoning` 不改）、長思考的進度文字不超過上限且保留結尾與「…」；以草稿模式跑累積超過上限的思考事件，斷言送出的每一則都不超過上限；實送一則照實作組出、達上限長度的草稿，`sendMessageDraft` 回 `ok`

- **核准日期**: 2026-11-19
- **核准來源**: 使用者 2026-11-19 對話以「那幫我開始下一個 dev-cycle 吧」推進，並選 #39「回答後看不到進度 (Recommended)」；同日 Q1「#39 這次要處理哪些入口」選「只做 Telegram (Recommended)」→ 範圍；Q2「按下回答、送出成功後，畫面要怎麼呈現」選「馬上在問題下方重新顯示進度 (Recommended)」→ AC-1；Q3「取消與逾時也要重新顯示進度嗎」選「也要，和回答一樣處理 (Recommended)」→ AC-2；Q4「能不能實送探測草稿」選「可以，我在手機上看 (Recommended)」→ 概述實送探測；Q5「要在哪些時候定期重送草稿」選「整個回合都維持，每 20 秒重送 (Recommended)」→ AC-3。同日總結確認「① AC-1 進度文字、問題訊息仍改「已送出回答。」② 送出失敗不重新顯示 ③ AC-2 進度文字 ④ AC-4 既有測試不改期望值 ⑤ TBD-2 端到端觀察不擋合併」選「照建議核准 (Recommended)」→ AC-1～AC-4、TBD-2。2026-11-19 PR #41 第一輪審查（https://github.com/raybird/Wukong/pull/41#pullrequestreview-5970194121，使用者本人）P2 建議「等待期間可繼續累積 role/reasoning/tool 資訊，但不要更新顯示；收到 QuestionAnswered 或 QuestionClosed 後再顯示」，並註明「優先確認 P2 的事件順序是否可能發生；若可能，先修正並補測試再合併」；確認可能發生（見步驟 5）→ AC-3 末句。2026-11-19 部署後觀察（見概述）後，使用者對「進度文字超過 Telegram 4096 字上限時，要怎麼顯示思考內容」選「放得下就全文，超過留最後面 (Recommended)」→ AC-5；對「這個修正要記在哪個 issue」選「繼續在 #39 修」→ 本 issue 的後續 PR

**風險**: Low — 只改 `wukong-telegram` 的進度顯示；最大的未知（Telegram 草稿在訊息之後能否重新顯示、不刷新多久消失）已由實送探測確認；回答 callback 與進度工作並行，以會等待回答的假 backend 測試覆蓋；容易回復。依一般相依順序實作與驗證。

## 實作與驗證步驟

1. ✅ **AC-4：改動前的回歸基準** — 驗證方式：記錄改動前 `cargo test -p wukong-telegram --locked` 的結果
   - 完成判準：既有測試在改動前全綠，數量已記錄
   - 證據（2026-11-19，分支起點 2a791e5）：lib 34 passed；`tests/` 3、1 passed；1 ignored
2. ✅ **AC-1～AC-3：先寫紅燈測試** — 驗證方式：`cargo test -p wukong-telegram --locked`，新測試在改動前失敗，失敗輸出對應概述的現況
   - 完成判準：記錄紅燈命令與輸出
   - 證據（AC-1、AC-2）：新增 `answered_question_shows_progress_again_as_draft`、`answered_question_shows_progress_again_as_status_message`、`failed_answer_does_not_show_progress_again`、`cancelled_question_shows_progress_again`、`expired_question_shows_progress_again`。假 backend `AnswerWaitingBackend` 送出提問後等待回答路徑，再在回合結束前檢查進度文字有沒有出現；resolver 照 bot 的更新迴圈送 callback 或觸發逾時清理，送出前先斷言進度文字還沒出現。改動前 `35 passed; 4 failed`，4 個都紅在 `run_question_turn(...)` 回傳 false（回合結束前沒有出現進度文字）；送出失敗那個本來就綠，是防退化用的。第一版把「回答前不出現」的檢查放在 backend 被喚醒之後，那時回答路徑已經跑完，會誤判，所以移到 resolver 送出之前；改完後用拿掉兩處 `notify` 的版本重驗，同樣 4 個紅在同一斷言
   - 證據（AC-3）：先把進度迴圈原樣抽成 `run_progress`（新增起始文字參數但尚未使用），既有測試全綠；新增 `idle_draft_progress_is_resent_every_20_seconds_except_while_a_question_waits` 與 `status_message_progress_is_not_resent_when_idle`（`#[tokio::test(start_paused = true)]`，`wukong-telegram` dev-dependency 為 tokio 加 `test-util`，`Cargo.lock` 不變）。改動前 `40 passed; 1 failed`，紅在 21 秒時應重送起始文字：`left: []`、`right: [(42, "🐵 收到，思考中…")]`
3. ✅ **AC-1～AC-3：回答／取消／逾時後重新顯示進度，草稿定期重送** — 驗證方式：步驟 1、2 的測試全綠
   - 完成判準：`cargo test -p wukong-telegram --locked` 全綠；修改前對每個要改的函式跑 GitNexus `impact` 並回報
   - GitNexus `impact`（upstream，2026-11-19）：`handle_message_with_responder`、`advance_or_reply`、`PendingQuestion` 為 CRITICAL（每一則 Telegram 回合都經過 `run_bot` → `dispatch_update_event` → `handle_message_with_responder`），`reject_and_clear_question` 為 LOW；直接呼叫端為 `complete_custom_answer`、`handle_callback_query`、`cleanup_expired_questions` 與 main.rs 自建 `PendingQuestion` 的測試。已向使用者回報；改動只在進度工作與問題結束後的通知，回合、回答送出與最終回答的送法不變
   - 實作（`crates/wukong-telegram/src/dispatch.rs`）：進度迴圈抽成 `run_progress`，以 `tokio::select!` 同時等事件與 `DRAFT_REFRESH`（20 秒）；草稿模式、沒有問題在等時，超過 20 秒沒有送出就以目前內容重送。`PendingQuestion` 新增 `resume: ProgressResume`，帶進度通道的 weak sender；`advance_or_reply` 送出成功後通知 `QuestionAnswered`，`reject_and_clear_question`（取消與逾時共用）通知 `QuestionClosed`。用 weak 是因為提問後沒回答就結束的回合（既有 `QuestionBackend` 測試就是這樣）會在 `PendingQuestions` 留下一筆，強參照會讓進度工作等不到通道關閉、回合收不了尾
   - 證據：`cargo test -p wukong-telegram --locked` lib 41 passed、0 failed。變異檢查：(a) 等問題期間也重送 → AC-3 測試紅在 `"resent while a question waits"`；(b) 回答送出失敗也通知 → `failed_answer_does_not_show_progress_again` 紅在 `run_question_turn` 斷言；(c) 狀態訊息模式也重送 → `status_message_progress_is_not_resent_when_idle` 紅在 `edits.len() == 1`
4. ✅ **交付檢查** — 驗證方式：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`；更新 `CHANGELOG.md` `[Unreleased]` 與 `docs/entrypoints.md`「即時狀態回饋」
   - 完成判準：三道命令全綠，兩份文件已更新
   - 提交前 GitNexus `detect_changes` 的索引過期，行號位移後把沒改的 `QuestionResponder`、`render_pending_question` 等也列為改動；改以 `git diff -U0` 的 hunk 核對，改動只在 `Progress`、`PendingQuestion`、`advance_or_reply`、`reject_and_clear_question`、`handle_message_with_responder`、新增的 `resumed_text`／`run_progress` 與測試
   - 證據（2026-11-19）：fmt 無差異；clippy 無警告；workspace 756 passed、0 failed；CHANGELOG `[Unreleased]` Fixed 與 entrypoints「即時狀態回饋」已更新
5. ✅ **AC-3 末句：PR #41 第一輪審查 P2，等待回答期間的事件不更新顯示** — 驗證方式：先紅後綠的單元測試，再重跑步驟 4 的三道命令
   - 完成判準：新測試改動前在草稿、狀態訊息兩種模式都紅在等待期間的斷言，改動後全綠；fmt、clippy、workspace 測試全綠
   - 會不會發生（2026-11-19 讀 `crates/wukong-gateway/src/opencode_server.rs` 的 SSE 讀取迴圈）：送出 `QuestionRequest` 後迴圈不暫停，之後 session 上的事件照樣經 `on_event` 轉成 `Progress`，只有 session idle 才結束。opencode 同一步可有多個工具呼叫，提問工具在等回答時另一個工具的 `ToolUse` 就會進來，所以審查描述的情況會發生，不能降級
   - 證據（紅燈）：新增 `progress_events_while_a_question_waits_show_only_after_it_is_answered`（paused tokio time，提問後注入 `Role(Fixer)`、`Reasoning`、`ToolUse`）。改動前草稿模式紅在 `"shown while a question waits"`：`left: (3, 0, 0)`、`right: (0, 0, 0)`（送了 3 個草稿）；暫時把兩種模式的順序對調重跑，狀態訊息模式紅在同一斷言：`left: (0, 1, 2)`（在問題下方新送一則狀態訊息並編輯 2 次），之後還原順序
   - 實作：`run_progress` 的 `if show` 改為 `if show && !question_waiting`；事件仍更新 `status`，回答、取消或逾時後顯示。重送計時器原本就在等待期間停用
   - GitNexus `impact`：索引早於本 PR，找不到 `run_progress`（UNKNOWN）；唯一呼叫端是 `handle_message_with_responder`（步驟 3 已回報 CRITICAL）。改動只影響等待回答期間進度工作送不送顯示
   - 證據（綠燈，2026-11-19）：`cargo test -p wukong-telegram --locked` lib 42 passed、0 failed；fmt 無差異；clippy 無警告；workspace 757 passed、0 failed；CHANGELOG `[Unreleased]` 與 entrypoints「即時狀態回饋」補上等待期間不更新
6. ✅ **AC-5：先寫紅燈測試** — 驗證方式：`cargo test -p wukong-telegram --locked`，新測試在改動前失敗，失敗輸出顯示進度文字超過上限
   - 完成判準：記錄紅燈命令與輸出
   - 證據（2026-11-19）：新增 `bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit`（6000 字思考，斷言不超過 4096 個 UTF-16 單位、以「💭　…」開頭、保留結尾、不含開頭）與 `long_reasoning_progress_drafts_stay_within_the_telegram_limit`（paused tokio time，草稿模式送 3 段 3000 字思考與工具事件，再閒置 25 秒讓重送也發生，斷言每則草稿都不超過上限、最後一則以「▸ 使用工具 bash」結尾）。改動前 `-- telegram_limit` 2 failed，分別紅在 `6025 units`、`6046 units`
7. ✅ **AC-5：進度文字超過上限時只留思考結尾** — 驗證方式：步驟 6 的測試與既有測試全綠；實送一則達上限長度的草稿；步驟 4 的三道命令
   - 完成判準：測試全綠、實送回 `ok`、fmt／clippy／workspace 全綠；修改前對 `bubble_text` 跑 GitNexus `impact`；CHANGELOG 與 entrypoints 更新
   - GitNexus `impact`（upstream，2026-11-19）：`bubble_text` 為 CRITICAL，每則 Telegram 回合的進度都經過它（`handle_message_with_responder` → `run_progress` → `bubble_text`；索引早於 `run_progress`，列為直接被 `handle_message_with_responder` 呼叫），已向使用者回報；改動只影響超過上限的長文字，短文字照原本全文顯示
   - 實作（`crates/wukong-telegram/src/dispatch.rs`）：`bubble_text` 組出的全文不超過 `PROGRESS_TEXT_LIMIT`（4000 個 UTF-16 單位）就照原本回傳；超過時保留角色那一行與「💭　…」，從思考內容結尾往前放到上限為止。草稿、群組狀態訊息、閒置重送都用這段文字
   - 證據（2026-11-19）：`cargo test -p wukong-telegram --locked` lib 44 passed、0 failed，既有 `bubble_text_keeps_full_reasoning` 不改；變異檢查把上限改成 4200，兩個新測試都紅在 `4200 units`，已改回。實送（`probe39e.py`，照實作格式含 emoji）：4000 單位（3750 字）與 4096 單位（3840 字）的草稿都回 `ok`，對照 `probe39d.py` 全為中文的 4097 字回 `MESSAGE_TOO_LONG`，上限以 UTF-16 單位計。fmt 無差異；clippy 無警告；workspace 759 passed、0 failed；CHANGELOG `[Unreleased]` Fixed 與 entrypoints「即時狀態回饋」已更新
8. ✅ **AC-5：補強長度測試（review-a8efb1c.md SHOULD FIX 1）** — 驗證方式：新斷言在現有實作下綠燈，在報告指出的兩種變異下各紅在新斷言
   - 完成判準：兩個變異各紅一次後還原；fmt／clippy／workspace 全綠
   - 改動：`bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit` 加下限斷言（不少於 3990 單位）；新增 `bubble_text_is_cut_only_past_4000_units`（全文剛好 4000 單位時原樣顯示、不加「…」，多 1 字就以「…」開頭；長度由測試裡的前綴字面值算出，不用實作常數）。這次是補強證據、不改行為，所以紅燈用變異取得
   - 證據（2026-11-19）：變異 (a) 把 `room` 改成 10 → 紅在 `>= 3990` 斷言，`32 units`；變異 (b) 把不截斷的比較由「不超過」改成「小於」→ 紅在 `bubble_text_is_cut_only_past_4000_units` 的 `assert_eq!`；兩者都已還原。fmt 無差異；clippy 無警告；workspace 760 passed、0 failed
9. ✅ **AC-5：補強 emoji 與閒置重送的斷言（review-ba4cf59.md SHOULD FIX 1、NICE TO HAVE 1、2）** — 驗證方式：新斷言在現有實作下綠燈，在對應變異下各紅在新斷言
   - 完成判準：變異各紅一次後還原；fmt／clippy／workspace 全綠
   - 改動：`bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit` 加一組 3000 個「😀」的思考內容（每個佔 2 個 UTF-16 單位），斷言不超過 4096、不少於 3990、以「😀結尾」結束；`long_reasoning_progress_drafts_stay_within_the_telegram_limit` 每輪用不同工具名稱（bash0～bash2），讓每次更新的結尾都不同，並斷言最後一則是前一則長文字的閒置重送；`docs/entrypoints.md` 改成「整則超過約 4000 字（Telegram 上限 4096）時只留最後面放得下的部分」
   - 證據（2026-11-19）：變異 (c) 把反向掃描的 `c.len_utf16()` 兩處改成 1（改數字元）→ 紅在 emoji 的上限斷言，`6026 units`；變異 (d) 把 `keep_alive` 改成恆為 false → 紅在 `the long text was not resent when idle`。第一次做 (d) 時工具名稱都一樣，截到最後 4000 單位後前後兩則本來就相同，變異沒被抓到，改成不同工具名稱後重做才紅；兩個變異都已還原。fmt 無差異；clippy 無警告；workspace 760 passed、0 failed

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-1 | Web Console 送出回答後是否也看不到進度（`chat-question-card.js` 換成「已送出回答。」、問題卡接在進度泡泡之後）。使用者 2026-11-19 Q1 決定只做 Telegram | 未重現；重現後另開 issue | 不影響本 issue |
| TBD-2 | 實際部署上的端到端觀察：RunWuKong 的 bot 正在 long polling，本機再跑一個會搶 `getUpdates`，所以合併前不做。窗口：包含 AC-5 修正（本 issue 後續 PR 合併後的提交）的版本發版，且 RunWuKong 的 `wukong-telegram` 容器升級到該版（以容器映像檔標籤判斷）。判準：使用者在私人聊天答一次 agent 提問，問題下方出現「已收到回答，繼續處理…」；按一次取消，出現「繼續處理…」；回合中超過 60 秒沒有新事件時進度草稿仍在 | 待確認（2026-11-19 第一次觀察：回答後的文字有出現；取消沒測到；草稿約 60 秒後消失，原因見概述「部署後觀察」，由 AC-5 修正，修正發版、部署後重測三項） | 不阻塞合併；行為由單元測試與實送探測覆蓋 |
| TBD-3 | PR #41 第一輪審查 P3：`run_progress` 在 `update_progress_display` 之後就重設計時，不管有沒有送成功；草稿與退回的一般訊息都失敗時，下一次重送要再等 20 秒 | 不影響本次交付（2026-11-19）。追蹤：無。上一次成功的草稿約 60 秒才消失，失敗後 20 秒會再送一次，仍在消失之前；兩個 API 都失敗代表當下連不到 Telegram，提早重試多半也會失敗，下一個事件也會再送 | 最差情況是進度晚 20 秒才重新出現 |

## Timeline

| 日期 | 異動 | 負責人 |
|------|------|--------|
| 2026-11-18 | GitHub #39 建立（討論後不併入 #36）；使用者確認發生在 Telegram | - |
| 2026-11-19 | 建立 issue 文件；Q1～Q5 照建議；實送探測確認草稿可在訊息後以同編號重新顯示、不刷新約 60 秒消失；AC-1～AC-4 核准 | - |
| 2026-11-19 | 實作完成：回答／取消／逾時後重新顯示進度、草稿閒置 20 秒重送（步驟 1～4） | - |
| 2026-11-19 | PR #41 第一輪審查（review 5970194121，COMMENT）：P2 依審查建議修訂 AC-3 末句並修正（步驟 5）；P3 記為 TBD-3 | - |
| 2026-11-19 | PR #41 第二輪審查（review 5970351019，審 68a8cd8）：建議合併，無阻塞性發現 | - |
| 2026-11-19 | PR #41 squash 合併為 3cfe9ab（樹與 68a8cd8 相同），#39 關閉；等待發版部署後的端到端觀察（TBD-2） | - |
| 2026-11-19 | v0.25.0 發版、RunWuKong 升級；TBD-2 觀察到草稿約 60 秒消失，查出進度文字超過 4096 字被拒收；AC-5 核准，於 #39 後續 PR 修正 | - |

---
**建立日期**: 2026-11-19  
**分級**: Small  
**風險**: Low\
**狀態**: AC-5 審查通過（4449e20），待推送與建立 PR
