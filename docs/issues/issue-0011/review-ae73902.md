# 審查報告
- 範圍：完整 PR（重點為上一輪審查後的增量 1034907..ae73902）。`gh pr view 15` 的 headRefOid 為 ae73902、base 為 main、狀態 OPEN；`git merge-base main ae73902` = e4a4e20，與固定範圍一致。
- Reviewed BASE SHA：e4a4e2072046d25a8f9115a7a25f27287214ac63
- Reviewed HEAD SHA：ae7390207b862117ea190fce0f83e2f2c248ebd0
- Reviewed patch-id：3b0bee24b2f2bd07916cc991a274a65ef11439af（2026-10-20 以 `git diff e4a4e20 ae73902 | git patch-id --stable` 重算，一致）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-20
- Review artifact：docs/issues/issue-0011/review-ae73902.md
- 前一輪報告：docs/issues/issue-0011/review-1034907.md

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
無。上一輪的 SHOULD FIX 1（探針只靠 `#[ignore]`）已解決，查核見下方「驗收與證據」。

### NICE TO HAVE
1. （新增）`crates/wukong-runtime/tests/planner_probe.rs:8`～`:11` 說實作計畫的條件是「一次只跑一支」，檔頭命令也帶 `<測試名>`。但實作計畫步驟 1 記錄的命令（`implementation-plan.md:28`）沒有指定測試名稱。步驟 1 當時檔案可能只有一支測試，所以紀錄本身沒錯。不過步驟 6 新增的補充只提醒重跑時要補上 `WUKONG_PLANNER_PROBE_MODEL`，沒有提醒要補上測試名稱。照步驟 1 的命令重跑，兩支會並行執行，`real_followup_turns` 的真實回合也會一起跑。檔頭已有警告，這只影響文件能否照著重現，不阻塞。
2. （延續上一輪，維持原分級）`crates/wukong-runtime/src/turn.rs` 與 `crates/wukong-gateway/src/prompt.rs` 各有一份 800 字元截斷。兩份行為一致，日後只改其中一份就會分歧。
3. （延續上一輪，維持原分級）`crates/wukong-orchestrator/src/router.rs` 的 `[Previous Turn]` 區塊沒有結束標記。
4. （延續上一輪，維持原分級）確定性測試沒有覆蓋 scope 只有一筆 event 的情況，也沒有覆蓋剛好 800 字元不截斷的 planner 路徑。
5. （延續上一輪，維持原分級）`需決策`：上一回合沒有時間上限。待決問題、可選做法 (a) 維持現狀／(b) 加時間窗，以及 reviewer 建議 (a)，都與 review-1034907.md 相同。這項不影響判定。

增量沒有碰到 router.rs、turn.rs、prompt.rs（見下方範圍核對），所以 2～5 項仍然成立，照原文延續。

## 已查核維度

### 驗收與證據
- **增量範圍**：`git log 1034907..ae73902` 只有兩個提交。
  - d17461b（父提交為 1034907）：`--name-status` 只有 `A docs/issues/issue-0011/review-1034907.md`，模式 100644，是單純的報告提交。
  - ae73902：只改 `crates/wukong-runtime/tests/planner_probe.rs`（+15/−7）與 `docs/issues/issue-0011/implementation-plan.md`（+1）。
  - `git diff 1034907 ae73902 --stat` 共 3 檔，與上面相符。
- **核心程式未變**：分別取 `git diff e4a4e20 1034907` 與 `git diff e4a4e20 ae73902` 在 `crates/wukong-orchestrator crates/wukong-runtime/src` 下的 diff，兩者 sha1 相同（ed689d63…）。上一輪對 router.rs、turn.rs 的正確性、characterization（BASE／HEAD prompt byte 相同）、變異測試與失敗面結論都適用於完整範圍。gateway 在增量中也沒有變更。
- **SCN-001～003**：程式碼與證據都與上一輪相同，結論沿用 review-1034907.md 的驗收表。真實模型層的數據沒有重跑（依指示）。
- **SHOULD FIX 修正查核**：
  - 未設變數會在呼叫模型前失敗：兩支測試的第一行都是 `let model = probe_model();`（`:37`、`:100`）。在它之前沒有建立 backend、臨時目錄或 `set_var`，所以 panic 發生在任何 opencode 呼叫與 `WUKONG_WORKSPACE` 改寫之前。自行執行確認兩支都在 0.00 秒內失敗。
  - 兩支都受保護：只有這兩支 `async fn`，都改用 `probe_model()`，預設值 `opencode/big-pickle` 已移除。
  - 檔頭命令與實作計畫條件一致：檔頭寫 `XDG_CONFIG_HOME=<空目錄>` 加上模型變數，與計畫步驟 1／4 記錄的隔離方式相同。模型名稱與計畫記錄的 `opencode/big-pickle` 相同。「一次只跑一支」的落差見 NICE TO HAVE 1。
  - 副作用警告準確：
    - 「agent 有完整工具權限、曾建出 ~/.wukong/memory.db」與計畫步驟 5 的副作用紀錄一致。
    - 「會改整個 process 的 WUKONG_WORKSPACE」與 `:109` 的 `std::env::set_var` 一致。
- **步驟 6 新增敘述是否誠實**：
  - 「兩支都在 0.00 秒內以 `set WUKONG_PLANNER_PROBE_MODEL …: NotPresent` 失敗」：reviewer 重跑，輸出完全相符（panicked at planner_probe.rs:25:10、`finished in 0.00s`）。
  - 「不帶 `--ignored` 為 2 ignored」：相符。
  - 「fmt、clippy 通過」：相符。
  - 敘述標明這是審查後修正並引用來源報告，沒有宣稱重跑了真實模型。整段誠實。

### 相關失敗面（增量）
| 失敗面 | 輸入／狀態 | 預期 | 覆蓋 | 判定 |
|---|---|---|---|---|
| 開發者跑 `cargo test --workspace -- --ignored`，沒有設探針變數 | `WUKONG_PLANNER_PROBE_MODEL` 未設 | 不呼叫模型，也不產生副作用 | reviewer 用 `env -u` 實測，兩支都在 0.00 秒 panic；panic 前沒有任何 I/O | 通過 |
| 一般 `cargo test`／CI | 沒有 `--ignored` | 2 ignored | 實測 `0 passed; 2 ignored` | 通過 |
| 變數設成空字串 | `WUKONG_PLANNER_PROBE_MODEL=` | — | 會通過檢查，並以 `--model ""` 呼叫 opencode | 可接受：設定變數本身就是明確 opt-in，不列為發現 |
| 與 repo 慣例一致性 | 對照 `crates/*/tests/managed_actual.rs` | 未 opt-in 時立刻失敗 | managed_actual 用 `std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap()`。探針用 `.expect(…)`，同一種機制，失敗訊息還更清楚 | 一致 |
| helper 是否過度設計 | `probe_model()` 4 行、2 個呼叫點 | — | 取代兩段重複的讀取程式，沒有新增設定項目 | 合理 |

### 需求、架構、安全、品質
- 需求、架構：增量只動測試檔與文件，不影響產品行為或 crate 相依方向。
- 安全：修正讓真實模型與完整權限 agent 都需要明確 opt-in，消除了上一輪指出的意外觸發路徑。探針中的 Telegram chat id 在 BASE 已存在，不是新的揭露。
- 品質：改動精簡；expect 訊息的英文與其他測試訊息風格相容；註解為繁體中文，與檔案原有風格一致。

### 豁免、待確認與限制
- 沒有 gate 豁免。
- 待確認事項：NICE TO HAVE 5（`需決策`）沿用上一輪的內容，不影響判定。
- 限制：依指示沒有設定 `WUKONG_PLANNER_PROBE_MODEL` 執行探針，真實模型層的數據直接採用計畫的記錄（與上一輪相同）。增量沒有變更產品程式碼，所以沒有重做上一輪的變異測試與 byte 比對，理由是核心路徑的 diff 經 sha1 確認相同。
- 本次只新增這份報告，沒有修改程式碼，沒有 commit 或 push，也沒有在 GitHub 留言。

### 自行執行的驗證（2026-10-20，工作區 = HEAD ae73902，執行前 `git status` clean）
| 命令 | 結果 |
|---|---|
| `env -u WUKONG_PLANNER_PROBE_MODEL cargo test -p wukong-runtime --test planner_probe -- --ignored` | exit 101；2 failed，兩支都 panicked at `planner_probe.rs:25:10`，訊息為 `set WUKONG_PLANNER_PROBE_MODEL (e.g. opencode/big-pickle) to call a real model: NotPresent`，`finished in 0.00s` |
| `cargo test -p wukong-runtime --test planner_probe` | `ok. 0 passed; 0 failed; 2 ignored` |
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --all-targets --locked -- -D warnings` | exit 0 |
| `cargo test --workspace --locked` | exit 0；加總 passed=633 failed=0 ignored=11，與上一輪及計畫記錄相同 |
| `git diff e4a4e20 ae73902 \| git patch-id --stable` | 3b0bee24b2f2bd07916cc991a274a65ef11439af |
| 核心路徑 diff 比對（e4a4e20..1034907 與 e4a4e20..ae73902，限 orchestrator 與 runtime/src） | sha1 相同 |

## 流程判定
PASS
