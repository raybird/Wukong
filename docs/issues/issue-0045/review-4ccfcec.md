# 審查報告
- 範圍：完整交付範圍（`f9bc86e..4ccfcec`，37 個提交、134 個檔案），以增量覆審進行
- Reviewed BASE SHA：f9bc86e73e261f3a7d04257b92eaa7e200787fe2
- Reviewed HEAD SHA：4ccfcec069d6d4332a493b9044d1543b2bbe6665
- Reviewed patch-id：12bc3203a964eab9d48843dcb5f0de44c91ee644（2026-11-27 以 `git diff f9bc86e 4ccfcec | git patch-id --stable` 計算）
- 獨立 reviewer：Claude Code subagent（沒有參與實作，也不是 review-65e960a、review-623c436 的 reviewer）
- 增量覆審：上一份報告是 review-623c436.md（完整交付範圍，PASS，Reviewed HEAD 623c436）；增量範圍 `623c436..4ccfcec`，共 2 個提交：d8f7eab 保存上一份報告與記帳，4ccfcec 是修正。這次增量是為了處理上一份 PASS 的建議。使用者在 README TBD-13 選了要修 NICE TO HAVE 1（Telegram 泡泡回報後回到悟空；實作者查證時發現 Web 的 `/chat` 進度與 Telegram 寫給 Web 的即時事件也一樣，一併修正）和 NICE TO HAVE 4（README 凍結內容兩處）。NICE TO HAVE 2、3 記為已知差距，不修。

增量的三個條件我都核對過：
1. review-623c436 綁定 623c436，記的是完整交付範圍，判定 PASS。623c436 是 4ccfcec 的祖先：d8f7eab 的父提交是 623c436，4ccfcec 的父提交是 d8f7eab。`git log --merges 623c436..4ccfcec` 沒有輸出，也沒有改寫歷史。
2. 本機 `main`（f9bc86e）與 4ccfcec 的 merge-base 仍是 f9bc86e，和上一份報告的 Reviewed BASE 相同。
3. 被審查提交上 `docs/AGENTS.md` 的流程契約是 2.7，不低於 2.6；協調者把上一份報告和新範圍一起交給我。

`**核准 commit**` 是 f0ce34a，不是 `待提交`，所以本報告不另記核准基線。`git diff f0ce34a 4ccfcec -- docs/issues/issue-0045/README.md` 在 `## Gherkin 驗收劇本` 沒有差異。`**核准來源**` 那一行只補了一個「。」（見下方 README 的結論），條件、動作與結果都沒變，原核准仍然有效。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
無。

### NICE TO HAVE

1. **文件說 Telegram 回報後回到「悟空」，畫面上其實是「🐵 思考中…」**：`CHANGELOG.md:32`、`docs/entrypoints.md:32`
   - CHANGELOG 寫「subagent 回報後回到『悟空』」。entrypoints.md 同一句裡先說「悟空自己處理時是『悟空』」，後面又說「回到『🐵　思考中…』」。
   - 實際畫面：Telegram 是 `bubble_text(None, …)`，顯示「🐵　思考中…」（`crates/wukong-telegram/src/dispatch.rs:717-720`）；Web 是 `roleProgressText('')`，顯示「🐵 思考中…」（`crates/wukong-web/static/lib/chat-progress.mjs:3-5`）。兩邊都不會出現「悟空」兩個字，只有排程的階段字面上是「悟空」（`crates/wukong-scheduler/src/executor.rs:302-304`）。
   - 顯示本身合理：它和派工前的文字一樣，🐵 就代表悟空，Telegram 與 Web 也一致。只是讀者照文件找「悟空」兩個字會找不到。
   - 建議下次動到這兩處時改成：「Telegram 與 Web 回到不帶角色的『🐵 思考中…』，排程顯示『悟空』」。

2. **「回報後取目前角色」的寫法在 Telegram 和 Web 各有一份**：`crates/wukong-telegram/src/dispatch.rs:1584-1588`、`crates/wukong-web/src/chat_api.rs:388-392`
   - 兩處都是先 `borrow_mut`，再 `reported(role)`，再 `current()`，目前完全相同，沒有分歧。
   - 可以讓 `ActiveSubagents::reported` 直接回傳 `current()`，或另加一個 `report(role) -> Option<Role>`，兩個入口就各少三行（code-simplify）。

3. **orchestrator 的模組說明還寫「只留角色的定義」**：`crates/wukong-orchestrator/src/lib.rs:3-5`
   - 現在這個 crate 也放了顯示用的狀態 `ActiveSubagents`。根目錄 `AGENTS.md:233` 的 crate 表也只寫角色定義。
   - project.md 只在新增 crate、改變依賴方向或改變 `run_turn` 資料流時要求更新 `AGENTS.md`，這次三者都沒有，所以不算漏更新常青文件。
   - 建議順手在 doc comment 補一句「以及入口顯示用的 `ActiveSubagents`」。

## 已查核維度

### 增量範圍內每個變動檔案的結論

| 檔案 | 提交 | 結論 |
|---|---|---|
| `crates/wukong-orchestrator/src/active.rs` | 4ccfcec | 正確。這是從排程原本的 `Vec<Role>` 邏輯抽出來的：`dispatched` 對應 `push`，`reported` 對應 `rposition` 加 `remove`，`current` 對應 `last`，語意逐行相同。兩項單元測試的期望值是手寫字面值，涵蓋交錯派出、同一角色派出兩次、沒派出過的角色回報，不是同義反覆 |
| `crates/wukong-orchestrator/src/lib.rs` | 4ccfcec | 匯出正確；模組說明見 NICE 3 |
| `crates/wukong-runtime/src/lib.rs` | 4ccfcec | 只是轉出 `ActiveSubagents`，依賴方向不變（runtime → orchestrator 本來就有） |
| `crates/wukong-scheduler/src/executor.rs` | 4ccfcec | 行為不變，見下方「排程改用 ActiveSubagents」 |
| `crates/wukong-telegram/src/dispatch.rs` | 4ccfcec | 正確，見下方「Telegram」 |
| `crates/wukong-telegram/README.md:32` | 4ccfcec | 和程式一致（「改回『🐵 思考中…』」） |
| `crates/wukong-web/src/chat_api.rs` | 4ccfcec | 正確，見下方「Web `/chat`」 |
| `crates/wukong-web/src/lib.rs` | 4ccfcec | 路由 `:804-807` 和其他 `/lib/*.mjs` 的註冊方式相同；靜態資源測試 `:1125-1130` 和新測試 `:1372` 見下方 |
| `crates/wukong-web/src/static_assets.rs:12`、`:50-52` | 4ccfcec | 和 `unread-marker.mjs` 的寫法相同，回 JS content type |
| `crates/wukong-web/static/components/wukong-chat.js:7`、`:877-878`、`:946-948` | 4ccfcec | 即時事件與 `/chat` 兩條路徑都改用 `roleProgressText`，整段都包在 `escapeHTML` 裡，見下方「前端與 XSS」 |
| `crates/wukong-web/static/lib/chat-progress.mjs` | 4ccfcec | 正確。空字串和 `undefined` 都回「🐵 思考中…」 |
| `crates/wukong-web/static/lib/chat-progress.test.mjs` | 4ccfcec | 期望值是字面值；`node --test` 通過 |
| `CHANGELOG.md:31-33` | 4ccfcec | 「都一樣」現在成立了（排程、Telegram、Web 回報後都回到 primary）；措辭見 NICE 1 |
| `docs/entrypoints.md:32`、`:53` | 4ccfcec | Web 的 `role` 事件順序（`step` 之後、空字串表示悟空）和 Telegram 即時事件的說明都和程式一致；Telegram 那句的措辭見 NICE 1 |
| `docs/issues/issue-0045/implementation-plan.md` Task 6.7 | 4ccfcec | 證據和我重跑的數字相同：scheduler 58 項、wukong-web 83 項、workspace 778／0／14。紅燈的描述和我讀舊程式的推論一致 |
| `docs/issues/issue-0045/README.md` | d8f7eab、4ccfcec | 見下方「流程記帳」 |
| `docs/issues/issue-0045/review-623c436.md` | d8f7eab | 新增後沒有再改過（`git log` 只有 d8f7eab）。`verify-artifact.py --reviewed 623c436 --head d8f7eab` 回 OK |

### 上一份報告選修的建議，逐項確認

- **NICE TO HAVE 1：已修好，範圍比原建議大（Web 一併修），擴大的部分有核准。**
  - TBD-13 記了使用者的決定和「查證時發現 Web 也一樣，一併修正」。Task 6.7 的驗收編號是 SCN-002，修的是既有行為，沒有新增驗收。
  - 排程、Telegram、Web 三個入口現在都用同一個 `ActiveSubagents` 判斷，不再各寫一份。
- **NICE TO HAVE 4：已修好。**
  - `**核准來源**`：`git log -G'核准來源'` 顯示 f9bc86e 那一版在「TBD-6」就結束了，f0ce34a 接著寫「2026-11-27 修訂：…」時少了分隔。4ccfcec 補成「TBD-6。2026-11-27 修訂：」，內容沒有其他變動。
  - `## 重構步驟概要` 的 Phase 3～6 改成 ✅。我對照 implementation-plan.md，Task 3.1～6.7 的狀態全部是 ✅ 已完成。
- **NICE TO HAVE 2、3：照使用者決定，沒有修，記在 TBD-13。**
  - 用名稱找角色的寫法仍是兩份：`crates/wukong-scheduler/src/executor.rs:444`、`crates/wukong-runtime/src/turn.rs:162`，比對方式還是相同。

### 排程改用 ActiveSubagents（行為不變）

- `Phase::Turn { running }` 的型別從 `Vec<Role>` 換成 `ActiveSubagents`，`on_change` 的兩個分支換成同名方法。`label` 的 `last()` 改成 `current()`，`failed` 的 `!is_empty()` 改成 `current().is_some()`，兩者等價。
- `SubagentChange` 的來源、`RefCell` 共用 `on_change` 的方式、channel 順序都沒有動。上一份報告查過的重入安全結論仍然成立。
- scheduler lib 測試 58 項全過，含 `the_phase_returns_to_wukong_after_the_subagent_reports` 和 `the_phase_names_the_dispatched_subagent`。這組測試在改動前後都是同一組，符合 verification.md「純重構」的等價證據要求。

### Telegram（`crates/wukong-telegram/src/dispatch.rs`）

- **`RefCell` 會不會在 borrow 期間重入**：不會。
  - `run_turn_traced_with_attachments` 的事件包裝（`crates/wukong-runtime/src/turn.rs:163-181`）依序執行：先呼叫 `on_role` 或 `on_step`，再呼叫入口的 `on_event`。三者是先後呼叫，不是巢狀呼叫。
  - `on_role`（`:1575-1580`）和 `on_step`（`:1581-1592`）的 `borrow_mut()` 都在自己的區塊裡結束：`on_step` 用大括號區塊取出 `current` 後才 `queue_live_event` 和 `send`。這兩個呼叫都不會回頭觸發 callback。
  - 入口的 `on_event` 不碰 `active`。
  - 回合裡 `run_streaming` 會跑兩次（空輸出時的修復，`turn.rs:270-286`），但兩次是先後執行、同一個 task，不會同時借用。
- **即時事件 `role` 的內容與順序**：
  - 派出時寫 `fixer`；回報時寫還沒回報的那一個，都回報了就寫空字串。
  - 寫入都經過同一個 `live_tx`，泡泡的更新都經過同一個 `tx`（`tx_ev` 是它的 clone），所以順序保留。
  - 回答的即時事件在回合結束後才寫，所以空的 `role` 一定排在 `answer` 前面。
  - 新測試斷言 roles 依序是 `["fixer", ""]`，我重跑通過。chat history 接受空字串的 content。
- **`bubble_text(None, …)` 的顯示**：「🐵　思考中…」，有思考內容時接在後面，和派工前（role 是 None 時）相同，所以合理。等待提問時，`Progress::Primary` 只更新 `status`、不顯示，回答後 `resumed_text(None, …)` 顯示「🐵　已收到回答，繼續處理…」，行為一致。文件措辭見 NICE 1。
- **新測試在舊程式下會紅燈**：舊程式的 `on_step` 是 `&mut |_, _| {}`。`DispatchBackend` 只送 SubagentStarted 和 SubagentReported 兩個事件，沒有思考內容，所以「悟空·fixer」之後不會再有任何編輯，第一個斷言就會失敗。即時事件也只有 `["fixer"]`，第二個斷言同樣會失敗。實作者記錄的紅燈（「只有一則『🐵　悟空·fixer 思考中…』的編輯」）和我的推論一致。
- `a_dispatched_subagent_shows_in_the_status_bubble_and_live_events` 仍然通過。

### Web `/chat`（`crates/wukong-web/src/chat_api.rs`）

- **順序**：`role_tx`、`ev_tx`、`step_tx` 都是同一個 `tx` 的 clone，所以是同一個 unbounded channel。`on_step` 先送 `Step`，再送 `Role`（`:378-395`），順序保留。`Answer` 在 `run_turn_traced` 回傳後才送，所以一定排在後面。
- **空字串 SSE 的格式**：`Event::default().event("role").data("")`（`:89`）輸出 `event: role\ndata: \n`，新測試就是以這個字串比對。依 SSE 規格，`data` 欄位會先在資料緩衝區留一個換行，所以緩衝區不是空的，事件照樣派送，去掉結尾換行後 `ev.data` 是 `""`。前端拿到空字串就顯示「🐵 思考中…」。
- **`RefCell`**：和 Telegram 相同的結構，不會重入。
- **新測試在舊程式下會紅燈**：舊程式在 `step` 之後不送任何 `role`；派出時那一個是 `data: fixer`，對不上 `data: \n`，所以 `find` 會失敗、panic。
- **靜態資源**：`serves_static_assets_with_content_types` 補上 `/lib/chat-progress.mjs`，舊程式沒有這條路由，會回 404，測試會紅燈。

### 前端與 XSS

- 兩條路徑（Telegram scope 的即時事件 `handleLiveEvent`，以及 `/chat` 的 `EventSource`）都改成 `escapeHTML(roleProgressText(…))`，跳脫包住整段文字。`roleProgressText` 回傳純文字，模組的說明也寫明「呼叫端負責跳脫」。
- role 本來就只會是五個固定名稱或空字串；就算即時事件被寫進別的內容，也會先跳脫才放進 `innerHTML`。
- 舊寫法遇到空字串會顯示「🐵 悟空· 思考中…」，新寫法改成「🐵 思考中…」。
- `node --test` 的紅燈是「模組不存在」。單看這一層，不算目標行為的紅燈；但外層的 Rust 測試有真的紅燈，而且 node 測試的期望值是字面值。所以雙迴圈的證據完整，不列為缺失。

### 驗收與證據（相對於 review-623c436）

| 編號 | 本輪 | 結論 |
|---|---|---|
| SCN-002 | 重新查核：三個入口在 subagent 回報後都回到 primary。Telegram、Web 各有先紅燈的新測試，排程以同一組測試證明行為不變 | 通過 |
| SCN-001、003～014 | 沿用。增量沒有動到相關程式：`git diff --stat 623c436 4ccfcec` 只涉及上表各檔，runtime 只改了一行 `pub use` | 通過（沿用） |

### 相關失敗面（本輪新增或重新查核）

| 輸入／狀態 | 預期 | 覆蓋 | 判定 |
|---|---|---|---|
| 派出 fixer、回報 fixer（Telegram） | 泡泡回到「🐵　思考中…」；即時事件 `fixer`、`""` | 新測試 | 已覆蓋 |
| 派出 fixer、回報 fixer（Web `/chat`） | `step` 之後、`answer` 之前有空的 `role` | 新測試 | 已覆蓋 |
| 交錯派出 explorer、fixer，fixer 先回報 | 顯示 explorer；都回報後回到悟空 | `ActiveSubagents` 單元測試；入口只是轉呼叫，沒有入口層的測試 | 可接受 |
| 同一角色派出兩次、回報一次 | 仍顯示該角色 | 單元測試 | 已覆蓋 |
| 回報的角色沒有派出紀錄（漏了開始事件） | 狀態不變；入口送出目前的角色（可能是空字串） | 單元測試（`Designer`）；入口讀程式 | 可接受 |
| 回報時正在等提問 | 只更新狀態，不顯示；回答後顯示「已收到回答…」，不帶角色 | 讀程式（`run_progress` 的 `question_waiting`） | 可接受，沒有專屬測試 |
| 空字串 SSE | 瀏覽器照樣派送，`ev.data === ''` | Rust 測試確認輸出 `data: \n`；SSE 規格 | 已覆蓋（瀏覽器端沒有實測） |
| 惡意或非預期的 role 字串 | 跳脫後才放進 `innerHTML` | 讀程式 | 可接受 |
| 排程的 `failed` 判斷 | 和舊的 `!is_empty()` 等價 | scheduler 58 項 | 已覆蓋 |

### 需求、架構、安全、品質

- **需求**：Web 那一半超出 NICE 1 的原文，但 TBD-13 記了使用者的決定（「查證時發現 Web 也一樣，一併修正」）。修的是 SCN-002「入口看得到誰在處理」的既有行為，沒有改任何 Scenario 的可觀察結果，也沒有新增驗收。
- **架構**：沒有新的 crate 依賴。`ActiveSubagents` 放在 orchestrator，和 `Role` 同一個 crate，runtime 轉出給沒有直接依賴 orchestrator 的 web 與 scheduler 用。Telegram 本來就直接依賴 orchestrator（`Cargo.toml:19`），寫法和它引用 `Role` 的方式一致。
- **安全**：前端的跳脫見上方。新路由只提供一個內嵌的靜態 JS，和其他 `/lib/*.mjs` 的待遇相同。
- **品質**：三個入口共用同一個型別，解掉了「各寫一份」的風險。剩下的小重複見 NICE 2，沒有過度設計，`Progress::Primary` 是最小的新變體。

### 流程記帳

- d8f7eab：新增 review-623c436.md；README 的 `**現行 review**` 改成 review-623c436.md，Timeline 多一列。verify-artifact 回 OK，`## 現況` 的位置仍在 `## 概述` 之後。
- 4ccfcec 的 README 變更：
  - 記帳區段：TBD-13、Timeline 兩列。
  - 凍結內容：`**核准來源**` 補分隔、`## 重構步驟概要` Phase 3～6 改 ✅。這兩處都在本次增量範圍內，上面已審過。
- implementation-plan.md、常青文件和程式的變更都在 4ccfcec，上表逐檔審過。
- `verify-artifact.py --reviewed 623c436 --head 4ccfcec` 回 `INVALID`（CHANGELOG.md 有變更，需要重新審查），符合預期，本報告就是那次重審。
- `**狀態**` 仍是「實作中」，沒有改。保存本報告的提交依流程把 `**現行 review**` 改成 review-4ccfcec.md。

### 我執行的驗證（HEAD 4ccfcec，工作樹乾淨，2026-11-27）

| 命令 | 結果 |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --all-targets --locked -- -D warnings` | exit 0（本機 toolchain；W-9：CI 的 stable 可能較新） |
| `cargo test --workspace --locked --no-fail-fast` | exit 0；778 passed、0 failed、14 ignored，和 Task 6.7 的數字相同 |
| `cargo test --locked -p wukong-orchestrator -p wukong-scheduler -p wukong-telegram -p wukong-web` | 都通過：orchestrator 4、scheduler lib 58、telegram lib 48、web lib 83 |
| `cargo test --locked -p wukong-telegram -p wukong-web -- returns_to_wukong returns_the_role` | 兩項新測試都有跑到並通過 |
| `node --test crates/wukong-web/static/lib/*.test.mjs`（node v22.20.0） | 18 pass、0 fail |
| `verify-artifact.py --repo . --reviewed 623c436 --head d8f7eab --artifact docs/issues/issue-0045/review-623c436.md` | OK（新增報告、Timeline、現況、現行 review） |
| `verify-artifact.py --repo . --reviewed 623c436 --head 4ccfcec --artifact docs/issues/issue-0045/review-623c436.md` | INVALID，需要重新審查（符合預期） |
| `git diff f0ce34a 4ccfcec -- docs/issues/issue-0045/README.md` | `## Gherkin 驗收劇本` 沒有差異 |
| `git log -G'核准來源' -- docs/issues/issue-0045/README.md` 與各版內容 | 確認「TBD-6」原本在行尾結束，補「。」是正確的分隔 |
| `git ls-remote origin refs/heads/main` | b65b487（見下方的交付提醒） |

### 沿用的結論

這些項目的程式與證據都沒有在增量裡變動，沿用 review-623c436：
- SCN-001、003～014 的驗收結論與測試真偽的抽查；
- 上一份報告「相關失敗面」表中，本輪沒有重列的各列（背景 shell、job 沒有 `kind`、828fe85 遷移、人格那一句的提醒等）；
- 派工評估原始紀錄的重判、TBD-10、TBD-11 的結論；
- 架構相依圖與 primary 權限。

其餘沿用 review-623c436.md。

### 豁免、待確認與限制

- **豁免**：沒有 Gate 豁免紀錄。
- **待確認事項**：TBD-1～TBD-13 都已解決，狀態我沒有改。
- **限制**（本輪新增）：
  - 舊程式下的紅燈，我是讀 623c436 的程式推論的，沒有另開 worktree 在舊版上實跑。
  - 我沒有開瀏覽器確認 `wukong-chat.js` 的兩條路徑。前端只有抽出來的 `roleProgressText` 有自動化測試。
  - 我沒有呼叫真實模型，也沒有啟動 V2 server、Telegram 或 docker。
- **交付提醒**（沿用上一份報告）：遠端 `origin/main` 仍是 b65b487，本機 `main` 是 f9bc86e，三個 issue 文件提交還沒推送。PR 若以遠端 main 為目標，merge-base 會和本報告的 BASE 對不上。開 PR 前先推本機 main，或者依 review-evidence.md「固定交付範圍」重新確認範圍。

## 流程判定
PASS

PASS 只適用於上列 Reviewed HEAD SHA；之後提交 README 記帳區段與本報告以外的檔案會讓它失效，提交前先告知使用者（review-evidence.md「審查之後的提交」）。
