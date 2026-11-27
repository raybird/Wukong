# Wukong Runtime Guidelines

你在 Wukong 框架下協助使用者開發此專案，底層由 `opencode` 驅動。人格與派工規則由 Wukong 管理的 agent 定義提供（`.opencode/agents/`），這份檔案只放專案規則，主要助手與各個 subagent 都會讀到。

## 🛠️ 運行準則與工具呼叫

1. **誰動手**：主要助手（agent `wukong`）可以讀檔與搜尋，但不能改檔，終端機也只能執行排程與長期記憶指令；建立或修改檔案、編譯、測試（例如 `cargo test`、`npm test`、`go test`）與其他指令，都派給 subagent 執行，不要自己先試。subagent 在 CWD（目前工作區目錄）內直接使用檔案與命令工具。
2. **安全修改與重構**：修改任何程式碼之前，先讀取相關檔案以防產生破壞性修改；修改後執行測試，確保沒有破壞現有功能。派工時把這兩件事寫進交代內容。

## 🧠 記憶與歷史整合
* 每次執行，Wukong 會在 `[相關記憶]` 區塊中提供與當前情境高度關聯的歷史決策或事件，請將其做為核心 context 納入考量。

### 兩套記憶不要混用

這個 runtime 可能同時存在兩套彼此獨立的記憶系統，資料不共用、不同步：

| | Wukong 記憶 | Memoria |
|---|---|---|
| 誰寫入 | Wukong 每回合自動 recall / remember | 你主動下 `memoria` 指令 |
| 你看到的形式 | `[相關記憶]` 區塊 | 你自己執行指令的輸出 |
| 內容 | 本 runtime 的對話與事件 | 跨 session 的決策、repo 狀態、技能效用 |

* `[相關記憶]` 是 Wukong 自己的記憶，**不需要**也不應該再用 `memoria` 去查一次。
* 主要助手的說明裡有「長期記憶能力」時才有 Memoria 這一層（subagent 可用 `command -v memoria` 確認）；沒有就是這套部署沒開，不要嘗試安裝。
* 有的時候：需要跨 session 的長期決策脈絡、或要記錄一個未來 session 也該知道的結論。
* 用完 `memoria recall` 請回報 `memoria feedback <recall_id> --score <0..1>`，那是效用訊號的唯一來源。
* 語意召回要**明確加上** `--mode vector`；不加的話走的是字面比對，不會用到語意層。

## 🌐 網路資訊檢索能力

Docker runtime 可能已預裝 `agent-reach` 與 `gh`，用來擴充即時網路資訊檢索能力。當使用者要求最新資訊、閱讀網頁、查 GitHub repository/issue、整理 YouTube/RSS/社群平台內容、或進行全網調研時，不要只依賴模型記憶，應先檢查可用工具。這些指令由 subagent 執行：主要助手派工時寫明要用哪個工具。

- 使用 `agent-reach doctor` 檢查目前 Agent Reach channel 狀態。
- 若尚未初始化，先向使用者說明需要一次性設定，並建議在互動式 CLI runtime 執行：`docker compose run --rm wukong agent-reach install --env=auto`。
- 需要登入、Cookie、Token 或平台帳號的 channel，必須先取得使用者明確同意，並提醒憑證會保存在 Docker volume 中。
- GitHub 查詢與操作優先使用 `gh`；若尚未登入，建議使用 `docker compose run --rm wukong gh auth login` 完成互動式認證。
- 若 Agent Reach 安裝或 opencode MCP 設定有變更，提醒使用者重啟 Web/Telegram/Scheduler 等常駐服務。
