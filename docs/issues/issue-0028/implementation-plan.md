# Issue 28 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)；現況見 README「已確認證據」。

## 設計

### A：本機模式保留期清理
- `RetentionPolicy` 區分「未設定」與「明確設定」。例如 `from_env` 另外回傳是否明確設定，或新增 `explicit: bool`；以不改動 server 模式行為為準，形式由實作時決定。
- `active_policy(backend, policy)`：
  - `Server`：維持原行為，未設定時用 30 天；
  - `Local`：只在明確設定、而且天數大於 0 時啟用；
  - `Cli`：維持停用。
- 停用時的啟動日誌要分得出原因：backend 不支援、本機模式但沒有明確設定、或天數為 0。
- 清理本身（`prune_opencode_sessions`、anchored 防護、每輪上限 500、5 分鐘預算）不改。
- 不做本機模式的 vacuum：entrypoint 只在 server 模式呼叫 `wukong opencode vacuum`，維持現狀。

### B：依 session 年齡輪替
- **取得建立時間**：在 `AiBackend` 加一個「查單一 session 的建立時間」的方法，預設回不支援。server backend 打 `GET /session/{id}` 讀 `time.created`（毫秒，步驟 1 (1)）；本機 backend 比照 `Operation::Delete` 啟動 serve 後呼叫 server backend 的同一個方法；CLI backend 沿用預設。
- **判斷位置**：`prepare_final_session_with_policy`。只在原本會做 compaction 的時點（`turn_count >= compact_every_turns`）才查年齡（TBD-3；SCN-010），而且在呼叫 `compact_session` 之前：
  - 超過門檻：回 `session_id: None, rotated_from: Some(old)`，不做 compaction（SCN-004）；
  - 未超過：照原本的 compaction（SCN-005）；
  - 查不到：照原本的 compaction、不輪替，記一行警告（SCN-006）。
  - 關閉 compaction（`compact_every_turns <= 0`）時沒有時點，也就不輪替。
- **門檻**：固定常數 7 天（TBD-1 定案前的暫定值），不開放設定。
- **刪除舊 session**：沿用 `run_turn` 既有的 `rotated_from` 路徑，取得新 session 後才刪。回合在取得新 session 前失敗時，scope 的指向不變（SCN-007；步驟 4 以測試確認既有路徑確實如此）。
- **上一回合注入**：`rotated_from.is_some()` 時，最後一棒的記憶也經過 `with_previous_turn`（沿用輔助棒的機制與 800 字上限）。

### C：live event 清理
- schedulerd 的維護 tick（`maintenance_ticks`）在記憶維護之外，呼叫 `history.prune_live_events_before(now - 86_400)`。
- 不受 `WUKONG_MEMORY_AUTO_MAINTENANCE` 影響（SCN-009）。失敗只記警告，不影響記憶維護。
- `--once` 模式是否也清：跟著記憶維護的 `--once` 路徑一起執行。

## 實作步驟

1. ✅ **首要驗證：opencode session 建立時間的契約探測**（README 首要驗證；SCN-004～006 的前提，也涵蓋 SCN-001 的列表）
   - 產出：本步驟下方的探測紀錄表，內容包括 opencode 版本、命令與實際回應。
   - 做法：拋棄式容器（`ghcr.io/raybird/wukong:v0.24.0`，opencode 1.19.4）與臨時資料目錄，不碰 RunWuKong 的 volume。
     - (1) 建立 session 後，`GET /session/{id}` 與 `GET /session?roots=true&limit=10000` 是否都有 `time.created`、單位為何（與當下 epoch 比對）；
     - (2) 對 session 執行 compaction（`POST /session/{id}/summarize`，與 `compact_session` 相同的請求），前後的 `time.created` 是否不變；
     - (3) 建立超過 100 個 session，經由 `LocalProcessBackend::list_sessions`（例如 debug binary 的 `wukong opencode prune --dry-run`）列出時，最舊的仍會出現。
   - 相依：無。
   - 完成判準：三項各有明確結論與原始回應摘錄；B 的取得方式依此選定並寫回「設計」。任何一項不符預期時，先停下來回報，不進入步驟 4。
   - 結果（2026-11-03，`ghcr.io/raybird/wukong:v0.24.0`，`/global/health` 回 `{"healthy":true,"version":"1.19.4"}`；拋棄式容器 `wk28-probe`，資料放在 tmpfs `/probe`，對外只開 `127.0.0.1:14096`）：

     | 項目 | 命令 | 實際回應 | 結論 |
     |---|---|---|---|
     | (1) 建立時間的欄位與單位 | `POST /session`（前一刻 `date +%s%3N` = `1793677006556`），接著 `GET /session/{id}` 與 `GET /session?roots=true&limit=10000` | 三者都帶 `"time":{"created":1793677006691,"updated":1793677006691}` | `time.created` 是毫秒 epoch，單一 session 與清單都有 |
     | (2) compaction 後是否不變 | 先送一則訊息（`opencode/big-pickle`），再 `POST /session/{id}/summarize` `{"providerID":"opencode","modelID":"big-pickle","auto":false}`（與 `compact_session` 相同的請求） | 回 `true`；之後 id 不變，`created` 仍是 `1793677006691`，`updated` 從 `1793677020291` 變成 `1793677027641` | compaction 不會重設 `time.created`，也不會換 session id |
     | (3a) 列表超過 100 筆 | 再建 150 個 session，共 151 個 | `GET /session` → 100 筆；`?roots=true&limit=10000` → 151 筆，含最舊的那個 | W-6 仍成立；既有的列表參數可以列出全部 |
     | (3b) 經由本機 backend 列表 | 在容器內 `WUKONG_AGENT_SERVER_URL= wukong opencode prune --dry-run` | `error: backend error: agent command failed (code None): 目前只有 opencode server backend 支援列出 session。` | **不符預期**：本機 backend 沒有 `list_sessions`（README 證據已更正） |
     | 附帶：本機 serve 的啟動成本 | 容器內 `opencode serve --port 0`，量到印出 `listening` 與第一次 `/global/health` 回應，共 3 次 | listening 664／613／664 ms，health 680／627／674 ms | 本機模式每多一次 opencode 操作，約多 0.7 秒 |

   - 對設計的影響：
     - A 要替 `LocalProcessBackend` 補上列出 session：比照 `Operation::Delete`，啟動本機 serve 後呼叫 `OpencodeServerBackend::list_sessions`。
     - B：建立時間用 `time.created`（毫秒）。本機模式每次查年齡都要多啟動一次 serve（約 0.7 秒）；2026-11-03 使用者決定只在 compaction 時點查（TBD-3），設計已改寫。

2. ✅ **A：本機模式保留期清理**（SCN-001、SCN-002、SCN-003）
   - 產出：
     - `active_policy` 的單元測試：三種 backend × 未設定／明確設定／0 天，期望值手寫；
     - 啟動日誌的停用原因；
     - 經由本機 backend 實際清理一輪的整合證據：沿用 `crates/wukong-schedulerd/tests/retention_daemon.rs` 的做法，或以 debug binary 加上縮短間隔的測試鉤子，對拋棄式 opencode 跑一次。
   - 相依：步驟 1 的 (3)。
   - 完成判準：先取得紅燈（Local backend 加明確設定時回 `None`），實作後轉綠。變異確認：「Local 不看是否明確設定」與「Server 也要求明確設定」都會讓測試紅燈。
   - 結果（2026-11-03，分支 `issue-0028-opencode-growth`）：
     - 實作：
       - `wukong_runtime::session_retention::retention_days_configured()`：只看環境變數有沒有設定；
       - `active_policy(backend, policy, configured)` 改回 `Result<RetentionPolicy, &str>`，`Err` 是寫進啟動日誌的停用原因；
       - `LocalProcessBackend` 新增 `Operation::List`，在按需啟動的 serve 上呼叫 `OpencodeServerBackend::list_sessions`。`execute` 改回傳 `Outcome`（一般回應或 session 清單）。
     - 影響分析（GitNexus）：`active_policy` 的直接呼叫端只有 schedulerd `run`，LOW；`LocalProcessBackend::execute` 是私有方法，呼叫端是同檔的 6 個 trait 方法，LOW。
     - 測試契約變更：原本的 `only_a_server_backend_with_retention_enabled_is_active` 改寫成 `which_backends_and_settings_turn_retention_on`。理由是 SCN-001、SCN-002 核准的新行為改變了簽章與停用原因。舊測試的三個斷言（server 30 天啟用、server 0 天停用、CLI 停用）都保留在新測試裡。
     - 紅燈：
       - `cargo test -p wukong-schedulerd --bin wukong-schedulerd which_backends`：`session_retention.rs:154`，`left: Err("needs the opencode server backend …") right: Ok(RetentionPolicy { retention_days: 30 })`，也就是本機模式明確設定時仍被停用；
       - `cargo test -p wukong-gateway --lib lists_sessions_through`：`Err(AgentFailed { stderr: "目前只有 opencode server backend 支援列出 session。" })`。
     - 綠燈：上面兩支轉綠。`cargo test --locked -p wukong-gateway -p wukong-runtime -p wukong-schedulerd` 全過（gateway lib 141、runtime 86、schedulerd bin 24，含 `retention_daemon` 整合測試）。`cargo clippy --all-targets --locked -p wukong-gateway -p wukong-runtime -p wukong-schedulerd -- -D warnings` 乾淨。
     - 變異（還原後 `git diff` 確認）：
       - M1 本機模式不看是否明確設定：`session_retention.rs:168` 紅燈；
       - M2 server 也要求明確設定：`:164` 紅燈。
     - 真實 opencode 整合（SCN-001、SCN-002）：
       - 環境：從 v0.24.0 映像拷出的 opencode 1.19.4（主機 glibc 2.43 編出的 binary 跑不了映像的 bookworm glibc 2.36，所以反過來在主機上跑）。`HOME`／`XDG_*` 全部指到 scratchpad 的沙盒，不碰使用者自己的 opencode 資料。
       - 資料：沙盒的 opencode.db 有 3 個 session，其中兩個用 sqlite3 把 `time_created`／`time_updated` 改成 40 天前；`agent_sessions` 讓 `user:tg-1` 指向其中一個舊的。
       - 執行：debug `wukong-schedulerd`，`WUKONG_AGENT_SERVER_URL=`、`WUKONG_AGENT_CMD="opencode run"`、`WUKONG_TEST_OPENCODE_RETENTION_INTERVAL_SECS=2`、`WUKONG_MEMORY_AUTO_MAINTENANCE=0`。
       - 未設定天數，跑 12 秒：日誌 `opencode session retention disabled (the on-demand local backend only cleans up when WUKONG_OPENCODE_SESSION_RETENTION_DAYS is explicitly set)`，3 個 session 都還在。
       - `WUKONG_OPENCODE_SESSION_RETENTION_DAYS=30`，跑 30 秒：日誌 `opencode session retention enabled retention_days=30 …`；第一輪 `listed=3 … protected=1 stale_protected=1 expired=1 deleted=1 failed=0`，之後每輪 `listed=2 … deleted=0`。結束後只剩被指向的舊 session 和新的那個；沒有殘留的 opencode 行程。
     - 觀察：本機模式每刪一個 session 都要啟動一次 serve（步驟 1 量到約 0.7 秒）。每輪上限 500 個、時間預算 5 分鐘，所以舊部署第一次清理時，一輪大約只刪得完 400 個左右，其餘留到 6 小時後的下一輪（TBD-4）。

3. ✅ **C：live event 清理**（SCN-008、SCN-009）
   - 產出：schedulerd 測試，資料含 24 小時前後各一筆事件（時間用字面值）。執行一次維護後，逐筆確認刪除與保留。
   - 相依：無。
   - 完成判準：先取得紅燈（維護後舊事件仍在），實作後轉綠。變異：邊界改成 `<=`、改用 `now - 7 天`，測試都要紅燈。
   - 結果（2026-11-03）：
     - 實作：`run_memory_maintenance` 改為接收 chat history、維護設定與現在時間（原本自己讀 `from_env()`，改由兩個呼叫端傳入，測試才能指定「關閉記憶維護」）。在記憶維護之前先執行 `prune_live_events_before(now - LIVE_EVENT_RETENTION_SECS)`，固定 24 小時；失敗只記警告。影響分析：直接呼叫端只有 `run`（常駐的維護 tick 與 `--once`），LOW。
     - 單迴圈合併：SCN-008、SCN-009 的可觀察結果就是 schedulerd 維護一次之後資料庫剩下什麼，測試用真的 SQLite 與真的 `ChatHistoryStore`，沒有另一層整合責任。daemon 迴圈有沒有接上，另外用真實 daemon 確認（見下）。
     - 紅燈：`cargo test -p wukong-schedulerd --bin wukong-schedulerd maintenance_prunes`：`left: ["older than a day", "exactly a day", "recent"] right: ["exactly a day", "recent"]`。
     - 綠燈：該測試轉綠；`cargo test --locked -p wukong-schedulerd` 全過（bin 25、retention_daemon 2）；clippy 乾淨。
     - 變異（還原後確認）：改成 7 天、邊界多 1 秒（等於 `<=`）、被 `policy.enabled` 擋住（SCN-009），三者都在 `main.rs:432` 紅燈。
     - 真實 daemon：在步驟 2 的沙盒記憶庫放 `old`（90,000 秒前）與 `new`（60 秒前）兩筆事件，以 `WUKONG_MEMORY_AUTO_MAINTENANCE=0` 跑 debug `wukong-schedulerd` 6 秒。日誌 `chat live events pruned deleted=1`，剩下 `new`。

4. ✅ **B：依年齡輪替與上一回合注入**（SCN-004～SCN-007、SCN-010）
   - 產出：
     - `session.rs` 測試：假 backend 回傳不同的建立時間（門檻前後各一、查詢失敗），在 compaction 時點前後各跑一次，斷言 `session_id`／`rotated_from`、是否呼叫 compaction、未到時點時沒有查年齡；
     - `turn.rs` 測試：輪替回合最後一棒的 prompt 含上一回合，未輪替時不含；取得新 session 後舊的被刪除；
     - 真實 opencode 的端到端：拋棄式容器中，`WUKONG_SESSION_COMPACT_EVERY_TURNS=1`、同一個 scope 連跑兩回合。session 的建立時間無法透過 API 改寫，所以用 debug 建置的測試鉤子縮短門檻，或直接改複本 `opencode.db` 的 `time_created`，方法在實作時決定並記錄。觀察 `session_rotated` 日誌、scope 指向的變化，以及舊 session 從清單消失。
   - 相依：步驟 1。
   - 完成判準：紅綠重構證據齊全，斷言用字面值的時間與 session id。變異：門檻比較方向顛倒、單位錯一千倍、輪替時沒有注入上一回合、查詢失敗時當成過期、每回合都查年齡，都要紅燈。
   - 結果（2026-11-03）：
     - 影響分析（GitNexus）：`prepare_final_session_with_policy` 判為 CRITICAL（每個回合都經過，四個入口都用到）；`AiBackend` 判為 CRITICAL（30 個實作）。已先告知使用者並取得同意。新方法有預設實作，只有 server 與本機 backend 覆寫。
     - 實作：
       - gateway：`AiBackend::session_created_ms`（預設回不支援）；server 打 `GET /session/{id}` 讀 `time.created`，缺少或不是整數就回錯誤；本機 backend 新增 `Operation::CreatedAt`；`AgentBackend` 分派。
       - runtime：`prepare_final_session_with_policy` 在 compaction 之前，以 `hold_session_lease` 包住年齡查詢。超過 `ROTATE_AFTER_MS`（7 天）就回 `rotated_from` 並記 `session_rotation_due`；查不到就記 `warning: session_age_unknown`，照原本 compaction。`run_turn` 在 `rotated_from` 有值時，最後一棒改用 `helper_hits`（帶上一回合）。
     - 紅燈：
       - gateway `cargo test -p wukong-gateway --lib session_created`：兩支都拿到 `stderr: "目前只有 opencode server API 支援查詢 session 建立時間。"`；
       - runtime `session::tests`：`an_old_session_rotates_instead_of_compacting`、`an_unknown_age_falls_back_to_compaction`、`deciding_to_rotate_leaves_the_pointer_for_the_next_turn` 失敗；`a_young_session_is_compacted_as_before`、`the_age_is_not_queried_before_the_compaction_point` 本來就綠（既有行為，作為回歸網）；
       - runtime `turn::tests`：第一次執行三支都紅，但 SCN-005、SCN-007 是紅在「完全沒有刪除任何 session」——planner 的 helper session 本來就會被刪，是測試假設錯誤。改成只檢查 `ses_old`（與既有的 `unsupported_compaction_rotates_after_new_session_succeeds` 相同）之後，只剩 SCN-004 紅在 `turn.rs:1334`（最後一棒沒有 `User: 晚餐吃什麼`），是目標行為。
     - 綠燈：gateway lib 143、runtime lib 94 全過。
     - 變異（`mut.py` 逐一套用、跑整個 runtime lib、還原）：
       - M1 門檻比較顛倒、M2 單位錯一千倍、M3 輪替時不帶上一回合、M4 查詢失敗當成過期、M5 每回合都查年齡，全部轉紅，而且紅的是對應的測試（例如 M3 只紅 `an_old_session_rotates_and_the_final_step_sees_the_previous_turn`，M5 紅 `the_age_is_not_queried_before_the_compaction_point`）。
       - 第一次跑時把兩個過濾字串傳給 `cargo test`，cargo 以參數錯誤結束，看起來像 5 個都紅，其實沒有跑任何測試。發現後改成解析真正失敗的測試名稱再跑一次，上面是第二次的結果。
     - 真實 opencode 端到端（SCN-004）：
       - 環境：opencode 1.19.4，`HOME`／`XDG_*` 指到 scratchpad 的沙盒，debug `wukong` CLI 加 `--scope project:e2e`，`WUKONG_SESSION_COMPACT_EVERY_TURNS=1`，模型是 opencode 預設的 `big-pickle`。
       - 第一回合「記住：我的代號是藍鯨。只要回覆「好」。」→ 回「好」，scope 指向 `ses_e5fe8af8…`，`turn_count=1`。
       - 用 sqlite3 把該 session 的 `time_created` 改成 8 天前（opencode 沒有 API 能改）。
       - 第二回合「我剛剛說我的代號是什麼？只回答代號。」→ 日誌依序出現 `session_rotation_due … created_ms=1792996059259`、`session_rotated old_session_id=ses_e5fe8af8… new_session_id=ses_e5fe860f…`、`session_deleted session_id=ses_e5fe8af8…`；回答「藍鯨」。opencode.db 只剩新 session，scope 指向它；沒有殘留的 opencode 行程。
     - 觀察：輪替後 `turn_count` 是 2，沒有歸零。原因是 `finish_agent_session_turn` 換 session 時只做 `turn_count + 1`（`crates/wukong-memory/src/store/mod.rs:632-650`，既有行為），所以下一回合仍在 compaction 時點，會對新 session 多查一次年齡、多做一次 compaction（TBD-5）。


5. ✅ **文件與全套驗證**
   - 產出：
     - `docs/docker.md`：保留期清理段落改寫本機模式條件，補上 B 的輪替與 C 的 live event 清理；
     - `AGENTS.md`「關鍵設計細節」：opencode session 保留期清理改寫本機模式條件，新增輪替；
     - `CHANGELOG.md` `[Unreleased]`。
   - 相依：步驟 2～4。
   - 完成判準：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` 全綠；沒有改 compose 與 `.env.example`，所以不需要 `scripts/test-docker-runtime.sh`（實作中若改到就要跑）。文件提到的每個行為都對應到步驟 2～4 的測試或探測（W-1）。
   - 結果（2026-11-03）：
     - 文件：`docs/docker.md`（本機模式條件、`WUKONG_SESSION_COMPACT_EVERY_TURNS` 說明、保留期清理依 backend 分列、新增「長期 session 的輪替」、本機模式不做 vacuum 的理由）、`docs/entrypoints.md`（自動輪替）、`AGENTS.md`「關鍵設計細節」（保留期清理的啟用條件、長期 session 的輪替、live event 清理）、`CHANGELOG.md` `[Unreleased]`。
     - 另改 `.env.example` 的保留天數說明（原本只寫「這個 opencode server」），所以加跑 `bash scripts/test-docker-runtime.sh`：`ok: all 37 WUKONG_* variables offered in .env.example reach a container`、`docker runtime persistence checks passed`。
     - 全套：
       - `cargo fmt --all -- --check` 通過；
       - `cargo clippy --all-targets --locked -- -D warnings`：第一次在新測試報 `MutexGuard` 跨 await（`turn.rs:1343`），調整斷言順序後乾淨；
       - `cargo test --workspace --locked` 全過，沒有任何 FAILED（gateway lib 143、runtime lib 94、schedulerd bin 25 等）。
     - W-1 對照：文件裡的每個行為都對應到測試或實測——
       - 本機模式的啟用條件：步驟 2 的單元測試與沙盒 daemon；
       - 約 0.7 秒：步驟 1 的量測；
       - 輪替、帶上一回合、查不到時照原本 compaction：步驟 4 的單元測試與端到端；
       - 24 小時：步驟 3；
       - `turn_count` 不歸零：步驟 4 的觀察。

### 步驟 1 附錄：正式主機的唯讀量測（TBD-1）
在正式主機上，對 `opencode.db` 的**複本**執行（不要對運行中的檔案寫入）：

```sql
-- 每個根 session 的大小與年齡
SELECT s.id, s.time_created, s.time_updated,
       (SELECT COUNT(*) FROM part p WHERE p.session_id = s.id) AS parts,
       (SELECT COUNT(*) FROM event e WHERE e.session_id = s.id) AS events
FROM session s WHERE s.parent_id IS NULL
ORDER BY parts DESC LIMIT 30;
```

opencode 的實際欄位名稱以複本的 `.schema` 為準，上面的欄位名稱是推測。另外記錄 `ls -l opencode.db*`，以及 `memory.db` 中 `agent_session_state` 指向的 session 清單。

## 使用方式對照
| 情境 | 變更前 | 變更後 |
|---|---|---|
| 容器部署（compose，本機模式） | schedulerd 日誌：`opencode session retention disabled (needs the opencode server backend …)` | `opencode session retention enabled retention_days=30 …` |
| 想停用 | — | `.env` 設 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS=0` |
| 預覽 | `wukong opencode prune --dry-run`（只有 server 模式有意義） | 本機模式也可預覽 |

## 測試策略
| 驗收 | 層級 | 說明 |
|---|---|---|
| SCN-001～003 | 單元（`active_policy`）加一次整合（真實 opencode 上清一輪） | 決策邏輯用單元測試；實際刪除需要真實 opencode，因為 W-6 這類行為假 server 測不到 |
| SCN-004、005、010 | 單元（session.rs、turn.rs）加真實 opencode 端到端 | 外部契約由步驟 1 處理，單元測試只驗決策與注入 |
| SCN-006、007 | 單元 | 假 backend 模擬查詢失敗與回合失敗 |
| SCN-008、009 | schedulerd 單元（真 SQLite） | 同層，可合併單迴圈，理由記在步驟 3 |

## 檢查清單
- [x] 步驟 1 三項探測都有結論
- [ ] TBD-1 已有量測，或在 PR 揭露以 7 天交付
- [x] 每個 Scenario 都有紅綠重構證據
- [x] 常青文件已更新
