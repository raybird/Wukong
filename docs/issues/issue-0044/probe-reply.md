# Task 2.5 探測：提問與權限的回覆

取樣：2026-11-24，主機 flashaim。只送固定的合成提示詞給免費模型，HOME 與 workspace 都是暫存目錄：

- V2：`@opencode/cli` 2.3.2，跑在 wukong:v0.25.1 映像的容器裡
- V1：同一映像內建的 opencode 1.19.6

## V2 的回覆端點（[scripts/capture-scenario.py](scripts/capture-scenario.py)）

```bash
SCENARIOS="form-custom form-cancel form-multi permission-reject" \
  bash docs/issues/issue-0044/scripts/capture-events.sh "$V2" "$WORK" "$OUT"
```

| 情境 | 呼叫 | 結果 | 事件順序 |
|---|---|---|---|
| form，選一個選項（Task 1.3 的 `form`） | `POST /api/session/{sid}/form/{id}/reply`，`{"answer":{"q0":"A"}}` | 204，`execution.succeeded`，回覆 `A` | `form.created` → `form.replied` → `tool.success` |
| form，自訂回答 | 同上，`{"answer":{"q0":"CUSTOM-42"}}` | 204，`execution.succeeded`，回覆 `CUSTOM-42` | 同上 |
| form，多選 | 同上，`{"answer":{"q0":["Red","Green"]}}` | 204，`execution.succeeded`，回覆 `Red, Green` | 欄位 `type` 為 `multiselect`，選項 `value` 與 `label` 相同 |
| form，取消 | `DELETE /api/session/{sid}/form/{id}` | 204，**`execution.interrupted`（reason `shutdown`）**，沒有新回覆 | `form.cancelled` → `tool.failed`（`aborted`：The user dismissed this question）→ `step.failed` |
| permission，允許一次（Task 1.3 的 `permission`） | `POST /api/session/{sid}/permission/{id}/reply`，`{"decision":"once"}` | 204，`execution.succeeded` | `permission.asked` → `permission.replied` |
| permission，拒絕 | 同上，`{"decision":"reject"}` | 204，**`execution.interrupted`（reason `shutdown`）**，沒有新回覆 | `permission.replied`（reply `reject`）→ `tool.failed`（The user declined this tool call）→ `step.failed` |

V2 沒有 question 的 reject 端點，取消要刪除 form。使用者自己按停止時，`execution.interrupted` 的 reason 是 `user`（Task 1.3 的 `interrupt` 樣本），與這裡的 `shutdown` 不同。

四個新樣本放在 `crates/wukong-gateway/tests/fixtures/opencode-v2/`（`server-form-custom`、`server-form-cancel`、`server-form-multi`、`server-permission-reject`）。已確認裡面沒有探測密碼與主機的家目錄路徑。

## V1 1.19.6 的同一動作（[scripts/v1-reject.py](scripts/v1-reject.py)）

| 情境 | 呼叫 | 結果 |
|---|---|---|
| question，取消 | `POST /question/{id}/reject` | 200，`question.rejected` → `session.idle`，沒有 `session.error`，最後一則 assistant 沒有文字 |
| permission，拒絕（設定 `{"permission":{"bash":"ask"}}`） | `POST /permission/{id}/reply`，`{"reply":"reject"}` | 200，`permission.replied` → `session.idle`，沒有錯誤，沒有文字 |

V1 的取消與拒絕都讓回合以空回覆正常結束。V2 改送 `execution.interrupted`，如果照 Task 2.2 的對應，會變成「回合被中止」的錯誤。所以 `ServerEventMapper` 看到 `form.cancelled`，或 `permission.replied` 的 reply 為 `reject` 之後，再收到 `execution.interrupted` 時改視為正常結束，與 V1 相同。

## 經 Wukong gateway 的真實回合

[scripts/local-v2-actual.sh](scripts/local-v2-actual.sh) 的第 4 個參數是測試篩選字串，傳 `replies` 時只執行 `local_question_and_permission_replies_against_real_v2`。這個測試經 `AgentBackend`（Telegram、Web 與排程共用的分派）完成以下五個回合：選項回答、自訂回答、取消、允許一次、拒絕。權限的兩個回合把 `OPENCODE_CONFIG_CONTENT` 設成 `{"permission":{"shell":"ask"}}`。

2026-11-24 17:36（台北）執行時，第一個回合就收到真實的 429：`UpstreamFailed { kind: RateLimited, status_code: Some(429), detail: "provider.free-limit statusCode=429: Free usage limit reached. …" }`。預設的 big-pickle 與 `opencode/deepseek-v4.1-flash-free` 都一樣，所以這次沒有完成。這筆結果同時是 SCN-007「額度用完或限流 → 上游限流」第一次取得的真實樣本，分類正確。等免費額度恢復後重跑：

```bash
bash docs/issues/issue-0044/scripts/local-v2-actual.sh "$V2" "$WORK" "" replies
```
