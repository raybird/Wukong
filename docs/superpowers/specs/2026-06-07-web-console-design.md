# Web Console(wukong-web)設計(F2)

**日期:** 2026-06-07
**狀態:** 已核可(roadmap 項目 F 的第二個子專案:Web Console)
**前置:** v0.7.0 turn engine(`run_turn`)、`wukong-render`、`wukong-memory`、`AgentCliBackend`。

## 目標

讓 Wukong 多一個瀏覽器進入點:在本機網頁聊天,後端以既有 turn engine 處理,SSE 串流進度與答案,答案以渲染後的 HTML 顯示。完全重用核心邏輯;前端遵循使用者的 plain-vanilla(`raybird/plainvanillaweb`)核心慣例:零建置、ES Modules、custom element、SafeHTML。

## 設計原則

- **重用、不重造**:直接呼叫 `wukong-cli::run_turn`;markdown 渲染重用 `wukong-render`。
- **零建置前端**:靜態頁 + ES Modules,無 node/打包;axum 以 `include_str!` 內嵌靜態檔,單一可執行檔自帶前端。
- **安全預設**:預設只綁 `127.0.0.1`;`WUKONG_WEB_TOKEN` 設了才驗;伺服器端渲染跳脫原始 HTML 防 XSS;前端用 SafeHTML 轉義使用者輸入。
- **進度回饋**:SSE 串流角色狀態(沿用 Telegram 學到的「進度很重要」)。
- **底層 agent 只以 opencode 為準。**

## 架構總覽

新增 crate `wukong-web`(bin + lib),axum 伺服器。

```
瀏覽器 <wukong-chat>  ──GET /chat?q=…(EventSource/SSE)──►  wukong-web (axum)
   ▲  event: role   → 更新「🐵 悟空·<role>…」進度列            │ run_turn(scope=WUKONG_WEB_SCOPE, stream=false)
   │  event: answer → innerHTML 插入 to_web_html 的 HTML        │   on_role → SSE role 事件
   │  event: done / error                                       │   最終答案 → wukong_render::to_web_html → SSE answer
   └─────────────────────────────────────────────────          ▼
                                                    wukong-memory / opencode(同 CLI 路徑)
```

## 1. `wukong-render` 補 `to_web_html`

```rust
/// GFM markdown → 完整安全 HTML(瀏覽器原生渲染:真 <table>、<pre><code>、清單)。
pub fn to_web_html(markdown: &str) -> String;
```

- 用 pulldown-cmark 解析(`ENABLE_TABLES | ENABLE_STRIKETHROUGH`),以 `pulldown_cmark::html::push_html` 產完整 HTML。
- **安全**:把 `Event::Html`/`Event::InlineHtml` 事件改寫成 `Event::Text` 後再餵 `push_html`,使原始 HTML 被當文字跳脫 → 杜絕 LLM 輸出注入 `<script>` 的 XSS。
- web 無長度上限,回單一 `String`。

## 2. `wukong-web` crate(後端)

### 路由

- `GET /` → `index.html`(內嵌)。
- `GET /app.js`、`GET /styles.css`、`GET /lib/html.js`、`GET /components/wukong-chat.js` → 對應內嵌靜態檔,正確 `content-type`(`text/html`、`application/javascript`、`text/css`)。
- `GET /chat?q=<訊息>[&token=…]` → **SSE**(`text/event-stream`):依序送 `role`(每棒)、`answer`(渲染 HTML)、`done`;失敗送 `error`。

### state 與泛型

```rust
pub struct AppState<B: AiBackend> {
    pub memory: std::sync::Arc<wukong_memory::Memory>,
    pub backend: std::sync::Arc<B>,
    pub scope: String,
    pub token: Option<String>,
}
pub fn build_router<B>(state: AppState<B>) -> axum::Router
where B: AiBackend + Send + Sync + 'static;
```

正式用 `AgentCliBackend`,測試用 `MockBackend`。

### `/chat` handler 流程

```
1. 若 state.token 為 Some 且 query token 不符 → 401。
2. 取 q(空則 400 或回 error 事件)。
3. let (tx, rx) = unbounded_channel::<SseMsg>();
4. spawn:
     let mut cfg = base_cfg(); cfg.scope = state.scope.clone(); cfg.stream = false;
     run_turn(&mem, &*backend, &cfg, &q, &mut |_|{}, &mut |role| { let _ = tx.send(SseMsg::Role(role)); })
       Ok(out) => { let _ = tx.send(SseMsg::Answer(wukong_render::to_web_html(&out.text))); }
       Err(e)  => { let _ = tx.send(SseMsg::Error(e.to_string())); }
     let _ = tx.send(SseMsg::Done);
5. 回 Sse::new(UnboundedReceiverStream::new(rx).map(|m| Ok::<_,Infallible>(m.into_event())))
```

- `SseMsg::into_event()` 把訊息轉成 `axum::response::sse::Event`(`.event("role"|"answer"|"done"|"error").data(...)`)。
- `on_role`(同步 callback)以 unbounded 的 `tx.send`(非阻塞)送事件;spawn 任務需 `'static`,故只搬 `Arc` clone。

### 設定(env)

- `WUKONG_WEB_HOST`(預設 `127.0.0.1`)、`WUKONG_WEB_PORT`(預設 `8787`)。
- `WUKONG_WEB_TOKEN`(設了才驗;未設 = 本機免驗)。
- `WUKONG_WEB_SCOPE`(預設 `global`)。
- 重用:`WUKONG_MEMORY_DB`、`WUKONG_AGENT_CMD`、`WUKONG_MD_DIR`、(feature `embed`)`WUKONG_EMBED`。Memory/backend 比照 cli `main` 建構。

## 3. 前端(`static/`,遵循 plainvanillaweb 核心慣例)

```
static/
├── index.html                  # <script type="module" src="/app.js">,掛 <wukong-chat>
├── app.js                      # 進入點:import 並 customElements.define('wukong-chat', WukongChat)
├── lib/html.js                 # 採用 raybird/plainvanillaweb 的 SafeHTML:html`` / unsafe() / escapeHTML()
├── components/wukong-chat.js   # custom element:訊息列 + 輸入框 + SSE 串接
└── styles.css                  # 極簡風格
```

- **`<wukong-chat>`** 繼承 `HTMLElement`(輕量自包含,不引入模板的 i18n/router/services)。
  - 送出 → 用 `html\`\`` 組使用者泡泡(輸入自動轉義)→ 開 `EventSource('/chat?q=' + encodeURIComponent(text) + tokenParam)`。
  - `role` 事件 → 更新單一進度列「🐵 悟空·<role>…」(沿用單泡泡精神)。
  - `answer` 事件 → 助手泡泡 `innerHTML = data`(伺服器已產安全 HTML;以 `unsafe()` 標記其受信任)。
  - `done` → 移除進度列、`EventSource.close()`(避免 EventSource 自動重連重送 turn)。
  - `error` → 顯示 `⚠️`。
- **`lib/html.js`** 直接採用使用者 repo 的實作,檔頭註明來源 `raybird/plainvanillaweb`。
- 各靜態檔由 axum `include_str!` 內嵌並以正確 content-type 路由(零新依賴)。

## 錯誤處理

- token 不符 → 401。
- 空 `q` → 回 `error` 事件(或 400)。
- `run_turn` 失敗 → `error` 事件 + 前端 `⚠️`。
- EventSource 在 `done`/`error` 後由前端主動 `close()`,避免重連重跑。

## 測試策略

- **`to_web_html`**(純函式,離線):粗體→`<strong>`、表格→`<table>`、code block→`<pre><code>`、`<script>`→跳脫、空輸入→空字串。
- **`wukong-web`**(axum `oneshot` + `MockBackend` + 暫存 sqlite):
  - `GET /` → 200 且 body 含 `<wukong-chat>`。
  - `GET /chat?q=hi`(MockBackend 腳本 `["oracle","**ans**"]`)→ SSE body 含 `event: role`、`event: answer` 且 answer 內含 `<strong>ans</strong>`、結尾 `event: done`。
  - 設 `token` 時 `GET /chat?q=hi` 無 token → 401;帶正確 token → 200。
- 前端 JS 不寫自動化測試;真實瀏覽器為手動煙霧(輸入問題 → 看到進度列 → 渲染答案含粗體/表格/code)。

## 非目標(YAGNI)

- 多使用者 / 登入系統(localhost 個人用,僅選用 token)。
- 對話歷史側欄 / 多 session 切換(第一版單一 scope)。
- 採用模板的 router / PWA / Service Worker / IndexedDB / i18n / 各式 service。
- WebSocket、答案 token 串流(opencode 不吐 token)。
- slash 指令(第一版純對話;之後可比照 Telegram 加接縫)。
