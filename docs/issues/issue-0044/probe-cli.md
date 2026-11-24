# Task 1.2 探測：V2 `opencode run` 的 CLI 契約

取樣：2026-11-24（UTC 07:45～07:50），主機 flashaim，@opencode/cli 2.3.2，容器用 wukong:v0.25.1 映像、全新的 HOME 與 workspace（不含正式對話）。需要模型的段落只送固定的合成提示詞給預設免費模型。

```bash
# V2 為 npm install -g --prefix V2 @opencode/cli@2.3.2 的目錄；WORK 為空目錄
bash docs/issues/issue-0044/scripts/probe-cli.sh "$V2" "$WORK"
```

[scripts/probe-cli.sh](scripts/probe-cli.sh) 第 1～7 段。映像沒有 `ps`，程序檢查改讀 `/proc/*/cmdline`。

## 結果

| 項目 | V2 2.3.2 實測 | 對 Wukong 的影響 |
|---|---|---|
| `--log-level ERROR` | 拒絕：`Invalid value for flag --log-level: "ERROR"`，只收小寫（`error`），rc=1 | `assemble_argv` 現在傳 `ERROR`，換版後 CLI backend 每次都失敗，要改小寫 |
| `--log-level error` 的效果 | stderr 仍有約 40 行 INFO（`run` 會起 `serve --stdio` 子程序，日誌等級沒傳下去） | stderr 不再只有錯誤，從 stderr 判斷上游錯誤要能略過 INFO 行 |
| 事件形狀（`--format json`） | `step_start`、`reasoning`、`text`、`step_finish` 與 V1 同名；`part.text` 仍是文字；`sessionID` 在最上層與 `part` 內 | `stream.rs` 的文字、思考、步驟解析大致可沿用；工具事件的形狀待 Task 1.3 取樣 |
| 錯誤事件 | `{"type":"error","error":{"type":"provider.no-route","message":…}}`，沒有 V1 的 `error.name` 與 `data.statusCode` | 上游錯誤分類（地雷 W-7）要改讀 `error.type`，由 Task 1.3 補齊錯誤樣本 |
| 上游連不到（`--network none`、預設模型） | 一直送 `retry` 事件（`attempt`、`nextRetryAt`、`error.type=provider.transport`），120 秒內 7 次，沒有結束，被 timeout 殺掉（rc=124） | 見下方「與 V1 對照」 |
| `-s` 接續 | 第二次執行的事件只帶第一次的 session ID，沒有新 session | 與 V1 相同 |
| 不加 `--standalone` | 這個容器裡結束後沒有殘留程序（`run` 起的是 `serve --stdio` 子程序） | 行為與 `--standalone` 相同；仍照計畫加 `--standalone`，不依賴背景 service 是否存在 |
| `--standalone` 結束後 | 沒有殘留程序（4 種情境都是 0） | SCN-002、SCN-010 的「不留程序」在 CLI 路徑成立 |

### 權限旗標（TBD-1，第 8 段與補充）

用 `OPENCODE_CONFIG_CONTENT` 把 `permission.read` 設成 `ask`，`stdin` 為 null（同 Wukong），請模型讀檔：

| 旗標 | 結果 |
|---|---|
| 不加 | read 兩次都是 error：`This non-interactive run cannot ask the user for permission, so the request was rejected.`，回合正常結束（rc=0） |
| `--dangerously-skip-permissions` | read 完成（另一次是檔案不存在），rc=0 |
| `--auto` | 同上 |

結論（2026-11-24）：V2 2.3.2 仍接受 V1 的 `--dangerously-skip-permissions`，效果與 `--auto` 相同；自訂 `WUKONG_AGENT_CMD` 帶這個旗標的使用者升級後不會壞。沒加旗標時，非互動執行遇到 `ask` 會直接拒絕，不會卡到逾時。附帶發現：V1 寫法的 `"permission":{"read":"ask"}` V2 接受，`opencode debug agents` 顯示為 `{"action":"read","resource":"*","effect":"ask"}`，Task 3.1 改 baseline 時可以從這裡開始。

另外：第 8 段沒有改設定時，讀 workspace 外的 `/etc/hostname` 三種旗標都直接完成，V2 預設的 external_directory 規則與 V1 不同，留給 Task 3.1 比對。

## 與 V1 對照：上游連不到（TBD-3）

同一映像的 V1（1.19.6），`--network none`、預設模型、`opencode run --format json`：65 秒後以 rc=1 結束，事件是一筆 `{"type":"error","error":{"name":"UnknownError",…"getaddrinfo EAI_AGAIN opencode.ai"}}`。

V2 在同樣條件下 120 秒仍在重試，Task 1.1 的 server 路徑也是 60 秒後 session 仍在執行。V1 會回錯誤，V2 不會；Wukong 照現在的寫法要等到 agent timeout（預設 20 分鐘）才失敗。這是換版造成的行為差異，Wukong 要自己收斂，收斂的條件與回報方式待決定（README TBD-3）。
