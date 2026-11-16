# Issue 32 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)。

## 設計

全部放在 `wukong-tg-client`（零內部依賴，`wukong-telegram` 與 `wukong-schedulerd` 都已依賴它），兩個呼叫端只換一行。

- **判斷 HTML 解析失敗**：依 `TgError::Api` 的字串判斷（`check_ok` 組成 `telegram api error <code>: <description>`），條件是 `400` 而且描述含 `can't parse entities`（步驟 1 實測：Telegram 拒收的 4 種樣本都是這個字樣，對照組沒有）。`TgError` 目前只有字串，不為此改成結構化欄位；判斷函式放在 `TgError` 上或 `client.rs` 的自由函式，實作時擇一。
- **HTML 轉純文字**：只處理 `wukong-render` 會輸出的 Telegram HTML 子集：
  - `<a href="URL">文字</a>` → `文字 (URL)`
  - 其他標籤拿掉，標籤之間的文字、空白、換行原樣保留
  - 還原 `&lt;`、`&gt;`、`&amp;`（`escape_html` 只跳脫這三個；`&quot;` 一併還原）；還原時 `&amp;` 最後處理，避免 `&amp;lt;` 被還原兩次
- **帶退回的送出**：`send_html_with_plain_fallback(client, chat_id, html, context) -> Result<i64, TgError>`
  1. `client.send_message_html(chat_id, html)`；成功就回傳（SCN-007）
  2. 錯誤不是解析失敗 → 原樣回傳（SCN-005）
  3. 解析失敗 → `eprintln!` 一行警告：呼叫端 `context` 與 Telegram 錯誤描述，不含 `html`（SCN-004）；再 `client.send_message(chat_id, &plain)`，結果原樣回傳（SCN-001、SCN-002、SCN-006）
- **呼叫端**：
  - `dispatch.rs:1514`：`log_send(send_html_with_plain_fallback(client, chat_id, c, "answer").await, "answer")`
  - `notify.rs:86`：`send_html_with_plain_fallback(client, chat_id, c, "schedule").await?`；`?` 保留，所以純文字也失敗時照現況中止（SCN-006）
- **mock**：`MockTgClient` 加一個設定，讓 `send_message_html` 依條件回解析失敗的 `TgError::Api`（字串照步驟 1 的實際回應），被拒的呼叫不寫進 `sent`；另一個設定回非解析錯誤，供 SCN-005。只拒指定段落（SCN-001 的第 2 段）的方式，實作時以最簡單的形式決定，例如依內容比對。

## 實作步驟

1. ✅ **首要驗證：Telegram 解析失敗的錯誤契約**（SCN-008 前半；README 首要驗證）
   - 做法：照 #31 步驟 1，token 由 RunWuKong `wukong-telegram` 容器以 `docker exec` 唯讀讀出、經 stdin 交給發送程式，不出現在命令列或檔案；只送到 allowed chat，不重啟、不改設定
   - 樣本：不支援的標籤（`<think>x</think>`）、沒有結尾的標籤（`<b>x`）、不合法的實體（`&foo;`），對照組為不是解析失敗的 400（例如空白 `text`，或不存在的 `chat_id`）
   - 產出：本步驟下方的探測紀錄表（HTML 原文、`error_code`、`description`）
   - 相依：無
   - 完成判準：3 則格式錯誤樣本的描述有共同的解析失敗字樣，對照組沒有；判斷條件依此寫回「設計」。字樣不一致時先停下來回報
   - 結果（2026-11-16，RunWuKong `wukong-telegram` 容器 `ghcr.io/raybird/wukong:v0.24.0` 的 bot；token 由 `docker exec wukong-telegram cat /data/settings.json` 經 stdin 交給 scratchpad 的 Python，未出現在命令列、檔案或輸出；HTML 樣本前綴 `#32 探測 N`）：

     | 樣本 | 送出的 HTML（去掉前綴） | 回應 |
     |---|---|---|
     | 不支援的標籤 | `<think>x</think>` | `400` `Bad Request: can't parse entities: Unsupported start tag "think" at byte offset 13` |
     | 沒有結尾的標籤 | `<b>x` | `400` `Bad Request: can't parse entities: Can't find end tag corresponding to start tag "b"` |
     | 多出的結尾標籤 | `x</b>` | `400` `Bad Request: can't parse entities: Unexpected end tag at byte offset 14` |
     | 屬性值沒有結尾引號 | `<a href="https://x.io>y</a>` | `400` `Bad Request: can't parse entities: Unclosed start tag at byte offset 13` |
     | 不合法的實體 | `a &foo; b` | `ok: true`（被接受） |
     | 超出範圍的數字實體 | `&#1114112;` | `ok: true`（被接受） |
     | 對照：不存在的 chat | `chat_id=1`，`<b>ok</b>` | `400` `Bad Request: chat not found` |
     | 對照：空白 text | `   ` | `400` `Bad Request: text must be non-empty` |

     第一次執行時，空白 text 那則在 TLS 握手遇到 `Connection reset by peer`，之後單獨重送一次。
   - 結論：Telegram 拒收的 4 種樣本都回 `400`，描述都以 `Bad Request: can't parse entities:` 開頭；兩則對照組都沒有這個字樣。判斷條件定為「`error_code` 400 而且描述含 `can't parse entities`」。不合法的實體不會被拒，所以不在退回範圍內，也不需要退回；原定的第 3 則樣本改用多出的結尾標籤與屬性值沒有結尾引號兩則（SCN-008 的樣本列表因此修訂，見 README）
2. ✅ **單元層：判斷與轉換**（SCN-003、SCN-005 的判斷部分）
   - 產出：`wukong-tg-client` 的單元測試——解析失敗判斷以步驟 1 的實際錯誤字串為正例、對照組與 403／網路錯誤為反例；HTML 轉純文字以 README SCN-003 的 4 組手寫期望值斷言完整字串
   - 紅燈：先寫測試，函式不存在或回傳未實作值時紅燈，記錄命令與輸出
   - 相依：步驟 1
   - 完成判準：`cargo test -p wukong-tg-client` 全綠
   - 完成證據（2026-11-16）：`client::tests` 新增 `html_parse_failures_from_telegram_are_recognised`（步驟 1 的 4 則實際描述，經 `check_ok` 組成 `TgError`）、`other_errors_are_not_html_parse_failures`（兩則對照組、403、網路錯誤）、`html_to_plain_strips_tags_unescapes_and_spells_out_links`（SCN-003 的 4 組加 `&amp;lt;` 只還原一次）。先放回傳 `false`／空字串的 stub：`cargo test -p wukong-tg-client --lib client::tests` 1 passed、2 failed（正例 panic 在第一則描述；轉換 `left: ""`、`right: "粗 斜 x"`）；反例在 stub 上本來就通過，作用是防判斷過寬。實作後 3 passed
3. ✅ **送出函式與 mock**（SCN-001、SCN-004～SCN-007 的送出部分）
   - 產出：`send_html_with_plain_fallback`、mock 的拒收設定、單元測試斷言 `sent` 的順序、`html` 旗標與內容：被拒時恰好一筆純文字；非解析錯誤時沒有純文字；純文字也失敗時回傳錯誤；HTML 成功時只有一筆 HTML
   - SCN-004 的 stderr 警告：以測試斷言警告字串的組成函式含 `context` 與描述、不含 `html`（`eprintln!` 本身不擷取）
   - 相依：步驟 2
   - 完成判準：先紅後綠，記錄命令與輸出
   - 完成證據（2026-11-16）：mock 新增 `reject_html_containing`／`reject_plain_containing`，錯誤經 `check_ok` 以 Telegram 回應格式產生，被拒的呼叫不寫進 `sent`。新增 `rejected_html_is_resent_as_plain_text`、`accepted_html_is_sent_once_without_fallback`（SCN-007）、`other_errors_are_returned_without_resending`（SCN-005）、`plain_resend_failure_is_returned`（SCN-006）、`fallback_warning_names_the_caller_and_reason_but_not_the_content`（SCN-004）。stub 為現況行為（直接 `send_message_html`）：`cargo test -p wukong-tg-client --lib client::` 13 passed、3 failed（退回、純文字也失敗、警告三項；HTML 被接受與其他錯誤兩項是現況防護，在 stub 上通過）。實作後 `cargo test -p wukong-tg-client` 33 passed
4. ✅ **呼叫端接線**（SCN-001、SCN-002、SCN-005、SCN-006）
   - 產出：`dispatch.rs`、`notify.rs` 改用新函式；整合測試：
     - `wukong-telegram`：3 段回覆、第 2 段被拒，`sent` 為 HTML／純文字／HTML（SCN-001）
     - `wukong-schedulerd`：2 段結果、第 1 段被拒，`notify_turn_result` 回 `Ok(true)`，`sent` 為純文字／HTML（SCN-002）；非解析錯誤時回 `Err` 且只送出被拒前的段落（SCN-005）
   - 紅燈：改呼叫端之前先寫整合測試，在 main 的呼叫端上紅燈（被拒段落遺失、排程回 `Err`）
   - 相依：步驟 3
   - 完成判準：兩個 crate 的測試全綠
   - GitNexus（2026-11-16，改動前）：`handle_message_with_responder` upstream 為 CRITICAL（直接呼叫者 4、受影響流程 12），`notify_turn_result` 為 HIGH（直接呼叫者 6，含 `run_scan`）；兩處都只換掉送出 HTML 的那一行，已在對話中告知使用者
   - 完成證據（2026-11-16）：
     - `wukong-schedulerd` 新增 `rejected_html_chunk_is_resent_as_plain_and_later_chunks_still_go_out`（SCN-002）、`non_parse_error_still_aborts_delivery`（SCN-005）、`failed_plain_resend_aborts_delivery`（SCN-006）；在 main 的呼叫端上 7 passed、2 failed（SCN-002 的 `unwrap()` 拿到解析失敗的 `Err`；SCN-006 拿到解析失敗而不是 `Too Many Requests`）；SCN-005 是現況防護
     - `wukong-telegram` 新增 `rejected_html_chunk_is_resent_as_plain_and_other_chunks_stay_html`（SCN-001）；在 main 的呼叫端上紅燈，實際只有 `(true, "甲…")`、`(true, "丙…")`，「乙」那段遺失。第一次的期望值少了切段保留的結尾 `\n\n`，屬測試本身的錯，改為比對 `trim_end()` 後的文字，紅燈原因仍只是「乙」遺失
     - 接線後 `cargo test -p wukong-telegram`、`-p wukong-schedulerd`、`-p wukong-tg-client` 全綠
   - 精煉（2026-11-16）：`is_html_parse_error`、`html_to_plain` 只在 `client.rs` 內使用，改為私有；其餘無可簡化處
5. ✅ **實機確認退回**（SCN-008 後半）
   - 做法：以步驟 1 的方式，把一則格式錯誤的 HTML 經 `send_html_with_plain_fallback`（`ReqwestTgClient`）送出，例如在 scratchpad 寫一個暫時的範例程式，不進 repo
   - 完成判準：HTML 請求被拒、純文字重送回 `ok: true`，回傳的 `result.text` 為預期的純文字；記錄 stderr 警告原文
   - 相依：步驟 4
   - 完成證據（2026-11-16，SCN-008 重新核准後）：scratchpad 的暫時程式（path 依賴本分支的 `wukong-tg-client`，未進 repo）以 `ReqwestTgClient` 呼叫 `send_html_with_plain_fallback`，token 照步驟 1 的方式經 stdin 傳入。送出 `<b>#32 實機退回</b> <think>x</think> a &lt; b <a href="https://x.io/?a=1&amp;b=2">docs</a>`：
     - stderr：`🐵 Telegram 拒收 HTML（live32），改送純文字：telegram api error: telegram api error 400: Bad Request: can't parse entities: Unsupported start tag "think" at byte offset 24`（不含訊息內容）
     - 純文字重送回 `ok`，`message_id=24`
     - 內容核對不經實作：以手寫的預期文字 `#32 實機退回 x a < b docs (https://x.io/?a=1&b=2)` 對同一則訊息呼叫 `editMessageText`，Telegram 回 `400 Bad Request: message is not modified: specified new message content and reply markup are exactly the same as a current content…`，代表送達的文字與預期逐字相同
6. ✅ **常青文件與交付檢查**
   - 產出：`docs/entrypoints.md`「格式渲染」、`AGENTS.md`「排程結果回送」、`CHANGELOG.md` `[Unreleased]` Fixed
   - 完成判準：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` 全綠
   - 相依：步驟 4（步驟 5 不改程式碼時不需重跑）
   - 完成證據（2026-11-16）：三份文件已更新；`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 無警告；`cargo test --workspace --locked` 740 passed、0 failed（main 為 728，新增 12）

7. ✅ **PR #35 第一輪審查處理**（SCN-003、SCN-004；README TBD-2～TBD-5）
   - 審查：2026-11-16 第三方審查 https://github.com/raybird/Wukong/pull/35#issuecomment-6907242018 （基線 e353b84），Conditional Approve：P0／P1 無，P2 ×2、P3 ×1，另列 3 項後續候選
   - 查證（2026-11-16）：scratchpad 的暫時程式（path 依賴 `wukong-render`，未進 repo）渲染含雙引號的網址：`[docs](https://x.io/a"b)`、`[docs](<https://x.io/a"b>)`、`<https://x.io/a"b>` 都輸出 `<a href="https://x.io/a"b">…</a>`；網址含 `>`、`<` 時輸出 `&gt;`、`&lt;`。以步驟 1 的方式實送 `<a href="https://x.io/a"b">docs</a>`，Telegram 回 `400 Bad Request: can't parse entities: Empty attribute name in the tag "a" at byte offset 13`，屬解析失敗、會觸發退回。審查舉的少了結尾引號的寫法，render 產生不出來
   - 處理：依使用者 2026-11-16 的四題回答——SCN-004 修訂為錯誤描述照原樣記錄、不含訊息本文（程式不改）；`html_to_plain` 的測試加上 render 實際會產生的雙引號連結，期望值手寫為 `docs (https://x.io/a"b)`；mock 不加嘗試紀錄；後續候選開 #36
   - 完成證據（2026-11-16）：新增的案例在 e353b84 的實作上直接通過（刻畫既有行為，程式未改，所以沒有紅燈階段）；`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 無警告；`cargo test --workspace --locked` 740 passed、0 failed（案例加在既有測試內，測試數不變）

## 使用方式對照

| 情境 | 變更前 | 變更後 |
|------|--------|--------|
| 回覆含 Telegram 不接受的 HTML | 該段（排程：該段之後）沒收到 | 收到純文字版本，stderr 多一行退回警告 |
| 其他情況 | — | 不變 |

## 測試策略

| 驗收 | 層級 | 說明 |
|------|------|------|
| SCN-003 | 單元 | 手寫期望值，不以同一套轉換邏輯重算 |
| SCN-001、SCN-002 | 呼叫端整合（mock client） | 外迴圈；斷言 `sent` 的 `html` 旗標與內容，不只斷言筆數 |
| SCN-004、SCN-005、SCN-006、SCN-007 | 送出函式單元 + 呼叫端整合 | SCN-005 的排程分支在呼叫端驗證 `?` 仍中止 |
| SCN-008 | 實機 | mock 的錯誤字串由此取得，避免判斷與 mock 同源（W-2）；對照組證明判斷不會過寬 |

外迴圈（步驟 4）與單元（步驟 2、3）覆蓋不同層級：前者驗證呼叫端確實改走新函式、排程不再中止，後者驗證判斷與轉換，保留雙迴圈。

## 檢查清單

- [x] 步驟 1 的實際字樣已寫回「設計」，mock 的錯誤字串與它一致
- [x] 每個 SCN 都有對應測試或實機紀錄
- [x] token 沒有出現在命令列、檔案或 log
- [x] 三道 CI 命令全綠；常青文件已更新
