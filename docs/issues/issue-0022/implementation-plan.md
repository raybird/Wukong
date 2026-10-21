# Issue 22 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)；現況見 README「已確認證據」。

## 設計
- **列出排程 scope（`wukong-memory`）**：新增 `Memory::scopes_with_prefix(prefix)`，回傳 `memories`、`sessions`、`agent_sessions`、`agent_session_state` 四張表中，以 `prefix` 開頭的不重複 scope。
  - 用 `substr(scope, 1, length(?1)) = ?1` 比對：區分大小寫，也沒有萬用字元。
  - 空字串回 `InvalidScope`。
- **清孤兒 scope（`wukong-scheduler` executor）**：新增 `clear_orphan_turn_scopes(store, memory) -> Result<u64>`。
  - 對每個 `agent:schedule-` 開頭的 scope，後綴必須能整段解析成 UUID。
  - 解析成功、而且 `get_job` 查不到時，才呼叫 `delete_scope`。
  - 回傳清掉的 scope 數。`get_job` 出錯就整個回錯誤，不刪任何東西（不確定就不刪）。
- **開始執行時推進（store）**：新增 `advance_next_run(job_id, worker_id, cron, now)`：
  - `UPDATE scheduler_jobs SET next_run_at = next_after(cron, now) WHERE id = ? AND locked_by = worker_id`；
  - 只在自己持有 lease 時才推進。
- **`run_claimed_job`**：在 `start_run` 之後呼叫 `advance_next_run`。回合正常完成時，`complete_claimed_job` 照原本的方式再推進一次，不變。
- **daemon**：
  - `run_scan` 開頭呼叫 `interrupt_stale_runs(now)`，數量大於 0 時印出來。
  - 啟動時，在 `recover_interrupted_runs` 旁呼叫 `clear_orphan_turn_scopes`；和它一樣，`--once` 不執行。
- **不做**：定期清孤兒 scope；回合總時間上限；逾時後接續。

## 實作步驟

所有步驟都要等 SCN-001～005 核准（TBD-AC）後才開始。

1. 📝 **首要驗證：只清排程不存在的 scope**（README 首要驗證；SCN-005 的資料層）
   - 產出：
     - `scopes_with_prefix` 測試：資料包含大小寫不同、含 `%`／`_` 的 scope，期望值手寫；
     - `clear_orphan_turn_scopes` 測試：SCN-005 那五種 scope，外加 `AGENT:SCHEDULE-<uuid>`，每個都有記憶與 session 指向，逐 scope 比對前後。
   - 相依：TBD-AC。
   - 完成判準：
     - 先用 stub 取得斷言失敗的紅燈，實作後轉綠；
     - 用變異確認：不檢查 UUID、改成不分大小寫比對、`get_job` 出錯時當成不存在，都會讓測試紅燈。
2. 📝 **紅燈**（SCN-001～004）
   - 產出：
     - executor 測試，SCN-001：開始執行之後，模擬行程結束（只取得、`start_run`，不完成），lease 到期後 `claim_due_jobs` 不再取得它，`next_run_at` 是開始時間之後的下一個時間點；
     - executor 測試，SCN-002：取得但沒開始，lease 到期後照常取得；
     - SCN-003 的 characterization；
     - daemon 測試，SCN-004：`run_scan` 在兩種 lease 狀態下的標記結果。如果 `run_scan` 依賴具體的 backend 而不好測，就抽出掃描前的標記步驟來測，真實效果由步驟 4 驗證。
   - 相依：步驟 1。
   - 完成判準：SCN-001、SCN-004 在現行程式碼下以斷言失敗；SCN-002、SCN-003 在現行程式碼下就是綠的。
3. 📝 **最小實作**
   - 產出：依「設計」實作。修改前對 `run_claimed_job`、`run_scan`、`claim_due_jobs` 跑 impact 分析。
   - 相依：步驟 2。
   - 完成判準：
     - 步驟 1、2 都綠，受影響 crate 的測試全綠；
     - 用變異確認：拿掉推進、改成取得時推進、拿掉掃描時的標記，各自讓對應的測試紅燈。
4. 📝 **真實行程驗證**（SCN-001／004／005）
   - 產出：在臨時目錄跑 main 與本分支的 binary，做法：
     - 假 agent 睡 30 秒；
     - daemon `--lease-secs 10` 開始執行後 `kill -9`，立刻重啟，並記錄這之後 30 秒內的情況；
     - 另外準備一個排程已不存在的 scope，確認重啟時被清掉。
   - 相依：步驟 3。
   - 完成判準：
     - main 會重跑同一次，舊 run 一直停在 running；
     - 本分支不重跑，舊 run 在 lease 到期後的那一輪掃描標成 interrupted，孤兒 scope 被清掉、其他 scope 不變。
5. 📝 **常青文件與交付**
   - 產出：
     - `AGENTS.md` 的「lease 在執行中續租」條目：刪掉「不處理的部分」中已經處理的兩項，補上新行為；「排程能力注入」條目更新當掉後 scope 殘留的說明；
     - `CHANGELOG.md`；
     - `docs/issues/issue-0013/`、`issue-0017/` 的 TBD-1 標註由 #22 處理。
   - 全量 fmt、clippy、`cargo test --workspace --locked --no-fail-fast`，以及 `gitnexus_detect_changes`。
   - 相依：步驟 1～4。

## 驗收對照
| Scenario | 步驟 | 證據 |
|---|---|---|
| SCN-001 | 2、3、4 | executor 測試；真實行程 |
| SCN-002 | 2、3 | executor 測試（characterization） |
| SCN-003 | 2、3 | executor 測試（characterization） |
| SCN-004 | 2、3、4 | daemon 測試；真實行程 |
| SCN-005 | 1、3、4 | memory 與 executor 測試；真實行程 |
