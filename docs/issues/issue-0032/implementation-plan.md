# Issue 32 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)。

## 設計

全部放在 `wukong-tg-client`（零內部依賴，`wukong-telegram` 與 `wukong-schedulerd` 都已依賴它），兩個呼叫端只換一行。

- **判斷 HTML 解析失敗**：依 `TgError::Api` 的字串判斷（`check_ok` 組成 `telegram api error <code>: <description>`），條件是 `400` 而且描述含步驟 1 實測到的解析失敗字樣（預期 `can't parse entities`）。`TgError` 目前只有字串，不為此改成結構化欄位；判斷函式放在 `TgError` 上或 `client.rs` 的自由函式，實作時擇一。
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

1. 📝 **首要驗證：Telegram 解析失敗的錯誤契約**（SCN-008 前半；README 首要驗證）
   - 做法：照 #31 步驟 1，token 由 RunWuKong `wukong-telegram` 容器以 `docker exec` 唯讀讀出、經 stdin 交給發送程式，不出現在命令列或檔案；只送到 allowed chat，不重啟、不改設定
   - 樣本：不支援的標籤（`<think>x</think>`）、沒有結尾的標籤（`<b>x`）、不合法的實體（`&foo;`），對照組為不是解析失敗的 400（例如空白 `text`，或不存在的 `chat_id`）
   - 產出：本步驟下方的探測紀錄表（HTML 原文、`error_code`、`description`）
   - 相依：無
   - 完成判準：3 則格式錯誤樣本的描述有共同的解析失敗字樣，對照組沒有；判斷條件依此寫回「設計」。字樣不一致時先停下來回報
2. 📝 **單元層：判斷與轉換**（SCN-003、SCN-005 的判斷部分）
   - 產出：`wukong-tg-client` 的單元測試——解析失敗判斷以步驟 1 的實際錯誤字串為正例、對照組與 403／網路錯誤為反例；HTML 轉純文字以 README SCN-003 的 4 組手寫期望值斷言完整字串
   - 紅燈：先寫測試，函式不存在或回傳未實作值時紅燈，記錄命令與輸出
   - 相依：步驟 1
   - 完成判準：`cargo test -p wukong-tg-client` 全綠
3. 📝 **送出函式與 mock**（SCN-001、SCN-004～SCN-007 的送出部分）
   - 產出：`send_html_with_plain_fallback`、mock 的拒收設定、單元測試斷言 `sent` 的順序、`html` 旗標與內容：被拒時恰好一筆純文字；非解析錯誤時沒有純文字；純文字也失敗時回傳錯誤；HTML 成功時只有一筆 HTML
   - SCN-004 的 stderr 警告：以測試斷言警告字串的組成函式含 `context` 與描述、不含 `html`（`eprintln!` 本身不擷取）
   - 相依：步驟 2
   - 完成判準：先紅後綠，記錄命令與輸出
4. 📝 **呼叫端接線**（SCN-001、SCN-002、SCN-005、SCN-006）
   - 產出：`dispatch.rs`、`notify.rs` 改用新函式；整合測試：
     - `wukong-telegram`：3 段回覆、第 2 段被拒，`sent` 為 HTML／純文字／HTML（SCN-001）
     - `wukong-schedulerd`：2 段結果、第 1 段被拒，`notify_turn_result` 回 `Ok(true)`，`sent` 為純文字／HTML（SCN-002）；非解析錯誤時回 `Err` 且只送出被拒前的段落（SCN-005）
   - 紅燈：改呼叫端之前先寫整合測試，在 main 的呼叫端上紅燈（被拒段落遺失、排程回 `Err`）
   - 相依：步驟 3
   - 完成判準：兩個 crate 的測試全綠
5. 📝 **實機確認退回**（SCN-008 後半）
   - 做法：以步驟 1 的方式，把一則格式錯誤的 HTML 經 `send_html_with_plain_fallback`（`ReqwestTgClient`）送出，例如在 scratchpad 寫一個暫時的範例程式，不進 repo
   - 完成判準：HTML 請求被拒、純文字重送回 `ok: true`，回傳的 `result.text` 為預期的純文字；記錄 stderr 警告原文
   - 相依：步驟 4
6. 📝 **常青文件與交付檢查**
   - 產出：`docs/entrypoints.md`「格式渲染」、`AGENTS.md`「排程結果回送」、`CHANGELOG.md` `[Unreleased]` Fixed
   - 完成判準：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` 全綠
   - 相依：步驟 4（步驟 5 不改程式碼時不需重跑）

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

- [ ] 步驟 1 的實際字樣已寫回「設計」，mock 的錯誤字串與它一致
- [ ] 每個 SCN 都有對應測試或實機紀錄
- [ ] token 沒有出現在命令列、檔案或 log
- [ ] 三道 CI 命令全綠；常青文件已更新
