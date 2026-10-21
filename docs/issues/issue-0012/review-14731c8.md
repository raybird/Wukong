# 審查報告
- 範圍：完整 PR（#19）
- Reviewed BASE SHA：5f756370c224f3899c1084a40a215934fc7d614a
- Reviewed HEAD SHA：14731c86485aca82c7fe5b4aeb70d4c089fb66d5
- Reviewed patch-id：4c882d044c39292df55047685663e77709622de6
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-21
- Review artifact：docs/issues/issue-0012/review-14731c8.md

範圍核對（2026-10-21）：

- `git ls-remote origin`：`issue-0012-lease-lookup-fix` 指向 `14731c8…`，`main` 指向 `5f75637…`。
  main 就是 BASE，所以 merge-base 也是 `5f75637`。
- 本機 `git diff 5f75637 14731c8 | git patch-id --stable` 得 `4c882d04…`，與指定值相同。工作區乾淨。
- 範圍內 4 個提交：`83abdac`（M-1 修正＋S-1 測試，cherry-pick 自 `4fa03e5`）、`0004c01`（文件，cherry-pick
  自 `6f4330d`）、`0a2eaba`（S-3，只動 `turn.rs` 2+/2-）、`14731c8`（文件與 `review-6f4330d.md`）。
- 異動檔案：`crates/wukong-memory/src/lib.rs`、`crates/wukong-runtime/src/turn.rs`、
  `docs/issues/issue-0012/README.md`、`implementation-plan.md`，新增 `review-6f4330d.md`。

## 問題與風險

### MUST FIX

無。

### SHOULD FIX（不阻塞）

**S-6　README Timeline 把「審查退回」記在合併之後，與實際先後相反**

- 現況：Timeline 依序是「2026-10-20 PR #18 由使用者合併……合併的是審查退回的版本」，接著
  「2026-10-21 PR #18 審查退回（M-1……），改為 best-effort」。
- 實際：`review-cfae511.md` 的審查日期是 2026-10-20，判定 RETURN；這份報告由 `989a890`
  （2026-10-20 17:54 +08）提交，也就是 PR #18 被合併的那個 head。PR #18 在 2026-10-20 21:33 +08 合併。
  所以退回在前、合併在後，而且合併進 main 的內容本身就含有那份 RETURN 報告。
- 讀者照現在的 Timeline 會以為審查是合併之後才退回的。這和同一表中「合併的是審查退回的版本」自相矛盾。
  原因是這一列是 `6f4330d`／`0004c01` 在不知道 PR #18 已合併時寫的，事後補上合併列時沒有調整。
- 修法：拆成兩列並依時間排序：
  - `2026-10-20｜PR #18 審查退回（review-cfae511.md，M-1 查詢失敗殘留 lease）`，放在合併列之前。
  - `2026-10-21｜改為 best-effort；使用者對 D-1 選維持現狀（TBD-5）`。
- 不列 MUST FIX 的理由：不涉及測試證據或程式行為；合併列本身已如實寫明「合併的是審查退回的版本」，
  沒有掩蓋事實，只是時序呈現有誤。

**S-7　沒有記錄「為什麼不寫 Gate 豁免紀錄」，讀者會以為 D-2 的第 2 小題被遺漏**

- `review-6f4330d.md` 的 D-2 有三個小題：交付方式、要不要記 Gate 豁免、issue #12 要不要重新開啟；
  並建議 (a) 記豁免。現行文件（plan 步驟 6 的 D-2、README Timeline）只記錄了第 1 小題的結論
  （使用者選開後續 PR），第 2、3 小題沒有結論，也沒有說明不記豁免的理由。
- 判斷：**不寫 Gate 豁免紀錄的處理本身符合規定**（理由見「豁免、待確認與限制」）。缺的只是把理由寫下來。
- 修法：在 plan 步驟 6 的 D-2 補一句，例如：
  「未記 Gate 豁免：docs/AGENTS.md 的豁免以使用者明確要求跳過 gate 為前提、須記原話；PR #18 的合併沒有
  這樣的表示。修正改由 PR #19 經獨立審查後進 main，因此 issue 交付的最終內容沒有未審查的部分。」
  第 3 小題的結論見下方 D-3。

### 需決策

**D-3　issue #12 已在 2026-10-20 關閉，PR #19 的 `Closes #12` 不會有任何效果**

- 事實：`gh issue view 12`：`state=CLOSED`、`closedAt=2026-10-20T13:33:58Z`、`stateReason=COMPLETED`，
  由 PR #18 自動關閉。GitHub 只在 issue 開啟時才會依 closing keyword 關閉它；對已關閉的 issue，
  `Closes #12` 只會建立連結（`closedByPullRequestsReferences` 已經同時列出 #18 與 #19），合併時不會改變任何狀態。
- 落差：README `**狀態**` 是「實作中」，main 也確實帶著 M-1，但 GitHub 上 issue 顯示已完成。
  落差只到 PR #19 合併為止，期間沒有發版（見下方），所以影響是追蹤上的不準確，不是使用者可見的缺陷。
- 待決問題：issue #12 在 PR #19 合併前要不要重新開啟。
- 可選做法：
  - (a) 現在重新開啟 #12（可附一句「PR #18 合併的版本有審查退回的 M-1，修正見 #19」），PR #19 維持
    `Closes #12`，合併後自動關閉。追蹤狀態與 README 一致，事後也看得出結案是由 #19 完成。
  - (b) 維持關閉，把 PR #19 說明改成 `Refs #12`，並在 #12 留言說明 main 上的缺陷與修正 PR。
  - (c) 不處理。`Closes #12` 留著無害，但 GitHub 上看不出 #12 曾在缺陷狀態下被標為完成。
- reviewer 建議：(a)。成本最低，且讓 GitHub 狀態在合併前後都與 README 一致。
  重新開啟或留言都是對外的 GitHub 操作，需使用者授權。

## 已查核維度

- 驗收與證據：
  - **程式碼等於已複審的修正加上 S-3。** `git diff 6f4330d 14731c8 -- crates` 只有一處變動：
    `previous_turn_records` 的兩行英文 doc comment 從 `notice_for_other_turns` 上方移到
    `previous_turn_records` 正上方，沒有任何其他程式碼差異。因此 `review-6f4330d.md` 對 M-1、S-1 的程式碼
    審查結論與變異實驗可以沿用到本 HEAD；依 review-evidence.md「相同 patch 不自動放行新 HEAD」，以下
    重新驗證了 fmt、clippy、全量測試、CI，並在本 HEAD 上重做 M-1 的變異探針。
  - **M-1 修正正確（自行確認）。** `turn.rs` 第 197 行 `prepare_final_session(...).await?` 取得 lease 之後、
    第 206 行 `let result = async {` 之前，只剩四個欄位讀取／clone，以及
    `notice_for_other_turns(memory.active_session_leases().await, &cfg.scope)`；這個呼叫回傳 `Option<String>`，
    沒有 `?`。`notice_for_other_turns` 的 `Err` 分支印 `concurrent_turn_lookup_failed` 後回傳 `None`；`Ok`
    分支過濾掉自己的 scope，沒有其他回合時回傳 `None`。`persona::concurrent_turns_notice` 與 `scheduling_bin`
    只做字串組合與讀環境變數，不會 panic。`result` 為 `Err` 時第 424–428 行釋放 lease 的路徑不變。
  - **S-3 已正確修復。** HEAD 的 `notice_for_other_turns` 上方只剩它自己的三行中文說明；
    `previous_turn_records` 正上方恢復 `/// The scope's previous turn (its two newest turn memories), oldest first.`
    與 `/// Called before this turn is remembered.`，文字與 `5f75637` 完全相同。
  - **變異探針（本 HEAD 重做）。** 在 scratchpad 的 git worktree（`14731c8`）把
    `Store::active_session_leases` 的表名改成不存在的 `agent_session_state_missing`，加入既有探針
    `m1_probe_lookup_failure_keeps_turn_and_releases_lease`，`CARGO_TARGET_DIR=…/scratchpad/mut-target`：
    - 修正版：印出 `concurrent_turn_lookup_failed scope=project:T: … no such table: agent_session_state_missing`，
      `turn_ok=true lease_until=None`，探針通過。
    - 再把呼叫處改成 `notice_for_other_turns(Ok(memory.active_session_leases().await?), &cfg.scope)`：
      探針紅燈 `turn_ok=false lease_until=Some(1792546747)`（取得時間＋900），
      `notice_lookup_failure_is_ignored` 仍綠燈——與複審 S-5 記錄的殘餘缺口一致。
    - 實驗後已 `git worktree remove`，主工作區未改動。
  - **PR #19 Proof 表與證據一致。**
    - 審查 M-1 列：紅燈 `turn_ok=false`、lease 殘留到取得時間＋900，綠燈 `turn_ok=true`、lease 已釋放，
      與 plan 步驟 6、`review-6f4330d.md` 及本次重做結果相符。
    - SCN-003（補強）列：「now 換成 0、`i64::MAX` 或毫秒時都會紅燈」中，0 是實作者的紅燈，
      `i64::MAX`、毫秒是複審者的變異結果（`review-6f4330d.md`），該報告已在 PR 說明中引用，可接受。
    - SCN-001、002、004 列沿用 PR #18 證據：本 PR 只改了查詢失敗分支，沒有錯誤時的行為由
      `notice_for_other_turns` 的 `Ok` 分支保持原本的過濾與組字串邏輯，相關測試在本次全量測試中都通過。
    - 已核准驗收編號集合（SCN-001～004）都在表內；M-1 是審查項目，另列一列，合理。
    - 交付範圍 `5f75637..14731c8`、規格 `802cb07:docs/issues/issue-0012/README.md` 與本報告範圍一致。
    - 全量數字 passed=641、failed=0、ignored=12 與本次自行執行結果相同。
  - **「這段期間沒有發布新版，正式部署不受影響」屬實（截至 2026-10-21）。**
    - `git tag --contains 5f75637` 無輸出；最新 tag `v0.22.1`（2026-10-07），`gh release list` 最新也是
      v0.22.1。
    - 映像只在推 `v*` tag 時由 `release.yml` 發布；`ci.yml` 不發布映像。
    - 本機 `docker ps`（唯讀）顯示 wukong-web／wukong-telegram／wukong-schedulerd 都跑
      `ghcr.io/raybird/wukong:v0.22.1`，不含 `5f75637`。未進入 `~/Documents/RunWuKong`。
    - 這句話對「到本 PR 合併」的部分是預期而非已發生；只要 PR #19 合併前不打 tag 就成立。
- 相關失敗面：

  | 失敗面 | 輸入／狀態 | 預期行為 | 覆蓋 | 結論 |
  |---|---|---|---|---|
  | 告知查詢失敗 | 資料庫錯誤（不存在的表） | 回合成功、不附告知、lease 釋放、印一行日誌 | 單元測試 `notice_lookup_failure_is_ignored`＋本 HEAD 變異探針 | 通過 |
  | 呼叫處被改回 `?` | 重構時誤改 | 應該被測試抓到 | 只有變異實驗抓得到，單元測試仍綠 | 已知殘餘缺口（S-5），已在 PR 限制中揭露，不阻塞 |
  | 查詢只回傳自己 | 只有本回合持有 lease | 不附告知 | `notice_lookup_failure_is_ignored` 第二個斷言 | 通過 |
  | Memory 層過期判斷 | 1970 年取得的過期 lease＋剛取得的 lease | 只回傳後者 | `active_session_leases_uses_the_current_time` | 通過；複審已驗證 0／`i64::MAX`／毫秒三種錯誤會紅燈 |
  | S-3 註解搬移破壞編譯或格式 | 只動註解位置 | 無行為變化 | fmt、clippy、全量測試、CI | 通過 |
  | main 在合併前被發版 | 5f75637 被打 tag | 帶 M-1 的版本出貨 | tag／release 查核 | 目前沒有；合併前不要打 tag |

- 需求、架構、安全、品質：
  - 範圍只在 `wukong-runtime`（`turn.rs`）與 `wukong-memory` 的測試模組，沒有新相依、沒有 schema 變更，
    符合根目錄 AGENTS.md 的 Monorepo 最小範圍原則。
  - 失敗策略（查不到就不告知）與 README 風險評估「失敗後果等於回到現狀」一致；錯誤訊息只含 scope 與資料庫
    錯誤字串，不含敏感資料。
  - 手術式修改：`0a2eaba` 修正了前一輪修正對 `previous_turn_records` 造成的損傷，沒有改到其他程式碼。
  - CHANGELOG：`[Unreleased]` 已有告知功能的條目；M-1 從未出貨，不需要另寫修正條目。根目錄 AGENTS.md
    的告知機制說明不受「查詢失敗時略過」影響，不需要更新。
  - TBD-5（多使用者時列出其他人的 chat id）已由使用者在 2026-10-21 決定維持現狀，有理由與後續方向，
    在 SCN-001 核准範圍內，不重複列出。
- 豁免、待確認與限制：
  - **不寫 Gate 豁免紀錄：符合 docs/AGENTS.md 與 review-evidence.md。** 理由：
    1. docs/AGENTS.md「Gate 豁免紀錄」的觸發條件是「使用者明確要求跳過 gate」，必記「使用者原話」；
       review-evidence.md 2.7 的對應條件是「使用者明確決定不重新審查」。PR #18 的合併是一個動作，
       沒有使用者的明確表示可以引用。在沒有原話的情況下寫豁免紀錄，等於替使用者編寫授權，反而違反規定。
    2. 使用者後來選擇開後續 PR 修正，這與「決定接受未審查內容」的意思相反。
    3. Gate 的作用是確保 issue 交付的內容經過審查。PR #19 合併後，main 上 issue-0012 的程式碼等於
       `review-6f4330d.md` 與本報告審查過的內容，沒有未經審查而留存的部分；不需要用豁免來掩護缺口。
    4. 合併事實已誠實記在 README Timeline（「合併的是審查退回的版本 `989a890`，所以 main 帶有 M-1」），
       讀者不會被誤導認為 PR #18 曾取得 PASS。
    - 需補的只有理由說明（S-7）與 Timeline 的時序（S-6）。若使用者其實是有意在 RETURN 下合併、並願意說明，
      那時才依規定把原話、日期、豁免項目「PR #18 未取得 PASS 即合併」與殘餘風險（本 PR 合併前 main 上查詢失敗
      會讓該 scope 最多 900 秒無法執行回合）記入 `## Gate 豁免紀錄`。
  - issue #12 的 GitHub 狀態見 D-3。
  - S-5（呼叫處的回歸只有變異證據）已在 PR 限制中揭露，屬揭露的測試限制，不構成缺失。
  - PR #19 合併後，README `**狀態**` 應改為 `已完成`，Timeline 記錄 PR #19 的實際合併日期與 main SHA；
    這是合併後的工作，不屬本次審查範圍。
- 自行執行的驗證（2026-10-21，本機 HEAD `14731c8`，工作區乾淨）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test --workspace --locked`：exit 0；51 個 test binary 合計 passed=641、failed=0、ignored=12，
    與實作者記錄相同。其中 `tests::active_session_leases_uses_the_current_time`、
    `turn::tests::notice_lookup_failure_is_ignored`、`turn::tests::failed_turn_releases_session_lease` 都是 ok。
    （`-p wukong-memory -p wukong-runtime` 包含在全量測試內，未另外重跑。）
  - `gh pr checks 19`：`fmt · clippy · test` pass（run 39526905135，`headSha=14731c86…`，event `pull_request`）。
  - 變異探針：見「驗收與證據」，worktree 已移除。
  - 未呼叫真實模型，未讀寫 `~/.wukong` 或 `~/Documents/RunWuKong`。

## 流程判定
PASS

理由：本 PR 的程式碼等於已複審 PASS 的修正加上只動註解位置的 S-3；M-1 修正在本 HEAD 上重新以變異實驗確認，
fmt、clippy、全量測試與 CI 都通過；PR 說明的範圍、SHA、Proof 與限制都與證據相符。S-6、S-7 是文件準確度問題，
D-3 是 GitHub 追蹤狀態的處理方式，三者都不影響程式行為與驗證，不阻塞。
