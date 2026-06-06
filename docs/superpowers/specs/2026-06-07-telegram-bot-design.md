# Telegram bot 進入點設計(F1)

**日期:** 2026-06-07
**狀態:** 已核可(roadmap 項目 F 拆出的第一個子專案:Telegram bot)
**前置:** v0.5.0 turn engine(`wukong-cli::run_turn`)、`Memory`、`AgentCliBackend`、`GatewayConfig`。

## 目標

讓 Wukong 多一個 Telegram 進入點:使用者在 Telegram 傳訊息,bot 以既有 turn engine 處理並回覆。完全重用核心對話邏輯,bot 只是一層傳輸 + 存取控制 + 進度呈現。

F 的另一個子專案 Web Console(F2)**不在本 spec 範圍**,之後另開。

## 設計原則

- **重用、不重造**:直接呼叫 `wukong-cli::run_turn`,核心邏輯零改動。
- **最少依賴**:原生 Telegram Bot API long-poll + `reqwest`,不用 teloxide。
- **安全預設**:白名單外的 chat 一律忽略;缺 token 啟動即退出;空白名單拒絕所有。
- **可擴充指令**:訊息分派預留 slash 指令接縫(仿 REPL `classify_line`),未來加 `/reset`、`/compact`、`/model` 等只是多加 match 臂,改動侷限一處。
- **底層 agent 只以 opencode 為準。**

## 架構總覽

新增 crate `wukong-telegram`(bin),作為 turn engine 的 Telegram 傳輸層。

```
Telegram ──getUpdates(long-poll, timeout=30)──► wukong-telegram
                                                  │ 1. parse_updates → (update_id, chat_id, text)
                                                  │ 2. is_allowed(chat_id)? 否 → 忽略
                                                  │ 3. classify_message(text):
                                                  │      "/cmd …" → MessageAction::Command  (v1 回「尚未支援」)
                                                  │      其餘       → MessageAction::Turn(text)
                                                  │ 4. scope = user:tg-<chat_id>
                                                  │ 5. run_turn(mem, backend, cfg{scope}, text, on_event=ignore, on_role)
                                                  │      on_role ──► 發 typing + 「🐵 悟空·<role>」狀態
                                                  │ 6. send_message(最終 out.text)
                                                  ▼
                                          wukong-memory / opencode(同 CLI 路徑)
```

## 依賴

workspace `Cargo.toml` 的 `[workspace.dependencies]` 加:

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
```

`rustls-tls` 免系統 OpenSSL,維持自包含。新 crate 依賴:`wukong-cli`、`wukong-memory`、`wukong-gateway`、tokio、serde_json、reqwest。

## 設定(env)

- `WUKONG_TG_TOKEN`(必填):bot token;缺則啟動報錯退出。
- `WUKONG_TG_ALLOWED`(必填語意):逗號分隔允許的 chat id;**空字串或未設 → 拒絕所有**(安全預設,啟動時警告)。
- 重用既有,Memory/backend 比照 cli `main` 建構:`WUKONG_MEMORY_DB`、`WUKONG_AGENT_CMD`、`WUKONG_AGENT_CONTINUE_ARGS`、`WUKONG_MD_DIR`、(feature `embed` 時)`WUKONG_EMBED`。

## 元件

### 純函式(免網路,單元測試)

```rust
/// 從 getUpdates 的 JSON 取出文字訊息。容忍非文字、缺欄位的 update。
pub struct TgMessage { pub update_id: i64, pub chat_id: i64, pub text: String }
pub fn parse_updates(json: &serde_json::Value) -> Vec<TgMessage>;

/// 白名單解析與檢查。
pub fn parse_allowlist(s: &str) -> Vec<i64>;   // "12,34" -> [12,34];空白容忍
pub fn is_allowed(chat_id: i64, allow: &[i64]) -> bool;

/// 每個 chat 對應的記憶 scope。
pub fn scope_for_chat(chat_id: i64) -> String;  // = format!("user:tg-{chat_id}")

/// 訊息分派接縫(仿 REPL classify_line)。
pub enum MessageAction {
    Turn(String),               // 一般對話
    Command { name: String, args: String },  // "/cmd args" — v1 僅佔位
}
pub fn classify_message(text: &str) -> MessageAction;
```

`classify_message`:`text` 去空白後以 `/` 開頭 → 切出指令名與其餘參數成 `Command`;否則 `Turn(text)`。**v1 對任何 `Command` 一律回覆「指令 /<name> 尚未支援」**;未來在 dispatch 的 `Command` 分支加 match 臂即可(例:`/reset` 重啟 opencode session、`/compact` 觸發 consolidation、`/model` 切換後端模型)。

### `TgClient` trait(可注入、可測)

```rust
pub trait TgClient {
    async fn get_updates(&self, offset: i64) -> Result<serde_json::Value, TgError>;
    async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), TgError>;
    async fn send_chat_action(&self, chat_id: i64, action: &str) -> Result<(), TgError>;
}
```

- 真實 impl `ReqwestTgClient`:對 `https://api.telegram.org/bot<token>/<method>` 發請求;`get_updates` 用 `timeout=30` 長輪詢。
- 測試 impl `MockTgClient`:腳本化 updates、記錄送出的訊息,驗 dispatch 流程不碰網路。

### dispatch loop

維護 `offset`(初始 0,處理後設為 `max(update_id)+1`)。每則允許訊息:

```rust
let scope = scope_for_chat(chat_id);
let mut cfg = base_cfg.clone();
cfg.scope = scope;
match classify_message(&text) {
    MessageAction::Command { name, .. } => {
        client.send_message(chat_id, &format!("指令 /{name} 尚未支援")).await?;
    }
    MessageAction::Turn(input) => {
        // 進度:旁路任務收 role → 發 typing + 狀態,避免在同步 callback 內 await。
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Role>();
        let status = { let c = client.clone(); tokio::spawn(async move {
            while let Some(role) = rx.recv().await {
                let _ = c.send_chat_action(chat_id, "typing").await;
                let _ = c.send_message(chat_id, &format!("🐵 悟空·{}", role.name())).await;
            }
        })};
        let _ = client.send_chat_action(chat_id, "typing").await;
        let result = run_turn(&mem, &backend, &cfg, &input, &mut |_| {}, &mut |r| { let _ = tx.send(r); }).await;
        drop(tx);
        let _ = status.await;
        match result {
            Ok(out) => client.send_message(chat_id, &out.text).await?,
            Err(e)  => client.send_message(chat_id, &format!("⚠️ 處理失敗:{e}")).await?,
        }
    }
}
```

`Role` 來自 `wukong_orchestrator::Role`(已 re-export `name()`)。`TgClient` 真實 impl 需 `Clone`(reqwest `Client` 為 `Clone`)。

## 錯誤處理

- 單則訊息處理失敗(opencode 錯誤等)→ 回一則 `⚠️ 處理失敗` 給該 chat、log、**continue**,bot 不崩。
- `get_updates` 網路錯誤 → log + 退避 `sleep`(如 3 秒)後續跑,offset 不前進。
- 缺 `WUKONG_TG_TOKEN` → 啟動報錯退出;空白名單 → 啟動警告(bot 會忽略全部訊息)。

## 測試策略

- **純函式單測**:
  - `parse_updates`:正常文字訊息、非文字(無 `text`)、空 `result`、缺 `message` 的 update。
  - `parse_allowlist`:`"12, 34"` → `[12,34]`;`""` → `[]`。
  - `is_allowed`:命中 / 未命中。
  - `scope_for_chat`:`-100` → `"user:tg--100"`(負 id 容忍)。
  - `classify_message`:`"/reset now"` → `Command{name:"reset",args:"now"}`;`"hello"` → `Turn("hello")`;`"  /x"` → `Command{name:"x"}`。
- **dispatch(MockTgClient + 假 backend)**:
  - 白名單外 chat 的訊息 → 不呼叫 run_turn、不回覆(或僅忽略)。
  - 白名單內一般訊息 → run_turn 被以 `user:tg-<id>` scope 呼叫 → 送出回覆。
  - `/cmd` → 回「尚未支援」、不呼叫 run_turn。
  - run_turn 回錯 → 送出 `⚠️ 處理失敗`。
- **真實 Telegram 煙霧(手動,需 token)**:設 `WUKONG_TG_TOKEN`/`WUKONG_TG_ALLOWED` 啟動,自手機傳訊,確認 typing + 角色狀態 + 最終答案;複雜任務看到多棒角色狀態。

## 非目標(YAGNI)

- 不做 webhook(只 long-poll)。
- 不做行內鍵盤/按鈕/多媒體(只純文字)。
- v1 不實作任何具體 slash 指令(只留分派接縫 + 「尚未支援」回覆)。
- 不做 Web Console(F2 另開)。
- 不做訊息長度切分(Telegram 4096 字上限);超長答案第一版容忍可能被 API 截斷,之後再加切分。
