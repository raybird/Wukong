# 各進入點:Telegram / Web Console / Session 控制

> ← 回到 [主 README](../README.md)｜相關文件:[CLI 參考](cli-reference.md)、[Docker 部署](docker.md)

## opencode session 控制

- **Session 接續**：預設以**每 scope 持久的 opencode session** 接續對話（透過 `-s <id>` 顯式指定並從 JSON 擷取），並預設帶入 `--thinking` 思考過程。
- **清除上下文 (`/new`)**：在 REPL、Telegram 或 Web 輸入 `/new` 可以清空該 scope 的 session 以開啟全新對話；一次性 CLI 則可使用 `wukong --new "…"`。
- **會話壓縮 (`/compact`)**：支援將 `/compact` passthrough 給當前 session（適用於 REPL、Telegram 與 Web）。
- **停用思考過程**：使用 `--no-thinking` 參數或設定環境變數 `WUKONG_THINKING=0` 可關閉思考過程顯示。
- **思考過程顯示效果**：
  - **REPL**：以 `💭` 符號即時印出思考內容。
  - **Telegram**：在狀態泡泡中即時更新顯示。
  - **Web**：以可折疊的「💭 思考過程」區塊呈現。
  - *注意：此功能僅在模型輸出明文推理時生效（例如 OpenAI 系推理模型的推理過程如為加密傳輸則無法顯示）。*

### 兩種 agent backend 的串流行為差異

Wukong 有兩條底層 agent 路徑，回答文字的串流方式**刻意不同**：

- **CLI backend（`opencode run`，預設）**：逐字（token）串流回答文字，`StreamEvent::Text` 即時吐出。
- **Server backend（`opencode serve`，設 `WUKONG_AGENT_SERVER_URL`）**：**不**串流回答文字的增量；只即時串流 `reasoning`／`tool`／`step` 活動。最終回答文字在該回合結束時，經 `list_messages` 一次性取回（`opencode_server.rs::extract_latest_assistant_text`）。這是刻意設計——server 事件流的 `text` part 若也逐段吐出，會與收尾的整段抓取**重複渲染**。

因此在 server backend 下，使用者會即時看到「思考過程／工具活動」，但**完整答案於收尾一次顯示**（非逐字浮現）。此為預期行為，非缺陷。

## Telegram bot（選用）

`wukong-telegram` 將對話引擎無縫串接至 Telegram，其內部運作流程如下：
- **基本流程**：透過 Long-Polling 接收訊息 $\rightarrow$ 白名單過濾過後 $\rightarrow$ 依據對話群組指派獨立 Scope（`user:tg-<id>`） $\rightarrow$ 重用核心的 `run_turn` $\rightarrow$ 回覆答案。
- **即時狀態回饋**：執行期間會建立一個**單一狀態泡泡**（原地隨調度角色即時更新狀態並保持 Typing 狀態），任務完成後該狀態泡泡會自動刪除並送出最終回答。
- **格式渲染**：最終答案會經由 `wukong-render` 渲染為 Telegram 支援的 HTML 格式（支援粗體、程式碼區塊、表格自動降級呈現）。
- **上傳與接續操作**：支援 Telegram `document`／`photo`（單檔 25 MiB、每則最多 5 份）。第一次上傳會把文字與 file part 一起送入目前 OpenCode session；後續可直接文字追問。回覆先前的檔案訊息會重新帶入該附件；上傳新檔並回覆舊檔可比較兩份。
- **原檔、工作副本、回傳產物分離**：原始檔保存於 `.wukong/uploads`，OpenCode 實際操作 `.wukong/workfiles` 的副本；要求「修改後傳回」時，成品寫入該回合專屬 `.wukong/artifacts`，Wukong 再以 Telegram document 回傳。
- **權限互動**：server backend 收到 OpenCode `permission.asked` 時會顯示「允許一次／本次工作階段總是允許／拒絕」按鈕；取消或逾時會拒絕該請求。
- **建立並回送排程**：可直接用自然語言請助手建立定時任務（見 [CLI 參考 — 用自然語言建立排程](cli-reference.md#用自然語言建立排程telegram--對話)）；之後 `wukong-schedulerd` 觸發時，會把該回合結果主動推回原聊天室。傳輸層由共用的 `wukong-tg-client` crate 提供，daemon 與 bot 共用同一個 `WUKONG_TG_TOKEN`。

沒有共享檔案系統的遠端 `opencode serve` 可設 `WUKONG_AGENT_SERVER_FILE_MODE=inline`，附件會以 data URL 傳送且單檔限制 10 MiB；遠端 inline 模式無法由 OpenCode 寫回本機 artifact 目錄，因此不提供自動檔案回傳。

```bash
export WUKONG_TG_TOKEN="<BotFather token>"
export WUKONG_TG_ALLOWED="<你的 chat id>"   # 空 = 忽略所有訊息(安全預設)
cargo run -p wukong-telegram
```

`/指令` 目前回「尚未支援」，但已預留分派接縫，未來可輕鬆擴充 `/reset`、`/compact`、`/model` 等功能。詳見 [`crates/wukong-telegram/README.md`](../crates/wukong-telegram/README.md)。

## Web Console（選用）

`wukong-web` 提供了零建置、隨開即用的瀏覽器進入點：
- **核心設計**：重用與 CLI 相同的 `run_turn` 引擎與記憶資料庫，透過 Server-Sent Events (SSE) 即時串流專家角色的執行進度與渲染後的答案。
- **前端實作**：採用原生 ES Modules 與自定義的 `<wukong-chat>` Custom Element（遵循 `raybird/plainvanillaweb` 核心慣例之 SafeHTML 設計）。
- **共享對話歷史**：Web、Telegram 與 Scheduler 共用 scope-based chat history；Web 對話頁可從來源選單切換 `Global`、`Project ...` 或 `Telegram <chat_id>`，查看同一份對話脈絡與排程推送紀錄。
- **打包部署**：所有靜態資源由 Axum 透過 `include_str!` 巨集直接內嵌於 binary 中，單一執行檔即自帶完整前端，無需額外外部部署。

```bash
WUKONG_AGENT_CMD="opencode run" cargo run -p wukong-web
# 然後開 http://127.0.0.1:8787/
```

環境變數:

- `WUKONG_WEB_HOST`(預設 `127.0.0.1`)、`WUKONG_WEB_PORT`(預設 `8787`)
- `WUKONG_WEB_TOKEN`(選用;設了則 UI 與 `/chat` 都需帶 token)
- `WUKONG_WEB_SCOPE`(預設 `global`)
- 重用:`WUKONG_MEMORY_DB`、`WUKONG_AGENT_CMD`、`WUKONG_MD_DIR`、(feature `embed`)`WUKONG_EMBED`

安全預設:只綁 `127.0.0.1`;伺服器端 `wukong-render::to_web_html` 把原始 HTML 跳脫防 XSS。

### Chat control commands

CLI/REPL、Web 與 Telegram 共用一組 allowlist 控制指令：

- `/compact`：要求 opencode 壓縮目前 scope 的 stored session。
- `/providers`：執行 `opencode providers list` 並回傳輸出。
- `/models`：執行 `opencode models` 並回傳輸出。
- `/set_models <model>`：持久化全系統預設模型，後續 Web、Telegram、Scheduler 與 CLI turns 都會套用。

未知 slash command 不會自動 passthrough 給 opencode。

### 執行緒隔離與 Token 安全驗證

- **非 Send Future 隔離機制**：由於對話引擎 `run_turn` 產生的 Future 內含非 `Send` 屬性（因為 `AiBackend` 包含 dynamic 的 `FnMut` 串流回呼），無法在 Axum 的異步調度中直接執行。Web 後端在處理對話請求時，會透過 `std::thread::spawn` 獨立出作業系統實體執行緒，並在內部以 `current_thread` 執行器運行 `block_on(run_turn)`，隨後將進度透過安全通道（mpsc channel）以 SSE 方式回傳。
- **Token 動態置換驗證**：若配置了 `WUKONG_WEB_TOKEN`，伺服器端在載入內嵌的 `index.html` 時，會動態將 token 置換寫入 `window.WUKONG_TOKEN` 進行 SPA 端與 API 端的雙向安全比對，以防範未授權的瀏覽器訪問。
