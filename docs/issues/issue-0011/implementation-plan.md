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

1. ✅ **首要驗證：planner 基線探針** — 產出：[`crates/wukong-runtime/tests/planner_probe.rs`](../../../crates/wukong-runtime/tests/planner_probe.rs)（`#[ignore]`，用真實 `route_options()` 與 `plan_skill_chain_with_preferences`，臨時工作目錄），固定案例各取 10 個樣本並印出計畫：
   - A 簡單追問：本次輸入「所以沒辦法跑成功嗎」（上一回合已回答 run #869 失敗於 session lease unavailable、#863／#870 成功、其餘逾時；基線的 planner 看不到這段）。
   - A-tg：A 加上 Telegram 附加的檔案互動規則。compose 預設的 shared 檔案模式下 `artifact_return_enabled()` 為真，所以這才是正式環境 planner 實際收到的輸入。
   - B 要求重新查核：「幫我重新查一下現在排程的狀態」。
   - C 複雜操作：「修好 scheduler 的逾時問題，並跑測試驗證」。
   相依：無。
   - 執行（2026-10-20）：`XDG_CONFIG_HOME=<空目錄> WUKONG_PLANNER_PROBE_SAMPLES=10 cargo test -p wukong-runtime --test planner_probe -- --ignored --nocapture`，exit 0。模型 `opencode/big-pickle`（CLI backend；managed backend 會丟掉 `--model`，見下方額外發現），OpenCode 1.18.31。隔離 config 目錄，避免本機的全域 AGENTS.md、plugin 混進 planner。每次呼叫 3.4～11.4 秒。
   - 結果（被測提交 `7bef17e`，planner 只看 `input`）：

     | 案例 | 棒數分布 | 計畫 |
     |---|---|---|
     | A | 10/10 一棒 | 10/10 `fixer\|systematic-debugging` |
     | A-tg | 9/10 一棒、1/10 兩棒 | 10/10 首棒帶 `systematic-debugging`（fixer 5、explorer 5）；兩棒那次再接 `fixer\|verification-before-completion` |
     | B | 10/10 一棒 | 10/10 `explorer\|none` |
     | C | 10/10 三棒 | 10/10 `fixer\|systematic-debugging` → `fixer\|test-driven-development` → `fixer\|verification-before-completion` |

   - 判讀：A／A-tg 多半已經是一棒，所以「棒數太多」不是主要失敗；失敗的是**技能選擇**。planner 沒有上下文，把一句追問當成除錯任務，每次都掛上 `systematic-debugging`。這個技能要求 agent 從讀錯誤、重現、查近期變更開始做根因調查，也就是 SCN-001 說的「重複啟動完整調查」。偶發的兩棒（1/10）與 2026-10-08 事故的多棒現象一致。方案 A 仍然對症（缺的是上下文），步驟 4 的判準改為同時看棒數與技能。
   - 未涵蓋：正式部署的 opencode 設定（baseline `opencode.json`／`user.json`）可能影響 planner；事故當時的實際計畫需要讀正式資料庫的 `chat_live_events`，本次未取得授權，沒有比對。
   - 額外發現：`build_backend_from_env` 遇到 `opencode run --model X` 時，`strip_model_args` 之後仍選 managed backend，而 managed backend 不接收模型參數，`--model` 被靜默丟掉。部署走 server URL，不受影響；與本 issue 無關，未修改。
2. ✅ **確定性紅燈** — 產出：`router.rs` 測試（有上一回合時 prompt 含其文字與分類規則；沒有時不含區塊、結尾與現行逐字相同）、`turn.rs` 測試（假 backend 記下 planner 收到的 prompt：scope 有上一回合時含其文字且被截斷，沒有時不含區塊）。相依：步驟 1。
   - 影響分析（2026-10-20）：`gitnexus_impact` 對 `plan_skill_chain_with_preferences`、`skill_planning_prompt_with_preferences`、`run_turn_traced_with_attachments` 皆為 CRITICAL（所有進入點每一回合都經過，18 條流程）。因此限定為新增選用參數，沒有上一回合時 prompt 不變（以 characterization 測試鎖住），執行器不動。
   - 先只加參數、不改行為（呼叫端一律傳 `None`），讓紅燈落在斷言而不是編譯錯誤。
   - 紅燈：`cargo test -p wukong-orchestrator --lib skill_planning_prompt_` exit 101，`skill_planning_prompt_with_previous_turn_adds_context_and_followup_rules` 失敗於 `prompt.contains("[Previous Turn]\nUser: run #869 成功了嗎…")`；`cargo test -p wukong-runtime --lib planner_` exit 101，`planner_receives_the_previous_turn_truncated` 失敗於 `planner.contains("[Previous Turn]\nUser: run #869 成功了嗎\n")`。兩個「沒有上一回合」的測試同時為綠（characterization）。
   - 斷言的期望值是手寫字面值：截斷測試以「`Assistant: 沒有。` 共 14 字、剩 786 個『細』」推得，另斷言不含 787 個。
3. ✅ **最小實作**（`e072870`） — router 在有上一回合時插入 `[Previous Turn]` 區塊與規則；turn.rs 拆出 `previous_turn_records`，同一批紀錄同時給輔助棒（原行為）與 planner（每筆截到 800 字元）。
   - 綠燈：步驟 2 的兩個命令皆 exit 0（6 passed、3 passed）。
   - 規則文字在步驟 4 依真實模型結果改過兩次（見步驟 4），每次改完都重跑同一組 router 測試。
   - 單迴圈說明：外部行為（planner 的計畫）只能用真實模型觀察，由步驟 4、5 承擔；確定性測試只保證 planner 收到上下文與規則，兩層各有證據，沒有合併。
   - code-simplify：拆出 `previous_turn_records` 是為了讓輔助棒與 planner 共用同一次查詢，避免重複讀取；`planner_previous_turn` 的截斷沿用 `compose_prompt` 的同一規則（該常數在 gateway 內部、不公開，所以就地重寫三行）。修正一處移位的 doc comment，其餘 no-op。
4. ✅ **真實模型複測（SCN-001～003）** — 同一個探針（`planner_plan_distribution`），模型、隔離方式與步驟 1 相同，每組 10 個樣本，2026-10-20。
   - 第一版規則（重新查核與「需要操作就規劃到驗證」寫在同一句）：A、A-tg 10/10 `oracle|none`；C 不變；**B 有 6/10 多出 `fixer|verification-before-completion`**，單純查狀態卻排了 fixer，可能做出沒被要求的修改。
   - 第二版把「重新查核」與「操作」拆成兩句：B 回到 10/10 `explorer|none`。但步驟 5 的真實回合顯示 B 會掛上 `systematic-debugging`，原因是探針的上一回合文字與真實回合不同。探針改用真實回合的上一回合（`FACTS_TURN` 與一次實際回覆）後，B 有 7/10 帶 `systematic-debugging`（基線 0/10）。
   - 第三版（最終，`e072870`）在重新查核那句補上「skill none unless the user asks for a cause or a fix」：

     | 案例 | 對照組（同探針、`WUKONG_PLANNER_PROBE_BASELINE=1`，不帶上一回合） | 最終版 |
     |---|---|---|
     | A | 10/10 `fixer\|systematic-debugging` | 10/10 一棒、不帶技能（`oracle` 8、`explorer` 2） |
     | A-tg | 8/10 一棒、2/10 兩棒，10/10 首棒帶 `systematic-debugging` | 10/10 一棒、不帶技能（`oracle` 6、`explorer` 4） |
     | B | 10/10 `explorer\|none` | 10/10 `explorer\|none` |
     | C | 10/10 三棒，含 fixer 與 `verification-before-completion` | 與對照組相同 |

   - 判準：A／A-tg 10/10 一棒且不帶 `systematic-debugging`（門檻 9/10）；B 仍為能查核的 explorer；C 未減少執行與驗證步驟。全部達標。
   - 對照組用同一個探針、同一天執行，所以差異來自上一回合，不是模型隨時間漂移。
   - 限制：規則是依這 4 個案例調出來的，又用同一組案例驗收，有過擬合的可能；只測了 `opencode/big-pickle` 一個模型，每組 10 個樣本。
5. ✅ **真實回合對照（SCN-001、SCN-003）** — `real_followup_turns`：每個樣本用新的臨時 scope、記憶庫與工作目錄，先跑 `FACTS_TURN` 交代事實，再問 A 或 B；各 3 個樣本。對照組在 main（`e4a4e20`）的臨時 worktree 跑同一個探針。

     | | main（修改前） | 最終版 |
     |---|---|---|
     | A 角色／工具 | explorer 1、fixer 2，3/3 都讀了技能檔（`read`） | 3/3 `oracle`，0 次工具 |
     | A 回答 | 2/3 引用 #863／#870 成功、#869 lease 衝突與 6 次逾時；1/3 只回「我會先依指定的系統化除錯流程…」，沒有答案 | 3/3 引用同樣的證據並直接回答「不是完全跑不成功」 |
     | A 耗時 | 13.1～13.9 秒 | 8.6～9.5 秒 |
     | B | 3/3 explorer 實際查核（8～9 次工具） | 3/3 explorer 實際查核（10～12 次工具） |

   - SCN-001：A 只有一棒、沒有重新調查，回答引用上一回合的成功與失敗證據。
   - SCN-003：B 每次都實際查核。回答主動指出「本機資料庫是空的，可能不是正式環境」，沒有把上一回合的資料當成最新狀態。
   - 第二版規則時的中間結果：B 有 2/3 只讀了 `systematic-debugging` 技能檔就結束，沒有實際查核。第三版修正後消失。
   - 與本 issue 無關、修改前就存在的模型行為：B 的回答在兩個版本都停在「我會再確認…」，沒有收尾結論（big-pickle 在多次工具呼叫後提早結束）。
   - 副作用：真實回合的 agent 有完整工具權限。B 的 agent 會離開臨時目錄、找到本機 repo，用 cargo 執行唯讀的 `wukong schedule list`。main 對照組的一次執行因此在 `~/.wukong/memory.db` 建了一個空資料庫（2026-10-20 12:07:34 建立，各表 0 列）。之後未再執行真實回合。2026-10-20 經使用者同意已刪除該檔。
6. ✅ **常青文件與交付** — 根目錄 `AGENTS.md`「一回合資料流」第 2 步補上 planner 帶上一回合；`CHANGELOG.md` `[Unreleased]` 新增 Changed 條目。全量檢查（2026-10-20，程式碼同 `e072870`，只差一處 doc comment）：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked` passed=633 failed=0 ignored=11。`gitnexus_detect_changes` 的實際改動限於上述三個函式與新測試，其餘列出的符號只是行號位移。
   - 審查後修正（2026-10-20，`review-1034907.md` 的 SHOULD FIX）：探針改成必須設 `WUKONG_PLANNER_PROBE_MODEL` 才會呼叫模型（原本預設 `opencode/big-pickle`，`cargo test -- --ignored` 就會呼叫真實模型）；檔頭補上實作計畫用的 `XDG_CONFIG_HOME` 隔離、一次只跑一支，以及真實回合的副作用警告。驗證：不設變數跑 `cargo test -p wukong-runtime --test planner_probe -- --ignored`，兩支都在 0.00 秒內以 `set WUKONG_PLANNER_PROBE_MODEL …: NotPresent` 失敗，沒有呼叫模型；不帶 `--ignored` 為 2 ignored；fmt、clippy 通過。上方步驟 1、4、5 的命令改用此變數重跑時需補上它。

## 驗收對照

| Scenario | 層級 | 證據 |
|---|---|---|
| SCN-001 | planner 輸入（確定性）＋真實模型 | 步驟 2／3 的紅綠燈；步驟 4 的 A 組分布；步驟 5 的 A 回合 |
| SCN-002 | 真實模型 | 步驟 4 的 C 組與對照組相同；執行器未改動，多棒計畫仍逐棒執行（既有測試） |
| SCN-003 | 真實模型 | 步驟 4 的 B 組；步驟 5 的 B 回合 |

SCN-002／003 是模型行為，確定性測試只能保證 prompt 有對應規則，無法保證模型照做；這部分以真實模型樣本為證據，樣本數與模型如實記錄。
