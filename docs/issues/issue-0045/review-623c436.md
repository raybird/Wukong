# 審查報告
- 範圍：完整交付範圍（`f9bc86e..623c436`，35 個提交、130 個檔案），以增量覆審進行
- Reviewed BASE SHA：f9bc86e73e261f3a7d04257b92eaa7e200787fe2
- Reviewed HEAD SHA：623c436ea9f31247b4c9a1c683219f1bdfddd2a8
- Reviewed patch-id：14a58b18b1cd45cf1fabafd4cd7bf13c78b5c80b（2026-11-27 以 `git diff f9bc86e 623c436 | git patch-id --stable` 計算）
- 獨立 reviewer：Claude Code subagent（未參與實作，也不是 review-65e960a 的 reviewer）
- Review artifact：docs/issues/issue-0045/review-623c436.md
- 增量覆審：上一份報告 review-65e960a.md（完整交付範圍，PASS，Reviewed HEAD 65e960a）；增量範圍 `65e960a..623c436`（6 個提交：d900bda、64e95b8、0c54bf0、a091c4f、61da5e5、623c436）。這次增量是為了處理那份 PASS 的建議，使用者選了要修的有 SHOULD FIX 1、2、3、6 與 NICE TO HAVE 9、13；SHOULD FIX 4 記為 TBD-11（接受並記錄），SHOULD FIX 5 改以 TBD-10 補跑處理（第 7 輪 super-vl）。

增量的三個條件我都核對過：
1. 65e960a 是 623c436 的祖先，中間沒有 merge 提交，也沒有改寫歷史。
2. 本機 `main`（f9bc86e）與 623c436 的 merge-base 仍是 f9bc86e。
3. 流程契約是 2.7（被審查提交上的 `docs/AGENTS.md:197`），不低於 2.6。

README 的 `**核准 commit**` 已在 d900bda 回填為 f0ce34a，所以本報告不另記核准基線。`git diff f0ce34a 623c436 -- docs/issues/issue-0045/README.md` 只有四段差異：`## 現況`、`**核准 commit**` 那一行、待確認事項、Timeline。`## Gherkin 驗收劇本` 沒有變，核准仍然有效。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
無。

### NICE TO HAVE

1. **Telegram 的狀態泡泡在 subagent 回報後仍停在「悟空·fixer」**：`crates/wukong-telegram/src/dispatch.rs:1563-1570`（只有 `on_role` 會改角色，`SubagentReported` 直接略過）、`:120`、`:711`
   - 上一份報告的 NICE 9 寫了「Telegram 的泡泡也有同樣的狀況」，a091c4f 只修了排程（`crates/wukong-scheduler/src/executor.rs:267-318`、`:437-460`），TBD-12 和 Task 6.6 也沒有記下 Telegram 這一半。
   - 現在排程會回到「悟空」，Telegram 不會，所以 `CHANGELOG.md:31-33` 說的「Telegram 的狀態泡泡、Web 的進度、排程執行中的訊息都一樣」已經不完全成立。
   - 建議：在 Telegram 的 `on_event` 收到 `SubagentReported` 時送出「回到悟空」的進度；或在 TBD-12 補記這是沒修的已知差距。待確認事項屬於記帳區段，補記不會讓本報告的 PASS 失效。

2. **新的工作區 AGENTS.md 範本還沒有用真實模型驗證過**：`workspace/AGENTS.md:7-8`、`docs/issues/issue-0045/dispatch-eval.md:144`、`docs/issues/issue-0045/scripts/dispatch-eval.sh:117`
   - 派工評估的種子工作區沒有 AGENTS.md（第 7 輪的紀錄也這樣寫），bench 的工作區同樣是空的。
   - 所以 0c54bf0 想達到的效果——primary 不再先試 shell、被擋下才派工——沒有實測。上一次在有 AGENTS.md 的部署狀態下觀察，只有 Task 6.3 的演練，用的是 828fe85 那一版舊範本。
   - 措辭和 `agent_assets::primary_permission` 的權限一致（`crates/wukong-runtime/src/agent_assets.rs:63-85`），我看不出它會讓直接回答題改去派工，所以不影響任何驗收。
   - 建議：之後再跑派工評估或演練時，在種子工作區放進 `workspace/AGENTS.md`，順便看 primary 工具清單裡還有沒有被擋下的 shell。

3. **用 V2 名稱找角色的寫法現在有兩份**：`crates/wukong-scheduler/src/executor.rs:446` 和 `crates/wukong-runtime/src/turn.rs:162`
   - 兩處都是 `Role::all().into_iter().find(|role| role.name() == agent)`，目前比對方式相同，沒有分歧。
   - 可以在 `wukong-orchestrator` 加一個 `Role::from_name`，兩邊共用（code-simplify）。

4. **NICE 13 裡屬於凍結內容的兩處仍沒有改**：`docs/issues/issue-0045/README.md:199`（`**核准來源**` 裡「TBD-62026-11-27 修訂」少了分隔）、`:221-224`（`## 重構步驟概要` 的 Phase 3～6 仍是 📝）
   - 上一份報告的建議是「下次改規格時一併處理」，所以不算漏修。
   - 這次反正要覆審，原本是順手修掉的時機。本報告之後再改這兩處，要再做一次增量覆審。

## 已查核維度

### 增量範圍內每個變動檔案的結論

| 檔案 | 提交 | 結論 |
|---|---|---|
| `crates/wukong-gateway/src/opencode_server.rs` | 64e95b8 | 正確，見下方 SHOULD FIX 1 |
| `crates/wukong-runtime/src/workspace_template.rs` | 0c54bf0 | 正確，見下方 SHOULD FIX 2、3 |
| `crates/wukong-runtime/assets/retired-agents-md/828fe85.md` | 0c54bf0 | 與 `git show 828fe85:workspace/AGENTS.md` 和 `git show 65e960a:workspace/AGENTS.md` 都逐位元組相同（`cmp`） |
| `workspace/AGENTS.md` | 0c54bf0 | 和權限一致；沒有真實模型的證據，見 NICE 2 |
| `crates/wukong-scheduler/src/executor.rs` | a091c4f | 正確，見下方 NICE 9 |
| `AGENTS.md` | 64e95b8、a091c4f、61da5e5 | 背景工作只算 subagent、排程階段回到悟空、白名單是防呆、入口整理工作區的時機，都和程式一致。CLI 的部分我對照了 `crates/wukong-cli/src/main.rs:124`：`opencode prune` 等子命令在這一行之前就 return 了 |
| `docs/docker.md:34`、`:44` | 61da5e5 | 同上，一致 |
| `docs/cli-reference.md:116` | a091c4f | 和 `Phase::label`／`failed` 一致 |
| `CHANGELOG.md:29-33` | 61da5e5 | 人格那一句的提醒現在和程式一致（我實跑確認過）；「都一樣」的說法見 NICE 1 |
| `docs/issues/issue-0045/dispatch/round-*.jsonl`（8 個） | 61da5e5、623c436 | 我重判了 7 輪，加上第 4 輪的重跑紀錄，結果都和 `dispatch-eval.md` 的表格相同。第 5、7 輪每一列的 `models` 都只有那一輪宣告的模型，18/18 `ok`。內容沒有家目錄路徑、金鑰或提示詞（grep 確認） |
| `docs/issues/issue-0045/dispatch-eval.md` | 61da5e5、623c436 | 結論、第 7 輪的敘述與原始紀錄一致。「整輪 18 分鐘」我以 `secs` 加總重算為 18.3 分。重判命令與模型名稱正確 |
| `docs/issues/issue-0045/implementation-plan.md` | 61da5e5、623c436 | Task 6.6 的證據我抽查過：opencode_server 79 項、workspace_template 6 項、scheduler 58 項，和我重跑的數字相同。Task 6.1 補記了 TBD-10 的結案 |
| `docs/issues/issue-0045/README.md` | d900bda、61da5e5、623c436 | 見下方「流程記帳」 |
| `docs/issues/issue-0045/review-65e960a.md` | d900bda | 新增後沒有再改過（`git log` 只有 d900bda 一個提交）。`verify-artifact.py --reviewed 65e960a --head d900bda` 回 OK，變更只有記帳區段 |

### 上一份報告選修的建議，逐項確認

- **SHOULD FIX 1（64e95b8）：已修好。**
  - `has_background_jobs`（`crates/wukong-gateway/src/opencode_server.rs:602-623`）在用戶端過濾，只有 `kind == "subagent"` 的工作才會讓回合繼續等。
  - 漏掉真正 subagent 工作的可能：
    - 真實 V2 的 job 確實帶 `kind: "subagent"`，`docs/issues/issue-0045/probe-subagent.md:116` 有實測紀錄（`[['subagent', 'fixer', 'running']]`）；上一份報告也讀 V2 2.3.2 的原始碼確認過四種 kind。
    - 子 session 再派出的 subagent，因為查詢帶 `descendants=true`，也一樣算進來。
    - job 沒有 `kind` 欄位時會被當成不是 subagent，但 V2 的 Job schema 這個欄位是必填。
    - 就算漏判，判斷回合結束時還有另外兩道條件：事件裡有執行中的子 session、inbox 有還沒送出的回報。這兩道沒有改。
    - 所以我判定沒有缺口。
  - 新測試 `background_shells_of_a_subagent_do_not_keep_the_turn_open`（`:1826-1848`）在舊行為下會紅燈：清單非空時會一直等，fake 的事件串流結束後，回合以 `event stream ended before session became idle`（`:768`）失敗，`unwrap` 就會 panic。實作者記錄的紅燈輸出和我的推論一致。
  - 既有測試 `:1765` 的 job 本來就帶 `kind: "subagent"`，所以前提沒有被改掉。

- **SHOULD FIX 2（0c54bf0）：已修好。**
  - 新範本把改檔、測試、`gh`、`agent-reach` 都交給 subagent。primary 可以跑的指令，範本寫的是「排程與長期記憶指令」，和 `primary_permission` 的白名單一致。
  - 828fe85 已加進 `RETIRED`，`every_retired_template_is_backed_up_and_replaced` 會自動涵蓋它。
  - `migrate_agents_md` 先比對 `CURRENT`，所以新範本不會被當成舊範本，反覆備份。
  - 新測試 `the_new_template_leaves_files_and_commands_to_subagents`：舊範本含「必須主動執行測試」，在舊範本上會紅燈。這是字串釘住測試，屬於證據持久力，不算假綠燈。

- **SHOULD FIX 3（0c54bf0）：已修好。**
  - `OUTDATED` 加了「你目前作為孫悟空」，`the_new_template_has_no_persona_or_skill_loading_section` 會檢查新範本不含任何一個 marker。
  - 單元測試改成期待兩段提醒，舊程式只會回 1 段，所以會紅燈。
  - 我用 `env -i` 實跑確認：改過的範本含人格那一句時，log 會提醒；828fe85 原樣的工作區會備份成 `.bak-828fe85`，然後換成新版。

- **SHOULD FIX 6（61da5e5）：已修好。** 各輪原始紀錄都進了版控，可以重判，結果和表格相同（見上表）。

- **NICE TO HAVE 9（a091c4f）：排程這一半已修好，Telegram 那一半沒修（NICE 1）。**
  - `RefCell` 共用的 callback 安全：
    - `run_turn` 的事件包裝（`crates/wukong-runtime/src/turn.rs:163-181`）先呼叫 `on_role`、再呼叫 `on_event`，兩者是先後執行，不是巢狀呼叫；
    - `on_change` 只把變化送進 unbounded channel，不會回頭呼叫 `on_event` 或 `on_role`；
    - `borrow_mut()` 的暫存值在那一行敘述結束時就釋放；
    - 這些呼叫都在同一個 task 裡同步執行，所以不會在 borrow 期間重入而 panic。
  - 多個同名 subagent：`running` 是依派出順序的 Vec，`rposition` 只移除最後一個同名的。同名的角色彼此等價，顯示結果正確。交錯派出（fixer、explorer）時，explorer 回報後顯示 fixer，也正確。
  - 沒有對應開始事件的回報（例如非 Wukong 角色），`rposition` 找不到，什麼都不做。
  - 名稱比對和 runtime 用的是同一個寫法，所以開始與回報的判定一致（重複的部分見 NICE 3）。
  - 開始與回報走同一個 channel，順序保留；回合結束後 `try_recv` 會清掉殘留的變化。
  - 新測試 `the_phase_returns_to_wukong_after_the_subagent_reports`：舊程式會得到「悟空·fixer 失敗」，紅燈。原有的 `the_phase_names_the_dispatched_subagent` 仍然通過，派出後還沒回報就失敗的情況照舊記成「悟空·fixer 失敗」。

- **NICE TO HAVE 13（61da5e5）：記帳與常青文件的部分已修好。**
  - Timeline 補上了舊基線 f9bc86e；
  - 白名單的說明補在 AGENTS.md 與 docker.md（併入 NICE 10）；
  - 入口整理工作區的時機已更正；
  - `## 現況` 在 d900bda 建立，位置在 `## 概述` 之後，`**現行 review**` 指向 review-65e960a.md。
  - 凍結內容的兩處見 NICE 4。

### 需決策兩項的處理

- **SHOULD FIX 4 → TBD-11：** 使用者選「接受並記錄」，`## 涉及檔案` 照建議 (a) 沒有改，例外寫在 TBD-11。技術結論沿用上一份報告（`add_column_if_missing`、`concurrent_open` 2 項）。
- **SHOULD FIX 5 → TBD-10（623c436）：**
  - 第 7 輪用 `nvidia/nvidia/nemotron-3.5-super-vl-120b-a12b`，屬於 Background「NVIDIA nemotron」的範圍；TBD-10 記了使用者的指示與「採用 super-vl 這輪」的同意。
  - 定義版本：`git diff f0ce34a 623c436` 在 `agent_assets.rs`、`persona.rs`、`workspace/SOUL.md` 都沒有差異，`role.rs` 只改了兩行 doc comment，所以第 7 輪驗的就是交付版本的 agent 定義。
  - SCN-003 的門檻（實作 3/3、直接回答 3/3，其餘至少 2/3）由第 7 輪在最終定義上達成，上一份報告提的版本落差已經消除。
  - 每次的實際派工都有原始紀錄可查。
  - 試跑其他模型的經過寫在第 7 輪的敘述裡，沒有混進判定；試跑前把 big-pickle 的額度用完照實記錄，沒有繞過額度。

### 驗收與證據（相對於 review-65e960a）

| 編號 | 本輪 | 結論 |
|---|---|---|
| SCN-001 | 沿用。增量沒有改 runtime 的回合流程；bench 工作區沒有 AGENTS.md，不受範本變更影響 | 通過（沿用） |
| SCN-002 | 重新查核：排程階段（a091c4f）、等待 subagent（64e95b8） | 通過；Telegram 回報後的顯示見 NICE 1 |
| SCN-003 | 重新查核：第 7 輪在最終定義上 6 題全過，我重判確認；原始紀錄可追溯 | 通過 |
| SCN-009 | 重新查核：828fe85 遷移、人格那一句的提醒，有單元測試，我也實跑確認 | 通過 |
| SCN-012 | 重新查核：回合結束的判定只多了 kind 過濾，上游錯誤分類沒有改 | 通過 |
| SCN-004～008、010、011、013、014 | 沿用。增量沒有動到相關程式（`git diff --stat 65e960a 623c436` 只涉及上表各檔） | 通過（沿用） |

### 相關失敗面（本輪新增或重新查核）

| 輸入／狀態 | 預期 | 覆蓋 | 判定 |
|---|---|---|---|
| subagent 留下常駐的背景 shell，或已結束、還沒讀取的 image 工作 | 回合在 primary 第一次 Idle 時收尾 | 新的 fake V2 測試，斷言只查詢一次 | 已覆蓋 |
| 清單裡有 `kind: subagent` 的工作 | 繼續等 | 既有的重播測試（`:1765`），加上真實 V2 的紀錄 | 已覆蓋 |
| job 沒有 `kind` | 當成不是 subagent；另外兩道等待條件仍有效 | 讀 V2 schema（必填欄位）推論 | 可接受，沒有專屬測試 |
| fixer 回報後 primary 失敗 | 記成「悟空失敗」 | 新的排程測試 | 已覆蓋 |
| 派出後還沒回報就失敗 | 記成「悟空·fixer 失敗」 | 既有測試 | 已覆蓋 |
| 同名 subagent 派出兩次、只回報一次 | 仍顯示該角色 | 讀程式（`rposition`），沒有專屬測試 | 可接受 |
| 工作區是 828fe85 原樣 | 備份後換新 | 單元測試；`env -i` 實跑 | 已覆蓋 |
| 改過的 AGENTS.md 含人格那一句 | 不動檔案，log 提醒 | 單元測試；`env -i` 實跑 | 已覆蓋 |

### 需求、架構、安全、品質

- **需求**：選修的各項都對應到 Task 6.6，沒有新增未核准的行為。範本措辭屬於既有 SCN-009 的「新版範本」內容，沒有改任何 Scenario 的可觀察結果。
- **架構**：沒有新的 crate 相依。`wukong-scheduler` 本來就依賴 orchestrator 的 `Role` 與 gateway 的 `StreamEvent`。
- **安全**：
  - `has_background_jobs` 只是讓判定變得更窄，沒有新的對外呼叫；
  - 原始紀錄沒有憑證或個人路徑；
  - 白名單的說明改成「防呆，不是隔離邊界」，和上一份報告的 NICE 10 一致。
- **品質**：重複的地方見 NICE 3，沒有過度設計。`SubagentChange` 和 `Phase::Turn { running }` 是滿足需求的最小結構。

### 流程記帳

- 增量裡的 README 變更：
  - d900bda：`## 現況`、核准 commit 回填、Timeline；
  - 61da5e5：TBD-11、TBD-12、Timeline 兩列；
  - 623c436：TBD-10 結案、Timeline 一列。
- 凍結內容的變更（dispatch-eval.md、implementation-plan.md、dispatch/、常青文件、程式）都在本次增量範圍內，上表逐檔審過。
- `verify-artifact.py --reviewed 65e960a --head 623c436` 回 `INVALID`（AGENTS.md 有變更，需要重新審查），符合預期，本報告就是那次重審。

### 我執行的驗證（HEAD 623c436，工作樹乾淨）

| 命令 | 結果 |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --all-targets --locked -- -D warnings` | exit 0（本機 toolchain；W-9：CI 的 stable 可能較新） |
| `cargo test --workspace --locked --no-fail-fast` | exit 0；774 passed、0 failed、14 ignored；本輪新增的 4 項測試和 `the_phase_names_the_dispatched_subagent` 都有跑到並通過 |
| `cargo test -p wukong-gateway --locked --lib opencode_server` | 79 passed |
| `cargo test -p wukong-scheduler --locked --lib` | 58 passed |
| `/usr/bin/python3 -I docs/issues/issue-0045/scripts/dispatch_judge.py docs/issues/issue-0045/dispatch/round-<N>.jsonl <MODEL>`（7 輪，加上 round-4-implement-retry） | 第 5、7 輪 exit 0，6 題 PASS；其餘各輪的 PASS／FAIL 與次數都和 dispatch-eval.md 的表格相同。拿第 7 輪對錯的模型重判，會判成全部無效，表示模型檢查有作用 |
| `cmp <(git show 828fe85:workspace/AGENTS.md) crates/wukong-runtime/assets/retired-agents-md/828fe85.md` | 相同 |
| 以 `env -i` 執行 `target/debug/wukong "hi"`（HOME、WUKONG_WORKSPACE、記憶庫都在 scratchpad；`WUKONG_AGENT_CMD='printf ok'`，不呼叫模型） | 改過的範本：log 提醒人格那一句；828fe85 原樣：備份成 `.bak-828fe85` 後換成新版，備份與新檔都用 `cmp` 確認過 |
| `verify-artifact.py --reviewed 65e960a --head d900bda` | OK（只有記帳區段的變更） |
| `verify-artifact.py --reviewed 65e960a --head 623c436` | INVALID，需要重新審查（符合預期） |
| `git ls-remote origin refs/heads/main` | b65b487（見下方的交付提醒） |

### 沿用的結論

這些項目的程式與證據都沒有在增量裡變動，沿用 review-65e960a：
- SCN-001、004～008、010、011、013、014 的驗收結論；
- 測試真偽的抽查；
- 上一份報告「相關失敗面」表中，本輪沒有重列的各列；
- 架構相依圖、primary 權限的 YAML 與順序、compose 掛載；
- NICE 7、8、11、12（TBD-12 記為已知差距）。

### 豁免、待確認與限制

- **豁免**：沒有 Gate 豁免紀錄。
- **待確認事項**：TBD-1～TBD-12 都已解決，狀態我沒有改；TBD-12 漏記 Telegram 那一半，見 NICE 1。
- **限制**：
  - 我沒有呼叫真實模型，也沒有啟動 V2 server 或 docker。
  - 上一份報告建議「修好後實際確認一次：fixer 的背景 shell 結束後，primary 的 inbox 仍收到回報」，這件事沒有做；第 7 輪的 18 個回合在 64e95b8 之後建置的版本上都成功，算是間接佐證。
  - 新範本的實際效果見 NICE 2。
  - 原始紀錄是補存進版控的（取自實作者本機的 `out/results.jsonl`），我只能確認它和表格一致，無法獨立確認它的來源。
- **交付提醒**：
  - 遠端 `origin/main` 是 b65b487，本機 `main` 是 f9bc86e，多了 18377ce、3b39ad5、f9bc86e 三個 issue 文件提交，還沒推送。
  - 本報告與上一份都以 f9bc86e 為 BASE。PR 若以遠端 main 為目標，merge-base 會變成 b65b487，PR 的 diff 會多出這三個提交，和本報告的固定範圍對不上。
  - 開 PR 前先推本機 main（project.md：文件提交直接放 main），或者依 review-evidence.md「固定交付範圍」重新確認範圍。

## 流程判定
PASS

PASS 只適用於上列 Reviewed HEAD SHA；之後提交 README 記帳區段與本報告以外的檔案會讓它失效，提交前先告知使用者（review-evidence.md「審查之後的提交」）。
