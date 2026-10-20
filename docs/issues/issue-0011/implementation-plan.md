# Issue 11 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)。

## 現況（2026-10-20 讀碼確認）

- 正式部署 RunWuKong 跑 `ghcr.io/raybird/wukong:v0.22.1`，該 tag 已含 PR #10；之後 `crates/` 沒有變更，所以 2026-10-08 的事故對應目前的程式碼。
- `run_turn_traced_with_attachments`（`crates/wukong-runtime/src/turn.rs`）先召回 `recall`（Relevant 模式），再由 `with_previous_turn` 補上同 scope 最新兩筆 `event`（上一回合的 User／Assistant）得到 `helper_hits`。接著呼叫 `plan_skill_chain_with_preferences(backend, input, …)`，**planner 只收到本次 `input`**。
- `skill_planning_prompt_with_preferences`（`crates/wukong-orchestrator/src/router.rs`）列出角色與技能，只有一句「Use a single step for simple tasks; at most three steps.」。對「所以沒辦法跑成功嗎」這種沒有上下文就看不懂的追問，planner 無從判斷它是不是簡單任務。
- 執行器依計畫逐棒執行：輔助棒注入 `helper_hits`、最後一棒接續 session 並注入 `recall`，前棒輸出經 `chain_context` 傳給後棒；沒有提前結束的機制（方案 B 已不採用）。
- planner 走 `run_ephemeral`、`agent: "plan"`、`model: None`，用的是部署的預設模型（`opencode/big-pickle`，見 #13 TBD-2）。

## 設計（方案 A）

- `turn.rs` 把上一回合的 User／Assistant 文字（`with_previous_turn` 已取得的同一批紀錄）交給 planner；沒有上一回合時不加任何區塊。
- `router.rs` 的 planner prompt 在有上下文時加上 `[Previous Turn]` 區塊與分類規則：可以直接由上一回合回答、且不要求新查證或操作的追問用一棒；要求重新查核、需要新資訊，或要真實操作與驗證的任務，照常規劃必要步驟。
- 上下文來源與長度（TBD-3）：先採「上一回合兩筆，各截斷到 800 字元」，與 `compose_prompt` 每筆記憶的上限一致；只用上一回合，不把相關召回或整段聊天歷史灌進 planner。步驟 1 的基線與步驟 4 的複測若顯示不夠，再回來調整。
- 已知限制：同 scope 若有排程回合寫入記憶，「上一回合」可能是排程產出而非使用者的對話；這由 #14（排程改用獨立 scope）處理，本 issue 不另行過濾。

## 實作步驟

1. 📝 **首要驗證：planner 基線探針** — 產出：`crates/wukong-runtime/tests/planner_probe.rs`（`#[ignore]`，用真實 `route_options()` 與 `plan_skill_chain_with_preferences`、`opencode run --model opencode/big-pickle`、臨時工作目錄），三組固定案例各取 5 個樣本並印出計畫：
   - A 簡單追問：上一回合已回答「run #869 失敗於 session lease unavailable、#863／#870 成功、其餘逾時」，本次輸入「所以沒辦法跑成功嗎」。
   - B 要求重新查核：同一個上一回合，本次輸入「幫我重新查一下現在排程的狀態」。
   - C 複雜操作：「修好 scheduler 的逾時問題，並跑測試驗證」。
   相依：無。完成判準：記錄目前程式碼（planner 只看 `input`）下每組的計畫分布。若 A 已經大多是一棒，表示 planner 缺上下文不是主因，停止並回報，重新檢討方案；若 A 常排多棒，就是要修的失敗基線。
2. 📝 **確定性紅燈** — 產出：`router.rs` 測試（有上下文時 prompt 含上一回合文字與分類規則；沒有上下文時 prompt 與現行完全相同）、`turn.rs` 測試（假 backend 記下 planner 收到的 prompt：scope 有上一回合時含其文字且被截斷，沒有時不含區塊）。相依：步驟 1 支持假設。完成判準：新測試因目前缺少上下文而失敗。
3. 📝 **最小實作** — 產出：router 新增上下文參數與 prompt 區塊、turn.rs 傳入上一回合文字。相依：步驟 2。完成判準：步驟 2 的測試轉綠；`cargo test -p wukong-orchestrator -p wukong-runtime` 與 clippy、fmt 全綠。編輯前先做 `gitnexus_impact`。
4. 📝 **真實模型複測（SCN-001～003）** — 產出：以步驟 1 的同一個探針重跑，與基線並列。相依：步驟 3。完成判準：A 的單棒比例明顯高於基線（5 個樣本至少 4 個一棒）；B、C 的計畫沒有比基線少掉查證或執行角色（C 仍含 fixer 這類執行角色）。
5. 📝 **真實回合對照（SCN-001、SCN-003 的回答內容）** — 產出：在臨時 scope 與臨時工作目錄連跑兩個真實回合（先建立事實，再問 A 與 B），記錄棒數、工具呼叫數與回答內容。相依：步驟 3。完成判準：A 只有一棒且回答引用上一回合的成功／失敗證據；B 有實際查核的工具活動，或明說缺少資訊，不把上一回合的資料當成最新狀態。
6. 📝 **常青文件與交付** — 根目錄 `AGENTS.md`「一回合資料流」的 plan 步驟補上「planner 會收到上一回合」；`CHANGELOG.md` `[Unreleased]` 記錄行為變更。全量 `cargo test --workspace`、clippy、fmt。

## 驗收對照

| Scenario | 層級 | 證據 |
|---|---|---|
| SCN-001 | planner 輸入（確定性）＋真實模型 | 步驟 2／3 的紅綠燈；步驟 4 的 A 組分布；步驟 5 的 A 回合 |
| SCN-002 | 真實模型 | 步驟 4 的 C 組與基線比較；執行器未改動，多棒計畫仍逐棒執行（既有測試） |
| SCN-003 | 真實模型 | 步驟 4 的 B 組；步驟 5 的 B 回合 |

SCN-002／003 是模型行為，確定性測試只能保證 prompt 有對應規則，無法保證模型照做；這部分以真實模型樣本為證據，樣本數與模型如實記錄。
