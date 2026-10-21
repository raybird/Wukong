# 審查報告
- 範圍：完整 PR（修正後複審）
- Reviewed BASE SHA：1af4528e12918a10a9eb687bd77a3be887054c15
- Reviewed HEAD SHA：6f4330d7f1d8b7d84bbd40eed1cdf1f7394d9644
- Reviewed patch-id：938ce6df3816712fc34c696830ed478419019985
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-21
- Review artifact：docs/issues/issue-0012/review-6f4330d.md

範圍核對（2026-10-21）：本機 `git rev-parse HEAD` 為 `6f4330d…`，工作區乾淨；
`git diff 1af4528 6f4330d | git patch-id --stable` 得 `938ce6df…`，與指定值相同。
修正提交為 `4fa03e5`（程式）與 `6f4330d`（文件），`989a890` 只新增上一輪報告。

**本地範圍與遠端 PR 不一致（依 review-evidence.md「固定交付範圍」先指出）**：

- `gh pr view 18`：PR #18 已於 2026-10-20 21:33:57（+08:00）由 raybird **squash 合併**進 main，
  合併提交 `5f75637`，PR 的 headRefOid 是 `989a890`。遠端的 `issue-0012-concurrent-turn-notice`
  分支已刪除（`git ls-remote` 只剩 main），issue #12 同時被關閉。
- `5f75637` 的 tree（`f2672a0`）與 `989a890` 的 tree 相同。也就是說，**目前 main 含有上一輪的 M-1**
  （查詢失敗時 lease 殘留），還含有判定為 RETURN TO execute-task 的 `review-cfae511.md`。
- 本次修正（`4fa03e5`、`6f4330d`）只在本機，不屬於任何 PR。相對於 main 的差異等於
  `git diff 989a890 6f4330d`（4 個檔案，+73／−11，patch-id `43e2d9b3e543cfc74a52d3e11a129ace8af79965`）。

因此，本報告的判定只綁定上列本機範圍 `1af4528..6f4330d`，**不能代表 PR #18**。
處理方式見下方「需決策」D-2。

## 問題與風險

### MUST FIX

無。

### SHOULD FIX（不阻塞）

**S-3　新函式插在既有 doc comment 與它的函式之間，`previous_turn_records` 的說明被移到 `notice_for_other_turns` 上**

- 位置：`crates/wukong-runtime/src/turn.rs` 約 473–491 行。
- 現況：`/// The scope's previous turn (its two newest turn memories), oldest first. /// Called before this turn is remembered.`
  原本是 `previous_turn_records` 的 doc comment。`4fa03e5` 把 `notice_for_other_turns` 插在這兩行之後，
  結果 rustdoc 把這兩行和新的中文說明合併成 `notice_for_other_turns` 的文件，第一句變成錯的描述，
  而 `previous_turn_records` 失去了說明。
- 修法：把新函式整段（含它的三行中文說明）移到這兩行 `///` 之前，或移到 `previous_turn_records` 之後。
  行為不變，不需要重跑紅綠燈。
- 不列 MUST FIX 的理由：沒有執行語意，屬可讀性問題（review-evidence.md：純可讀性建議不阻塞）。
  但這是本次修正對既有程式碼造成的損傷（違反根目錄 AGENTS.md「手術式修改」），建議在下一個提交一併修正。

**S-4　「PR 說明已更正」與平台上的狀態不符；issue 文件沒有記下 PR #18 已合併**

- `implementation-plan.md` 步驟 6 寫「S-2 措辭：已更正步驟 4 與 PR 說明」。GitHub 上 PR #18 的說明仍然寫
  「後開始的排程回合兩棒都收到」，更正後的文字只存在本機草稿 `scratchpad/pr12-body.md`（2026-10-21 09:06）。
  PR 已合併，這份草稿實際上會給新的 PR 使用。
- README 的 Timeline 寫「2026-10-21 PR #18 審查退回……改為 best-effort」，狀態為「實作中」，沒有提到
  PR #18 已於 2026-10-20 合併、issue #12 已關閉。讀者會以為修正會經由 PR #18 交付。
- 修法：把步驟 6 的措辭改為「已更正步驟 4；PR 說明草稿已更正，用於後續 PR」。在 Timeline 補一列
  2026-10-20 PR #18 合併的事實，以及 D-2 的處理結論。
- 不列 MUST FIX 的理由：這兩處都不涉及測試證據。驗證結果本身（紅綠燈、變異、全量測試）都經 reviewer
  重做確認為真（見下方），所以不構成驗證方面的不實回報。

**S-5（證據持久力）　呼叫處的回歸沒有自動化測試**

- `notice_lookup_failure_is_ignored` 守住的是 helper 本身的語意。如果有人把呼叫處改回
  `notice_for_other_turns(Ok(memory.active_session_leases().await?), …)`，這支測試仍然是綠燈
  （下方變異 `callsite_regression` 實測）。呼叫處只由 implementation-plan 記錄的變異實驗守住。
- 「資料庫錯誤在 runtime 測試中無法以合理成本注入」這個說法成立：
  - 取得 lease 與這次查詢之間沒有任何回呼或 backend 呼叫可以當作注入點。
  - 查詢與取得 lease 讀的是同一張 `agent_session_state` 表，不能靠刪表讓查詢失敗、又讓取得 lease 成功。
  - `Memory` 的 store 對 runtime crate 不可見。要注入，得替 `Memory` 加測試專用的失敗開關，或改用 trait 抽象。
    為一行軟性告知做這些，成本與收益不相稱。
- 型別設計也降低了這個回歸發生的機率：helper 吃 `Result`，把 `?` 放在呼叫處需要刻意寫成 `Ok(…?)`。
  所以單元測試加上變異實驗作為證據是足夠的。這一項只記錄殘餘缺口，不要求修改。

### NICE TO HAVE

- 上一輪的 N-1～N-6 都沒有處理，仍然不阻塞。
  - N-1：`implementation-plan.md` 第 7 行仍寫「回傳 `(scope, lease_until)`」，實作只回傳 scope。
  - N-6：issue #12 已經隨 PR #18 關閉，可以在 issue 留言指向 README 與後續 PR。
- 根目錄 `AGENTS.md`「同時執行的回合告知」可以補一句「查詢失敗時略過告知，不影響回合」。
  這是本次新增的行為，目前只寫在程式註解與 issue 文件中。

### 需決策

**D-2　PR #18 在 RETURN 判定下已合併，main 含有 M-1；修正要怎麼交付、這次合併要怎麼記錄**

- 待決問題：
  1. 修正要怎麼交付：開新 PR，還是用其他方式。
  2. 這次合併沒有取得 PASS，要不要在 README `## Gate 豁免紀錄` 記錄。
  3. issue #12 要不要重新開啟。
- 可選做法：
  - (a) 從 main（`5f75637`）開新分支，cherry-pick `4fa03e5` 與 `6f4330d`（另視需要加上 S-3、S-4 的修正），
    開新 PR 並寫 `Refs: #12`。在 README 記錄 PR #18 的合併事實，並依使用者的說法記入 Gate 豁免紀錄：
    豁免項目是「PR #18 未取得 PASS 即合併」，殘餘風險是「後續 PR 合併前，main 上告知查詢失敗會讓該 scope
    最多 900 秒無法執行回合」。issue #12 重新開啟，或由新 PR 關閉。
  - (b) 同 (a)，但不記豁免，把 PR #18 當成流程外合併，在 Timeline 說明即可。
- reviewer 建議：(a)。docs/AGENTS.md 規定使用者要跳過 gate 時要記錄日期、使用者原話與殘餘風險。
  此外 main 目前確實帶著已知缺陷，豁免紀錄可以讓後續讀者知道 main 上有一段期間不符合審查結論。
- 新 PR 的審查有效性：如果新 PR 的 `git diff 5f75637 <新 HEAD>` 與 `git diff 989a890 6f4330d`
  完全相同（patch-id `43e2d9b3…`），而且新 HEAD 的 tree 與 `6f4330d` 相同，那麼新 PR 的內容就是本報告
  審查過的內容。但依 review-evidence.md，相同 patch-id 不會自動放行新 HEAD。協調者仍需在新 PR 上核對範圍、
  規格與測試（至少比對 tree，並跑一次 CI）。若加入 S-3、S-4 的修正，那個提交只動註解位置與文件，
  可以由協調者決定做局部複審，或記錄豁免。

D-1 已由使用者決定（TBD-5），本輪不再列出。

## 已查核維度

- 驗收與證據：
  - **M-1（上一輪 MUST FIX）已修好。**
    - 在 `turn.rs` 中，`prepare_final_session` 取得 lease 之後、`let result = async {…}` 之前，只剩四個
      `clone`／欄位讀取，以及 `notice_for_other_turns(memory.active_session_leases().await, &cfg.scope)`。
      這個呼叫不再回傳 `Result`，沒有 `?`。
    - 確認 helper 本身不會 panic：`persona::concurrent_turns_notice` 只做字串組合；`scheduling_bin()`
      只讀環境變數，有預設值。所以這段區間已經沒有會失敗的操作，恢復了上一輪指出的不變式。
    - `prepare_final_session` 內部的錯誤路徑都會先釋放 lease，上一輪已查核過，這次沒有修改。
  - reviewer 在獨立 worktree 重做的變異實驗（`CARGO_TARGET_DIR=scratchpad/mut-target`，結果見
    `scratchpad/rev2/mutants.log`）：

    | 變異 | 預期 | 結果 |
    |---|---|---|
    | 查詢的資料表改成不存在的 `agent_session_state_missing`，加探針（跑一回合，斷言回合成功、lease 可重新取得） | 綠 | 綠：印出 `concurrent_turn_lookup_failed scope=project:T: … no such table`，`turn_ok=true lease_until=None` |
    | 同上，再把呼叫處改回 `Ok(…await?)` | 探針紅 | 探針紅：`turn_ok=false lease_until=Some(1792545886)`；`notice_lookup_failure_is_ignored` **仍綠**（S-5） |
    | 錯誤分支改成 `panic!` | 紅 | `notice_lookup_failure_is_ignored` 紅 |
    | 錯誤分支改成回傳 `Some(告知)` | 紅 | 紅（第 826 行斷言） |
    | 不過濾自己的 scope | 紅 | 紅（第 833 行斷言） |

    結論：實作者記錄的紅燈（`turn_ok=false lease_until=Some(取得時間+900)`）與綠燈
    （`turn_ok=true lease_until=None`）都重現得出來。回歸測試對「錯誤時 panic」「錯誤時回傳 Some」
    「未過濾自己」三種錯誤實作都會紅燈。
  - **S-1 已補上。** `active_session_leases_uses_the_current_time` 建立兩筆 lease：一筆在 1970 年
    （now=100）取得、lease_until=105，另一筆用真實時間取得、lease_until=now+900。測試斷言只回傳後者。
    變異結果：wrapper 傳 `0`、`i64::MAX`、`now_unix() * 1000`（秒誤寫成毫秒）都紅燈，不改則綠燈。
    這支測試會漏掉的只有偏差小於約 900 秒的時鐘錯誤，對這個用途可以接受。
  - **S-2 已更正，與紀錄相符。** 檢查 `scratchpad/repro12/after2/ws/prompt-*.txt`：
    - 排程回合的 planner prompt（`prompt-schedule-3096796.txt`）沒有告知。
    - oracle 那一棒（`3097141`）與以 `[修復回覆]` 開頭的修復呼叫（`3097279`）都含
      `[其他回合正在執行]` 與 `- 對話 user:tg-42`。
    - 聊天回合的三個 prompt 都沒有告知。
    這與步驟 4 更正後的措辭一致。PR 說明的部分見 S-4。
  - SCN-001～004：被測的產品行為只在「查詢失敗」這條路徑上改變。`Ok` 分支的過濾與組字和
    `cfae511` 的寫法等價（同樣先過濾自己、清單為空時回 `None`，同一個 `concurrent_turns_notice` 呼叫），
    `concurrent_turns_notice`、store 查詢與 SQL 都沒有修改。因此上一輪對 SCN-001～004 的查核
    （單元測試變異、跨版本 prompt 比較、重現腳本）可以沿用。各 SCN 的測試在本輪全量測試中也都是綠燈。
- 相關失敗面：

  | 失敗面 | 輸入／狀態 | 預期 | 覆蓋 | 判定 |
  |---|---|---|---|---|
  | 告知查詢失敗 | 資料庫錯誤 | 回合成功、不附告知、lease 正常釋放、印一行日誌 | 單元測試＋變異探針（上表） | 通過；呼叫處只有變異證據（S-5） |
  | 查詢只回傳自己 | 只有本回合持有 lease | 不附告知 | `notice_lookup_failure_is_ignored` 第二個斷言，`no_filter` 變異紅 | 通過 |
  | 查詢失敗時其實有其他回合 | 資料庫錯誤＋另一回合執行中 | 告知漏報 | 刻意取捨 | 與 README 風險評估「失敗後果等於回到現狀」一致，可接受 |
  | Memory 層時鐘 | wrapper 傳錯 now | 只列有效 lease | `active_session_leases_uses_the_current_time`，三種變異紅 | 通過 |
  | 錯誤訊息洩漏 | `MemoryError` 的 Display | 只寫 stderr | 內容是 SQL／sqlx 錯誤與 scope，與既有 `session_compaction_failed scope=…: {error}` 同級 | 通過 |
  | 上一輪其餘失敗面 | — | — | 被測內容沒有改變 | 沿用 `review-cfae511.md` 的結論 |

- 需求、架構、安全、品質：
  - best-effort 的取捨與 README 風險評估一致。README 把告知定位成軟性提示，失敗後果是「告知不準……
    等於回到現狀」。讓查詢失敗連帶使回合失敗、甚至鎖住 scope，與這個定位相反。現在的做法是漏報不誤鎖，
    符合這個定位。
  - eprintln 格式 `concurrent_turn_lookup_failed scope={own_scope}: {err}`，與既有的
    `session_compaction_backoff_failed scope=…: {reset_error}`、`memory_consolidate_failed scope=…`
    一樣採「snake_case 事件名＋`key=value`＋冒號接錯誤」，風格一致。
  - 架構：沒有新增相依，沒有 schema 變更；`MemoryError` 本來就是 `wukong-memory` 的公開型別。
  - code-simplify：`notice_for_other_turns` 把「查詢結果 → 告知」集中在一處，原本呼叫處的
    `let others … .then(…)` 被搬進 helper，程式碼沒有重複，也沒有新增抽象層。可以再精簡的只有
    `let others: Vec<String>` 的型別標註（可省略），屬個人偏好，不列為問題。唯一的品質問題是 S-3 的 doc comment 位置。
  - 新修正沒有引入新的行為問題。
- 豁免、待確認與限制：
  - TBD-5（D-1）符合 docs/AGENTS.md 對 `不影響本次交付` 的要求：有結論（維持現狀）、理由（目前單一使用者，
    且在 SCN-001 核准範圍內）、日期（2026-10-21），並以「追蹤：無」加上具體的後續處理條件（開放給多個使用者前
    改為只顯示「另一個對話」）。它也符合 review-evidence.md「需決策……結論記入 README `## 待確認事項`」的規定。
    「目前只有單一使用者」是部署事實，依指示 reviewer 沒有讀取 `~/Documents/RunWuKong` 驗證，
    只能採信使用者的說法。
  - 驗收區塊與核准 commit `802cb07` 比對：只多了核准 commit 回填、狀態、TBD-5 與 Timeline，
    Gherkin 與核准紀錄表沒有改動，原核准仍有效。
  - 未處理的流程事項：PR #18 已合併，見 D-2。
- 自行執行的驗證（2026-10-21，工作區為乾淨的 `6f4330d`，repo 自己的 target）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test -p wukong-memory -p wukong-runtime --locked`：exit 0，合計 passed=184、failed=0、ignored=3；
    新增的 `notice_lookup_failure_is_ignored` 與 `active_session_leases_uses_the_current_time` 都是 ok。
  - `cargo test --workspace --locked`：
    - 第一次執行時 `/tmp` 的磁碟配額已滿，`wukong-web` 的 `serves_static_assets_with_content_types` 因
      SQLite `disk I/O error`（code 4874）失敗，cargo 也因此中止。這是環境問題，與本 PR 無關。
    - reviewer 刪除 scratchpad 中兩個舊的建置快取（`base-target`、`m12-target`，都只是 target 目錄，
      不是證據檔）後，以 `--no-fail-fast` 重跑：exit 0，passed=641、failed=0、ignored=12，與實作者紀錄相同。
  - 變異實驗：腳本 `scratchpad/rev2/mutate.py`、`scratchpad/rev2/run-mutants.sh`，結果
    `scratchpad/rev2/mutants.log`。使用 `git worktree`（`scratchpad/rev2/wt`，做完已移除）與
    `CARGO_TARGET_DIR=scratchpad/mut-target`，沒有使用 repo 的 target。
  - 遠端狀態以唯讀方式查詢：`gh pr view 18`、`gh api …/commits/5f75637`、`git ls-remote origin`、
    `gh issue view 12`。沒有執行 fetch、push 或留言。
  - 沒有呼叫真實模型，也沒有碰 `~/.wukong` 或 `~/Documents/RunWuKong`。

  （`scratchpad` 指 `/tmp/claude-1000/-home-kevin-Documents-RCodes-Wukong/aec8f965-8483-49ca-a8fe-11acc774e792/scratchpad`。）

## 流程判定
PASS

理由：
- 上一輪的 M-1 已修正，並有回歸測試與 reviewer 重做的變異實驗作為證據。S-1、S-2 已處理。
  必要驗收 SCN-001～004 的被測內容沒有改變，上一輪的證據可以沿用。本輪沒有 MUST FIX。
- 這個 PASS 只綁定本機範圍 `1af4528..6f4330d`（patch-id `938ce6df…`），**不代表已合併的 PR #18**。
  PR #18 合併時的 HEAD 是 `989a890`，main 上仍有 M-1。
- 修正需要經由新的 PR 交付，並依 D-2 的說明在新 PR 上核對範圍。S-3、S-4 建議在同一個 PR 一併處理。
