# wukong-telegram

> 進入點 ──「身外身・赴會」：Telegram bot

把 Wukong 的對話引擎接上 Telegram。原生 long-poll 收訊息 → 白名單過濾 → 每 chat 一個記憶 scope → 重用 `wukong-cli::run_turn` 處理 → 回覆。核心對話邏輯零改動,本 crate 只是傳輸 + 存取控制 + 進度呈現。

## 啟動

```bash
export WUKONG_TG_TOKEN="<BotFather 給的 token>"
export WUKONG_TG_ALLOWED="<你的 chat id>[,<其他 id>…]"
cargo run -p wukong-telegram
```

用 `@userinfobot` 可取得自己的 chat id。

## 環境變數

| 變數 | 說明 | 預設 |
| :--- | :--- | :--- |
| `WUKONG_TG_TOKEN` | bot token(必填,缺則退出) | — |
| `WUKONG_TG_ALLOWED` | 逗號分隔的允許 chat id;**空 → 忽略所有訊息**(安全預設) | 空 |
| `WUKONG_MEMORY_DB` | 記憶資料庫(與 CLI 共用) | `$HOME/.wukong/memory.db` |
| `WUKONG_AGENT_CMD` | 底層 agent 指令 | `opencode run` |
| `WUKONG_MD_DIR` | 設定後每次 remember 同步鏡像 markdown | 未設則不寫 |
| `WUKONG_EMBED` | 設 `1` 啟用語意層(需 build `--features embed`) | off |

## 行為

- **存取控制**:白名單外的 chat 一律靜默忽略。
- **記憶 scope**:每個 chat → `user:tg-<chat_id>`(依階層 fallback 自動含 `global`)。
- **進度呈現(單泡泡)**:收到訊息先發一個「🐵 收到,思考中…」狀態泡泡;處理期間每 ~4 秒補送 `typing`(opencode 慢且不吐 token,Telegram typing 僅約 5 秒);協作鏈每進一棒**原地 edit** 該泡泡為「🐵 悟空·<role> 思考中…」;完成後**刪除狀態泡泡**,再發答案。
- **Markdown 渲染**:答案經 `wukong-render` 轉成 Telegram HTML(`parse_mode=HTML`)——粗體/斜體/行內 code/code block(`<pre>`)/連結/清單/引用正確顯示,**表格降級為等寬 `<pre>`**,超過 4096 字自動分段。
- **錯誤**:單則訊息失敗 → 回 `⚠️ 處理失敗` 並續跑,bot 不崩;getUpdates 網路錯誤 → 退避重試。

## Slash 指令(可擴充接縫)

`classify_message` 把 `/指令` 與一般訊息分流(仿 `wukong-cli` 的 REPL `classify_line`)。**v1 對任何 `/指令` 回「尚未支援」**;未來新增 `/reset`(重啟 session)、`/compact`(壓縮 context)、`/model`(切換模型)等,只需在 `dispatch::handle_message` 的 `Command` 分支加 match 臂,改動侷限一處。

## 測試

純函式(parse/allowlist/scope/classify)與 dispatch 流程全部以 `MockTgClient` + 假 backend + 暫存 sqlite 離線測試;真實 Telegram 為手動煙霧(需 token)。

## 非目標

webhook、行內鍵盤/按鈕/多媒體、訊息 4096 字切分、Web Console(F2 另開)。

依賴方向:`wukong-telegram → { wukong-cli, wukong-memory, wukong-gateway, wukong-tg-client }`(單向,進入點層)。Telegram 傳輸層(Bot API client、`parse`/`scope` 純函式、`MockTgClient`)已抽到零內部依賴的 `wukong-tg-client`,由本 crate `pub use` 轉出(`crate::client`/`crate::parse`/`crate::error` 路徑不變),並與 `wukong-schedulerd` 共用。

詳見 [`docs/superpowers/specs/2026-06-07-telegram-bot-design.md`](../../docs/superpowers/specs/2026-06-07-telegram-bot-design.md)。
