# Telegram / Web 顯示 thinking 設計

**日期:** 2026-06-07
**狀態:** 已核可
**前置:** v0.9.0(`StreamEvent::Reasoning`、`run_turn` 帶 `--thinking`、Telegram 單泡泡進度、Web SSE)。

## 目標

把 opencode 的 reasoning(`--thinking`)呈現在 **Telegram** 與 **Web**(REPL 已以 `💭` 顯示),**只在 reasoning 非空時顯示**。

## 背景(已對真實 opencode 1.16.2 驗證)

- `--thinking` 在 `--format json` 下產生獨立的 `{"type":"reasoning","part":{...,"text":"…"}}` 事件;`parse_event` 已對應到 `StreamEvent::Reasoning(text)`。
- **reasoning 文字是否可讀取決於模型**:目前預設(OpenAI 系推理模型)把推理**加密**在 `part.metadata.openai.reasoningEncryptedContent`,`part.text` 為**空字串** → 串流拿不到明文。DeepSeek / Anthropic 類通常會在 `part.text` 吐明文。
- 因此本設計**與模型無關**:只在 reasoning 非空時顯示;加密模型自動安靜,換到吐明文的模型即生效。

## 設計原則

- **非空才顯示**:空 reasoning 不產生任何 UI(不留空泡泡、不建空折疊區)。
- **reasoning 一律當純文字**:Telegram 用無 parse_mode 的純文字泡泡;Web 用 `textContent` 追加 → 防 XSS。
- **重用既有進度通道**:Telegram 沿用單一狀態泡泡;Web 沿用 SSE。
- **底層 agent 只以 opencode 為準。**

## 1. REPL 小修

- `crates/wukong-cli/src/render.rs`:`StreamEvent::Reasoning(t)` 僅在 `!t.trim().is_empty()` 時寫 `  💭 {t}`。
- `crates/wukong-cli/src/main.rs` 的 `run_one` inline sink:同樣只在非空時 `eprintln!("  💭 {t}")`。

## 2. Telegram(`crates/wukong-telegram/src/dispatch.rs`)

- 進度通道型別由 `Role` 改為:

```rust
enum Progress {
    Role(Role),
    Reasoning(String),
}
```

- `run_turn` 的 callback:
  - `on_role` → `tx.send(Progress::Role(r))`。
  - `on_event` → 對 `StreamEvent::Reasoning(t)` 且 `!t.trim().is_empty()` 送 `Progress::Reasoning(t)`;其餘事件忽略。
  - 兩個 callback 各持一份 `tx.clone()`(unbounded `Sender` 可 clone、`send(&self)`)。
- 進度任務狀態:`role: Option<String>`(目前角色名)、`reasoning: String`(累積)、`last_edit: Option<Instant>`。
  - 泡泡文字 `bubble_text()`:
    - 基底 `🐵 悟空·{role} 思考中…`(無 role 時 `🐵 思考中…`)。
    - reasoning 非空時附加 `\n💭 {tail}`,`tail` = `reasoning` 的尾端最多 200 個 char(`reasoning.chars().rev().take(200)...` 反轉回正序)。
  - 收到訊息處理(throttle):
    - `Progress::Role(r)`:更新 `role`,**即時** `edit_message_text(chat, mid, &bubble_text())`,記 `last_edit = now`。
    - `Progress::Reasoning(t)`:`reasoning.push_str(&t)`;若 `last_edit` 為 None 或距今 ≥ 1.5s,才 `edit_message_text` 並更新 `last_edit`(否則只累積、跳過本次 edit)。
  - 通道關閉(`rx.recv()` 回 None)即結束;主流程隨後刪除泡泡。
- 泡泡為純文字(無 parse_mode)→ reasoning 不需跳脫。
- 完成 / 失敗路徑不變(刪泡泡 → 發 `to_telegram_html` 答案;或 edit 成錯誤)。

## 3. Web

### 後端(`crates/wukong-web/src/lib.rs`)

- `SseMsg` 新增 `Reasoning(String)`;`into_event()` → `Event::default().event("reasoning").data(s)`。
- `chat` handler spawned thread 內的 turn 分支:`on_event` 改為對 `StreamEvent::Reasoning(t)` 且非空 `tx.send(SseMsg::Reasoning(t))`(需一份 `tx.clone()`,與既有 `role_tx` 並列)。其餘不變。
- `/compact` 等指令分支不涉及(無 reasoning)。

### 前端(`crates/wukong-web/static/components/wukong-chat.js` + `styles.css`)

- `send()` 內新增 `let thinking = null;`(本回合的折疊區,惰性建立)。
- 監聽 `reasoning` 事件:
  - 若 `thinking` 尚未建立:`thinking = document.createElement('details'); thinking.className = 'thinking'; thinking.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';` 並 `this.log.appendChild(thinking)`(預設收合,無 `open`)。
  - `thinking.querySelector('.reasoning').textContent += ev.data;`(純文字追加)。`this.log.scrollTop = this.log.scrollHeight;`。
- `answer` / `done` / `error`:照舊;**不移除** `thinking`(留在答案上方可展開回看)。`progress` 仍照舊移除。
- `styles.css` 新增:

```css
.thinking { align-self: flex-start; max-width: 80%; font-size: 0.85rem; opacity: 0.75; }
.thinking summary { cursor: pointer; }
.thinking .reasoning { white-space: pre-wrap; margin: 0.3rem 0 0; max-height: 12rem; overflow-y: auto; }
```

## 測試策略

- **REPL `render.rs`**:`Reasoning("")` → 無輸出;`Reasoning("步驟一")` → err 含 `💭` 與內容。
- **Telegram `dispatch.rs`**:新增「在 `run_streaming` 內 emit Reasoning 事件」的測試用 backend(覆寫 `run_streaming`:先 `on_event(StreamEvent::Reasoning("想一下".into()))` 再回 `AgentResponse{text:"答案", session_id:None}`)。`handle_message` 後斷言 `client.edits` 至少一筆含 `💭` 與「想一下」。既有測試(MockBackend 只實作 `run`)行為不變(進度只有 role)。
- **Web `lib.rs`**:同樣以覆寫 `run_streaming` 發 Reasoning 的 backend;`GET /chat?q=hi` 的 SSE body 含 `event: reasoning` 且含「想一下」。空 reasoning 不應出現 `event: reasoning`(另一測試:backend emit `Reasoning("")` → body 不含 `event: reasoning`)。
- **前端**:無自動化測試;真實瀏覽器手動煙霧(需切到會吐明文推理的模型才看得到內容)。
- 真實 opencode 手動煙霧:目前預設模型 reasoning 加密 → 預期 Telegram/Web 不顯示 thinking(驗證「非空才顯示」不誤觸);如可切到明文推理模型則確認顯示。

## 非目標(YAGNI)

- 解密 OpenAI 加密推理(不可能)。
- 空 reasoning 顯示。
- Telegram 折疊區(無原生;用瞬時狀態泡泡)。
- token 級串流(opencode 不吐 delta;reasoning 為片段事件)。
- 在 Web 把 thinking 與答案綁成單一可摺疊容器(各自獨立元素即可)。
