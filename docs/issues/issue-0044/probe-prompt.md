# Task 2.2 探測：V2 的 prompt、附件、agent、模型與壓縮

取樣：2026-11-24，主機 flashaim，@opencode/cli 2.3.2，容器用 wukong:v0.25.1 映像、全新的 HOME 與 workspace。只送固定的合成提示詞給預設免費模型。

```bash
# V2 為 npm install -g --prefix V2 @opencode/cli@2.3.2 的目錄；WORK、OUT 為空目錄
SCENARIOS="attach-file attach-data attach-image agent-model deny-question compact" \
  bash docs/issues/issue-0044/scripts/capture-events.sh "$V2" "$WORK" "$OUT"
```

情境寫在 [scripts/capture-scenario.py](scripts/capture-scenario.py)。

## 結果

| 情境 | 結果 | 對 Wukong 的影響 |
|---|---|---|
| attach-file：`files:[{"uri":"file:///p/workspace/secret.txt","name":…}]` | PASS，回覆附件裡的字 `PINEAPPLE42` | shared 附件模式沿用 `file://` URI |
| attach-data：`data:text/plain;base64,…` | PASS，回覆 `MANGO77` | inline 附件模式沿用 data URI |
| attach-image：`data:image/png;base64,…` | prompt 200；預設模型回「this model does not support image input」 | 請求格式可用；能不能看圖取決於模型，與 V1 相同 |
| agent-model：`POST …/agent {"agent":"plan"}`、`POST …/model {"model":{"id","providerID"}}` | 兩者 204；之後的 `session.step.started` 帶 `agent=plan` 與該模型 | V2 的 agent 與模型是 session 狀態，在 prompt 前設定 |
| deny-question：建立 session 時 `permissions:[{"action":"question","resource":"*","effect":"deny"}]` | 沒有 `form.created`，模型沒有嘗試任何工具 | V1 的 `tools:{"question":false}` 改成建立 session 時的權限規則 |
| compact：回覆一次後 `POST …/compact {}` | 200；事件依序為 `inbox.enqueued`、`execution.started`、`compaction.started`、`compaction.delta`…、`compaction.ended`、`execution.succeeded` | 壓縮不必指定模型，送出後與回合一樣等 execution 事件結束 |

壓縮與 file 附件兩份事件存進 `crates/wukong-gateway/tests/fixtures/opencode-v2/`（`server-compact.ndjson`、`server-attach-file.ndjson`），存檔前確認沒有密碼、主機名稱、使用者名稱或正式對話。

## 端到端（改完 Task 2.2 後）

同日以本分支建出的 `target/debug/wukong` 接上容器裡的 V2 server（`-p 127.0.0.1::4096`，全新資料），`--scope project:e2e`，記憶庫與設定都在暫存目錄：

1. 「Reply with exactly the word PONG and nothing else.」：rc=0，回覆以 PONG 開頭；規劃用的暫時 session 建立後刪除（`helper_session_deleted`）
2. `WUKONG_SESSION_COMPACT_EVERY_TURNS=1`，問上一則要求回覆的字：rc=0，回覆 PONG；stderr 有 `session_compacted`（對同一個 session）；回合結束後 server 上只剩 1 個 session，就是 `agent_session_state` 指向的那個（turn_count=1）
