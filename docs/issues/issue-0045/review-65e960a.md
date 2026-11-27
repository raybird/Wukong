# 審查報告
- 範圍：完整交付範圍（`f9bc86e..65e960a`，29 個提交、120 個檔案）
- Reviewed BASE SHA：f9bc86e73e261f3a7d04257b92eaa7e200787fe2
- Reviewed HEAD SHA：65e960ade309d234c61a981540ea53f5fc29e0ed
- Reviewed patch-id：d60b8b66d1285553228ba1d07e54acac4c160d4e（2026-11-27 以 `git diff f9bc86e 65e960a | git patch-id --stable` 重算，與協調者提供的值相同）
- 獨立 reviewer：Claude Code general-purpose subagent（未參與實作）
- Review artifact：docs/issues/issue-0045/review-65e960a.md
- 核准基線：f0ce34ad4769324e73e406dbc2bcb15703169d34。README 的 `**核准 commit**` 仍是 `待提交`，依 acceptance.md「規格修訂的查核」以 `git log -1 --format=%H -G '^[[:space:]]*([-*+][[:space:]]+)?\*\*核准來源\*\*[[:space:]]*(:|：)' 65e960a -- docs/issues/issue-0045/README.md` 推導，結果與協調者相同。`git diff f0ce34a 65e960a -- docs/issues/issue-0045/README.md` 只改了 TBD-9 的狀態、新增 TBD-10 與 Timeline 四列，`## Gherkin 驗收劇本` 與 `## Gherkin 核准紀錄` 沒有變，所以核准仍然有效。上一個基線是 f9bc86e（= BASE），f9bc86e→f0ce34a 之間 Gherkin 只改了 SCN-001，核准表標為 2026-11-27 重新核准，與核准來源一致。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX

1. **等背景工作時沒有限定 subagent，subagent 開了常駐的背景 shell，回合就會掛到逾時**：`crates/wukong-gateway/src/opencode_server.rs:598-616`（查詢）、`:826-833`（Idle 時判斷）
   - 現況：`has_background_jobs` 查 `GET /api/experimental/job?sessionID=…&descendants=true`，沒有帶 `kind`。我讀了映像裡 V2 2.3.2 的程式（`SessionBackground.list`），這份清單的 kind 有 `subagent`、`shell`、`image`、`codemode` 四種；`descendants=true` 也會列出子 session 的背景 shell。
   - 影響：使用者說「把 dev server 跑起來」，fixer 用背景 shell 執行 `npm run dev`，這類不會自己結束的工作會一直留在清單裡。primary 的最後一次執行結束時清單不是空的，`consume_event_stream` 就繼續等，之後不會再有事件，等到 `WUKONG_AGENT_TIMEOUT_SECS`（預設 1200 秒）才以逾時失敗，並中斷子 session。#45 之前，V2 回合在 Idle 時直接收尾，不會這樣。這是本次新增的失敗面，目前沒有測試，也沒有探測覆蓋。
   - 建議：查詢加上 `&kind=subagent`。這一題只是要等 subagent 回報；primary 的 inbox 還有項目沒處理時，現有的 `inbox_pending` 已經會讓回合繼續等。另外補一個 fake V2 測試：清單只有 `{"kind":"shell","status":"running"}` 時，回合在第一次 Idle 就收尾。修好之後要實際確認一次：fixer 的背景 shell 結束後，primary 的 inbox 仍然會收到回報。

2. **新的工作區 AGENTS.md 範本，跟 primary 不能改檔、不能跑指令的限制互相衝突**：`workspace/AGENTS.md:3`、`:7-10`、`:26`、`:33-39`
   - 現況：V2 會把這份範本給所有 agent，包括 primary。範本仍要求「使用命令執行工具執行編譯、測試」「修改後必須主動執行測試」「先 `command -v memoria`」「使用 `agent-reach doctor`」「GitHub 查詢優先使用 `gh`」。Task 6.1 之後，primary 的 shell 只允許 `<bin> schedule *` 和 `memoria *`，這些指令都會被擋下。
   - 影響：rehearsal.md 第 6 節與 dispatch-eval.md 第 3 輪都看得到 primary「先試 shell，被權限擋下，才派工」，每試一次就多一次 LLM 呼叫（tool 任務的步數 4 → 中位數 7，有一部分來自這裡）。要查網路、查 GitHub 的問題，primary 也只能先失敗，再派給 fixer。這段範本是 Task 2.3 寫的，早於 Task 6.1 收緊權限，兩個 Task 之間沒有對齊。
   - 建議：在範本裡寫明「改檔、執行指令（測試、`gh`、`agent-reach`）都交給 subagent」，或把這些段落移到 subagent 適用的位置。改範本的同時，要把 828fe85 那版加進 `workspace_template.rs` 的 `RETIRED`。

3. **CHANGELOG 寫 log 會提醒「人格那一句」，實際不會**：`CHANGELOG.md:30` 與 `crates/wukong-runtime/src/workspace_template.rs:40-49`（`OUTDATED`）
   - `OUTDATED` 只認 `[技能規範指引]` 和 `.wukong/skills`，三版舊範本都有的「你目前作為孫悟空」不在其中。
   - 我用 `env -i` 實跑 `target/debug/wukong` 驗證：工作區的 AGENTS.md 是 65511d2 的前三行加一條自訂規則，啟動時沒有任何提醒，跟 CHANGELOG 的描述不符（W-1）。
   - 建議：在 `OUTDATED` 加上人格那一句，並補一個單元測試；或者把 CHANGELOG 改成只寫「技能動態加載機制」。

4. **`需決策`：README 把 `wukong-memory` 列為「不可觸及：記憶與 lease 的 schema 不變」，實作卻新增了欄位**：`docs/issues/issue-0045/README.md:68`；`crates/wukong-memory/src/store/mod.rs:62`、`:537-560`、`:640-652`、`:682-698`、`:1112-1127`
   - Task 3.3 在 `agent_session_state` 新增 `session_started_at`，也改了 `set_agent_session` 與 `finish_agent_session_turn` 的 SQL。implementation-plan 的「做法調整」與 Timeline 2026-11-26 都有寫，但沒有紀錄使用者接受這次超出邊界的改動，`## 涉及檔案` 也沒有更新。
   - 技術上我看不出問題：遷移走 `add_column_if_missing`，符合 W-4；`concurrent_open` 2 項通過；舊版 binary 讀寫都用具名欄位，退版不受影響。
   - 待決問題：是否接受這次 schema 變更？可選做法：
     - (a) 接受，在 `## 待確認事項` 或 Timeline 記下決定。這兩節屬於記帳區段，不會讓本報告的 PASS 失效。
     - (b) 同時更新 `## 涉及檔案`。這一節是凍結內容，改了要重新審查。
   - reviewer 建議 (a)。

5. **`需決策`：TBD-10 的補跑計畫會讓本報告的 PASS 失效；SCN-003 的門檻目前不是在交付版本的定義上達成的**：`docs/issues/issue-0045/README.md:249`、`docs/issues/issue-0045/dispatch-eval.md`
   - TBD-10 寫「補跑結果只更新 dispatch-eval.md 與這一列」。依 review-evidence.md「審查之後的提交」，evidence 檔案屬於凍結內容，改了 `dispatch-eval.md`，`verify-artifact.py` 就會判定需要重審（至少要增量覆審）。
   - 交付影響：SCN-003 的門檻是第 5 輪 big-pickle 18/18，用的是 f0ce34a 之前的定義；兩者只差派工說明裡「派工後就結束這次回覆」一句。最終定義的佐證：
     - 第 6 輪 nemotron：探索、架構、設計、直接回答通過，實作題無效，查找 1/3；
     - bench f0ce34a 的 tool 任務 7/7 派給 fixer，short 任務 7/7 不派工（我重算 `bench/f0ce34a.ndjson` 的 `steps_by_agent` 確認）；
     - rehearsal 的 l-fixer、s-fixer 2/2 派給 fixer。

     我判斷風險不高，但 Proof of Test 的 SCN-003 那一列要寫明版本落差。
   - 待決問題：PR 合併前，要
     - (a) 補跑後再做一次增量覆審，還是
     - (b) 在 `## Gate 豁免紀錄` 記下「SCN-003 以前一版定義的證據交付」？
   - reviewer 建議：額度恢復前就要開 PR 時選 (b)，補跑完成後再走 (a)。

6. **派工評估的原始紀錄沒有保存，SCN-003「每次的實際派工都記錄下來」只有彙總**：`docs/issues/issue-0045/dispatch-eval.md`
   - `dispatch-eval.sh` 的 `out/results.jsonl` 沒有進版控。Task 6.2 有把原始 ndjson 放進 `bench/`，這裡沒有。
   - 第 5 輪架構、設計、直接回答各次的實際派工對象只寫成「3/3」，判定可以信，但無法逐次覆核。
   - 這屬於證據持久力，不是假綠燈。建議之後補跑時，把 results.jsonl 一起存進 `docs/issues/issue-0045/`。

### NICE TO HAVE

7. **`ObservedStep.skill_name` 已經用不到，`run_turn_traced*` 跟 `run_turn_observed*` 重複**（code-simplify）
   - V2 下 `skill_name` 固定為 None：`crates/wukong-runtime/src/turn.rs:26-30`、`:171-176`。
   - 連帶的遺留：
     - Web 的 `SseMsg::Step.skill`：`crates/wukong-web/src/chat_api.rs:28-33`、`:96`、`:375`，註解仍寫 helper baton；
     - 前端 `role + ' + ' + skill` 的標籤：`crates/wukong-web/static/components/wukong-chat.js:966-980`；
     - `crates/wukong-web/src/lib.rs:1356` 的斷言把 `"skill":null` 釘成契約。
   - 現在唯一用到 `run_turn_traced` 的是 Web，可以改用 `run_turn_observed`，然後刪掉 `ObservedStep`、兩個 traced 入口和這個 SSE 欄位。前端要相容舊的 step 事件時，可以保留讀取端。

8. **固定提示有兩條組裝路徑**：agent 定義（`crates/wukong-runtime/src/agent_assets.rs:93-142`）和 CLI backend 的 prompt（`crates/wukong-runtime/src/turn.rs:227-252`）。
   - CLI 路徑缺 `[派工]`；自訂的 opencode 命令帶 `--agent wukong` 時，兩份內容會重複（Task 4.1 已揭露）。
   - 目前兩邊共用 persona 的同一組函式，還沒有分歧。之後新增區塊時容易只改到一邊，可以考慮抽成同一個清單。

9. **排程階段在 subagent 回報後仍停在「悟空·fixer」**：`crates/wukong-scheduler/src/executor.rs:261-296`
   - `Phase` 只由 `on_role` 推進。fixer 回報、primary 寫最終回覆（甚至在這時失敗）的期間，訊息仍是「悟空·fixer」，失敗也記成「悟空·fixer 失敗」。這符合計畫寫的「最近派出的 subagent」，但和 SCN-002「顯示目前的角色」有一點落差。
   - 可以讓 `SubagentReported` 把階段改回「悟空」。Telegram 的泡泡也有同樣的狀況。

10. **shell 白名單是防呆，不是隔離**：`crates/wukong-runtime/src/agent_assets.rs:64-85`
    - 我讀了 V2 2.3.2 的 shell 掃描器（`wP`）：`;`、`&&`、管線與命令替換都會拆成多個 resource 逐一比對，所以串接指令繞不過去。但轉向寫在允許的指令上（例如 `wukong schedule list > sample/x.py`）時，resource 是整段文字，仍然符合 `<bin> schedule *`，primary 可以用這種方式覆寫檔案。
    - subagent 本來就有完整權限，primary 還能派工，所以這不構成提權。風險只是 primary 繞過派工。可以在 docs/docker.md 那句權限說明補上「防呆，不是資安邊界」。
    - YAML 跳脫與「先全拒再逐項允許」的順序，我實跑確認過：`WUKONG_BIN='/opt/my "bin"/wukong'` 產生的 frontmatter 可以用 `yaml.safe_load` 正確解析。

11. **只有改寫定義的那個行程會等 server 重新載入**：`crates/wukong-runtime/src/agent_assets.rs:154-167`
    - compose 同時啟動 web、telegram、schedulerd，只有搶先寫入的行程會記下待確認的內文。其他行程看到內容已經相同就不寫，也不等；如果它們立刻送出回合（例如 Telegram 的積壓訊息），仍可能用到舊定義（TBD-8 的情境）。
    - 時間窗很窄。可以在「內容相同但檔案的 mtime 在幾秒內」時也等一次。

12. **subagent 的上游錯誤沒有分類**（W-7 的延伸）
    - fixer 被限流時，primary 收到 `Subagent failed` 的回報，可能照樣寫出回覆，回合就以成功收尾，排程狀態也是 success。SCN-012 只要求回合本身的上游錯誤，不構成缺口。
    - 建議之後在 `SubagentReported` 帶出 `state` 與錯誤類型，排程訊息可以顯示「fixer 上游限流」。

13. **流程文件小問題**（記帳區段可以直接改，不影響 PASS）
    - Timeline 2026-11-27「規格修訂並重新核准 SCN-001」那列沒有寫舊基線 f9bc86e（acceptance.md 要求寫在那一列）。
    - 以下兩處屬於凍結內容，下次改規格時一併處理即可：
      - `**核准來源**` 裡「TBD-62026-11-27 修訂」少了分隔；
      - `## 重構步驟概要` 的 Phase 3～6 仍是 📝。
    - README 沒有 `## 現況`：流程契約 2.7 要求保存報告的提交在 `## 概述` 之後建立這一節，並把 `**現行 review**` 指向本報告。
    - docs/docker.md:34 寫「四個入口每次啟動都會整理」，但 CLI 只在要跑回合時才整理。

## 已查核維度

### 驗收與證據

| 編號 | 證據位置 | 結論 |
|---|---|---|
| SCN-001 | bench-v2-slim.md 與 `bench/*.ndjson`（我重算：b65b487 short 每回合 3 步、啟動 2 次；f0ce34a short 每回合 1 步、啟動 1 次；兩邊成功率都是 7/7；tool 步數 4 → 中位數 7）；rehearsal 的 l-who、s-who；turn.rs `a_turn_is_one_call_to_the_wukong_agent_and_is_remembered`；cli_backend 測試確認只有一次串流呼叫 | 通過。步數只量了本機按需模式，server 模式「不另跑規劃」由演練涵蓋。f0ce34a 之後沒有 runtime 程式異動（`git diff --stat f0ce34a 65e960a` 只有文件、compose 與 role.rs 的註解），證據適用 HEAD |
| SCN-002 | event_map `a_dispatched_subagent_is_reported_with_its_agent_and_output`（真實 fixture 重播）；四個入口的顯示測試；Web 與 Telegram 的 managed_actual（真實 V2，nemotron）；rehearsal 的 SSE | 通過。排程的顯示只有單元測試；回報後的階段文字見 NICE 9 |
| SCN-003 | dispatch-eval.md 第 5 輪 big-pickle 6 題全過；判準自檢我重跑，8/8 PASS | 門檻成立，但用的是前一版定義；交付影響見 SHOULD FIX 5，原始紀錄見 SHOULD FIX 6 |
| SCN-004 | turn.rs `a_continuing_session_gets_no_previous_turn`、`an_old_session_rotates_and_the_turn_sees_the_previous_turn`；rehearsal 的 l-old、s-old 跨升級答對 | 通過。輪替後追問答對靠的是 prompt 帶上一回合的機制（單元測試）；真實模型的輪替回合見 probe-session-layer |
| SCN-005 | probe-managed-skills.sh 12 項；`run_turn_leaves_skills_to_v2`；rehearsal server 模式載入 systematic-debugging | 通過。本機模式沒有觀察到技能載入（已揭露）；改過的 AGENTS.md 依 SCN-009 保留原段落 |
| SCN-006 | probe-session-layer（4 個回合的 user message）、rehearsal 第 7 節；`the_scope_goes_to_the_session_layer_not_the_prompt`；Telegram `the_artifact_instruction_is_only_the_directory_line` | 通過。Telegram 路徑的 opencode.db 只有組裝層級的證據（共用 run_turn），已揭露 |
| SCN-007 | gateway 的 entries 測試 4 項（有反向自檢）；probe-session-layer 回合 1、2（含真實輪替）；rehearsal rc.46 由 schedulerd 實際執行；compose 檢查我在 f0ce34a 的 compose 上重跑出 4 項 FAIL、HEAD 通過 | 通過 |
| SCN-008 | `the_memoria_capability_follows_the_switch`、`run_turn_advertises_memoria_only_when_enabled`；probe-session-layer 修正後連跑 3 次 | 通過（TBD-8 已解決） |
| SCN-009 | workspace_template 5 項單元測試；probe-agents-md-migration（三種工作區，真實模型）；rehearsal 升級；我確認三版舊範本與 git 歷史逐位元組相同，並實跑 ac60ebe 的遷移與備份 | 通過；log 提醒的範圍見 SHOULD FIX 3 |
| SCN-010 | session.rs 8 項、turn.rs 3 項、記憶庫 3 項；`.env.example` 與 compose 已移除；test-docker-runtime 通過 | 通過；schema 變更見 SHOULD FIX 4 |
| SCN-011 | settings `legacy_planner_preferences_field_is_ignored`；Web `skills_preference_api_is_gone`、`chat_ignores_legacy_planner_preferences` | 通過。Telegram、CLI、schedulerd 都用同一個 `load_settings`，沒有另外寫入口測試 |
| SCN-012 | `a_turn_rejected_by_the_lease_makes_no_backend_call`（我看了 run_turn：lease 確實在 recall 與任何 backend 呼叫之前）；修復提示 2 項與 `the_repair_prompt_keeps_the_turn_model`；`failed_turn_releases_session_lease`；stream.rs 分類邏輯只多了 match 分支；dispatch-eval 第 4 輪真實的 429 分類 | 通過；subagent 的上游錯誤見 NICE 12 |
| SCN-013 | `a_new_session_selects_the_agent_and_model_before_the_prompt`、`the_repair_prompt_keeps_the_turn_model`（有反向自檢）；probe-turn-dispatch 6/6（nemotron，確認 primary 與 subagent 的模型） | 通過 |
| SCN-014 | backend.rs 的 argv 測試 3 項；probe-cli-agent 6 項（真實 V2） | 通過 |

### 測試真偽

抽查的斷言都有獨立的真相來源：
- 派工 fixture 的回報內文是字面值；
- system 內文以實測的 `/api/agent` 回應為樣本；
- compose 檢查我獨立重現了紅燈；
- 背景等待的測試以重排或刪除 fixture 事件構造反例，並斷言查詢次數。

沒有看到同義反覆或 mock 掉核心行為的測試。`the_system_text_is_what_v2_lists_for_the_definition` 是釘住測試，實作者已揭露。

### 我執行的驗證（HEAD 65e960a，工作樹乾淨）

| 命令 | 結果 |
|---|---|
| `cargo fmt --all -- --check` | exit 0 |
| `cargo clippy --all-targets --locked -- -D warnings` | exit 0（本機 rustc 1.96.0；W-9：CI 的 stable 可能較新） |
| `cargo test --workspace --locked --no-fail-fast` | 771 passed、0 failed、14 ignored |
| `cargo test -p wukong-memory --test concurrent_open --locked` | 2 passed |
| `bash scripts/test-docker-runtime.sh` | 通過（37 個變數都接到容器） |
| `bash scripts/test-installer-upgrade.sh all` | 通過 |
| `bash scripts/test-release-workflow.sh` | 通過 |
| `/usr/bin/python3 -I docs/issues/issue-0045/scripts/dispatch_judge.py --self-check` | 8/8 PASS |
| 以 `env -i` 執行 `target/debug/wukong`（HOME、WUKONG_WORKSPACE、記憶庫都在暫存目錄；`WUKONG_AGENT_CMD='printf …'`，不呼叫模型） | 產出 6 份定義、8 個技能；ac60ebe 範本備份後換新；改過的範本含人格那一句時沒有提醒 |
| 從測試映像 `wukong-issue45:e70e4e2` 以 `docker create`／`docker cp` 取出 opencode 2.3.2 binary，讀 shell 掃描器、權限比對與 job 清單的程式 | 只讀檔案，沒有啟動 server |

### 相關失敗面

| 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|
| 同一個 scope 已有回合，第二個回合進來 | 立即拒絕，不呼叫 LLM | turn.rs 測試；程式順序 | 已覆蓋 |
| primary 派工後先結束一次，fixer 還在跑；job 清單漏列 | 等到最終回覆 | 重播測試 3 項；真實 V2 6 次 | 已覆蓋 |
| subagent 留下常駐的背景 shell | 回合照常收尾 | 無 | **缺口**（SHOULD FIX 1） |
| 回合失敗或被取消時，子 session 還在跑 | primary 與子 session 都送 interrupt | 2 項測試 | 已覆蓋 |
| 定義寫入失敗（工作區唯讀） | 入口照常啟動，回合以 Agent not found 失敗並回報 | 程式只寫 warning；probe-agents 確認不存在的 agent 會讓回合失敗 | 可接受（失敗會被看到），沒有專屬測試 |
| 常駐 server 在定義改寫後立刻收到回合 | 等 `/api/agent` 載入新定義，最多 5 秒 | gateway 2 項、runtime 2 項；真實探測 3 次 | 已覆蓋；多個行程的情況見 NICE 11 |
| CLI backend 或自訂 `WUKONG_AGENT_CMD` | 只對 opencode 補 `--agent wukong`；使用者自己指定的 agent 不動 | argv 測試 3 項；真實 V2 | 已覆蓋；其他寫法（例如短旗標、`opencode2`）不認得，屬於既有判定方式的延伸 |
| CLI 模式下 `--dangerously-skip-permissions` 和 primary 的 deny | deny 仍然有效 | 讀了原始碼（dispatch-eval.md「其他確認」） | 只有原始碼證據，沒有實跑，已揭露 |
| shell 白名單：串接、命令替換、轉向 | 只允許排程與 memoria 指令 | YAML 單元測試；第 3 輪真實模型的 rejected | 串接與替換會被拆開比對；轉向可以繞過（NICE 10，不是資安邊界） |
| YAML 跳脫（`WUKONG_BIN` 含空白、引號） | frontmatter 合法 | 單元測試；我實跑後以 yaml 解析確認 | 已覆蓋 |
| session 跨越 7 天、升級前的 session 沒有開始時間 | 回合開始時輪替；向 backend 補查一次；查不到就從現在起算 | session.rs 8 項、記憶庫 3 項；真實探測 | 已覆蓋 |
| 最終輸出是空的 | 同一個 session 補送一次修復提示，仍為空就回固定字串 | 2 項，另有模型與 agent 的測試 | 已覆蓋 |
| 上游限流、模型下架、連不到 | 錯誤分類與 #44 相同 | 既有分類測試未改；真實 429 | 已覆蓋；subagent 的錯誤見 NICE 12 |
| 舊設定檔帶 `planner_preferences` | 照常載入，存檔不再寫回 | settings 與 Web 測試 | 已覆蓋 |
| server 模式下對話建立的排程 | 寫進共用的記憶庫 | compose 檢查（紅綠我重現）；rc.46 實際執行 | 已覆蓋。opencode-server 因此能讀寫 `/data`，與預設的本機模式相同（本機模式的 agent 本來就在入口容器裡），沒有擴大權限範圍 |
| 多個服務同時遷移 `agent_session_state` | 不會出現 duplicate column | `add_column_if_missing`；concurrent_open | 已覆蓋 |
| server 密碼 | 沒有新的外洩途徑 | diff 掃描：沒有金鑰、沒有家目錄路徑；演練以 token 產生、不印出 | 無異常 |

### 需求、架構、安全、品質

- **需求**：14 個 Scenario 都有實作與證據。使用者的四個決定都照做：primary 的權限、e70e4e2 在 #45 修、TBD-9 不修、Task 5.2 移除整頁；TBD-10 見 SHOULD FIX 5。
- **架構**：相依方向符合根目錄 AGENTS.md 的新圖（我對照各 Cargo.toml 確認）。`wukong-orchestrator` 只剩角色定義，沒有依賴；`wukong-web` 不再依賴 skills 與 orchestrator。唯一越過規劃邊界的是 wukong-memory 的 schema（SHOULD FIX 4）。
- **安全**：
  - primary 的 permission 順序與 YAML 正確；
  - 白名單是防呆，不是資安邊界（NICE 10）；
  - compose 的掛載與本機模式一致；
  - 沒有洩漏憑證。
- **品質（跨 Task 的重複與過度設計）**：
  - 以下三項都還沒有分歧成不一致的行為，所以不列 MUST FIX：
    - 遺留的 `skill_name` 與 traced 入口（NICE 7）；
    - 兩條固定提示的組裝路徑（NICE 8）；
    - `write_if_changed` 放在 agent_assets，同時給 skill_assets 和 workspace_template 用，模組歸屬可以再整理。
  - AGENTS.md 範本與 Task 6.1 的權限之間是跨 Task 的語意衝突（SHOULD FIX 2）。
- **常青文件（W-1）**：
  - 根目錄 AGENTS.md 的相依圖、資料流與設計細節，和程式一致；
  - docs/docker.md、entrypoints.md、cli-reference.md、memory.md、crate README 抽查一致；
  - CHANGELOG 有一處不符（SHOULD FIX 3），docker.md:34 有一處小落差（NICE 13）。

### 豁免、待確認與限制

- 豁免：沒有 Gate 豁免紀錄。
- 待確認事項維持原狀態：
  - TBD-1～TBD-9 已解決；
  - TBD-10 待確認，交付影響見 SHOULD FIX 5，其中 dispatch-eval.md 在審查之後屬於凍結內容。
- 限制：
  - 我沒有呼叫真實模型，也沒有啟動 V2 server；SHOULD FIX 1 的掛起情境是讀 V2 原始碼推論的，尚未實測；
  - 沒有碰任何正式容器或使用者的 opencode 服務；
  - CI 以 0be51c8 的紀錄為準，我在 HEAD 重跑了三道，結果相同；
  - Telegram 的真實 bot 沒有驗證，實作者已揭露，以 MockTgClient 加真實 V2 代替。

## 流程判定
PASS

PASS 只適用於上列 Reviewed HEAD SHA；之後提交 README 記帳區段與本報告以外的檔案會讓它失效，提交前先告知使用者（review-evidence.md「審查之後的提交」）。
