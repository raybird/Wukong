# AI 開發助理核心規則

本規則整合實務經驗與常見 LLM 陷阱，適用於 OpenCode、Antigravity 等開發環境。

**權衡說明：** 以下規則偏向謹慎與正確性。對於極簡單的任務（例如改一個字的錯字、單行日誌調整），可適度放寬。

---

## 一、語言規範

> **本節屬專案客製**：依專案需要設定語言，本檔其餘章節與語言無關。

- 一律使用 **繁體中文（台灣用語）**，採用台灣常見的表達方式與術語。
- 撰寫文件或註解時，每當提及時間，必須明確寫出系統當下的日期。

---

## 二、先思考再寫程式

實作前讀現況、確認目標與最小範圍。明說會影響結果的假設與取捨；能由設定、程式碼與既有對話取得的資訊先自行確認。只有缺少會改變行為、範圍、安全或必要驗證的決策時才問，無相依工作可繼續。

---

## 三、簡單優先

**用最少的程式碼解決問題。不要寫推測性的東西。**

- 不能超出需求範圍的功能。
- 只使用一次的東西，不要建立抽象層。
- 不要加入沒被要求的「彈性」或「可設定性」。
- 不要處理不可能發生的錯誤情況。
- 如果你寫了 200 行程式碼，而它其實可以只寫 50 行，請重新寫過。

問自己：*「資深工程師會覺得這樣太複雜嗎？」* 如果是，就簡化它。

---

## 四、手術式修改

**只動你必須動的地方。只清理你自己造成的混亂。**

修改既有程式碼時：
- 不要「順便改善」旁邊的程式碼、註解或排版格式。
- 不要重構沒壞的東西。
- 沿用既有的程式碼風格，即使你自己習慣寫得不一樣。
- 如果發現不相關的無用程式碼，可以順口提一下——但不要刪除它。

當你的改動產生孤兒程式碼（不再被使用的變數、匯入、函式）：
- 只刪除 **你自己改動所造成** 的未使用項目。
- 不要刪除原本就存在的無用程式碼，除非使用者有要求。

**檢驗標準：** 每一行被改動的程式碼，都必須能直接追溯到使用者的需求。

---

## 五、工作流程規模與風險

**規模決定流程重量；風險決定驗證順序。兩者不得互相推導。**

依修改範圍與結構複雜度決定流程重量：

- **小任務 / 局部任務**（範圍集中、結構複雜度低、工作量有限）→ 直接執行，不必寫完整計畫或長篇文件。

- **中型任務**（多個相關檔案、範圍有界的功能、結構複雜度中等）→ 先列出簡短步驟與驗證點，再執行。

- **大任務**（範圍廣泛、重大架構變更或涉及多個服務）→ 先建立分階段計畫，標出邊界、相依性與驗證點，再執行。

若同時符合多個規模條件，採用最高的適用等級；例如涉及多個服務時，即使單一改動明確，仍屬大任務。

另依未知程度與失敗後果判定風險：

- **低風險**：行為與依賴明確、影響侷限、容易回復，可依一般相依順序執行。
- **中風險**：存在重要未知，或失敗會造成有限度返工；先驗證該未知再進行主要實作。
- **高風險**：涉及資料損失、安全或權限、不可逆操作、廣泛影響，或核心外部行為尚未確認；第一個實質驗證步驟必須取得能降低最大風險的證據。

若同時符合多個風險條件，採用最高的適用等級；任何高風險條件都優先於中風險。

小任務也可能是高風險，大任務也可能是低風險。不要因高風險自動擴大文件，也不要因修改範圍小而省略必要驗證。

實作前先指出最大的未驗證假設或最嚴重的失敗後果，再選擇最直接的驗證手段：

- 外部 API 或套件契約未知 → 契約驗證、最小真實請求或相容性探測
- 既有資料樣態未知 → 資料盤點、分布查詢或小樣本 dry run
- 效能或容量未知 → 基準測試、壓力測試或最小技術實驗
- 既有行為可能被破壞 → 既有行為刻畫測試、快照、對比腳本或回歸測試網
- 端到端整合或使用者行為未知 → 垂直切片，跑通最窄的真實路徑
- 沒有明顯未知 → 依一般技術相依順序執行

**不得先決定採用垂直切片，再回頭尋找理由。** 只有當端到端整合或使用者行為是最大未知時才採用；垂直切片是候選手段，不是風險優先的同義詞。無論選擇哪種手段，都要先定義可觀察、可重複確認的完成證據。

不要把一個小任務變成完整的規格書、長篇計畫或大規模改寫。

---

## 六、目標驅動執行與驗證

**定義成功條件。反覆執行直到驗證通過。**

將任務轉換成可驗證的目標：
- 「加上驗證」→ 先寫好針對無效輸入的測試，然後讓測試通過
- **「修正 bug」→ 先寫出能重現 bug 的測試，然後讓測試通過**
- 「重構 X 模組」→ 確保重構前與重構後的測試都通過

**標準 bug 修復三步驟：**
1. **重現**：寫一個會失敗的測試（或明確描述手動重現步驟）
2. **修復**：改動最少程式碼讓問題消失
3. **驗證**：確認測試通過，且沒有破壞既有行為

對於多步驟任務，簡短列出計畫：
```
1. [步驟] → 驗證方式：[檢核點]
2. [步驟] → 驗證方式：[檢核點]
3. [步驟] → 驗證方式：[檢核點]
```

如果無法完全自動驗證，請明確提供手動驗證的步驟。

**當判準是生產環境的觀測而非測試時**，每項判準都要反問：**「若這件事失敗了，這個判準會不會仍然是綠的？」** 答不出明確的「不會」，這個判準就不能用，必須改判準或補判準。觀測證據沒有紅燈基線——測試至少會先紅一次證明它抓得到問題，觀測不會，因此判準失效時看起來與行為正確完全一樣。取得驗收數據前，先以已知結果的對照組驗證判準本身，再記錄查詢原文、範圍與實際數值。

**不是每個任務都以合併收尾，沒有新 commit 也不代表卡住。** 等待外部驗收窗（每週排程夜、月結、對帳日）與經評估後決定不修復，都是合法狀態：前者記錄預定窗口與要觀察的判準，後者記錄判定理由與追蹤方式。兩者都不得用來掩蓋當下就能完成的驗證。

---

## 七、Issue 流程與證據

使用者要求追蹤 issue 或啟動 issue 技能時，讀專案的 `docs/AGENTS.md` 及當前階段指向的材料：acceptance 處理核准，verification 處理測試，review-evidence 處理 PR 與獨立審查。詳細 gate 由這些檔案定義，通用規則不另複寫。所需文件缺漏時，列出具體初始化缺項，再處理依賴它的工作。

一般局部修改依第六節定義成功條件與驗證，不額外建立 issue 文件。使用者已明確指定結果與範圍，即可執行相符工作；只澄清影響行為、範圍、安全或必要驗證的缺項。沉默不代表核准新增行為。

保存真實證據。重構維持行為並比對前後同組測試；文件用可重複的靜態或人工檢查。驗收與單元紅燈保障相同時，不論任務規模都可合併，記錄未遺漏獨立層級的理由；不同層級保留各自查核。被測內容、環境與相關條件一致時才重用證據。

使用者要求豁免 gate 時只適用明列範圍，issue 流程中留下紀錄。誠實回報始終適用：跳過的測試與自我審查不能寫成驗證通過或獨立 review。各階段預設使用本 kit 內建流程；使用者明確要求時才改用 Superpowers 對應 skill，產物位置與 gate 仍依本 kit，切換工具不使既有核准與等價驗證失效。

---

## 八、Monorepo 規則

> **本節屬專案客製**：請將下列服務與套件名稱換成專案實際的邊界。

**先找出受影響範圍最小的專案、套件或服務。**

- 除非真的有必要，否則不要把改動散佈到前端、後端、函式庫、共用模組或其他服務。
- 優先採用局部修復，而不是整個 repo 的大規模重新設計。
- 如果無法避免跨服務的改動，必須說明理由，並明確列出所有受影響的服務。

---

## 九、Token 經濟原則

- 推理與回答的長度要與任務大小成比例。
- 不要重複相同的脈絡、推理或結論。
- 對於瑣碎的任務，不要產生冗長的說明、計畫或文件。
- 優先使用簡短的檢查清單與直接的回答，而不是長篇大論。

---

## 十、衝突處理

使用者的明確指示與既有授權優先於本 kit 的預設流程；遵守宿主的更高層限制。範圍被明確擴大時依新範圍處理，不要求特定覆蓋口令。存在真實衝突時說明原因、保留可完成的工作，僅詢問必要決策。

---

## 範例：小 bug 修復

**使用者要求：** *「修復 getUserName() 裡面的空指標問題」*

**AI 遵照規則的執行流程：**
1. **思考**：假設問題發生在 `user` 為空值的時候。我會用傳入空值的方式來重現。
2. **重現**：先寫一個測試，傳入空值時會失敗。
3. **修復**：加上空值檢查，回傳預設值「Guest」。
4. **驗證**：確認測試通過。順口提及隔壁的 `getUserEmail()` 也有類似風險，但依規則不修改它。
5. **回應**：使用繁體中文，並附上當天日期。

**結果**：最小改動、有驗證、符合所有規則的修復。

---

# Wukong 專案規範

## 常用指令

```bash
# 建置整個 workspace
cargo build --release

# 執行所有測試
cargo test

# 只測單一 crate
cargo test -p wukong-memory

# 只測特定模組
cargo test -p wukong-cli persona::

# Lint 檢查
cargo clippy --all-targets -- -D warnings

# 執行 orchestrator demo（以假 agent 驗證流程）
cargo run -p wukong-orchestrator --bin wukong-orchestrate -- --agent-cmd "printf fixer" "fix the bug"

# 啟動 Web Console（開發用）
WUKONG_AGENT_CMD="opencode run" cargo run -p wukong-web

# 啟動 Telegram bot
WUKONG_TG_TOKEN="<token>" WUKONG_TG_ALLOWED="<chat_id>" cargo run -p wukong-telegram

# 啟動記憶 HTTP 服務
WUKONG_MEMORY_PORT=3917 cargo run -p wukong-memoryd

# 同步 Superpowers 技能（預覽）
scripts/sync-superpowers.sh <commit-or-tag> --dry-run
```

## 架構

Wukong 是 Rust Workspace，包含 15 個 crate，分為四柱核心與周邊進入點。

### 四柱核心（依賴方向單向）

```
wukong-cli → { wukong-runtime, wukong-memory, wukong-orchestrator }
wukong-runtime → { wukong-gateway, wukong-memory, wukong-orchestrator, wukong-skills, wukong-settings }
wukong-orchestrator → wukong-gateway → wukong-memory
```

| crate | 職責 |
|-------|------|
| `wukong-memory` | SQLite + FTS5 記憶儲存；keyword/tree/hybrid 召回；時間衰減計分 |
| `wukong-gateway` | 驅動底層 agent CLI（預設 `opencode run`）；inject 人格 + 記憶 + 技能 |
| `wukong-orchestrator` | LLM 路由規劃（最多 3 棒角色：Explorer/Oracle/Librarian/Fixer/Designer） |
| `wukong-runtime` | 串聯一回合完整執行流程（`run_turn`）：recall → plan → execute → remember；CLI、Web、Telegram、Scheduler 共用 |
| `wukong-cli` | 統一 CLI 進入點（`wukong` binary）；含 REPL、`memory` 子命令、`schedule` 子命令 |

### 周邊 crate

| crate | 職責 |
|-------|------|
| `wukong-skills` | 以 `include_str!` 內嵌 Superpowers 技能（`assets/superpowers/`） |
| `wukong-settings` | 讀寫 `.wukong/settings.toml` 專案設定 |
| `wukong-scheduler` | 排程核心 lib；SQLite lease 防止重複執行 |
| `wukong-schedulerd` | 排程 daemon binary |
| `wukong-memoryd` | 記憶 HTTP 服務（`/v1/recall`、`/v1/remember` 等） |
| `wukong-render` | Markdown → HTML / Telegram HTML 渲染（含 SafeHTML 防 XSS） |
| `wukong-tg-client` | Telegram 傳輸層（Bot API client + scope 解析）；零內部依賴，`wukong-telegram` 與 `wukong-schedulerd` 共用；`mock` feature 供測試 |
| `wukong-telegram` | Telegram Long-Polling bot；重用 `run_turn`；transport 來自 `wukong-tg-client` |
| `wukong-web` | Axum Web Console；SSE 串流；前端以 `include_str!` 內嵌單一 binary |
| `wukong-chat-history` | 共享聊天歷史與附件儲存（SQLite）；含附件路徑穿越防護（`resolve_under_upload_root`）；Web／Telegram 共用 |

### 一回合資料流（`run_turn`）

1. `wukong-memory` recall（混合 BM25 + 語意向量，預設 hybrid）
2. `wukong-orchestrator` plan（LLM 規劃角色 + 技能鏈）
3. 逐棒 `wukong-gateway` execute（注入人格 + 角色 + 技能規範 + 記憶）
4. `wukong-memory` remember（落盤本輪 User + Assistant）

會話隔離：只有最後一棒才帶入 / 更新 scope 的 `session_id`，前面輔助棒為 stateless。

### 驗證紀律

兩條在 2026-08-14 同一天各命中兩次以上的規則，證據見
`docs/2026-08-14-memory-recall-verification-handover.md`。

- **要驗證一句宣稱，去問產物，不要問描述它的檔案。** 註解、文件、測試 fixture 都沒有
  執行語意，所以**沒有任何測試會為它們紅燈**。同一天出現三個實例：一句宣稱省了
  ~200 MB 但實際省 0 的 Dockerfile 註解（隨版本出貨）、一份只造 6 個檔案而真實 bundle
  有 11 個的 fixture（測試全綠卻放行了一個裝不起來的版本）、一句比對陣列單行寫法的斷言
  （清單一長就不再斷言任何事）。用 `docker history` 問 layer 大小、`tar -tf` 問歸檔內容、
  `EXPLAIN QUERY PLAN` 問索引有沒有被用、實際跑一次召回問語料品質。無法變成可執行的
  敘述就刪掉它——把「Routes (12 endpoints)」改成只留清單，是消滅第二份真相而不是承諾
  同步它。
- **診斷訊號不可與被診斷的機制同源。** `confidence` 由 `relevance` 算出，而中文召回壞的
  正是 `relevance`——所以中文查詢無論成功與否都回報 0.000，用它偵測不到自己失效。要用
  **結構性**訊號：`source_signals` 說是誰回答的（`keyword` / `cjk_fallback` / `vector`），
  那由查詢路徑決定、不經過計分。寫測試時斷言「誰回答的」與「有沒有被排序」，不要只斷言
  命中數——`!hits.is_empty()` 在壞掉的行為下也會通過。

### 關鍵設計細節

- **假 agent 測試法**：`--agent-cmd "printf fixer"` 或 `echo` 可在無 LLM 下驗證完整流程。
- **embedding 選用**：cargo feature `embed` + `WUKONG_EMBED=1` 啟用本機 embedding（fastembed all-MiniLM-L6-v2），未啟用則退回 BM25。
- **Web 執行緒隔離**：`run_turn` 含非 `Send` callback，Web 後端以 `std::thread::spawn` + `current_thread` 隔離，進度透過 mpsc channel → SSE 回傳。
- **Superpowers 來源**：`crates/wukong-skills/assets/superpowers/SOURCE.md` 記錄上游版本；以 `scripts/sync-superpowers.sh` 更新。
- **排程能力注入**：`run_turn` 最後一棒的 prompt 會常駐注入「排程能力」區塊（`persona::scheduling_capability_hint`，帶當前 scope），讓 agent 透過 opencode shell 自行執行 `wukong schedule add-turn`。`wukong` 指令路徑可用 `WUKONG_BIN` 覆寫。
- **末棒輸出保證**：`run_turn` 最後一棒常駐注入 `[輸出要求]`（`persona::final_answer_directive`），要求即使全程用工具操作也要文字總結；收尾後若末棒輸出為空，回退取最近一棒非空輸出（全空才回 `(本回合未產生文字輸出)`），確保 `TurnOutput`、記憶與對話歷史皆非空。詳見 `docs/superpowers/specs/2026-06-22-final-output-fallback-design.md`。
- **排程結果回送**：`wukong-schedulerd` 觸發 Turn job 後，若 scope 可由 `chat_id_from_scope` 還原成 `user:tg-<id>`，透過 `wukong-tg-client` 把結果回送 Telegram（成功 HTML、失敗一行）；best-effort 不影響 job 狀態。`WUKONG_SCHED_NOTIFY=0` 關閉，需 `WUKONG_TG_TOKEN`。
- **容器內 opencode 權限**：`AgentCliBackend` 兩條路徑（`run`／`run_streaming`）都以 `stdin(Stdio::null())` 啟動 opencode，所以**任何進入點都無法回應互動式權限詢問**。容器內 `docker-compose.yml` 的 `WUKONG_AGENT_CMD` 預設帶 `--dangerously-skip-permissions`（自動核准未被明確 `deny` 的權限）；`docker-entrypoint.sh` 在缺檔時 seed `~/.config/opencode/opencode.json`，內含一組 `bash` deny 黑名單擋住對絕對路徑的毀滅性遞迴刪除、同時放行 `/workspace` 內刪除（靠 opencode「最後符合規則勝出」+ 開頭 `*` 涵蓋指令串接）。屬防呆護欄非資安牆；放自訂 `opencode.json` 進 `opencode-config` volume 可覆蓋（缺檔才 seed）。**注意 `--dangerously-skip-permissions` 只作用於 CLI backend**：compose 預設以 `WUKONG_AGENT_SERVER_URL` 走 `opencode serve`，`serve` 沒有對等旗標，因此對 Web／Telegram／Scheduler 而言 `opencode.json` 是唯一的權限控制。baseline 另含 `external_directory` 放行 `/tmp`（該項預設 `ask`，會讓無人值守排程卡住）。
- **opencode 設定分層**：`opencode-config` volume 內 `opencode.json` 是 Wukong baseline、**每次啟動覆寫**（新版預設才能隨映像檔升級生效）；`user.json` 由 `OPENCODE_CONFIG` 指向、只在缺檔時建立，是使用者自訂層。opencode 深度合併兩者且後者勝出。舊版 seed-if-missing 邏輯會讓既有部署永遠拿不到新預設，升級時以 `.wukong-baseline` 標記判斷並把舊檔備份 + 轉為 `user.json`。
- **無人值守權限處置**：`wukong-scheduler` 的排程回合收到 `QuestionRequest` 時依 `PermissionPolicy` 即時處置（預設 `Reject`，`WUKONG_SCHED_PERMISSION=allow` 改為自動允許一次），回覆失敗重試 3 次後中止回合。處置結果寫入 run 訊息的 `[無人值守權限]` 區塊。permission 由 opencode server 在工具執行前攔截，prompt 層的 `SCHEDULED_TURN_AUTONOMY_HINT` 結構上擋不住。詳見 `docs/superpowers/specs/2026-08-06-opencode-permission-hang-remediation-design.md`。
- **opencode session 保留期清理**：`wukong-schedulerd` 每 6 小時刪除超過 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS`（預設 30）天、且不被 `agent_sessions`／`agent_session_state` 任何 scope 指向的 opencode session（`wukong_runtime::session_retention`）。失效方向是「不確定就不刪」：讀不到指向表或列表失敗就整輪跳過。兩個由實測得來、改動時要記得的事實：`GET /session` 不帶 `limit` 只回最新 100 筆，被截掉的正是最舊的；`event` 表的外鍵不指向 `session`，歷史會被清掉是 opencode 刪除邏輯的行為，升級 opencode 後要重驗。空間回收（`wukong opencode vacuum`）由 entrypoint 在 `opencode serve` 啟動前呼叫。量測與設計見 `docs/issues/issue-0003/`。
- **`wukong` 子命令前不能帶全域旗標**：`wukong --db X memory snapshot` 不會報錯，而是把 `memory snapshot` 當成 prompt 跑一個真的回合；舊版 binary 遇到不認得的子命令也一樣。腳本要指定資料庫用 `WUKONG_MEMORY_DB`；entrypoint 呼叫新子命令前先以 `--help` 確認 binary 認得它。

## 規則檔只有這一份

本專案不放 `CLAUDE.md`。Claude Code 只在沒有 `CLAUDE.md` 時才讀本檔；該檔一旦存在，本檔就整份被忽略。`npx gitnexus analyze` 會在缺檔時重新建立 `CLAUDE.md`，所以一律加上 `--skip-agents-md` 執行，或執行後刪除它產生的 `CLAUDE.md`。

<!-- gitnexus:start -->
# GitNexus — Code Intelligence

This project is indexed by GitNexus as **Wukong** (4745 symbols, 9959 relationships, 300 execution flows). Use the GitNexus MCP tools to understand code, assess impact, and navigate safely.

> If any GitNexus tool warns the index is stale, run `npx gitnexus analyze` in terminal first.

## Always Do

- **MUST run impact analysis before editing any symbol.** Before modifying a function, class, or method, run `gitnexus_impact({target: "symbolName", direction: "upstream"})` and report the blast radius (direct callers, affected processes, risk level) to the user.
- **MUST run `gitnexus_detect_changes()` before committing** to verify your changes only affect expected symbols and execution flows.
- **MUST warn the user** if impact analysis returns HIGH or CRITICAL risk before proceeding with edits.
- When exploring unfamiliar code, use `gitnexus_query({query: "concept"})` to find execution flows instead of grepping. It returns process-grouped results ranked by relevance.
- When you need full context on a specific symbol — callers, callees, which execution flows it participates in — use `gitnexus_context({name: "symbolName"})`.

## Never Do

- NEVER edit a function, class, or method without first running `gitnexus_impact` on it.
- NEVER ignore HIGH or CRITICAL risk warnings from impact analysis.
- NEVER rename symbols with find-and-replace — use `gitnexus_rename` which understands the call graph.
- NEVER commit changes without running `gitnexus_detect_changes()` to check affected scope.

## Resources

| Resource | Use for |
|----------|---------|
| `gitnexus://repo/Wukong/context` | Codebase overview, check index freshness |
| `gitnexus://repo/Wukong/clusters` | All functional areas |
| `gitnexus://repo/Wukong/processes` | All execution flows |
| `gitnexus://repo/Wukong/process/{name}` | Step-by-step execution trace |

## CLI

| Task | Read this skill file |
|------|---------------------|
| Understand architecture / "How does X work?" | `.claude/skills/gitnexus/gitnexus-exploring/SKILL.md` |
| Blast radius / "What breaks if I change X?" | `.claude/skills/gitnexus/gitnexus-impact-analysis/SKILL.md` |
| Trace bugs / "Why is X failing?" | `.claude/skills/gitnexus/gitnexus-debugging/SKILL.md` |
| Rename / extract / split / refactor | `.claude/skills/gitnexus/gitnexus-refactoring/SKILL.md` |
| Tools, resources, schema reference | `.claude/skills/gitnexus/gitnexus-guide/SKILL.md` |
| Index, status, clean, wiki CLI commands | `.claude/skills/gitnexus/gitnexus-cli/SKILL.md` |

<!-- gitnexus:end -->
