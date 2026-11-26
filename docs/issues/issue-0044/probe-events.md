# Task 1.3 探測：V2 事件與互動樣本

取樣：2026-11-24（UTC 07:5x），主機 flashaim，@opencode/cli 2.3.2，容器用 wukong:v0.25.1 映像、全新的 HOME 與 workspace。只送固定的合成提示詞給預設免費模型；retry 情境在 `--network none` 的另一個容器取。

```bash
# V2 為 npm install -g --prefix V2 @opencode/cli@2.3.2 的目錄；WORK、OUT 為空目錄
bash docs/issues/issue-0044/scripts/capture-events.sh "$V2" "$WORK" "$OUT"
```

[scripts/capture-events.sh](scripts/capture-events.sh) 起 V2 server，[scripts/capture-scenario.py](scripts/capture-scenario.py) 在容器裡訂閱 `/api/event` 跑單一情境。CLI 樣本取自 Task 1.2（[probe-cli.md](probe-cli.md)）。

## 結果

| 情境 | 判定 | 事件數 |
|---|---|---|
| text-tool：跑一次 shell 工具後回覆 | PASS，`session.execution.succeeded`，有 `session.tool.success` | 46 |
| permission：shell 設為 ask | PASS，`permission.asked` 後 `POST …/permission/{id}/reply`（once）回 204，完成 | 35 |
| form：question 工具 | PASS，`form.created` 後 `POST …/form/{id}/reply` 回 204，完成 | 26 |
| interrupt：長回答中途中止 | PASS，`interrupt` 回 200，`session.execution.interrupted` | 21 |
| no-route：模型不存在 | PASS，`session.execution.failed`，`error.type=provider.no-route` | 8 |
| retry：上游連不到 | PASS，兩次 `session.retry.scheduled`（`error.type=provider.transport`）後 interrupt，`session.execution.interrupted` | 22 |

樣本存在 [crates/wukong-gateway/tests/fixtures/opencode-v2/](../../../crates/wukong-gateway/tests/fixtures/opencode-v2/README.md)（6 份 server、5 份 CLI）。存檔前檢查過：沒有密碼、主機名稱、使用者名稱或正式對話；`session.instructions.updated` 只帶系統指示的雜湊。

## V1 → V2 事件對照（給 Task 2.2、2.3、2.5）

Wukong 的 server backend 現在讀的 V1 事件：

| V1（Wukong 現在讀的） | V2 對應 | 備註 |
|---|---|---|
| `message.part.updated`（text） | `session.text.started`／`session.text.delta`（`data.delta`）／`session.text.ended` | V2 送增量，V1 送整段快照 |
| `message.part.updated`（reasoning） | `session.reasoning.started`／`.delta`／`.ended` | 同上 |
| `message.part.updated`（tool） | `session.tool.called`、`session.tool.progress`、`session.tool.success`／`session.tool.failed` | 工具名稱與輸出在 `data` |
| `session.idle`、`session.status` | `session.execution.succeeded`／`.failed`／`.interrupted` | 完成判定改看 execution 事件 |
| `session.error` | `session.execution.failed`（`data.error`）、`session.step.failed` | error 形狀為 `{type, message, status?, retryAfterMs?}`（openapi 的 `Session.StructuredError`） |
| `permission.asked` | `permission.asked`（`data.id`、`action`、`resources`） | 名稱相同，欄位不同；回覆後有 `permission.replied` |
| `question.asked` | `form.created`（`data.form.fields[].key`、`options`） | 回覆 body 為 `{"answer": {"q0": "A"}}`；之後有 `form.replied` |
| 無 | `session.retry.scheduled`（`attempt`、`at`、`error`） | SCN-007、SCN-016 的 90 秒收斂從這裡算 |

中止時另有 `session.step.failed`（`error.type=aborted`），完成判定要以 execution 事件為準，不能把它當成上游錯誤。

## 沒取到的樣本

- **額度用完或限流、模型下架**：無法在不影響正式額度的情況下觸發，這次沒有真實樣本。V2 openapi 的 `Session.StructuredError` 有整數欄位 `status`（HTTP 狀態碼），`ProviderRequestError` 的 `reason` 有 `rate_limit`。Task 2.3 以 `status`（429、410）做結構化判定，與 V1 依 `statusCode` 判定的做法相同（地雷 W-7）；`error.type` 的實際字串要等真的遇到時補樣本，Task 2.3 交付時把這項列為已知缺口。
