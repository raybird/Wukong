# 審查報告

- 範圍：局部 diff（ea6e7e0..b115650，註解與文件；程式審查見 review-1d65567.md、review-c959b8a.md）。GitHub PR #23（issue #22），TARGET=`main`
- Reviewed BASE SHA：`c08c03d3ccc64193ddc89bf01bae2cf8e64f8ab9`
- Reviewed HEAD SHA：`b11565069648a3f201580e2307cc2249401e9c1d`
- Reviewed patch-id：`b2dad0a87a65d678e66a82ba0868b15d9abadba6`（完整 PR `c08c03d..b115650`）。增量 `ea6e7e0..b115650` 的 patch-id 為 `72e25bec429ac27cad65a9befe3955f502273863`。兩者都在 2026-10-21 以 `git diff A B | git patch-id --stable` 計算
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），非原實作者；審查日期 2026-10-21
- Review artifact：`docs/issues/issue-0022/review-b115650.md`

## 問題與風險

### MUST FIX

無。

### SHOULD FIX

- **S-5 即將更新的 PR 說明，「驗證」段的交付範圍與審查狀態停在 c959b8a**（協調者 scratchpad 的 `pr22-body.md`，「驗證」段第 1 與第 4 點；不在 commit 範圍內）
  - 現況：寫「交付範圍：`c08c03d..c959b8a`（patch-id `efd4174`）」與「增量審查進行中」。分支 HEAD 已是 `b115650`，完整 PR patch-id 是 `b2dad0a…`；`review-c959b8a.md` 已判定 PASS（報告提交 `ea6e7e0`），`b115650` 由本報告審查。
  - 影響：依 `docs/agents/review-evidence.md:42`，PR 證據要同時列出被審查 HEAD 與報告提交 HEAD。照現在的內文更新上去，讀者會以為交付範圍不含 `b115650`，也看不到 `ea6e7e0` 是只加報告的提交。
  - 建議：更新 PR 前把交付範圍改成 `c08c03d..b115650`（patch-id `b2dad0a`），審查列成 `review-1d65567.md`（RETURN）→ `review-c959b8a.md`（PASS，報告提交 `ea6e7e0`，`verify-artifact.py` OK）→ `review-b115650.md`。「全量檢查（程式碼同 `c959b8a`）」可補一句 `b115650` 只改一行 `//` 註解。這不是本次 diff 的缺失，PR 目前在 GitHub 上的是舊版，尚未更新。

### NICE TO HAVE

- **N-8** PR 說明「限制與待確認」第 6 點只寫了設定檔讀不到的情況，沒有提記憶維護可能先跑。AGENTS.md:287 寫了兩種情況。這只是時間先後，不影響結果，PR 說明不寫也不會誤導，要不要補由實作者決定。
- **N-9** `docs/issues/issue-0022/README.md:143` 的 D-3 結論寫「PR 說明…已改正」，在 PR 實際更新之前不成立。照協調者的流程，PR 會在提交後更新，所以只要更新了就成立；記在這裡，避免漏掉那一步。

## 已查核維度

### ea6e7e0 的本機 artifact 有效性（`docs/agents/review-evidence.md:36-44`）

| 條件 | 查核 | 結果 |
|---|---|---|
| 1. 被審查 SHA 的直接後繼、非 merge、報告可讀 | `git rev-list --parents -n1 ea6e7e0` 只有一個父提交 `c959b8a8…`；`git show ea6e7e0:docs/issues/issue-0022/review-c959b8a.md` 可讀 | 成立 |
| 2. 唯一差異是新增這份報告、一般檔案模式 | `git diff --name-status c959b8a ea6e7e0` 只有 `A docs/issues/issue-0022/review-c959b8a.md`；`git ls-tree` 模式 100644。`verify-artifact.py --reviewed c959b8a… --head ea6e7e0 --artifact …` 輸出「OK：只有新增本次報告」，exit 0 | 成立 |
| 3. PR 證據同時列出被審查 HEAD 與報告提交 HEAD | 待 PR 說明更新，見 S-5 | 待協調者處理 |

`review-c959b8a.md` 的判定是 PASS（第 148 行），所以 `c959b8a` 的 PASS 沿用到 `ea6e7e0`。`b115650` 是新的提交，不在這條例外內，由本報告審查。

### 程式行為

- `git diff ea6e7e0 b115650 -- '*.rs'` 只有 `crates/wukong-schedulerd/src/main.rs:213` 一行 `//` 註解的替換，不是 doc comment，不影響編譯產物。其他三個檔案都是 Markdown。沒有程式行為變更，所以不重跑測試，也沒有建置。
- `cargo fmt --all -- --check` 通過（2026-10-21，主工作區，不產生 target）。

### 新說法與程式的對照

依據 `crates/wukong-schedulerd/src/main.rs`（b115650）：

- 主迴圈（`:125-152`）：`ticks.tick()` 分支先呼叫 `refresh_model_settings`，失敗就印警告後 `continue`（`:128-131`），`run_scan` 不會執行。`tokio::select!` 沒有 `biased;`，`maintenance_ticks`（`:102-107`）是 `interval`，第一次 tick 和 `ticks` 一樣在啟動時就緒；維護分支在 select 裡 await 完才回到迴圈，`ticks` 的 tick 隨後才被處理。`retention_ticks` 用 `session_retention::ticker`，第一次在一個完整間隔之後（`session_retention.rs:41`），不影響啟動時的順序。
- `run_scan`（`:200-221`）：第一件事是 `interrupt_stale_runs`，之後才 `claim_ready_jobs`（裡面的 `backend.check_ready()` 在 `:192`）。`--once`（`:84-98`）直接呼叫 `run_scan`，不經過 `refresh_model_settings`。啟動流程（`:58-82`）只有 `clear_orphan_scopes`，沒有任何標記 run 的呼叫。

逐處核對：

| 位置 | 新說法 | 對照 | 判定 |
|---|---|---|---|
| `crates/wukong-schedulerd/src/main.rs:212-213` | 「`--once` 也會標記。啟動時不另外檢查，所以設定檔讀不到、主迴圈跳過掃描時，標記也跟著延後。」 | `--once` 經 `run_scan` 標記；啟動流程沒有標記；設定檔失敗時 `continue` 跳過 `run_scan`。這裡的「所以」是邏輯推論（沒有啟動時的檢查，唯一的標記點又被跳過），不是無證據的設計理由。`review-c959b8a.md:24-29` 的真實行程實驗（HEAD、`{not json`、run 停在 running）也證實 | 正確 |
| `AGENTS.md:287` | 「標記只在掃描時做，啟動時不另外檢查：設定檔讀不到時，主迴圈整輪跳過掃描，標記也跟著延後到設定檔恢復；同一時間到期的記憶維護也可能先跑，讓第一輪掃描晚一點開始。」 | 前半同上。「延後到設定檔恢復」：恢復後下一個 tick 讀取成功就會掃描並標記。後半：兩個 interval 啟動時都就緒，select 隨機挑選，用「也可能」「晚一點」措辭，沒有誇大。舊的「常駐模式的第一輪在啟動時立刻執行，所以啟動時不另外檢查」這個無證據的「因為」已刪掉 | 正確 |
| `docs/issues/issue-0022/README.md:19` | 「設定檔讀不到、主迴圈跳過掃描時，標記也跟著延後（D-3）。」 | 同上 | 正確 |
| `docs/issues/issue-0022/implementation-plan.md:145` | 「`run_scan` 每輪的標記在常駐模式與 `--once` 都會做」，加上帶日期的更正說明 | 正確；被推翻的寫法以「2026-10-21 增量審查 S-4 更正」保留，符合 `docs/AGENTS.md` 工作流程第 4 點「被推翻的分析保留帶日期的 NOTE」 | 正確 |

repo 內（排除審查報告）以 `git grep` 搜「立刻執行」「啟動時立刻」「第一輪」，`b115650` 已沒有「常駐模式第一輪在啟動時立刻執行」這種說法；剩下的「第一輪」都屬於 session retention 或其他 issue，與本次無關。

### D-3 紀錄與使用者決定

- `README.md:143`：事項引用增量審查 S-4，內容與 `review-c959b8a.md:18-34` 一致；「（reviewer 推論，未實測）」只接在記憶維護那一句後面，和原報告一致（設定檔那部分原報告有真實行程實驗）。狀態「已解決」，結論寫了 2026-10-21 使用者選「不改程式，改正說法」與理由「設定檔壞掉時本來就不執行任何排程，影響只在顯示」，和協調者轉述的使用者決定、原報告選項 (a) 一致。限制：reviewer 看不到使用者對話本身，依協調者轉述判斷。
- `README.md:153` Timeline 新增一列，記錄增量審查 PASS 與 D-3 的決策來源，屬語意事件。

### 規格

- `c8efc62` 與 `b115650` 的 README，抽出 ```` ```gherkin ```` 區塊（49 行）後 `diff`，沒有差異。這次只改「變更後流程」一句、待確認事項加 D-3、Timeline 加一列。核准有效，不需重新核准。

### PR 說明（`pr22-body.md`）中關於啟動標記的說法

- 「改變後的行為」第 2 點：「`run_scan` 在取得排程之前先呼叫 `interrupt_stale_runs`，backend 還沒準備好時也會標記」——標記在 `claim_ready_jobs`（含 `check_ready`）之前，正確。已不再寫「第一輪在啟動時立刻執行」。
- 「限制與待確認」第 6 點：「設定檔讀不到時主迴圈整輪跳過掃描，殘留的 run 會停在 running，直到設定檔恢復（main 在啟動時就標記）…影響只在顯示」——與程式及原報告實驗一致，正確。沒寫記憶維護見 N-8。
- 其他段落的版本資訊過時，見 S-5。

### 相關失敗面

| 失敗面 | 輸入／狀態 | 預期 | 覆蓋 | 判定 |
|---|---|---|---|---|
| 註解改動夾帶程式變更 | `.rs` diff | 只有註解 | `git diff -- '*.rs'` 只有一行 `//`；fmt 通過 | 通過 |
| 文件仍宣稱啟動時立刻標記 | repo 全文 | 不再出現 | `git grep` | 通過 |
| 新說法誇大或引入無證據的因果 | 四處新說法 | 與程式一致 | 逐處讀碼對照 | 通過 |
| 規格被順手改動 | README Gherkin | 與核准版逐字相同 | `diff` 無差異 | 通過 |
| PR 證據指向舊版本 | `pr22-body.md` | 指向 b115650 與各審查報告 | 讀檔 | S-5 |

### 需求、架構、安全、品質

- 需求：沒有新增行為，只改正說法，符合 D-3。
- 架構、安全：沒有程式變更，不適用。
- 重複與過度設計：同一個限制分別寫在 AGENTS.md（常青文件）、README（決策紀錄）、plan（更正 NOTE）、程式註解，各自是該文件應有的內容，不算重複。

### 豁免、待確認與限制

- 無 gate 豁免。D-1～D-3 都已記錄使用者 2026-10-21 的決定。TBD-1～TBD-3 的狀態不變，這次的改動不影響它們。
- 沒有重跑測試、沒有建置，理由是沒有程式行為變更；程式與測試的證據沿用 `review-c959b8a.md`（被審查 HEAD `c959b8a`，與 `b115650` 的程式差異只有一行註解）。
- 沒有建立任何 worktree 或 target 目錄，不需清理。

## 流程判定

PASS

理由：`ea6e7e0` 符合本機 artifact 例外，`c959b8a` 的 PASS 沿用。`b115650` 只改一行 `//` 註解與三份 Markdown，沒有程式行為變更；新說法對照主迴圈與 `run_scan` 都正確，刪掉了舊的無證據的「因為」，沒有新的誇大；D-3 與使用者決定一致；Gherkin 未改動。S-5 是協調者尚未發出的 PR 內文，更新 PR 前要改，不阻擋本次 diff 的判定。
