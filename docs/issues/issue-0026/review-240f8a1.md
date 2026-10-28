# 審查報告
- 範圍：完整 PR（含審查後修正）。GitHub raybird/Wukong #27，TARGET `main`，`6609f3f..240f8a1`，8 個提交、30 個檔案。重點放在 `5a76caa..240f8a1` 的修正提交 `240f8a1`（S-1、S-2、N-3），其餘部分只在與修正有關的地方重新核對。
  - **本機 HEAD 與 PR 不同**：2026-10-27 查詢時，PR 的 `headRefOid`（以及 `origin/issue-0026-per-schedule-model`）仍是 `378ade32eb400e9d2db51d97e2559749354c5fc6`，`240f8a1` 只在本機、尚未推送。本報告審查的是本機 `240f8a1`，不能代表遠端 PR 目前的內容。合併前要先推送，並確認 PR 的 `headRefOid` 等於 `240f8a1`（或等於「`240f8a1` + 只新增本報告的提交」，依 review-evidence.md「本機 artifact 提交後的有效性」判斷）。
  - BASE 核對：`git merge-base origin/main 240f8a1` = `6609f3f`，與 `origin/main` 相同。本機 `main` 停在 `995e9d1`（分支上的提交），不影響審查範圍。
- Reviewed BASE SHA：6609f3f3b21f4ddfad294841bb5aa09c1de62651
- Reviewed HEAD SHA：240f8a1e3d9856163f570215e9ca88823c5539a1
- Reviewed patch-id：28457fa451a95e2d35af0cd12670134353cc4e71（2026-10-27 以 `git diff 6609f3f 240f8a1 | git patch-id --stable` 重算，與指定值一致）
- 獨立 reviewer：Claude Code 隔離 subagent（Claude，獨立審查角色）。沒有參與本 PR 的實作或修正，也不是 `review-378ade3.md` 的審查者。
- Review artifact：docs/issues/issue-0026/review-240f8a1.md

## 問題與風險

### MUST FIX
無

### SHOULD FIX
無

### NICE TO HAVE

1. **較新的 `load()` 在 `fetch` 本身拋出（網路中斷）時，排程頁會停在舊卡片，「儲存模型」按了沒有任何反應**（`crates/wukong-web/static/components/wukong-schedules.js:15-58`）
   - 狀況：舊的 load A 已畫出卡片、正在等模型清單，此時使用者按停用，觸發 load B。B 的 `fetch('/api/schedules')` reject（不是 HTTP 錯誤碼，是 promise 被拒）。A 等完清單後發現世代已過期，直接 return；B 拋出例外，沒有被處理。畫面停在 A 畫的卡片：模型區塊沒有選單，「儲存模型」沒有綁定事件，按下去沒有 PATCH，也沒有任何狀態訊息。
   - 實測（jsdom 模擬，見下方「S-1 獨立重現」）：`240f8a1` 的結果是 `pickerCounts [0,0]`、按儲存 0 個 PATCH；`5a76caa` 在同一情境下，舊的 load 會掛上選單，所以儲存可以送出。
   - 影響：極低。這時網路本來就斷了，卡片狀態也是舊的（修正前一樣）。`load()` 沒有處理 fetch reject 是改版前就有的問題，這次修正只改變了症狀。HTTP 錯誤碼（例如 500）的分支正常：會顯示「無法讀取排程：HTTP 500」。
   - 建議：要處理的話，在 `load()` 外層用 try/catch 包起來，失敗時把 `this.list` 改成錯誤訊息（同樣要先檢查世代）。不處理也可以接受。

2. **計畫裡 S-2 的 impact 描述與程式不一致**（`docs/issues/issue-0026/implementation-plan.md` 步驟 4「審查後修正」S-2 第一點）
   - 原文寫「直接呼叫者有 2 個」，後面卻列了三項，並說「三者都是在逾時時放棄等待」。實際上 CLI `/providers`、`/models`（`crates/wukong-cli/src/command.rs:106-119`）沒有逾時，會等到完成。Telegram 的同名指令經 `run_session_command` 間接走到同一段程式（`crates/wukong-telegram/src/dispatch.rs:1120`），清單沒有列出。
   - 結論沒有受影響：`kill_on_drop` 只在 future 被丟棄時才有作用，正常完成時行為不變。我核對過，CLI REPL、`main.rs` 與 Telegram 的呼叫端都不會中途取消這個 future。
   - 建議：把這一段改成正確的呼叫者與行為。只是證據文字的準確性，不阻擋合併。

### 範圍外、保留的項目（不列為缺口）
使用者同意的修正範圍是 S-1、S-2、N-3。`review-378ade3.md` 的 N-4（PATCH body `{}` 等同清除模型）、N-5（PATCH 沒有 401 測試）、N-6（正規化邏輯有兩份）仍然存在，維持原報告的建議等級。

## 已查核維度

### 審查後修正是否真的解決問題

#### S-1：排程頁 `load()` 重入（`wukong-schedules.js`）
- 做法：`load()` 開始時同步遞增 `this.loadGeneration`，三個 `await`（schedules fetch、`resp.json()`、`loadModelCatalog`）之後都檢查世代，過期就 return。
- 逐一推演的交錯情境：
  - act() → load()：世代在新的 load 開始時（第一個 await 之前）就遞增，所以舊的 load 不論卡在哪個 await，醒來都會放棄，不會在新畫面上掛選單。
  - setModel() → load()：同上。連按兩次儲存會送出兩個相同內容的 PATCH（冪等），最後只有一次 load 掛選單。
  - 並行的 load：只有最新的一次會寫畫面。另外修好了一個原報告沒提到的問題：較慢的舊回應不會再蓋掉較新的清單（見下方重現）。
  - 空清單與錯誤分支：都在世代檢查之後才寫畫面，過期的 load 不會蓋掉新結果。最新那次的 HTTP 錯誤會正常顯示。
  - 提早 return 是否會讓畫面卡住：被放棄的一定是舊的 load，最新的那次會繼續完成，所以不會卡住。唯一例外是最新的 load 在 fetch 時拋出，見 NICE TO HAVE 1。
  - `connectedCallback` 重複呼叫（元素被移動）：第二次呼叫會換掉 `this.list` 並遞增世代，舊的 load 會放棄，不會寫到已脫離的節點。
  - 同一世代內，`turns` 與 `this.list` 的卡片一定對應（只有 `load()` 會改 `this.list`），所以原報告指出的 `card` 為 `null` 時的 TypeError 不會再發生。
- **S-1 獨立重現**（2026-10-27，jsdom 20.0.0 + 模擬 `fetch`，元件原始碼取自 `git show 5a76caa:` 與 `git show 240f8a1:`，`/lib/` 指到 repo 內未修改的 `static/lib`；腳本在 scratchpad，沒有加進 repo）。兩個 Turn 排程，`model` 都是 `custom/old`，模型清單延遲 300 ms 才回應。在清單回來之前操作，等畫面穩定後，在最後一張卡片選 `opencode/new-model` 並按儲存：

  | 情境 | `5a76caa`（修正前） | `240f8a1`（修正後） |
  |---|---|---|
  | 載入期間按停用 | 按一次儲存送出 2 個 PATCH：`custom/old` 與 `opencode/new-model` | 1 個 PATCH：`opencode/new-model` |
  | 載入期間刪除第一張 | 未處理的 `TypeError: Cannot read properties of null (reading 'querySelector')` | 剩 1 張卡、選單掛 1 次、沒有錯誤 |
  | 較新的 load 回 HTTP 500 | 顯示錯誤，但仍有同一個 TypeError | 顯示「無法讀取排程：HTTP 500」，沒有錯誤 |
  | 較新的 load 回空清單 | 顯示空清單，但仍有同一個 TypeError | 顯示空清單，沒有錯誤 |
  | 第一次 load 的 schedules 回應較慢，第二次較快 | 舊回應蓋掉新結果，畫面顯示 2 張（應為 1 張） | 1 張 |
  | 較新的 load 的 fetch 拋出 | 舊卡片保留，選單有掛上 | 舊卡片保留，選單沒有掛上，儲存沒有反應（NICE TO HAVE 1） |
  | 沒有任何干擾 | 每張 1 個選單，1 個 PATCH | 相同 |

  這是模擬環境，不是真實瀏覽器。實作者另有 Chrome 實測（計畫步驟 4），兩邊結論一致。
- **刪除情境沒有另外跑紅燈是否可以接受**：可以。上表的刪除情境在 `5a76caa` 確實會紅（原報告描述的 TypeError），在 `240f8a1` 是綠的。它和停用情境是同一個根因（過期的 load 繼續執行），由同一個世代檢查修正，所以補上這次的獨立紅燈後，證據就完整了。前端 DOM 層仍然沒有自動化測試，這和原報告的判斷相同，以實測作為等價證據。

#### S-2：`run_fixed` 加上 `kill_on_drop(true)`（`crates/wukong-gateway/src/backend.rs:442-457`）
- 測試 `run_fixed_kills_the_child_when_the_caller_gives_up` 是真的測試：
  - 用 `sh -c "sleep 1; touch <marker>"`，100 ms 後放棄等待，再等 1.5 秒確認 marker 不存在。`sh` 被 SIGKILL 之後就不會執行 `touch`，所以 marker 只在子行程沒被終止時才會出現，斷言和失敗模式直接相關。
  - `assert!(gave_up.is_err())` 擋住了另一種假綠燈：子行程根本沒啟動、立刻回錯誤時，測試會紅。
  - **紅燈獨立重現**：用 `git archive 240f8a1` 匯出到 scratchpad，只拿掉 `.kill_on_drop(true)`，執行 `cargo test -p wukong-gateway --lib --locked run_fixed_kills` → `FAILED`，`panicked at ... the abandoned child kept running`。repo 內沒有任何修改。
  - **穩定性**：在 repo 上連跑 10 次全過（每次約 1.60 秒）；另外在所有 CPU 都被佔滿的情況下再跑 5 次，也全過。只有放棄等待晚了超過 1 秒才會誤紅，可以接受。代價是測試套件多花約 1.6 秒。
- 對 CLI `/providers`、`/models` 與 Telegram 同名指令的影響：這些呼叫端都會等到 future 完成，不會丟棄它。`kill_on_drop` 只在丟棄時送 SIGKILL，對已經結束的子行程沒有作用，所以行為不變。
- 對 Web 的影響：`run_opencode_diagnostic` 逾時後子行程會被終止，System 診斷頁也一起受益。client 中途斷線時 axum 會丟棄 handler，這時 `ModelCatalogCache` 的 mutex guard 會一併釋放，正在跑的 `opencode models` 也會被終止，不會寫入快取，下一個請求會重新抓取。這是合理的行為。
- 限制（不列為缺失）：`kill_on_drop` 只終止直接的子行程。如果 `opencode` 自己又產生子行程，那些孫行程不會被終止。這次沒有驗證 `opencode models` 會不會產生子行程。

#### N-3：Settings「儲存模型」預設停用（`wukong-settings.js:19`）
- jsdom 模擬（同上）：

  | 情境 | `5a76caa` | `240f8a1` |
  |---|---|---|
  | 正常載入，清單延遲 300 ms | 20 ms 時可以按，按下顯示「模型不可為空。」 | 20 ms 時停用；載完後啟用 |
  | `/api/models` 502 | 同上 | 載完後啟用（手動輸入可用） |
  | 環境變數鎖定（`editable:false`） | 先可以按，之後停用 | 一直停用 |
  | `GET /api/settings/model` 回 500 | 按下後錯誤訊息被「模型不可為空。」蓋掉 | 停用，錯誤訊息保留 |

- **有沒有造成新的卡住狀態**：`loadModelCatalog` 自己會 catch，一定回傳（失敗時回 `null` 並退回手動輸入），所以清單失敗時按鈕照樣會啟用。只有 `GET /api/settings/model` 失敗時按鈕會一直停用，但這時選單本來就沒有掛上（從 `378ade3` 起就是這樣），修正前按下去也只會顯示「模型不可為空」。
  - 和 `6609f3f` 相比：當時這種情況下還能手動輸入並儲存。但 `put_model_settings` 用的是 `load_settings(...).unwrap_or_default()`（`crates/wukong-web/src/lib.rs:404`），設定檔讀不到時會用預設值整份覆寫，Telegram 設定也會被蓋掉。現在停用反而比較安全，所以不列為退化。
- submit 事件只在 `connectedCallback` 綁一次，`saveModel → loadModel` 重複執行時不會累積事件。

### 回歸與證據
- 本次重跑（2026-10-27，HEAD `240f8a1`，工作區乾淨）：
  - `cargo fmt --all -- --check` → exit 0
  - `cargo clippy --all-targets --locked -- -D warnings` → exit 0
  - `cargo test --workspace --locked --no-fail-fast` → 705 passed／0 failed／12 ignored，exit 0（比 `378ade3` 多 1 個，就是新增的 S-2 測試）
  - `node --test crates/wukong-web/static/lib/*.test.mjs crates/wukong-web/static/components/*.test.mjs` → 17 pass／0 fail
  - 這些數字與計畫裡實作者回報的一致（705 passed、node 17 passed）。
- 依 verification.md 判斷證據真偽：
  - S-2 的紅綠燈是真的，我已獨立重現紅燈。
  - S-1 的紅燈（停用情境、送出 2 個 PATCH）與我的模擬結果一致；刪除情境的紅燈由我補上。
  - N-3 只有瀏覽器實測，它是 NICE TO HAVE 等級的 UI 調整，可以接受。
  - 沒有看到刪除、略過或弱化既有測試的情形。
- `240f8a1` 改了 3 個產品檔案，每一行都對應 S-1、S-2、N-3，沒有順便改其他地方。

### 驗收規格
- `git diff dcb901e 240f8a1 -- docs/issues/issue-0026/README.md`：只有「核准 commit: dcb901e」的回填，以及 Timeline 新增一列（PR #27 建立、`378ade3` 審查結果、使用者決定先修再合併）。Gherkin SCN-001～005 與核准時相同。
- 這次修正沒有改變任何 SCN 的可觀察行為，只修了過期載入與子行程殘留。所以 `review-378ade3.md` 對 SCN-001～005 的證據判定仍然有效，不另外重列。

### 完整 PR 的抽查（只看與修正有關的部分，以及原報告可能漏掉的地方）
- `set_turn_model` 先讀後寫：S-1 修正之前，最可能出現的同時寫入就是重複送出的 PATCH，現在已經消失。其餘結論和原報告相同。
- `/api/models` 與 PATCH 仍然在 token middleware 之內，這次修正沒有動到路由。
- CHANGELOG：S-1、N-3 修的是尚未發版的新功能，已經包含在 `[Unreleased]` 的 Added 條目裡。S-2 也會改善已發版的 System 診斷頁（逾時後不再留下背景行程），但使用者在畫面上看不到差異，不補條目也可以。
- 架構：沒有改 `Cargo.toml`，沒有新增相依，也沒有跨 crate 的新呼叫。
- 原報告沒有提到的部分：舊的 schedules 回應較慢時，會蓋掉較新的清單。這也是同一個根因，已經被這次的世代檢查修好（見上表）。

### 豁免、待確認與限制
- 沒有 Gate 豁免。
- 計畫步驟 4 的「待辦：新 HEAD 重新獨立審查」由本報告完成。README 狀態仍是「實作中」，這是交付前的正常狀態。
- W-9：本機 clippy 是綠燈，不代表 CI 也會是綠燈。推送後要看 CI 結果。

## 流程判定
PASS

（沒有 MUST FIX。S-1、S-2、N-3 都已修正，紅綠證據是真的，其中 S-2 的紅燈與 S-1 刪除情境的紅燈由本次獨立重現。完整測試套件全綠，Gherkin 沒有變動。兩項 NICE TO HAVE 不阻擋合併。本機 HEAD `240f8a1` 尚未推送，PR 的 `headRefOid` 仍是 `378ade3`。推送後要核對 PR head 是否等於被審查的 SHA，或只多了新增本報告的提交。）
