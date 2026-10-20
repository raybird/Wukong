# 審查報告
- 範圍：完整 PR
- Reviewed BASE SHA：1af4528e12918a10a9eb687bd77a3be887054c15
- Reviewed HEAD SHA：cfae511400646766516cc6810efff639ec130362
- Reviewed patch-id：227391ebfc4b3016dcdff4f7873417ebd9a664ef
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-20
- Review artifact：docs/issues/issue-0012/review-cfae511.md

範圍核對（2026-10-20）：`gh pr view 18` 的 headRefOid 為 `cfae511…`、base 為 `main`；本機 `git diff 1af4528 cfae511 | git patch-id --stable` 得 `227391eb…`，與指定值相同。PR CI（fmt · clippy · test）為 pass。

## 問題與風險

### MUST FIX

**M-1　告知查詢失敗時，剛取得的 session lease 不會釋放，該 scope 會被鎖住到 lease 過期（預設 900 秒）**

- 位置：`crates/wukong-runtime/src/turn.rs`，`prepare_final_session` 之後的
  `memory.active_session_leases().await?`。
- 問題：這個 `?` 位在 `let result = async { … }.await;` 之外。`result.is_err()` 時才呼叫的
  `release_agent_session_lease` 只保護 async 區塊裡的錯誤，所以查詢失敗會直接從函式返回，
  而 lease 仍由本回合持有。變更前，取得 lease 之後到 `result` 區塊之間沒有任何會失敗的操作；
  其他取得 lease 的路徑（`session.rs:79/108/112`、`wukong-cli/src/command.rs:64/86/91/100`）
  也都在錯誤時先釋放。本 PR 打破了這個既有不變式。
- 後果：同一個 scope 接下來的每個回合都會在 `prepare_final_session` 以
  `session lease unavailable for scope …` 失敗，直到 lease 過期。對 Telegram 使用者是 15 分鐘
  無法對話，症狀正好是 #12 原本要處理的錯誤。發生機率低（WAL 模式、同一個 pool 剛完成寫入後的讀取），
  但 pool 取得逾時或 I/O 錯誤都會觸發，而且這個功能只是軟性告知，不值得承擔這種後果。
- 重現（變異實驗，獨立 `CARGO_TARGET_DIR`）：把查詢的資料表名改成不存在的
  `agent_session_state_missing`，加一支探針測試：跑一個回合後讀 `agent_session_state`。
  結果：回合失敗，且 `lease leaked until Some(1792490750)`，也就是 `lease_owner` 仍在、
  `lease_until` 為取得時間加 900。
- 建議修法（擇一，兩者都只動一兩行）：
  1. 把查詢移進 `result` async 區塊，失敗時由既有的 release 處理，維持「查詢失敗即回合失敗」。
  2. 改為 best-effort：查詢失敗就 `eprintln!` 一行並當作沒有其他回合（不附加告知）。
     reviewer 傾向此做法，因為 README 的風險評估寫明「失敗後果是告知不準……等於回到現狀」，
     讓告知失敗連帶使回合失敗，與這個定位不一致。實驗中以 `.unwrap_or_default()` 驗證過，
     回合會成功、lease 正常釋放。
- 不論選哪一種，請把上述探針改寫成回歸測試並先取得紅燈（目前沒有任何測試覆蓋這條路徑）。
  要在 runtime 測試中讓查詢失敗，可能需要一個可注入失敗的方式；若無法以合理成本做到，
  至少在 implementation-plan 記錄手動重現步驟與結果。

### SHOULD FIX（不阻塞）

**S-1　SCN-003 的「過期」在 Memory／runtime 層沒有測試，綁定時鐘的那一行可被改壞而全綠**（證據持久力）

- 變異 `Memory::active_session_leases` 改傳 `0` 取代 `now_unix()`（等於把所有曾持有、未釋放的
  lease 都當成有效）：三支相關測試全部仍為綠燈。
- SQL 本身的過期判斷由 store 測試覆蓋，而且邊界與 NULL 都會紅燈（見下方實驗），所以本次驗收的
  綠燈有效；缺的只是 wrapper 那一行。可補一支 Memory 層測試（`acquire` 時 `lease_secs` 下限為 1 秒，
  需要等待約 2 秒），或在 runtime 測試直接寫入一筆 `lease_until` 已過的資料列。

**S-2　重現證據的措辭與實際紀錄不符**

- `implementation-plan.md` 步驟 4 與 PR 說明都寫「後開始的排程回合兩棒都收到告知」。實際上
  `repro12/after2` 的排程回合只有一棒（oracle）；第二個呼叫是空輸出的修復呼叫
  （`prompt-schedule-3097279.txt` 含 `[修復回覆]`），因為假 agent 的輸出沒有被解析成文字。
- 「取得 lease 之後的每個呼叫都帶告知」這個性質仍然成立，多棒的覆蓋由單元測試
  `other_running_turns_are_announced_to_every_step`（explorer、fixer）承擔，所以不構成不實回報；
  但請把措辭改成「唯一一棒與空輸出修復呼叫都帶告知」。

### NICE TO HAVE

- **N-1** `implementation-plan.md`「設計」寫查詢回傳 `(scope, lease_until)`，實作只回傳 `scope`。
- **N-2** 告知附在最後一棒的 prompt 上，會留在 opencode session 歷史中。之後的回合即使沒有其他回合
  在執行，也不會有任何文字推翻先前的告知。這屬於 TBD-4（模型行為）的範圍，可考慮在 AGENTS.md
  的限制中補一句。回合中途對方結束導致告知過時，目前只隱含在「只在取得 lease 的那一刻算一次」，
  可以明寫。
- **N-3** scope 原樣插入 prompt。Web 的 scope 由請求參數決定（`selected_scope` 只做 trim），
  名稱中若含換行，就能在其他回合的告知裡插入任意行。只有通過授權的使用者做得到，而且他們本來就能
  對共用工作目錄下指令，風險低；可考慮把 scope 中的控制字元轉義，或只輸出單行。
- **N-4** `/compact` 指令（`wukong-cli/src/command.rs`）也會取得 lease，壓縮期間會被列成
  「對話 <scope>」。壓縮不改工作目錄，屬短暫誤報。
- **N-5** 沒有「同時有多個其他回合」的測試。迴圈很單純，但這是 `ORDER BY scope` 與多行輸出唯一的
  驗證機會。
- **N-6** GitHub issue #12 本文仍是 2026-10-08 的原草案，而 PR 寫 `Closes #12`。README 已說明
  「草案從未核准，本文件取代它」，符合 acceptance.md；可在 issue 留一句指向 README，
  避免讀 issue 的人以為原草案的 SCN-002（取消與逾時後回收鎖）已被驗收。

### 需決策

**D-1　多聊天部署下，其他使用者的 scope 會出現在本回合的 prompt 中**（不阻塞）

- 待決問題：`WUKONG_TG_ALLOWED` 可以列多個 chat。A 的回合告知會列出「對話 user:tg-<B 的 chat id>」，
  agent 可能在回覆中把這件事告訴 A。SCN-001 核准的範例就是列出 scope，所以這在核准範圍內；
  共用工作目錄與 `wukong schedule list`（列出所有排程）也已讓 agent 看得到跨使用者資訊，
  新增的暴露有限。
- 可選做法：(a) 維持現狀；(b) 本回合以外的 `user:` scope 只顯示「另一個對話」，排程仍顯示 id；
  (c) 只列排程，不列其他對話。
- reviewer 建議：部署只有一個使用者時維持 (a)；預期會有多個使用者時採 (b)，這是一行的改動，
  而且不影響 SCN-001 對「類型」的要求。

### 範圍外觀察（不屬於本 PR）

- 重跑重現腳本時看到，排程回合最後一棒的排程能力提示寫的是
  `--scope "agent:schedule-<id>"`（#14 的行為）。在排程回合內新增的排程，擁有者會是 agent scope，
  結果不會回送 Telegram。是否刻意如此，請另外確認。

## 已查核維度

- 驗收與證據：
  - SCN-001：`other_running_turns_are_announced_to_every_step` 兩個方向、explorer 與 fixer 兩棒
    都檢查，並檢查不列出自己。變異「不過濾自己」「只加到最後一棒」「標籤對調」都會紅燈。
    重現腳本由 reviewer 以 HEAD binary 重跑（`scratchpad/rev-repro`）：後開始的排程回合帶
    `- 對話 user:tg-42`，先開始的聊天回合沒有告知，planner 呼叫沒有告知。足以支持。
  - SCN-002：runtime 測試比較同版本的兩個記憶庫，變異「沒有其他回合仍附加區塊」會紅燈。
    跨版本由 reviewer 補證：以 BASE（`1af4528`，獨立 target 目錄）與 HEAD 各建一個 `wukong`，
    用記錄每次 prompt 的假 agent 跑相同的兩個回合，比較全部 prompt：
    - 沒有其他 lease：BASE 與 HEAD 逐字相同（`diff -r` 無差異）。
    - 另有一筆已過期 lease（`agent:schedule-old`，`lease_until = now-5`）與一筆已釋放的 lease
      （`project:Other`）：逐字相同。
    - 另有一筆有效 lease（`agent:schedule-live`）：唯一差異是 explorer、fixer 與修復呼叫三個
      prompt 結尾多出告知區塊，planner prompt 相同。
    因此「與變更前逐字相同」在跨版本也成立。
  - SCN-003：store 測試涵蓋有效、過期、已釋放三種，並在 `now == lease_until` 的時刻斷言為空。
    變異「納入過期」「`>=` 邊界」都會紅燈。runtime 層只測已釋放與自己的 lease。過期判斷完全在 SQL，
    由 store 測試承擔，本次足夠；Memory 層的時鐘綁定未覆蓋，見 S-1。
  - SCN-004：reviewer 重跑重現腳本，聊天與排程都是 exit 0，`edits.log` 顯示兩邊每一棒都交錯執行。
  - 紅燈紀錄：實作者以回傳空清單的 stub 取得 memory 與 runtime 注入測試的紅燈，SCN-002 標為
    characterization（變更前已綠），符合 verification.md。期望值都是手寫字面值，不是同義反覆。
- 相關失敗面：

  | 失敗面 | 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
  |---|---|---|---|---|
  | `lease_until` 為 NULL | 已釋放的資料列 | 不列入 | store 測試 `project:Done`、runtime 已釋放 lease 測試；SQL `NULL > ?` 為假 | 通過 |
  | `lease_until == now` | lease 剛好到期 | 不列入，與 acquire 的 `lease_until <= now` 可取得一致 | store 測試在 110 斷言為空；`>=` 變異紅燈 | 通過 |
  | 查詢失敗 | 讀取錯誤 | 回合失敗或略過告知，lease 都要釋放 | 無；變異實驗證實 lease 外洩 | **M-1** |
  | 先開始的回合不知道後來者 | 聊天先、排程後 | 已揭露的限制 | AGENTS.md、CHANGELOG、重現紀錄 | 已揭露 |
  | 回合中途對方結束 | 告知過時 | 限制 | 只隱含在「只算一次」 | N-2 |
  | 多個其他回合 | 兩筆以上有效 lease | 逐行列出、依 scope 排序 | 無測試 | N-5 |
  | scope 特殊字元 | 名稱含換行 | 不污染 prompt 結構 | 無 | N-3 |
  | `agent:schedule-` 前綴誤判 | `agent:main`、`agent:fixer` | 標為對話 | 前綴完整比對 `agent:schedule-`，只有排程 namespace 會符合 | 通過 |
  | 非回合持有 lease | `/compact` 指令 | — | 會被列為對話 | N-4 |
  | 告知寫入記憶或聊天紀錄 | 有其他回合時跑回合 | 記憶只存使用者原文與回答 | `remember` 寫 `user_text` 與 `answer.output`；e2e 記憶庫 `text LIKE '%其他回合正在執行%'` 為 0；聊天紀錄由進入點存使用者輸入與回答，不經 prompt | 通過 |
  | 告知留在 opencode session | 最後一棒 | — | 未揭露 | N-2 |
  | 注入順序 | 告知在 `[輸出要求]`、排程能力、memoria 提示之後；修復指令接在告知之後 | 不影響既有機制 | 沒有程式依賴 prompt 結尾；修復呼叫沿用含告知的 prompt，符合「每一個呼叫都告知」 | 通過 |
  | 與 #14 的交互 | 排程 scope `agent:schedule-<uuid>` | 標為排程 uuid，`schedule list` 可對應 | 單元測試與重現 | 通過 |
  | 與 session_retention 的交互 | 保留期清理讀 session 指向 | 不受影響 | 新查詢唯讀、不改 `session_id`；M-1 外洩的 lease 不影響保留判斷 | 通過 |
  | 跨使用者資訊 | 多個 Telegram chat | — | 在核准範圍內 | D-1 |

- 需求、架構、安全、品質：
  - 需求：實作與核准的 SCN-001～004 一致；「不阻擋」成立；沒有超出範圍的排隊或續租。
  - 架構：只沿 runtime → memory 的既有方向新增一個唯讀查詢，沒有 schema 變更，沒有新增 crate 相依。
  - 安全：D-1、N-3 為低風險資訊暴露與 prompt 結構問題，都只有通過授權的使用者觸發得到。
  - 品質：改動精簡（程式碼約 60 行，其餘為測試），類型判斷集中在 `concurrent_turns_notice`，
    重用 `scheduling_bin()`，沒有重複或過度設計；code-simplify 記為 no-op 合理。
- 豁免、待確認與限制：
  - TBD-2（超過 900 秒不可見，追蹤 #13）、TBD-3（當掉殘留誤報，追蹤：無，附理由）、
    TBD-4（不驗模型行為）都有使用者接受的日期與去向，符合 docs/AGENTS.md 對「不影響本次交付」的要求。
  - 範圍重新界定：README 概述說明原草案從未核准、由本文件取代；核准來源記錄使用者兩個選擇與
    「都接受」的日期；核准 commit `802cb07` 與 HEAD 的驗收區塊比對無差異（README 只改了核准 commit
    回填與狀態），符合 acceptance.md。
  - README 狀態「實作中」在審查期間合理。
  - AGENTS.md 與 CHANGELOG 的描述（每一棒、只算一次、planner 看不到、900 秒限制）與實際行為一致。
- 自行執行的驗證（2026-10-20，工作區為乾淨的 `cfae511`）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo test -p wukong-memory -p wukong-runtime --locked`：memory 78 passed、runtime 81 passed，
    其餘測試目標全綠，exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test --workspace --locked`：passed=639、failed=0、ignored=12，與實作者紀錄相同。
  - 變異實驗（HEAD 的 `git archive` 副本，`CARGO_TARGET_DIR=scratchpad/mut-target`，只跑三支相關測試）：

    | 變異 | 結果 |
    |---|---|
    | 不過濾自己的 scope | SCN-001、SCN-002 測試紅燈 |
    | SQL 納入過期 lease | store 測試紅燈 |
    | SQL 邊界改 `>=` | store 測試紅燈 |
    | 只加到最後一棒 | SCN-001 測試紅燈 |
    | 沒有其他回合也附加區塊 | SCN-002 測試紅燈 |
    | 排程標籤改成對話 | SCN-001 測試紅燈 |
    | Memory wrapper 傳 `0` 取代 `now_unix()` | **全綠（存活）**，見 S-1 |
    | 查詢失敗 + lease 探針 | 回合失敗且 lease 外洩，見 M-1 |
    | 同上，改為 `.unwrap_or_default()` | 回合成功、lease 已釋放 |

  - 跨版本 prompt 比較：腳本 `scratchpad/tools/xver.sh`、`xagent.sh`，輸出在 `scratchpad/xver/`，
    結果見上方 SCN-002。
  - 重現腳本：`repro12/run.sh <HEAD 的 wukong 複本> scratchpad/rev-repro`，兩邊都是 exit 0，結果見
    SCN-001、SCN-004。所有端到端執行的 HOME、`WUKONG_MEMORY_DB`、`WUKONG_WORKSPACE` 都指向臨時目錄，
    沒有呼叫真實模型。

  （`scratchpad` 指 `/tmp/claude-1000/-home-kevin-Documents-RCodes-Wukong/aec8f965-8483-49ca-a8fe-11acc774e792/scratchpad`。）

## 流程判定
RETURN TO execute-task

理由：M-1 是本 PR 引入的邏輯錯誤。在錯誤路徑上，它讓 session lease 外洩，導致同一個 scope
最多 900 秒無法執行回合，而且目前沒有任何測試覆蓋。修正與回歸測試完成後，需要對新 HEAD 重新審查；
S-1、S-2、N-*、D-1 不阻塞。
