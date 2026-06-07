# wukong-render

> 渲染層:把 LLM 的 markdown 轉成各傳輸要的格式

LLM(opencode)吐的是 GitHub-flavored markdown。各進入點(Telegram、未來 web)需要不同呈現格式。本 crate 把渲染與傳輸分離,作為共用的純函式層。

## 公開 API

```rust
use wukong_render::to_telegram_html;

let chunks: Vec<String> = to_telegram_html("**重點**\n\n```rust\nlet x = 1;\n```");
// 每段為合法的 Telegram HTML、長度 ≤ 4096
```

`to_telegram_html(markdown) -> Vec<String>`:

- 用 `pulldown-cmark` 解析 GFM。
- 輸出 Telegram 支援的 HTML 子集:`**粗體**`→`<b>`、`*斜體*`→`<i>`、`~~刪除~~`→`<s>`、行內 `code`→`<code>`、` ``` ` 區塊→`<pre>`、`[文字](url)`→`<a>`、標題→`<b>`、清單→`• …`、引用→`<blockquote>`。
- **表格**(Telegram 無原生支援)降級為等寬 `<pre>` 對齊區塊。
- 文字與原始 HTML 一律跳脫 `& < >`(避免破壞 parse、防注入)。
- 輸出超過 4096 字在換行邊界切成多段;空輸入回空 `Vec`。

## 未來

web 版進入點(F2)會加 `to_web_html`,共用同一 pulldown-cmark 解析核心。

依賴:`pulldown-cmark`。被 `wukong-telegram` 使用。
