# opencode serve 排程 decode failure 交接

日期：2026-07-01

## 背景

Wukong 近期新增 `opencode serve` backend，Docker 模式會透過 `WUKONG_AGENT_SERVER_URL=http://opencode-server:4096` 讓 Web、Telegram、schedulerd 共用同一個 opencode server。

目前已發布：

- `v0.16.18-rc.1`：新增 Docker-first `opencode serve` backend。
- `v0.16.18-rc.2`：新增 server backend streaming，使用 `POST /session/:id/prompt_async` + `GET /event`。
- `v0.16.18-rc.3`：修正 real opencode event 將 `sessionID` 放在 `properties.sessionID` 時 reasoning/tool event 被丟掉的問題。

## 目前使用者看到的錯誤

Telegram 收到排程失敗通知：

```text
⏰ GitHub Trending 每日推送 執行失敗：backend error: agent command failed (code None): opencode server request failed: error decoding response body
```

對應 Docker log：

```text
wukong-schedulerd  | job 82f1f26a-0bee-495b-913b-0cad8a24708f failed: backend error: agent command failed (code None): opencode server request failed: error decoding response body
wukong-schedulerd  | job 82f1f26a-0bee-495b-913b-0cad8a24708f result delivered to telegram
```

`opencode-server` log 只有啟動資訊，沒有對應錯誤細節：

```text
wukong-opencode-server  | Warning: OPENCODE_SERVER_PASSWORD is not set; server is unsecured.
wukong-opencode-server  | opencode server listening on http://0.0.0.0:4096
```

## 已確認不是同一個問題

這個錯誤不是先前 benchmark 觀察到的「Wukong CLI 在 serve streaming 模式 stdout 空白」。

CLI stdout 空白的原因是目前 server backend streaming 只 emit reasoning/tool/step progress event，不 emit final `StreamEvent::Text`；final answer 仍放在 `AgentResponse.text`。Web Console 使用 `AgentResponse.text` 顯示最終答案，因此不等同於 backend request failure。

本次排程錯誤是 `reqwest` 在讀 HTTP response body 時直接失敗，錯誤來源為 `crates/wukong-gateway/src/opencode_server.rs` 的 `http_error()`：

```rust
fn http_error(err: reqwest::Error) -> GatewayError {
    GatewayError::AgentFailed {
        code: None,
        stderr: format!("opencode server request failed: {err}"),
    }
}
```

## 已做過的本地/容器驗證

### 本機 benchmark

環境：本機 `opencode 1.17.12`，`target/debug/wukong`，短 prompt：`請只回答 OK，不要解釋`。

有效 final response 測試使用 `--no-stream`，兩邊 stdout 都是 `OK`：

| Backend | Run 1 | Run 2 | Run 3 | 平均 |
| --- | ---: | ---: | ---: | ---: |
| CLI backend (`opencode run`) | 20.54s | 32.25s | 25.13s | 25.97s |
| serve backend (`opencode serve`) | 16.99s | 19.67s | 12.45s | 16.37s |

serve backend 在短 prompt 下可正常完成，且平均約快 37%。

### Docker deployment 驗證

在 `/home/raybird/Documents/RunWuKong`：

```bash
docker compose ps
docker compose exec -T opencode-server opencode --version
docker compose exec -T wukong-web curl -fsS http://opencode-server:4096/global/health
```

結果：

- `opencode-server`、`wukong-web`、`wukong-telegram`、`wukong-schedulerd` 都在跑。
- `opencode --version` 是 `1.17.12`。
- `/global/health` 回 `{"healthy":true,"version":"1.17.12"}`。

容器內短 prompt 走 server backend 成功：

```bash
docker compose exec -T wukong-web env \
  WUKONG_AGENT_SERVER_URL=http://opencode-server:4096 \
  WUKONG_MEMORY_DB=sqlite:///data/bench-check.db \
  wukong --no-stream --new --scope project:scheduler-diagnose \
  "請只回答 OK，不要解釋"
```

輸出：

```text
🐵 悟空·oracle
OK
```

因此目前不是「server backend 在 Docker 中全面不可用」。

## 相關排程資訊

`wukong schedule list` 顯示失敗的是：

```text
82f1f26a-0bee-495b-913b-0cad8a24708f enabled=true cron=0 0 * * * next=1782950400 name=GitHub Trending 每日推送 kind=turn(scope=user:tg-915354960)
```

其他排程也在同一個 Telegram user scope 下。

## 目前推論

目前可確定：

- `opencode serve` health OK。
- 短 prompt server backend 成功。
- 錯誤發生在 schedulerd 執行實際 turn job 時。
- 錯誤訊息來自 `reqwest` 讀 response body 階段，而非 opencode HTTP status error。

目前尚未確定是哪個 endpoint 失敗，因為所有 request body decode errors 都被包成同一個：

```text
opencode server request failed: error decoding response body
```

最可能位置：

- `GET /event` SSE stream：`consume_event_stream()` 裡 `response.chunk().await` 解碼失敗。
- `GET /session/:id/message`：stream idle 後 `list_messages()` 抓 final message list 時 body decode 失敗。
- 較不可能但仍可能：`POST /session`、`POST /session/:id/prompt_async` 或 health check 的 response body 解碼失敗。

可能觸發條件：

- 該排程 prompt 比短測試更長，執行時間更久，SSE connection 更容易被中斷。
- 該 Telegram scope 可能接續既有 opencode session，session history 較長。
- opencode server 在長回合或工具使用期間可能關閉/重置 chunked body。
- Wukong 對 reqwest decode error 沒標示 request phase，因此目前無法區分是哪一段。

## 建議下一步

### 1. 先增加錯誤階段資訊

在 `crates/wukong-gateway/src/opencode_server.rs` 讓不同 request path 包出不同錯誤前綴，例如：

- `opencode server health_check failed: ...`
- `opencode server create_session failed: ...`
- `opencode server prompt_async failed: ...`
- `opencode server event_stream failed: ...`
- `opencode server list_messages failed: ...`

最小做法：新增帶 context 的 helper，或在各呼叫點 `map_err` 包出 endpoint/phase。

這一步不一定修 bug，但能讓下一次排程失敗直接定位是 SSE 或 final message fetch。

### 2. 讓 `consume_event_stream()` 的 chunk error 更明確

目前：

```rust
chunk = response.chunk() => chunk.map_err(http_error)?,
```

建議改成明確 context：

```text
opencode server event stream failed while reading chunk: error decoding response body
```

如果是這裡，就代表主問題在 SSE stream 穩定性或 server-side connection closure。

### 3. 讓 `list_messages()` 的 decode error 更明確

如果錯誤其實發生在 stream idle 後抓 message list，應顯示：

```text
opencode server list messages failed: error decoding response body
```

這時可考慮：

- 對 `list_messages()` 做短 retry，因為 opencode server 可能剛 idle 但 message API 還未穩定。
- 或改為在 SSE 過程中累積 final text/part snapshot，降低對最後一次 message fetch 的依賴。

### 4. 增加測試

建議新增單元測試/假 server 測試：

- `send_json` / `send_empty` / `consume_event_stream` 發生 reqwest decode-like error 時，錯誤訊息包含 phase。
- 如實作 retry，測 `list_messages()` 第一次失敗、第二次成功。

注意專案規則：改 function/class/method 前先跑 GitNexus impact；bugfix 需先寫 failing test。

### 5. 重新跑真實排程或手動 trigger

更新部署後，對該 job 手動觸發：

```bash
docker compose exec -T wukong-schedulerd wukong schedule trigger --id 82f1f26a-0bee-495b-913b-0cad8a24708f
```

然後看：

```bash
docker compose logs --no-color --since 30m wukong-schedulerd
docker compose logs --no-color --since 30m opencode-server
```

## 重要檔案

- `crates/wukong-gateway/src/opencode_server.rs`：server backend HTTP/SSE 實作，錯誤包裝點在這裡。
- `crates/wukong-runtime/src/turn.rs`：排程 turn 會走 `run_turn()`，內部呼叫 `backend.run_streaming()`。
- `crates/wukong-scheduler/src/executor.rs`：排程執行與錯誤包裝，`execute_job()` 將錯誤轉成 Telegram 通知訊息。
- `crates/wukong-telegram/src/dispatch.rs` / `crates/wukong-schedulerd/src/notify.rs`：通知呈現路徑。

## 注意事項

- 目前 worktree 有使用者/環境既有 dirty files：`AGENTS.md`、`CLAUDE.md`，不要誤改或 revert。
- `opencode serve` 的 RC 最新是 `v0.16.18-rc.3`。
- Docker bundle 來自 GitHub release；若修完要在 Docker 驗證，需要 push/tag 新 RC 後更新部署。
