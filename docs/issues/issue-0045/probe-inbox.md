# 探測：V2 session inbox 的 queue 行為（Task 6.4）

> ← 回到 [README](README.md)｜計畫見 [implementation-plan.md](implementation-plan.md#task-64--inbox-評估)

2026-11-26 在隔離容器實測：

- 映像 `wukong-issue44:v2-local`（opencode v2.3.2）；
- 憑證只有 NVIDIA 一筆，模型 `nvidia/nvidia/nemotron-3-ultra-550b-a55b`；
- 只送固定的合成提示。

命令：

```bash
bash docs/issues/issue-0045/scripts/probe-inbox.sh wukong-issue44:v2-local "$(mktemp -d …/p45inbox.XXXX)" \
    …/p45-nvidia-auth.json nvidia/nvidia/nemotron-3-ultra-550b-a55b
```

腳本是 [scripts/probe-inbox.sh](scripts/probe-inbox.sh) 與 [scripts/probe_inbox.py](scripts/probe_inbox.py)。

## 介面

取自 V2 server 的 `GET /openapi.json`。

- `POST /api/session/{id}/prompt` 可以帶 `delivery`，值是 `steer` 或 `queue`。
  - 回應的 `data` 是這則 inbox 項目，記著實際的 delivery。
  - 不帶時是 `steer`，Wukong 現在送的就是這種。
- `GET /api/session/{id}/inbox` 列出還沒送進執行的項目，依進 inbox 的順序排列。
- `PATCH /api/session/{id}/inbox/{inboxID}` 可以改項目的 delivery；`DELETE` 可以把項目移除。
- `command`、`synthetic`、`compact`、`move` 也都收 `delivery`。

## 實測

1. 在同一個 session 先送 prompt A：「請用繁體中文寫一段大約 300 字的短文」，不帶 delivery。
2. 一看到 A 的 `session.execution.started`，就送 prompt B：「只回覆 QUEUED」，帶 `delivery: "queue"`。
3. B 送出後立刻列一次 inbox。

| 秒數（從送出 A 起算） | 事件 | delivery |
|---|---|---|
| 0.48 | `session.inbox.enqueued`（A） | steer |
| 0.48 | `session.execution.started` | |
| 0.49 | 送出 B（回 200）；這時 inbox 列出一筆 user 項目 | queue |
| 0.49 | `session.inbox.delivered`（A） | |
| 0.49 | `session.inbox.enqueued`（B） | queue |
| 21.49 | `session.inbox.delivered`（B）：A 的那一步寫完之後 | |
| 22.76 | `session.execution.succeeded`：整段只有這一次 | |

- 訊息依序是 user A、assistant A（完整的短文）、user B、assistant B（`QUEUED`）。
- 兩個 step 各回答一則 prompt。結束時 inbox 是空的。
- 探測在等第二次 `execution.succeeded`，但它沒有出現，所以等到 280 秒的上限才結束。

## 結論

- **queue 不會另開一次執行。**
  - B 等 A 那一步（一次 LLM 呼叫）結束後才送進去，接在同一次執行裡回答。
  - `steer` 則是一進 inbox 就送進去。
- **A 有工具呼叫、會分好幾步時，B 是在下一步之前送進去，還是整次執行結束後才送，這次沒有驗證。** 這次的 A 只有一步。

## 對 Wukong 的影響

Wukong 判斷回合結束的依據，是 primary 的執行結束，而且 inbox 已清空（Task 3.1）。如果改成 queue：

- 兩個 Wukong 回合會併進同一次執行。
- 第一個回合要等到 B 也答完才結束，最後的文字會是 B 的回答。
- 第二個回合看不到自己那次執行的開始與結束。

要用 inbox，回合邊界與輸出歸屬都得重新設計。結論與建議見 [implementation-plan.md 的 Task 6.4](implementation-plan.md#task-64--inbox-評估)。
