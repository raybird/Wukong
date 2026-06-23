# Web Console 可折疊中間棒(Collapsible Intermediate Steps）設計

**日期:** 2026-06-23
**狀態:** 已實作(Phase 1 即時顯示 + Phase 2 落盤與歷史重載)
**前置:** 協作鏈(`2026-06-06-collaboration-chain-design.md`)、末棒空輸出回退(`2026-06-22-final-output-fallback-design.md`)、Web Console(`2026-06-07-web-console-design.md`)、共用對話歷史(`2026-06-18-shared-chat-history-design.md`)、思考顯示(`2026-06-07-thinking-display-design.md`)。

## 背景與問題

`run_turn` 的協作鏈最多 3 棒(Explorer/Oracle/Librarian/Fixer/Designer)。目前**只有末棒的產出**會被當成最終答覆顯示並落盤;中間(輔助)棒的 `resp.text` 只進 `prior`、透過 `chain_context` 餵給下一棒當上下文(`crates/wukong-runtime/src/turn.rs:56,85`),對使用者**完全不可見、也不入庫**。

Web Console 現況(`crates/wukong-web`):

- `run_turn` 只外露 `on_event`(`StreamEvent::Reasoning`)與 `on_role`(棒次開始),**從不外露每棒 output**。
- SSE 事件僅 `role` / `reasoning` / `answer` / `error` / `done`(`crates/wukong-web/src/lib.rs:112-130`)。
- 前端 `reasoning` 已用一個累積式 `<details class="thinking">` 呈現思考過程(`crates/wukong-web/static/components/wukong-chat.js:273-282`),但**沒有任何呈現中間棒結果的管道**。

問題:多棒協作時,使用者看不到「答案是怎麼被一棒棒組出來的」。當末棒答案有疑義或出錯時,無從得知是哪一棒歪掉;也少了「展開看推理鏈」的透明度與信任感。

## 目標

1. Web Console 中,中間棒產出以**預設折疊的卡片**呈現在最終答案上方;一般使用者只看答案,要看再展開。
2. 末棒最終答案維持**乾淨的主訊息**地位,中間棒視覺上明確「次要」,不與最終答案搶焦點或互相矛盾。
3. **不污染**主對話時間軸(`chat_messages`)與記憶召回(`wukong-memory`)。
4. 改動 blast radius **鎖在 `wukong-web`**,不波及其他進入點與既有測試。

### 非目標(YAGNI)

- 不改 `run_turn` 既有簽章(下游 Telegram / CLI / Scheduler 四進入點與全部既有測試零修改)。
- 不在本期讓 Telegram 跟進中間棒呈現(預留介面,日後再接)。
- 不替中間棒額外注入「給使用者看的摘要要求」(會多吃 token 與延遲);中間棒以 raw 產出 + 次要樣式呈現。
- 不重構「每棒一段 reasoning」的分段(現況跨棒累積維持不變)。

## 關鍵約束:`run_turn` 是 CRITICAL 共用核心

對 `crates/wukong-runtime/src/turn.rs:run_turn` 做上游 impact 分析:**risk = CRITICAL**,直接呼叫點 13、受影響流程 15、模組 10。生產端呼叫點 5 個:

- `chat`(`crates/wukong-web/src/lib.rs`)←本案目標
- `handle_message`(`crates/wukong-telegram/src/dispatch.rs`)
- `execute_job_inner`(`crates/wukong-scheduler/src/executor.rs`)
- `run_repl_loop`(`crates/wukong-cli/src/repl.rs`)
- `run_one`(`crates/wukong-cli/src/main.rs`)

外加 `turn.rs` / `dispatch.rs` 內大量測試。**結論:直接改 `run_turn` 簽章不可行**,會強制改五個進入點 + 全部測試。

## 設計

### ① `run_turn_observed` 委派層(治本:鎖住 blast radius)

新增一個帶 step callback 的函式,讓既有 `run_turn` 委派給它,簽章維持 100% 不變:

```rust
// crates/wukong-runtime/src/turn.rs
pub async fn run_turn(
    memory, backend, cfg, input,
    on_event: &mut dyn FnMut(StreamEvent),
    on_role: &mut dyn FnMut(Role),
) -> Result<TurnOutput, WukongError> {
    // 既有簽章不變 → telegram / cli / scheduler / 既有測試 0 改動
    run_turn_observed(memory, backend, cfg, input, on_event, on_role,
                      &mut |_role, _output| {}).await
}

/// 與 run_turn 相同;每完成一個「非末棒」且輸出非空的棒次,回呼 on_step(role, output)。
pub async fn run_turn_observed(
    memory, backend, cfg, input,
    on_event: &mut dyn FnMut(StreamEvent),
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(Role, &str),   // ← 新增
) -> Result<TurnOutput, WukongError> {
    // ... 原 run_turn body，唯一差異在逐棒迴圈尾端：
    let text = resp.text;
    if !is_final && !text.trim().is_empty() {
        on_step(role, &text);              // 末棒走既有 Answer 路徑，不回呼
    }
    prior.push(Outcome { role, output: text });
    // ... 其餘（session、回退、remember、return）不變
}
```

只有 `wukong-web` 改呼叫 `run_turn_observed`。blast radius 從「5 進入點 + 全部測試」縮到「runtime 新增一函式 + web 一處」。

末棒不回呼的理由:末棒(或其非空回退,見前置文件)本來就走既有 `Answer` 路徑成為主訊息,重複回呼只會多一張和最終答案重疊的卡片。

---

### ② Phase 1 — 即時顯示(不落盤)

當回合內可展開中間棒;重整後消失。風險最低,可獨立上線。

**後端 SSE 新事件 `step`(`crates/wukong-web/src/lib.rs`)**

`SseMsg` 加變體,用 JSON 帶 role + 已渲染 HTML(沿用 `wukong_render::to_web_html`,與 `answer` 同為 SafeHTML):

```rust
enum SseMsg {
    Role(String), Reasoning(String),
    Step { role: String, html: String },   // ← 新增
    Answer(String), Error(String), Done,
}
// into_event():
SseMsg::Step { role, html } => Event::default().event("step")
    .data(serde_json::json!({ "role": role, "html": html }).to_string()),
```

`chat()` 把 `run_turn` 換成 `run_turn_observed`,多傳第三個 callback:

```rust
let step_tx = tx.clone();
let result = run_turn_observed(
    mem.as_ref(), backend.as_ref(), &cfg, &q,
    &mut |ev| { /* 既有 Reasoning */ },
    &mut |role| { let _ = role_tx.send(SseMsg::Role(role.name().to_string())); },
    &mut |role, output| {
        let html = wukong_render::to_web_html(output);
        let _ = step_tx.send(SseMsg::Step { role: role.name().to_string(), html });
    },
).await;
```

> callback 在 `run_turn` 的非 `Send` 環境執行,但此處已在 `std::thread::spawn` + current-thread runtime 內(`lib.rs:262`),`tx` 為 `Send` channel,沿用既有模式即可。

**前端折疊卡片(`crates/wukong-web/static/components/wukong-chat.js`)**

仿既有 `reasoning` 的 `<details>` 寫法,在 `answer` 之前插入折疊卡:

```js
es.addEventListener('step', (ev) => {
  const { role, html } = JSON.parse(ev.data);
  const d = document.createElement('details');
  d.className = 'baton';
  d.innerHTML = '<summary>🔍 悟空·' + escapeHTML(role) + ' 的產出</summary>'
              + '<div class="baton-body">' + html + '</div>';   // server SafeHTML，可信
  this.log.appendChild(d);
  this.enhanceCodeBlocks(d);
  this.log.scrollTop = this.log.scrollHeight;
});
```

`answer` listener 不變。視覺順序:思考過程 → 各棒折疊卡 → 最終答案。

**樣式(`crates/wukong-web/static/styles.css`)**

複用 `.thinking`(`styles.css:155-195`)折疊風格,新增 `.baton` / `.baton-body`,刻意做「次要」視覺權重(灰字、左側細邊、縮排),避免搶最終答案焦點。

---

### ③ Phase 2 — 落盤 + 歷史重載

讓中間棒在重新整理後仍在。

**新表 `turn_steps`(`crates/wukong-chat-history/src/lib.rs`)**

獨立表,外鍵掛到該回合**末棒 assistant 訊息**,`ON DELETE CASCADE`。刻意不塞進 `chat_messages` 主時間軸,避免污染分頁;`wukong-memory` 召回完全不動(召回本來只吃末棒)。

```sql
CREATE TABLE IF NOT EXISTS turn_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL,   -- FK → chat_messages.id（末棒 assistant 訊息）
    seq INTEGER NOT NULL,          -- 棒次順序
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    content_html TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(message_id) REFERENCES chat_messages(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS turn_steps_message_id_idx ON turn_steps(message_id);
```

新增方法:`insert_step(message_id, seq, role, content, content_html, created_at)`、`list_steps(message_id) -> Vec<TurnStep>`。

**寫入時機(`crates/wukong-web/src/lib.rs`)**

step callback 不能直接 await DB,故把 `(role, content, html)` **緩衝進 Vec**;`run_turn_observed` 回 Ok 後,先 `insert_message` 拿到末棒 `message_id`(`lib.rs:349` 已 `RETURNING id`,目前丟棄回傳值,改成接住),再逐筆 `insert_step`。best-effort,失敗不影響答案。

**歷史重載(懶載入,推薦)**

- 新端點 `GET /api/chat/messages/:id/steps`(帶 `?token=`)→ `list_steps`。
- 前端 `messageNode()`(`wukong-chat.js:120`)渲染 assistant bubble 時附一個**空的** `<details class="baton-group">`,首次 `toggle` 展開才 fetch 填入。
- 理由:history 分頁 payload 維持精瘦;折疊預設關閉,多數中間棒永不被抓。
- 替代方案:`get_chat_messages` 直接 join steps 內嵌回傳,較簡單但 payload 變大、且中間棒可能很長。覺得懶載入太繁可改此案。

## 測試

**Phase 1**

- runtime:`run_turn_observed` 多棒鏈對非末棒回呼 `on_step`、末棒不回呼、空輸出不回呼(仿 `turn.rs` 既有假 backend 測試)。
- runtime:`run_turn` 回歸——委派後行為與原本一致。
- web:假 backend 跑多棒,SSE body 含 `event: step` 且排在 `event: answer` 之前(仿 `lib.rs:843` 既有 SSE 測試)。

**Phase 2**

- chat-history:`insert_step` / `list_steps` round-trip;刪 assistant 訊息時 steps 一併 cascade。
- web:跑完一回合後 `turn_steps` 筆數與 `seq` 順序正確;`/api/chat/messages/:id/steps` 回傳正確。

## 風險與取捨

| 項目 | 處理 |
|------|------|
| `run_turn` CRITICAL blast radius | `run_turn_observed` 委派,鎖在 web,不碰其他進入點與測試 |
| 中間棒未經 `final_answer_directive`,品質參差 | 次要樣式 + 預設折疊;不混入最終答案 |
| 中間棒可能很長/很多 | Phase 2 懶載入,不塞進 history 主 payload |
| 污染記憶召回 | 獨立 `turn_steps` 表;`wukong-memory` 不動 |
| reasoning 折疊跨棒累積、與 step 卡混排 | 本期不改;未來可做每棒分段(列為後續) |
| Telegram 跟進 | 不在本期;`run_turn_observed` 已預留介面 |

## 改動檔案清單

**Phase 1**

- `crates/wukong-runtime/src/turn.rs` — 新增 `run_turn_observed` + `run_turn` 委派。
- `crates/wukong-web/src/lib.rs` — `SseMsg::Step` + 改呼叫 `run_turn_observed`。
- `crates/wukong-web/static/components/wukong-chat.js` — `step` listener。
- `crates/wukong-web/static/styles.css` — `.baton` / `.baton-body`。

**Phase 2**

- `crates/wukong-chat-history/src/lib.rs` — `turn_steps` 表 + `insert_step` / `list_steps` + `TurnStep`。
- `crates/wukong-web/src/lib.rs` — 緩衝寫入 + `GET /api/chat/messages/:id/steps` 端點與路由。
- `crates/wukong-web/static/components/wukong-chat.js` — 歷史 assistant bubble 的懶載入折疊。

## 實作順序建議

1. Phase 1 ①+② 一起做(runtime 委派層 → web SSE → 前端 → 樣式),可獨立上線驗證體驗。
2. 觀察實際中間棒內容品質與長度,再決定 Phase 2 懶載入 vs 內嵌。
3. Phase 2 落盤 + 歷史重載。
