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
- **取得建立時間**：依步驟 1 的探測結果決定。候選做法是在 `AiBackend` 加一個「查單一 session」的方法（server 與本機 backend 打 `GET /session/{id}`，CLI backend 回不支援）。如果探測發現清單的 `time.created` 已經足夠、而且查單一 session 不可行，再改採其他做法，並把選擇記在步驟 1。
- **判斷位置**：`prepare_final_session_with_policy`。取得 lease、確認有 `session_id` 之後先查年齡：
  - 超過門檻：回 `session_id: None, rotated_from: Some(old)`，不做 compaction；
  - 未超過：照原本的 compaction 規則；
  - 查不到：照原本的規則、不輪替，記一行警告（SCN-006）。
- **門檻**：固定常數 7 天（TBD-1 定案前的暫定值），不開放設定。
- **刪除舊 session**：沿用 `run_turn` 既有的 `rotated_from` 路徑，取得新 session 後才刪。回合在取得新 session 前失敗時，scope 的指向不變（SCN-007；步驟 4 以測試確認既有路徑確實如此）。
- **上一回合注入**：`rotated_from.is_some()` 時，最後一棒的記憶也經過 `with_previous_turn`（沿用輔助棒的機制與 800 字上限）。

### C：live event 清理
- schedulerd 的維護 tick（`maintenance_ticks`）在記憶維護之外，呼叫 `history.prune_live_events_before(now - 86_400)`。
- 不受 `WUKONG_MEMORY_AUTO_MAINTENANCE` 影響（SCN-009）。失敗只記警告，不影響記憶維護。
- `--once` 模式是否也清：跟著記憶維護的 `--once` 路徑一起執行。

## 實作步驟

1. 📝 **首要驗證：opencode session 建立時間的契約探測**（README 首要驗證；SCN-004～006 的前提，也涵蓋 SCN-001 的列表）
   - 產出：本步驟下方的探測紀錄表，內容包括 opencode 版本、命令與實際回應。
   - 做法：拋棄式容器（`ghcr.io/raybird/wukong:v0.24.0`，opencode 1.19.4）與臨時資料目錄，不碰 RunWuKong 的 volume。
     - (1) 建立 session 後，`GET /session/{id}` 與 `GET /session?roots=true&limit=10000` 是否都有 `time.created`、單位為何（與當下 epoch 比對）；
     - (2) 對 session 執行 compaction（`POST /session/{id}/summarize`，與 `compact_session` 相同的請求），前後的 `time.created` 是否不變；
     - (3) 建立超過 100 個 session，經由 `LocalProcessBackend::list_sessions`（例如 debug binary 的 `wukong opencode prune --dry-run`）列出時，最舊的仍會出現。
   - 相依：無。
   - 完成判準：三項各有明確結論與原始回應摘錄；B 的取得方式依此選定並寫回「設計」。任何一項不符預期時，先停下來回報，不進入步驟 4。

2. 📝 **A：本機模式保留期清理**（SCN-001、SCN-002、SCN-003）
   - 產出：
     - `active_policy` 的單元測試：三種 backend × 未設定／明確設定／0 天，期望值手寫；
     - 啟動日誌的停用原因；
     - 經由本機 backend 實際清理一輪的整合證據：沿用 `crates/wukong-schedulerd/tests/retention_daemon.rs` 的做法，或以 debug binary 加上縮短間隔的測試鉤子，對拋棄式 opencode 跑一次。
   - 相依：步驟 1 的 (3)。
   - 完成判準：先取得紅燈（Local backend 加明確設定時回 `None`），實作後轉綠。變異確認：「Local 不看是否明確設定」與「Server 也要求明確設定」都會讓測試紅燈。

3. 📝 **C：live event 清理**（SCN-008、SCN-009）
   - 產出：schedulerd 測試，資料含 24 小時前後各一筆事件（時間用字面值）。執行一次維護後，逐筆確認刪除與保留。
   - 相依：無。
   - 完成判準：先取得紅燈（維護後舊事件仍在），實作後轉綠。變異：邊界改成 `<=`、改用 `now - 7 天`，測試都要紅燈。

4. 📝 **B：依年齡輪替與上一回合注入**（SCN-004～SCN-007）
   - 產出：
     - `session.rs` 測試：假 backend 回傳不同的建立時間（門檻前後各一、查詢失敗），斷言 `session_id`／`rotated_from`，以及是否呼叫 compaction；
     - `turn.rs` 測試：輪替回合最後一棒的 prompt 含上一回合，未輪替時不含；取得新 session 後舊的被刪除；
     - 用假 agent 的端到端：同一個 scope 連跑兩回合，第二回合前把 session 的建立時間調到門檻之前，觀察 `session_rotated` 日誌與 scope 指向的變化。
   - 相依：步驟 1。
   - 完成判準：紅綠重構證據齊全，斷言用字面值的時間與 session id。變異：門檻比較方向顛倒、單位錯一千倍、輪替時沒有注入上一回合、查詢失敗時當成過期，都要紅燈。

5. 📝 **文件與全套驗證**
   - 產出：
     - `docs/docker.md`：保留期清理段落改寫本機模式條件，補上 B 的輪替與 C 的 live event 清理；
     - `AGENTS.md`「關鍵設計細節」：opencode session 保留期清理改寫本機模式條件，新增輪替；
     - `CHANGELOG.md` `[Unreleased]`。
   - 相依：步驟 2～4。
   - 完成判準：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` 全綠；沒有改 compose 與 `.env.example`，所以不需要 `scripts/test-docker-runtime.sh`（實作中若改到就要跑）。文件提到的每個行為都對應到步驟 2～4 的測試或探測（W-1）。

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
| SCN-004、005 | 單元（session.rs、turn.rs）加假 agent 端到端 | 外部契約由步驟 1 處理，單元測試只驗決策與注入 |
| SCN-006、007 | 單元 | 假 backend 模擬查詢失敗與回合失敗 |
| SCN-008、009 | schedulerd 單元（真 SQLite） | 同層，可合併單迴圈，理由記在步驟 3 |

## 檢查清單
- [ ] 步驟 1 三項探測都有結論
- [ ] TBD-1 已有量測，或在 PR 揭露以 7 天交付
- [ ] 每個 Scenario 都有紅綠重構證據
- [ ] 常青文件已更新
