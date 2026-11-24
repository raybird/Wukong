# Task 2.4 探測：本機按需模式接 V2 2.3.2

取樣：2026-11-24，主機 flashaim，@opencode/cli 2.3.2。V2 在主機上執行：

- 環境：只有隔離的 HOME（`env -i HOME=<暫存目錄>`），不碰使用者自己的 opencode 資料
- 提示詞：只送固定的合成提示詞給免費模型
- 注意：這台主機上另有使用者自己的 V2 managed service（`opencode serve --service`）。`opencode models` 這類指令會去連它的固定埠；`serve --port 0` 不會，所以 Wukong 的本機模式不受影響

## V2 `serve` 的啟動行為（斷網容器，不送提示詞以外的請求）

| 項目 | V2 2.3.2 實測 | 對 Wukong 的影響 |
|---|---|---|
| `serve --hostname 127.0.0.1 --port 0 --print-logs` | 接受，stdout 印 `server listening on http://127.0.0.1:<埠>` | V1 是 `opencode server listening on`，啟動偵測要改，否則一直等不到位址（見下方「改動前」） |
| stderr | `--print-logs` 為 `timestamp=… level=INFO …` 格式；斷網時重試期間只有 `Failed to fetch models.dev` 一行 ERROR，沒有 `message="stream error"` | V1 時期的 stderr 判斷（1.19.3 SSE 靜默時從 stderr 抓上游錯誤）保留，不會在重試時提早誤判；V2 的錯誤以事件為主 |
| `OPENCODE_SERVER_PASSWORD` | 經環境變數設定，帳號固定 opencode | 每次啟動自設隨機密碼 |

## 真實回合

```bash
# V2 為 npm install -g --prefix V2 @opencode/cli@2.3.2 的目錄；WORK 為空目錄
bash docs/issues/issue-0044/scripts/local-v2-actual.sh "$V2" "$WORK"
```

[scripts/local-v2-actual.sh](scripts/local-v2-actual.sh) 執行 `crates/wukong-gateway/tests/local_v2_actual.rs`（`#[ignore]`）。依序跑以下步驟，每步之後掃 `/proc` 確認沒有命令列含 V2 執行檔路徑的程序：

1. 請模型執行 `echo probe` 後回覆 DONE：回覆 `DONE`，記錄 session。串流有 StepStart 與 `shell` 工具（`command: echo probe`）。回合中 serve 程序存在，其 `/proc/<pid>/environ` 的密碼（64 字元）不在命令列裡
2. 以同一個 session 問上一則回覆的字：回覆 `DONE`，session 相同
3. 帶 `secret.txt` 附件（file:// URI）：回覆含 `PINEAPPLE42`
4. 指定模型 `opencode/deepseek-v4.1-flash-free`（預設為 big-pickle）：回覆 `MODEL`
5. `run_ephemeral`：回覆 `HELPER`，不回傳 session，列表筆數不變
6. 壓縮與刪除第 1 步的 session：都成功，列表不再有它

結果：1 passed（45 秒），結束後殘留程序 0 個。第一次執行指定的是 `opencode/deepseek-v4-flash-free`，V2 回 `provider.no-route`，Wukong 正確分類為「模型已下架」（`UpstreamFailed { kind: ModelEol }`）；改用 V2 列出的免費模型後通過。

## 經 wukong CLI 的多棒回合

`WUKONG_AGENT_CMD="<V2>/bin/opencode run"`、隔離的 HOME、記憶庫與 workspace，`--scope project:multi`：

1. 「先列出目前資料夾有哪些檔案，再建立 hello.txt 並寫入一行 hi，最後讀回 hello.txt 確認內容。」：rc=0，93 秒
   - 規劃出 Explorer → Fixer 兩棒，串流顯示思考與工具（`shell`、`read`）
   - `hello.txt` 內容為 `hi`
   - `agent_session_state` 記錄 session，turn_count=1
2. 「上一則我請你建立的檔案叫什麼名字？只回答檔名。」：rc=0，回覆 `hello.txt`，同一個 session，turn_count=2
3. 兩回合結束後殘留程序 0 個

### 改動前（同日，`target/debug/wukong` 還沒重建時）

同一個指令在 Task 2.4 改動前的建置上，等不到 V1 的啟動字串，到外部 timeout 都沒有輸出：多棒回合 500 秒，單棒「Reply with exactly the word PONG.」150 秒。外部 `timeout` 以 SIGTERM 結束 wukong 後，各殘留 1 個 `opencode serve`（已依 PID 確認 HOME 為探測目錄後停掉）。原因是 serve 跑在自己的 process group（`process_group(0)`），wukong 被訊號結束時 Drop 不會執行，V1 時就是這樣，不在本 Task 範圍。
