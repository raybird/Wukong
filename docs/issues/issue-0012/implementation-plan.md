# Issue 12 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)；現況與重現見 README「程式碼現況」「已確認證據與重現」。

## 設計

- **查詢**：`wukong-memory` 新增 `active_session_leases(now)`，回傳 `lease_until > now` 的 `(scope, lease_until)`。只讀現有的 `agent_session_state`，不改 schema。
- **注入**：`run_turn_traced_with_attachments` 在 `prepare_final_session` 取得 lease 之後查一次，濾掉自己的 scope。有其他回合時，用 `persona` 產生告知區塊，附加到之後每一棒的 prompt。
- **告知文字**：
  - 列出每個 scope 並標示類型：`agent:schedule-<id>` 標為「排程 <id>」，其他標為「對話 <scope>」。
  - 說明它們與本回合共用同一個工作目錄，可能正在修改檔案。
  - 要求 agent 不要修改或提交不是本回合造成的變更；碰到預期外的變動，先說明再處理。
  - 想知道排程是什麼，可以用 `wukong schedule list`。
- **為什麼加到每一棒**：輔助棒（例如 fixer）也會改檔案。這和只放在最後一棒的排程能力提示不同。
- **不做**：排隊、等待、續租、工作目錄隔離（TBD-1、TBD-2）。

## 實作步驟

所有步驟都要等 SCN-001～004 核准（TBD-AC）後才開始。

1. 📝 **首要驗證：lease 是否在整個回合期間可見**
   - 產出：在臨時目錄跑一個假 agent 回合，讓它睡得夠久。執行期間從另一個行程查 `SELECT scope, lease_owner, lease_until FROM agent_session_state`，記錄執行中與結束後的結果。
   - 相依：TBD-AC。
   - 完成判準：執行中查得到本回合 scope 的有效 lease，結束後已釋放。若查不到，回 README 重新評估設計。
2. 📝 **紅燈**（SCN-001～003）
   - `wukong-memory`：`active_session_leases` 的測試，只回傳未過期的 lease。
   - `wukong-runtime`：用假 backend 記下每一棒的 prompt。
     - 另一個 scope 持有有效 lease 時，每一棒都含告知區塊，並正確標示類型；兩個方向都測（SCN-001）。
     - 沒有其他 lease 時，prompt 與變更前逐字相同，以 characterization 測試鎖住（SCN-002）。
     - 過期的 lease 與自己的 lease 不列入（SCN-003）。
   - 期望值手寫，不由被測函式產生。
   - 相依：步驟 1。完成判準：在現行程式碼下以斷言失敗，不是編譯失敗。
3. 📝 **最小實作**
   - 先跑 `gitnexus_impact`（`run_turn_traced_with_attachments` 等），回報風險等級。
   - 相依：步驟 2。完成判準：步驟 2 轉綠；`cargo test -p wukong-memory -p wukong-runtime` 全綠。
4. 📝 **重現腳本複驗**（SCN-001、SCN-004）
   - 用 README 的重現腳本在變更後再跑一次：兩邊都是 exit 0。另外讓假 agent 把收到的 prompt 存檔，確認後開始的回合含有告知區塊。
   - 相依：步驟 3。
5. 📝 **常青文件與交付**
   - 根目錄 `AGENTS.md`「關鍵設計細節」補上告知機制與它的限制。
   - `CHANGELOG.md` `[Unreleased]`。
   - 全量 fmt、clippy、`cargo test --workspace --locked`，以及 `gitnexus_detect_changes`。
   - 相依：步驟 1～4。

## 驗收對照
| Scenario | 層級 | 證據 |
|---|---|---|
| SCN-001 | 單元（runtime，假 backend）＋重現腳本 | 步驟 2／3、步驟 4 |
| SCN-002 | 單元（characterization） | 步驟 2／3 |
| SCN-003 | 單元（memory、runtime） | 步驟 2／3 |
| SCN-004 | 重現腳本（真實行程） | 步驟 4 |

模型是否理會告知不在驗收範圍（TBD-4）。
