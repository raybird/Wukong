# Issue 13 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)；現況見 README「已確認證據」。

## 設計
- **session lease 續租（`wukong-runtime`）**：`run_turn` 取得 session lease 後，把逐棒執行的 future 和一個計時器放在同一個 `select!` 裡，每隔 lease 時間的三分之一呼叫一次 `renew_agent_session_lease`。兩者在同一個 task：回合結束或行程中止，續租就停止。續租失敗只記一行 log，因為 lease 已經被別人拿走，回合結束時取得 lease 的檢查會照現行邏輯處理。壓縮期間也要續租。
- **job lease 續租與進度（`wukong-scheduler`）**：`run_claimed_job` 多收一個 lease 時間參數。回合執行時，同一個 task 每 30 秒做兩件事：
  - 續租 job lease（`locked_by` 必須是自己）。
  - 把目前階段與已執行秒數寫進 run 訊息（只更新 `status = 'running'` 的 run）。

  換階段時立刻更新一次。階段來自 `run_turn` 的 `on_role`：還沒收到任何 `on_role` 時是「規劃中」，收到第 k 次時是「第 k 棒 <角色>」。
- **失敗訊息**：`execute_job` 失敗時，在原錯誤前面加上「<階段>失敗，已執行 N 秒：」。
- **手動 trigger**：`claim_job` 拿不到 job 時，再查一次 job，區分不存在、已停用與正在執行（`locked_until > now`）。
- **不做**：重啟後立刻重跑、回合總上限、接續、成果驗證（TBD-1～3）。

## 實作步驟

所有步驟都要等 SCN-001～007 核准（TBD-AC）後才開始。

1. 📝 **首要驗證：在真實行程重現 lease 過期後的重複執行**（SCN-003、SCN-004）
   - 產出：一支重現腳本，放在 scratchpad，結果抄錄進本計畫。內容：
     - 臨時目錄、假 agent，每次呼叫睡 N 秒。
     - `WUKONG_SESSION_LEASE_SECS` 與 daemon 的 `--lease-secs` 都設成遠小於回合時間。
     - 用 `wukong-schedulerd --once` 執行排程，跑到 lease 過期後，另開行程 `wukong schedule trigger` 同一個排程。
     - 記錄兩邊的 exit code、run 紀錄、`agent_session_state`，以及假 agent 收到的 session。
   - 相依：TBD-AC。
   - 完成判準：現行版本至少重現下列其中一項，作為紅燈基線：trigger 重複執行、共用 session、原回合以 `session lease unavailable` 失敗。若都沒有重現，回 README 重新評估。
2. 📝 **紅燈**
   - `wukong-memory`／`wukong-runtime`：用假 backend 讓一棒的執行時間超過 session lease 時間。執行中，從另一個 owner 嘗試取得同一個 scope 的 lease，預期拿不到；回合成功並寫回 session（SCN-003）。
   - `wukong-scheduler`：
     - job lease 很短、回合較長時，執行中 `claim_job` 拿不到 job，原回合完成後 `Completed`（SCN-004）。
     - 回合執行中，run 訊息出現「規劃中」與「第 k 棒 <角色>」以及已執行秒數（SCN-001）。
     - 在規劃階段與第 2 棒失敗時，訊息開頭標出階段（SCN-002）。
     - 短回合成功時，run 訊息等於最終輸出（SCN-006，characterization）。
   - `wukong-scheduler` store：區分 trigger 拿不到 job 的三種原因（SCN-007）。
   - 期望值手寫，不由被測函式產生。測試中的時間用很短的 lease 與可控制的假 backend，不睡到真實的 30 秒：進度更新間隔要能在測試中調短。
   - 相依：步驟 1。完成判準：在現行程式碼下以斷言失敗，不是編譯失敗。
3. 📝 **最小實作**
   - 先跑 `gitnexus_impact`（`run_turn_traced_with_attachments`、`run_claimed_job`、`claim_job` 等），回報風險等級。
   - 相依：步驟 2。完成判準：步驟 2 轉綠；受影響 crate 的測試全綠。
4. 📝 **重現腳本複驗與中止測試**（SCN-003、SCN-004、SCN-005）
   - 用步驟 1 的腳本在變更後再跑一次：trigger 印「排程正在執行中」，原回合成功，沒有共用 session。
   - 另跑一次：回合執行中用 SIGKILL 強制終止 daemon，確認 job lease 與 session lease 在 lease 時間後到期，之後可以再次 trigger（SCN-005）。
   - 相依：步驟 3。
5. 📝 **常青文件與交付**
   - 根目錄 `AGENTS.md`「關鍵設計細節」：補上 lease 續租與 run 進度訊息；更新 #12「同時執行的回合告知」裡「回合超過 lease 時間就看不到」這項限制。
   - `docs/cli-reference.md`：多個 daemon 的描述、`schedule runs` 的進度訊息、trigger 的訊息。
   - `CHANGELOG.md` `[Unreleased]`。
   - 全量 fmt、clippy、`cargo test --workspace --locked`，以及 `gitnexus_detect_changes`。
   - 相依：步驟 1～4。

## 驗收對照
| Scenario | 層級 | 證據 |
|---|---|---|
| SCN-001 | 單元（scheduler，假 backend） | 步驟 2／3 |
| SCN-002 | 單元（scheduler，假 backend） | 步驟 2／3 |
| SCN-003 | 單元（runtime）＋重現腳本 | 步驟 1、2／3、4 |
| SCN-004 | 單元（scheduler）＋重現腳本 | 步驟 1、2／3、4 |
| SCN-005 | 強制終止行程的腳本 | 步驟 4 |
| SCN-006 | 單元（characterization） | 步驟 2／3 |
| SCN-007 | 單元（scheduler store、CLI） | 步驟 2／3 |
