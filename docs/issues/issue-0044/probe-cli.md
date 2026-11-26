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

## Task 2.6：Wukong 改傳的旗標（2026-11-24）

容器用 wukong:v0.25.1 映像，`--network none`，全新的 HOME 與 workspace。

### `--standalone` 與背景 service

V2 的 `run --help` 說明 `--standalone`：「Run with a private server instead of the background service」。在容器裡先執行 `opencode serve --service`，再執行 `run --print-logs --log-level error --format json`：

| 旗標 | stderr | 解讀 |
|---|---|---|
| 不加 `--standalone` | 只有 1 行（收到 SIGTERM） | 連上了背景 service，沒有自己起 server |
| 加 `--standalone` | 44 行，第一行為 `cli starting … args="[\"serve\",\"--stdio\",\"--port\",\"0\"]"` | 起了私有的 `serve --stdio` |

Task 1.2 的容器裡沒有背景 service，所以兩者看起來一樣。主機上使用者自己跑著 V2 managed service（`opencode serve --service`）時，CLI backend 不加 `--standalone` 就會把回合跑進使用者的 service 與資料。所以 opencode 命令一律加 `--standalone`，只有使用者在 `WUKONG_AGENT_CMD` 裡自己指定了 `--standalone` 或 `--server` 時例外。

### `--log-level error` 的 stderr

不論有沒有 `--standalone`，stderr 都約有 43 行 INFO，只有 1 行 ERROR（斷網時是 `Failed to fetch models.dev`）。等級傳不到私有 serve。這份 stderr 存成 fixture `cli-stderr-offline.txt`。V1 傳 `ERROR` 時 stderr 只有錯誤，為了維持這個行為，`run` 以非 0 結束時，回報的 stderr 會濾掉 INFO 以下的日誌行。

### 經 wukong CLI 的回合

`--agent-cmd "<wrapper>/opencode run --dangerously-skip-permissions"`。wrapper 把 Wukong 傳來的 argv 原樣交給容器裡的 V2，容器裡同樣先起背景 service：

- 規劃那一棒的 argv：`run --dangerously-skip-permissions --standalone --print-logs --log-level error --agent plan --format json <prompt>`。V2 接受了所有旗標，起了私有 serve。改動前傳 `ERROR` 時 V2 直接 rc=1 拒絕，見上方「結果」表
- 斷網時規劃那一棒用預設模型，重試超過 90 秒後中止，rc=1：`上游模型錯誤（上游連不到）…（重試 7 次、超過 90 秒仍未成功，已中止）`，96 秒結束
- 結束後殘留 1 個容器（Task 2.3 記過：被結束的是 docker client），已依 ID 確認掛的是本次探測的目錄後刪除

要模型回覆的部分（取得回覆與 session、以 `-s` 接續）這次沒有跑。2026-11-24 17:36 起免費模型額度已用完（見 probe-reply.md），額度恢復後補上。

### 真實模型的回合（2026-11-25）

免費額度仍未恢復，使用者決定改用 NVIDIA 的模型。執行 [scripts/cli-v2-actual.sh](scripts/cli-v2-actual.sh)：

- 映像 `wukong-issue44:v2-local`，內建 `@opencode/cli` 2.3.2，容器有網路
- 隔離的 HOME 只放 nvidia 一筆憑證，模型由 `OPENCODE_CONFIG_CONTENT` 指定為 `nvidia/nvidia/nemotron-3.5-lightning-30b-a3b`，argv 不另加 `--model`
- 主機上的 wukong 用 `--agent-cmd "<wrapper>/opencode run --dangerously-skip-permissions"`。帶了額外旗標才走 CLI backend；只寫 `opencode run` 會走按需本機 backend
- 同一個 scope 跑兩個回合，提示詞是固定的合成字串

Wukong 傳出的 argv（不含提示詞）：

```
--version
run --dangerously-skip-permissions --standalone --print-logs --log-level error --agent plan --format json
run --dangerously-skip-permissions --standalone --print-logs --log-level error --format json
--version
run --dangerously-skip-permissions --standalone --print-logs --log-level error --agent plan --format json
run --dangerously-skip-permissions --standalone --print-logs --log-level error -s ses_dee9d420effeCkUxnypWqUzmB4 --format json
```

| 檢查 | 結果 |
|---|---|
| 第一回合拿到回覆 | PASS，rc=0，輸出 `OK` |
| 第一回合結束後容器裡的 opencode 程序 | PASS，0 個 |
| argv 帶 `--standalone --print-logs --log-level error` | PASS |
| 第一回合把 V2 session 存進 scope | PASS，`agent_session_state` 記 `ses_dee9d420…` |
| 第二回合拿到回覆 | PASS，rc=0，輸出 `AGAIN` |
| 第二回合以 `-s` 接續同一個 session | PASS |
| 第二回合結束後容器裡的 opencode 程序 | PASS，0 個 |

- V2 的 `opencode.db` 裡，該 session 的 `session_message` 依序是 user、assistant、idle、user、assistant、idle，兩個回合接在同一個 session；規劃那一棒用自己的 session
- 反向自檢：另起一個容器、留著一個 `opencode serve`，同一段程序檢查數到 1 個

### NVIDIA 模型下架（410）的真實樣本

挑模型時，`nvidia/deepseek-ai/deepseek-v4.1-flash` 與 `nvidia/openai/gpt-oss-20b` 都回 410：

```
{"type":"error",…,"error":{"type":"provider.invalid-request","message":"The model 'deepseek-ai/deepseek-v4.1-flash' has reached its end of life on 2026-11-16T08:00:00Z and is no longer available.","status":410,…}}
```

V2 的錯誤類型是通用的 `provider.invalid-request`，只能靠 `status` 410 判斷是下架。這份輸出存成 fixture `cli-model-eol.ndjson`，由 `upstream_error.rs` 的 `a_real_end_of_life_410_is_model_eol` 驗證 Wukong 判為「模型已下架」。
