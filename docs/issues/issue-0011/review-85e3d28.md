# 審查報告
- 範圍：完整 PR（重點為上一輪審查後的增量 ae73902..85e3d28，純文件）。`gh pr view 15` 的 headRefOid 為 85e3d28、base 為 main、狀態 OPEN；`git merge-base main 85e3d28` = e4a4e20，與固定範圍一致。
- Reviewed BASE SHA：e4a4e2072046d25a8f9115a7a25f27287214ac63
- Reviewed HEAD SHA：85e3d28c722dc3627f79de8ee6a243e3fdbfab7a
- Reviewed patch-id：ac112650a3c89db784ec682e4d48db72b9f177ee（2026-10-20 以 `git diff e4a4e20 85e3d28 | git patch-id --stable` 重算，一致）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-20
- Review artifact：docs/issues/issue-0011/review-85e3d28.md
- 前一輪報告：docs/issues/issue-0011/review-ae73902.md

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
無。

### NICE TO HAVE
1. （新增）README TBD-4 的「影響」欄寫了結論（a）、日期與後續去向，但「理由」只寫「使用者選 (a)」，是決策來源而不是理由。docs/AGENTS.md「待確認事項」要求 `已解決` 需結論、理由與日期。實際理由在 review-1034907.md 第 27 行：規則只有在上一回合已能回答時才收斂成一棒，最後一棒仍可用工具查證，所以風險低。可以補一句，讓讀者不用翻審查報告就知道為何不設時間窗。這是可讀性與證據持久力的改善，不阻塞。
2. （延續上一輪，維持原分級）`crates/wukong-runtime/src/turn.rs` 與 `crates/wukong-gateway/src/prompt.rs` 各有一份 800 字元截斷。兩份行為一致，日後只改其中一份就會分歧。
3. （延續上一輪，維持原分級）`crates/wukong-orchestrator/src/router.rs` 的 `[Previous Turn]` 區塊沒有結束標記。
4. （延續上一輪，維持原分級）確定性測試沒有覆蓋 scope 只有一筆 event 的情況，也沒有覆蓋剛好 800 字元不截斷的 planner 路徑。

上一輪 NICE TO HAVE 1（步驟 6 沒提醒要指定測試名稱）已由 85e3d28 解決。上一輪 NICE TO HAVE 5（`需決策`：上一回合沒有時間上限）已由使用者決定並記入 TBD-4，不再列為待決。增量沒有碰到 crates/，所以 2～4 項照原文延續。

## 已查核維度
- 驗收與證據：
  - 增量範圍：`git log ae73902..85e3d28` 只有 7a13ef5 與 85e3d28，線性（7a13ef5 的父提交為 ae73902，85e3d28 的父提交為 7a13ef5，都不是 merge）。
    - 7a13ef5：`--name-status` 只有 `A docs/issues/issue-0011/review-ae73902.md`，模式 100644，是單純的報告提交。
    - 85e3d28：`git diff ae73902 85e3d28 --name-status` 除了上面的 A 之外，只有 `M docs/issues/issue-0011/README.md`（+1）與 `M docs/issues/issue-0011/implementation-plan.md`（+1/−1）。沒有程式、測試或設定變更。
  - 程式碼未變：`git diff e4a4e20 ae73902 -- crates` 與 `git diff e4a4e20 85e3d28 -- crates` 以 `cmp` 比對，完全相同。上一輪（review-ae73902.md）對 router.rs、turn.rs、planner_probe.rs 的正確性結論與測試結果（workspace passed=633 failed=0 ignored=11、fmt、clippy、探針未設變數時在 0.00 秒失敗）適用於完整範圍。依 verification 規則，被測內容相同，所以重用證據，沒有重跑全量測試。
  - SCN-001～003：程式碼與證據都沒變，沿用 review-1034907.md 的驗收表。
  - TBD-4 是否忠實：
    - 事項「帶給 planner 的上一回合是否要有時間上限」與 review-1034907.md NICE TO HAVE 4、review-ae73902.md NICE TO HAVE 5 的待決問題一致；兩份報告確實都標了 `需決策`。
    - 結論「(a) 維持現狀，不加時間窗」與協調者轉述的使用者 2026-10-20 回覆「a」一致（reviewer 看不到該對話，以協調者的轉述為準）。
    - 「隔多久都帶入上一回合」與程式碼相符：`previous_turn_records`（turn.rs:467）以 `memory.records(Some(scope), Some(MemoryKind::Event), 2)` 取最新兩筆，沒有時間條件。
    - 後續去向「上線後若觀察到過舊的上一回合誤導 planner，另開 issue 處理」與 reviewer 原建議「上線後觀察真實追問再決定」一致。`已解決` 不要求追蹤位置，寫了也不衝突。
    - 格式：編號 TBD-4 接續、未重編；狀態 `已解決` 是合法值；有結論與日期。理由不足見 NICE TO HAVE 1。
  - 步驟 6 的修改：
    - 新句「照上方步驟 1、4、5 的命令重跑時，要補上這個變數與測試名稱（`planner_plan_distribution` 或 `real_followup_turns`），不指定名稱時兩支會並行」。
    - 兩個名稱與 `crates/wukong-runtime/tests/planner_probe.rs` 的 `async fn planner_plan_distribution`（:36）、`async fn real_followup_turns`（:99）一致。reviewer 用 `--list --ignored <名稱>` 確認各自只篩出 1 支（不帶名稱則為 2 支），所以這兩個名稱當成過濾字串也正確。
    - 「不指定名稱時兩支會並行」符合 libtest 預設的多執行緒行為。
    - 對應關係（步驟 1、4 用 `planner_plan_distribution`，步驟 5 用 `real_followup_turns`）在步驟 4、5 的本文已寫明，讀者可推得，不另列發現。
- 相關失敗面：
  | 失敗面 | 輸入／狀態 | 預期 | 覆蓋 | 判定 |
  |---|---|---|---|---|
  | 文件夾帶非文件變更 | ae73902..85e3d28 | 只有文件 | `--name-status` 與 crates diff `cmp` 相同 | 通過 |
  | 照文件重跑卻兩支並行（觸發真實回合副作用） | 照步驟 1 命令重跑 | 文件提醒指定名稱 | 步驟 6 新句；名稱經 `--list` 驗證 | 通過 |
  | 決策紀錄與程式行為不符 | TBD-4「隔多久都帶入」 | 程式沒有時間條件 | 讀 turn.rs:467～477 | 通過 |
  | 報告提交被夾帶其他內容 | 7a13ef5 | 只新增一份報告 | `--name-status` 只有 A，模式 100644 | 通過 |
- 需求、架構、安全、品質：增量只動 issue 文件，不影響產品行為、crate 相依方向或權限。文件用語與既有段落一致，日期為具體日期（2026-10-20）。
- 豁免、待確認與限制：
  - 沒有 gate 豁免。
  - 待確認事項 TBD-1、TBD-3、TBD-4 為 `已解決`，TBD-2 為 `不影響本次交付` 且有後續去向，沒有 `待確認` 項目。
  - 限制：沒有執行真實模型探針（依指示）；全量測試沿用上一輪結果，理由是 crates diff 位元相同。使用者的決定以協調者轉述為準。
  - 本次只新增這份報告，沒有修改既有檔案，沒有 commit、push 或在 GitHub 留言，也沒有碰 ~/.wukong 或 ~/Documents/RunWuKong。
- 自行執行的驗證（2026-10-20，工作區 = HEAD 85e3d28，執行前 `git status` clean）：
  | 命令 | 結果 |
  |---|---|
  | `git diff e4a4e20 85e3d28 \| git patch-id --stable` | ac112650a3c89db784ec682e4d48db72b9f177ee |
  | `git merge-base main 85e3d28`；`gh pr view 15 --json headRefOid,baseRefName,state` | e4a4e20；85e3d28／main／OPEN |
  | `git diff ae73902 85e3d28 --name-status`；`git show --name-status 7a13ef5` | 見上方增量範圍 |
  | `cmp <(git diff e4a4e20 ae73902 -- crates) <(git diff e4a4e20 85e3d28 -- crates)` | 相同 |
  | `cargo test -p wukong-runtime --test planner_probe -- --list --ignored [名稱]` | 不帶名稱 2 tests；`planner_plan_distribution`、`real_followup_turns` 各 1 test（只列出，不執行） |

## 流程判定
PASS
