# OpenCode V2 真實事件樣本

2026-11-24 由 issue-0044 Task 1.2、1.3、2.2、2.5、2.6 從 @opencode/cli 2.3.2 實際取得，取法見 `docs/issues/issue-0044/probe-events.md` 與 `probe-prompt.md`。只含固定的合成提示詞與全新的 HOME，沒有正式對話。測試以這些樣本當輸入，期望值不得從被測程式碼推導。

| 檔案 | 來源 | 情境 |
|---|---|---|
| server-text-tool.ndjson | `GET /api/event` | 跑一次 shell 工具後回覆，正常完成 |
| server-permission.ndjson | `GET /api/event` | shell 設為 ask，permission.asked 後回覆 once，完成 |
| server-form.ndjson | `GET /api/event` | question 工具產生 form，回覆後完成 |
| server-interrupt.ndjson | `GET /api/event` | 長回答中途 interrupt |
| server-no-route.ndjson | `GET /api/event` | 模型不存在，execution.failed（provider.no-route） |
| server-compact.ndjson | `GET /api/event` | 回覆一次後 `POST /compact`，壓縮完成 |
| server-attach-file.ndjson | `GET /api/event` | prompt 帶 `file://` 附件，回覆附件內的字 |
| server-retry.ndjson | `GET /api/event`（`--network none`） | 上游連不到，retry.scheduled 兩次後 interrupt |
| cli-text-reasoning.ndjson | `opencode run --format json --thinking` | 正常回覆，含思考 |
| cli-no-route.ndjson | `opencode run --format json` | 模型不存在，error（provider.no-route） |
| cli-retry-transport.ndjson | `opencode run --format json`（`--network none`） | 上游連不到，retry 持續到被 timeout 結束 |
| cli-tool-completed.ndjson | `opencode run --format json --dangerously-skip-permissions` | read 工具完成 |
| cli-tool-permission-rejected.ndjson | `opencode run --format json`（read 設為 ask） | 非互動執行拒絕權限，工具 error |
| cli-stderr-offline.txt | `opencode run --standalone --print-logs --log-level error --format json`（`--network none`）的 stderr | 斷網時的 stderr：43 行 INFO、1 行 ERROR（`--log-level` 沒傳給私有 serve） |
