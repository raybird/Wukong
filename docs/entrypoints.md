# 各進入點:Telegram / Web Console / Session 控制

> ← 回到 [主 README](../README.md)｜相關文件:[CLI 參考](cli-reference.md)、[Docker 部署](docker.md)

## opencode session 控制

- **Session 接續**：預設以**每 scope 持久的 opencode session** 接續對話（本機／server 以 session API，純 CLI 以 `-s <id>`），並預設帶入 `--thinking` 思考過程。
- **清除上下文 (`/new`)**：在 REPL、Telegram 或 Web 輸入 `/new` 可以清空該 scope 的 session 以開啟全新對話；一次性 CLI 則可使用 `wukong --new "…"`。
- **會話壓縮 (`/compact`)**：支援將 `/compact` passthrough 給當前 session（適用於 REPL、Telegram 與 Web）。
- **自動輪替（2026-11-03 起）**：session 開始超過 7 天時，下一個回合開始時換新的 session，這個回合另外帶上一回合的內容，舊 session 隨後刪除。2026-11-26 起 Wukong 不再依回合數自動 compaction（`WUKONG_SESSION_COMPACT_EVERY_TURNS` 不再讀取），壓縮交給 OpenCode 自己的自動壓縮；手動的 `/compact` 照舊。細節見 [docker.md](docker.md) 的「長期 session 的輪替」。
- **停用思考過程**：使用 `--no-thinking` 參數或設定環境變數 `WUKONG_THINKING=0` 可關閉思考過程顯示。
- **思考過程顯示效果**：
  - **REPL**：以 `💭` 符號即時印出思考內容。
  - **Telegram**：在狀態泡泡中即時更新顯示。
  - **Web**：以可折疊的「💭 思考過程」區塊呈現。
  - *注意：此功能僅在模型輸出明文推理時生效（例如 OpenAI 系推理模型的推理過程如為加密傳輸則無法顯示）。*

### Agent backend 與問答（2026-10-04）

預設 `WUKONG_AGENT_CMD="opencode run"` 且沒有非空 `WUKONG_AGENT_SERVER_URL` 時，每次執行啟動監聽 loopback 可用埠的 OpenCode 控制程序，收尾後退出。session 保存在 OpenCode 資料庫，可跨程序接續；CLI／REPL、Web 與 Telegram 都能回答或取消工具問答。

CLI 顯示選項編號，輸入編號或選項文字回答；多選用逗號分隔，允許自訂答案時可直接輸入文字。`/cancel` 或 stdin EOF 取消問題，`--no-stream` 也保留問答。REPL 的問題與對話共用同一個 stdin reader。

本機控制程序與顯式 server backend 共用事件轉接：即時顯示 reasoning／tool／step 活動，完整回答於回合結束時取回並顯示一次。任意自訂 CLI 命令或帶額外 run 旗標的命令沿用 subprocess backend，依命令的 JSON Text 事件輸出，沒有問題回覆控制通道。模型旗標由 Wukong 設定解析後隨每次請求傳入。

設定非空 `WUKONG_AGENT_SERVER_URL` 會選用既有 server adapter；預設本機模式不會建立永久背景 daemon。

## Telegram bot（選用）

`wukong-telegram` 將對話引擎無縫串接至 Telegram，其內部運作流程如下：
- **基本流程**：透過 Long-Polling 接收訊息 $\rightarrow$ 白名單過濾過後 $\rightarrow$ 依據對話群組指派獨立 Scope（`user:tg-<id>`） $\rightarrow$ 重用核心的 `run_turn` $\rightarrow$ 回覆答案。
- **即時狀態回饋**：執行期間會建立一個**單一狀態泡泡**（原地更新並保持 Typing 狀態；顯示正在處理的角色：悟空自己處理時是「悟空」，派工後換成「悟空·fixer」這類 subagent 名稱，subagent 都回報後拿掉角色、回到「🐵　思考中…」，2026-11-26 起，#45；回報後回到悟空自 2026-11-27 起），任務完成後該狀態泡泡會自動刪除並送出最終回答。私人聊天用 `sendMessageDraft` 草稿，群組退回一則狀態訊息；草稿不刷新約 60 秒會被 Telegram 收掉，所以超過 20 秒沒有更新就以目前內容重送。agent 提問時進度先收起、問題訊息留在最下方，等待回答期間的角色、思考與工具事件只累積、不顯示；回答送出成功後馬上在問題下方重新顯示「已收到回答，繼續處理…」，取消或逾時後顯示「繼續處理…」。思考內容放得下就全文顯示；整則超過約 4000 字（Telegram 上限 4096）時只留最後面放得下的部分、前面加「…」（#39）。
- **格式渲染**：最終答案會經由 `wukong-render` 渲染為 Telegram 支援的 HTML 格式（支援粗體、程式碼區塊、表格自動降級呈現）。清單每項各占一行，子清單每層縮 2 空白、符號依層級為 •、◦、▪，編號清單保留原號碼，項目裡的第二段與換行對齊該項文字；H1 為粗體加底線、其他標題為粗體；`<think>` 區塊連同內容不輸出（巢狀、表格內與沒有結尾的都算；程式碼裡的，以及 HTML 屬性值、註解、處理指令、CDATA、宣告裡的照常顯示）；巢狀引言攤平成一層；連結網址裡的 `"`、`<`、`>` 寫成 `%22`、`%3C`、`%3E`（Telegram 會無聲丟掉含這些字元的連結），網址跳脫後超過 2048 bytes 的連結改寫成「文字 (網址)」純文字；超過 4096 bytes 的回答切成多則，切點不落在實體中間。某一段 HTML 被 Telegram 拒收（400 `can't parse entities`）時，同一段改以純文字重送（拿掉標籤、連結寫成「文字 (網址)」），stderr 記一行 `Telegram 拒收 HTML（answer）` 警告；排程結果回送也一樣（警告標 `schedule`）。
- **上傳與接續操作**：支援 Telegram `document`／`photo`（單檔 25 MiB、每則最多 5 份）。第一次上傳會把文字與 file part 一起送入目前 OpenCode session；後續可直接文字追問。回覆先前的檔案訊息會重新帶入該附件；上傳新檔並回覆舊檔可比較兩份。
- **原檔、工作副本、回傳產物分離**：原始檔保存於 `.wukong/uploads`，OpenCode 實際操作 `.wukong/workfiles` 的副本；要求「修改後傳回」時，成品寫入該回合專屬 `.wukong/artifacts`，Wukong 再以 Telegram document 回傳。
- **權限互動**：本機／server backend 收到 OpenCode `permission.asked` 時會顯示「允許一次／本次工作階段總是允許／拒絕」按鈕；取消或逾時會拒絕該請求。
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
- **核心設計**：重用與 CLI 相同的 `run_turn` 引擎與記憶資料庫，透過 Server-Sent Events (SSE) 即時串流執行進度與渲染後的答案。
- **角色與產出（2026-11-26，#45）**：送出後先顯示「🐵 收到，思考中…」；悟空派工時 SSE 送出 `role` 事件，進度換成「🐵 悟空·fixer 思考中…」，subagent 回報時送出 `step` 事件，對話裡多一張「🔍 悟空·fixer 的產出」卡片；接著再送一次 `role`，內容是還沒回報的 subagent，都回報了就是空字串，進度回到「🐵 思考中…」（2026-11-27 起；Telegram 寫給 Web 的即時事件也一樣）。產出卡片在重新開啟對話時照樣看得到。技能偏好頁（`Skills` 分頁與 `/api/skills/*`）已移除：派工與技能由悟空和 OpenCode V2 自己決定，設定檔裡留著的 `planner_preferences` 照常載入但不使用。
- **前端實作**：採用原生 ES Modules 與自定義的 `<wukong-chat>` Custom Element（遵循 `raybird/plainvanillaweb` 核心慣例之 SafeHTML 設計）。
- **共享對話歷史**：Web、Telegram 與 Scheduler 共用 scope-based chat history；Web 對話頁可從來源選單切換 `Global`、`Project ...` 或 `Telegram <chat_id>`，查看同一份對話脈絡與排程推送紀錄。
- **模型設定**：Settings 的「預設模型」與排程頁每個對話排程的卡片，都用「先選 Provider，再選模型」的兩級選單。清單來自 `GET /api/models`：後端執行 `opencode models --standalone`，依 Provider 分組，快取 5 分鐘。所以只會列出容器裡已設定或有憑證的 Provider。清單取不到時退回手動輸入；已存的模型不在清單中時，照原值顯示並標示「目前的模型不在清單中。」。排程卡片的「改回預設」會清除該排程的模型（`PATCH /api/schedules/:id`，body `{"model": null}`）。
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
- `/providers`：執行 `opencode auth list --standalone` 並回傳輸出：已設定的 provider、憑證類型與是否已存，不印密鑰（2026-11-25 起；V2 沒有 `providers list`）。
- `/models`：執行 `opencode models --standalone` 並回傳輸出。這兩個命令都加 `--standalone`，用一次性的私有 server，不連也不啟動 V2 的背景 service。
- `/set_models <model>`：持久化全系統預設模型，後續 Web、Telegram、CLI turns 與沒有指定模型的排程都會套用；有指定模型的排程不受影響。

未知 slash command 不會自動 passthrough 給 opencode。

### 執行緒隔離與 Token 安全驗證

- **非 Send Future 隔離機制**：由於對話引擎 `run_turn` 產生的 Future 內含非 `Send` 屬性（因為 `AiBackend` 包含 dynamic 的 `FnMut` 串流回呼），無法在 Axum 的異步調度中直接執行。Web 後端在處理對話請求時，會透過 `std::thread::spawn` 獨立出作業系統實體執行緒，並在內部以 `current_thread` 執行器運行 `block_on(run_turn)`，隨後將進度透過安全通道（mpsc channel）以 SSE 方式回傳。
- **Token 動態置換驗證**：若配置了 `WUKONG_WEB_TOKEN`，伺服器端在載入內嵌的 `index.html` 時，會動態將 token 置換寫入 `window.WUKONG_TOKEN` 進行 SPA 端與 API 端的雙向安全比對，以防範未授權的瀏覽器訪問。
