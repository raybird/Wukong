# 輸出渲染(wukong-render)+ Telegram 訊息整併設計

**日期:** 2026-06-07
**狀態:** 已核可(F1 體驗強化;接在 `feat/telegram-progress` 分支上)
**前置:** v0.6.0 Telegram bot;同分支已完成「即時 ack + 持續 typing」(commit `91d86c4`)。

## 目標

解決 Telegram 實測抓到的兩個體驗問題:

1. **訊息太多框**:目前 ack +「收到,思考中…」+ 每棒角色狀態 + 最終答案 = 多個泡泡,雜亂。
2. **Markdown 未渲染**:LLM 吐的 GitHub-flavored markdown(粗體、表格、` ``` ` code block、標題、清單)在 Telegram 是純文字,難讀。

並為**未來 web 版**鋪路:把 markdown 渲染抽成共用 crate。

## 設計原則

- **渲染與傳輸分離**:LLM 輸出維持 markdown(真相來源);各傳輸有自己的渲染目標。共用解析核心放獨立 crate。
- **單泡泡進度**:一個會原地變化的狀態泡泡,完成後刪除、發乾淨的渲染答案。
- **穩健優先**:Telegram 用 HTML parse_mode(比 MarkdownV2 跳脫單純);文字內容跳脫 `< > &`。
- **底層 agent 只以 opencode 為準。**

## 架構總覽

新增 crate `wukong-render`(markdown → 目標格式);`wukong-telegram` 改用它渲染答案,並整併訊息。

```
LLM(opencode)── GFM markdown ──► wukong-render::to_telegram_html ──► Vec<HTML chunk ≤4096>
                                                                       │
wukong-telegram dispatch:                                              ▼
  單一狀態泡泡(send_message)→ on_role 原地 edit → 完成後 delete → send_message_html(答案)
```

## 1. `wukong-render` crate(新)

`crates/wukong-render/`(lib only)。依賴 `pulldown-cmark`(加進 `[workspace.dependencies]`)。

```rust
/// GFM markdown → Telegram 支援的 HTML 子集,切成 ≤4096 字的多段。
/// 段數至少 1(空輸入回 vec!["(無內容)"] 由呼叫端決定;本函式空輸入回空 Vec)。
pub fn to_telegram_html(markdown: &str) -> Vec<String>;
```

### 事件映射(pulldown-cmark Event → Telegram HTML)

Telegram HTML 僅支援:`<b> <strong> <i> <em> <u> <s> <code> <pre> <a> <blockquote>`(及 `tg-spoiler`)。映射:

| Markdown | 輸出 |
| :--- | :--- |
| `**粗體**` / `__粗體__` | `<b>…</b>` |
| `*斜體*` / `_斜體_` | `<i>…</i>` |
| 行內 `` `code` `` | `<code>…</code>` |
| ` ``` ` 區塊 | `<pre>…</pre>` |
| `[文字](url)` | `<a href="url">文字</a>` |
| 標題 `#`…`######` | `<b>…</b>` + 換行 |
| 清單項 | `• …`(換行分隔) |
| 引用 `>` | `<blockquote>…</blockquote>` |
| 表格 | 等寬 `<pre>` 對齊區塊(欄以空白填齊、列以換行) |
| 水平線 `---` | `——————`(一行) |

- **跳脫**:所有文字節點與 `<pre>`/`<code>` 內容跳脫 `&`→`&amp;`、`<`→`&lt;`、`>`→`&gt;`(避免 LLM 輸出含 `<` 破壞 parse)。`<a href>` 的 url 也跳脫。
- **切段**:累積輸出超過 4096 時,在區塊/換行邊界切;不切斷 HTML 標籤(以「已閉合的區塊」為切點)。每段自身為合法 HTML。

### 表格降級

pulldown-cmark 啟用 `Options::ENABLE_TABLES`。收集每列儲存格純文字,計算各欄最大寬度,以空白對齊成等寬文字,整塊包進 `<pre>`。不追求完美(全形字寬度以字元數近似)。

## 2. `TgClient` 擴充(`wukong-telegram`)

```rust
pub trait TgClient {
    fn get_updates(&self, offset: i64) -> impl Future<Output = Result<serde_json::Value, TgError>> + Send;
    /// 送純文字訊息,回傳 message_id。
    fn send_message(&self, chat_id: i64, text: &str) -> impl Future<Output = Result<i64, TgError>> + Send;
    /// 送 HTML(parse_mode=HTML)訊息,回傳 message_id。
    fn send_message_html(&self, chat_id: i64, html: &str) -> impl Future<Output = Result<i64, TgError>> + Send;
    /// 原地更新訊息文字(純文字)。
    fn edit_message_text(&self, chat_id: i64, message_id: i64, text: &str) -> impl Future<Output = Result<(), TgError>> + Send;
    /// 刪除訊息。
    fn delete_message(&self, chat_id: i64, message_id: i64) -> impl Future<Output = Result<(), TgError>> + Send;
    fn send_chat_action(&self, chat_id: i64, action: &str) -> impl Future<Output = Result<(), TgError>> + Send;
}
```

- `ReqwestTgClient`:`send_message`/`send_message_html` 解析回應的 `result.message_id`;`send_message_html` 帶 `parse_mode: "HTML"`;`edit_message_text`→`editMessageText`;`delete_message`→`deleteMessage`。
- `MockTgClient`:`sent`(含是否 HTML)、`edits: Vec<(i64,i64,String)>`、`deletes: Vec<(i64,i64)>`;`send_*` 回遞增 message_id(如從 1 起)。

## 3. dispatch 訊息流(整併)

`handle_message` 的 `Turn` 分支改為:

```
let mid = client.send_message(chat_id, "🐵 收到，思考中…").await? // 狀態泡泡
// typing 刷新任務(每 4 秒,沿用既有)
// 進度任務:rx.recv() role → client.edit_message_text(chat_id, mid, "🐵 悟空·<role> 思考中…")
let result = run_turn(mem, backend, &cfg, &input, &mut |_|{}, &mut |r| { let _ = tx.send(r); }).await;
// 停 typing、停進度
match result {
    Ok(out) => {
        let chunks = wukong_render::to_telegram_html(&out.text);
        let _ = client.delete_message(chat_id, mid).await;
        if chunks.is_empty() {
            let _ = client.send_message(chat_id, "(無內容)").await;
        } else {
            for c in &chunks { let _ = client.send_message_html(chat_id, c).await; }
        }
    }
    Err(e) => { let _ = client.edit_message_text(chat_id, mid, &format!("⚠️ 處理失敗：{e}")).await; }
}
```

- 取代目前「ack + 每棒新訊息 + 答案」多泡泡;改為單一狀態泡泡原地變化,完成後刪除、發渲染答案。
- 進度任務只在角色變化時 edit(role 不重複,天然避免 Telegram「message not modified」)。
- mid 取得失敗(送狀態泡泡就錯)→ 記 log、放棄該則(極少見)。

## 錯誤處理

- 渲染 chunks 為空 → 發「(無內容)」。
- `edit_message_text` / `delete_message` 失敗一律 `let _ =` 忽略,不影響答案送出。
- `send_message_html` 失敗(理論上 HTML 不合法)→ log;不額外退回純文字(渲染器保證合法 HTML)。

## 測試策略

- **`wukong-render`**(純函式,離線):
  - 粗體/斜體/行內 code/code block/連結/標題/清單/引用各別映射為對應標籤。
  - 表格 → `<pre>` 且含各欄文字。
  - HTML 跳脫:`<script>alert(1)</script>` → `&lt;script&gt;…`。
  - 超過 4096 → 多段,每段 ≤4096。
  - 空輸入 → 空 Vec。
- **`wukong-telegram`**(MockTgClient + 假 backend + 暫存 sqlite):
  - 狀態泡泡建立(send_message 回 mid)→ 隨角色 edit(edits 非空)→ 完成後 delete(deletes 含 mid)→ 以 HTML 發答案(sent 標記 HTML)。
  - 錯誤路徑:run_turn 回錯 → edit 成 `⚠️ 處理失敗`、無 HTML 答案。
  - 既有 dispatch 測試更新為新 `send_message` 簽章(回 i64)。
- 真實 Telegram 為手動煙霧(粗體/表格/code block 正確顯示、單泡泡進度、最終乾淨答案)。

## 非目標(YAGNI)

- web 版渲染(F2 再做;`wukong-render` 已預留 `to_web_html` 擴充點)。
- 互動按鈕 / inline keyboard。
- code block 語法高亮(`<pre>` 等寬即可)。
- 表格完美欄寬(全形字以字元數近似)。
- 答案串流(opencode 不吐 token,維持完成後一次發)。
