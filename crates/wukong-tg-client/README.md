# wukong-tg-client

> 共用傳輸層 ──「分身的傳令」：Telegram Bot API client + 純解析

Telegram 的傳輸原語，從 `wukong-telegram` 抽出，**零 Wukong 內部依賴**（只用 `reqwest`/`serde_json`/`thiserror`），讓 Telegram bot 與排程 daemon 都能送訊息，而不必互相牽連整包 CLI。

## 內容

| 模組 | 內容 |
| :--- | :--- |
| `client` | `TgClient` trait、`ReqwestTgClient`（真實實作）、`mock::MockTgClient`（測試用，`mock` feature） |
| `parse` | `parse_updates` / `highest_update_id`（long-poll）、`parse_allowlist` / `is_allowed`（白名單）、`scope_for_chat` / `chat_id_from_scope`（scope ↔ chat_id 互轉） |
| `error` | `TgError`（HTTP / API 錯誤） |

`scope_for_chat(id) → "user:tg-<id>"`；`chat_id_from_scope` 為其反向，非 Telegram 來源的 scope（如 `project:X`）回傳 `None`，確保排程結果不會誤送到聊天室。

## 使用者

- `wukong-telegram`：`pub use` 轉出 `client`/`parse`/`error`，long-poll 收送訊息。
- `wukong-schedulerd`：排程觸發後，用 `chat_id_from_scope` + `TgClient` 把 Turn 結果回送原聊天室。

## `mock` feature

`MockTgClient` 以 `#[cfg(any(test, feature = "mock"))]` 設限——本 crate 自身測試自動可用；依賴方在 `[dev-dependencies]` 加 `features = ["mock"]` 即可在其測試中使用，且不會編入 release。
