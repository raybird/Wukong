# 審查報告
- 範圍：完整 PR（GitHub PR #4，`issue-0003-opencode-session-retention` → `main`；審查的是 BASE..HEAD 的完整 diff，32 個檔案。這個範圍實際有五個提交：18cdda8、6037672、cbd65ef、46cd815、a09b2ad——18cdda8 只動 issue 文件，交辦說明與上一份報告列的提交清單都漏了它，diff 本身有涵蓋。依限制未連線 GitHub，見「豁免、待確認與限制」）
- Reviewed BASE SHA：c613028e74127c633fecf408bacb93f0dceb915c
- Reviewed HEAD SHA：a09b2ad0ac19e027f382194b88a5ca3f911722ed
- Reviewed patch-id：dfbb94f8eddafcde0eea7036bc10cabf20721a58（2026-10-01 自行以 `git diff BASE HEAD | git patch-id --stable` 重算，相符；`git rev-parse HEAD` 相符；`git merge-base main HEAD` 即 BASE；審查前後 `git status --short` 皆為空）
- 獨立 reviewer：Claude Code 隔離 subagent（claude-opus-5-5）；未參與本次實作，也不是 review-6037672、review-46cd815 的作者
- Review artifact：docs/issues/issue-0003/review-a09b2ad.md
- 審查日期：2026-10-01
- 風險：High（不可逆刪除）

## 問題與風險

### MUST FIX

無。

### SHOULD FIX

**S-1　a09b2ad 把「server 清單是空的」一律算成對得上，連「記憶庫有指向、server 卻一個都沒列出」也算；這個組合與 SCN-010 的字面不一致，也沒有測試**

- 位置：`crates/wukong-runtime/src/session_retention.rs:255-260`（`listing.sessions.is_empty() || …`）；欄位註解 `:140-142`；測試 `:441-454` 與 `crates/wukong-cli/tests/opencode_prune.rs:165-172`（兩者的記憶庫都是空的）；`docs/cli-reference.md:89`、`docs/docker.md:218`、`docs/issues/issue-0003/implementation-plan.md:61`
- review-46cd815 的 N-2 描述的是「server 與記憶庫都是空的」的全新部署。實作的條件只看清單是否為空，所以也涵蓋「記憶庫指向某個 session、server 列出 0 個」——這正是 SCN-010 的 Given（「記憶庫指向的 session 沒有任何一個出現在 opencode server 的清單裡」）。
- 重現（2026-10-01，HEAD 的 debug binary 對拋棄式假 server；記憶庫 `agent_sessions` 有一筆 `ses_elsewhere`，清單回 `[]`）：
  - `wukong opencode prune`（含 `--dry-run`）：輸出「已刪除 0 個…／共列出 0 個 session」，結束碼 0，沒有原因說明。46cd815 在同樣輸入下是「未清理：…」與結束碼 1。
  - schedulerd（只把 `INTERVAL` 改成 3 秒的匯出複本）：`opencode_session_retention … listed=0 … anchored=true … deleted=0`，沒有 `warning:` 行，也沒有記憶庫位置。
- 影響：不會刪到任何東西（清單是空的，沒有可挑的）。受影響的是訊號：`anchored=true` 在這裡的意思與它自己的註解（「這份記憶庫指向的 session 至少有一個出現在 server 的清單裡」）和 `docs/docker.md:228` 的說法相反；`WUKONG_AGENT_SERVER_URL` 指到一個全新的 server、`opencode-state` 被清空、或 server 換了 project 而列不出舊 session（上一輪列為未查證、這一輪實作者也沒查證的那一項）時，原本會出現的「對不上」說明現在不會出現。一旦 server 上出現任何一個不在記憶庫裡的 session，防護與警告就恢復。
- 為什麼不列 MUST FIX：SCN-010 的 Then（不刪、不列待刪）成立；And 的用意是讓「防護持續拒絕」看得見，而清單是空的時候沒有東西被拒絕，「共列出 0 個」本身就是原因。上一輪的審查者也把「空 server 回結束碼 1」視為瑕疵而非規格要求。但字面上這是已核准劇本的一個輸入，行為在本次提交被改掉，且沒有人把這個組合拿去問過使用者。
- 建議（二擇一）：把條件收窄為「清單是空的**且**記憶庫沒有任何指向」，並補一個「記憶庫有指向、清單為空」的 fixture；或維持現狀，把這個解讀記進 README（屬 SCN-010 的措辭釐清，需使用者同意）並同步更新 `anchored` 的註解與三處文件。

**S-2　schedulerd 實際執行的那條路徑——寫出日誌、傳入記憶庫位置、迴圈呼叫清理——沒有任何測試會為它紅燈（證據持久力）**

把 HEAD 匯出到 repo 之外做變異，下列三個變異後 `cargo test -p wukong-schedulerd session_retention` 仍是 6 項全過：

| 變異 | 被釘住的應該是 | 位置 |
|---|---|---|
| `run_within` 不再把 `log_lines` 的結果寫出去 | SCN-010 在 schedulerd 的輸出（上一輪的 M-1） | `crates/wukong-schedulerd/src/session_retention.rs:74-76` |
| `main` 傳給 `run_once` 的記憶庫位置改成空字串 | 「所用的記憶庫」 | `crates/wukong-schedulerd/src/main.rs:141` |
| 迴圈的清理分支永遠不呼叫 `run_once` | SCN-001、SCN-003 的定期執行 | `crates/wukong-schedulerd/src/main.rs:139-145` |

新增的 `a_memory_that_does_not_match_the_server_is_explained_in_the_log` 測的是 `log_lines` 這個函式回傳什麼，不是 daemon 寫出了什麼。HEAD 的實際行為我以匯出複本觀察過是正確的（見「驗收與證據」的 SCN-010），實作者也揭露了「這條路徑沒有以真正的 schedulerd 觀察」，所以不構成假綠燈，本次驗收判定不受影響。建議：讓 `run_within` 接受一個寫出函式（或回傳要寫的行），由測試以「記憶庫對不上的假 server」跑一次並斷言寫出的內容含傳入的位置；迴圈接線可比照 `tests/new_session_flag.rs` 以 binary 加可注入的間隔測。

**S-3　TBD-6 仍是「待確認」：SCN-008 在 CLI backend 不成立，要由使用者決定，而不是留在表格裡**

- 位置：`docs/issues/issue-0003/README.md:167`；`crates/wukong-gateway/src/backend.rs:82-84`（trait 的空實作，`AgentCliBackend` 只實作 `run` 與 `run_streaming`）；`docs/issues/issue-0003/implementation-plan.md:104`（「`wukong --new`｜舊 session 被刪除」沒有限定 backend，與 `CHANGELOG.md:29`、`:39` 的說法不一致）
- TBD-6 的描述屬實（讀程式碼確認；依限制沒有以 CLI backend 實際執行）。binary 模式的預設 backend 就是 CLI，所以 `wukong --new` 最常見的用法在修正後行為不變。
- 我判定不阻塞的依據：核准當時的計畫（`git show c613028:…implementation-plan.md` 第 18、122 行）已同時記錄「CLI backend 沒有實作 `delete_session`」與步驟 7 的產出「`--new` 與 `/new` 行為一致」，後者已達成且有 binary 層級的測試；不刪是安全方向；上一輪以相同資訊列為 SHOULD FIX。但 SCN-008 的文字沒有限定 backend，「限定為 server backend」或「另開 issue 補齊」都是規格層的決定，審查者與實作者不能代答。建議合併前向使用者問這一題，把 TBD-6 改成已解決或開出後續 issue，並把計畫第 104 行補上限定。

### NICE TO HAVE

- **N-1**　compose 兩份檔案用 `${WUKONG_OPENCODE_SESSION_RETENTION_DAYS:-30}`（`docker-compose.yml:241`、`docker-compose.release.yml:183`）。`.env` 裡把這個變數留空的人得到的是 30 天而不是停用；程式本身把空字串視為停用（`policy_defaults_to_thirty_days_and_zero_disables` 有涵蓋 `""`），但空值到不了程式。「不確定就不刪」的方向在這一層被反過來了。改用 `${VAR-30}`，或在 `.env.example` 寫明「留空等於 30」。
- **N-2**　TBD-7（截斷時被截掉的受保護子 session）只記在 issue README；`docs/docker.md` 對 `truncated=true` 的說明與 `CHANGELOG.md` 的已知限制都沒有提。不修的理由我查核過是成立的（見「豁免、待確認與限制」），補一句即可。
- **N-3**　`parse_session_listing` 的錯誤訊息會帶上整份回應或整筆 session（`crates/wukong-gateway/src/opencode_server.rs:898`、`:917`）。session 物件含標題，回應不是陣列時可能很大；這些會進 schedulerd 日誌。截短並只留 id 較妥。
- **N-4**　紀錄上的小出入：README「涉及檔案」的 Cargo 註記沒有提到 schedulerd 新增的 `tokio` `test-util` 測試相依；`CHANGELOG.md:41-43` 的已知限制清單中間（第 42 行）多一個空行，最後一項被切成另一個清單；交付範圍的提交清單漏列 18cdda8。
- **N-5**　`scripts/test-docker-runtime.sh:212-215` 仍只比對 entrypoint 內的字串，拿掉 `||` 之後的降級它還是綠的（review-6037672 的 N-2，維持原樣）。行為我以替身指令與 `strace` 另外確認過。

### review-46cd815 各項發現的處置

| 項目 | 處置 | 查核 |
|---|---|---|
| M-1 SCN-010 的說明在 schedulerd 路徑沒有實作 | 已確實解決 | 以只改 `INTERVAL`（3 秒）的匯出複本執行真正的 `wukong-schedulerd`，記憶庫指向別處：摘要行之後出現 `warning: opencode session retention skipped: 記憶庫 sqlite:///…/other.db 指向的 session 沒有任何一個出現在…整輪不刪。…`，每輪都有，server 只收到 `GET /session?limit=10000`、沒有 `DELETE`。`docs/docker.md:228` 已說明 `anchored`。測試只釘住產生文字的函式，見 S-2 |
| M-2 兩份記憶庫共用一個 server | 合法的不處理，揭露已更正 | 重現仍成立（以「主機」記憶庫執行，`c_chat` 被刪、結束碼 0）。`docs/docker.md:219` 改為獨立一類並寫明「防護擋不住」與處置；`.env.example`、`CHANGELOG.md`、`AGENTS.md`、README 的 TBD-5 一致。使用者在 SCN-010 的確認題中只選了一種防護，不加新防護符合該決定 |
| S-1 `--new` 在 CLI backend 不刪 | 文件已更正，決定未做 | `CHANGELOG.md:29`、`:39` 已限定；README 記為 TBD-6「待確認」，見本報告 S-3 |
| S-2 四個行為沒有測試會紅燈 | 已解決 | 四個變異全部轉紅：`ticker()` 立即觸發、拿掉「記憶庫：」那一行、結束碼恆為 0（另試「只忽略刪除失敗」也轉紅）、父不在清單的子 session 被當成根。成環的 fixture 已補 |
| S-3 截斷時被截掉的受保護子 session | 不修，記為 TBD-7 | 重現仍成立（清單恰 10,000 筆、`root_r` 被列為將刪除）。不修的理由查核成立，見下 |
| N-1 重啟間隔短於 6 小時永不清理 | 已揭露 | `docs/docker.md:228`、`CHANGELOG.md:41`、TBD-8 |
| N-2 全新部署回結束碼 1 | 已解決，範圍比建議寬 | 見本報告 S-1 |
| N-3 `SQLITE_TMPDIR` 與資料庫位置不一致 | 已解決 | 見「驗收與證據」的 SCN-007 |
| N-4 `cli-reference` 的標題 | 已解決 | `docs/cli-reference.md:44` |
| N-5 `--new` 與 `/new` 兩份寫法 | 未動 | 上一輪已判定可接受 |
| N-6 核准來源少列一個選項 | 已解決 | README 第 119 行列出四個選項。Gherkin 區塊未受影響 |
| 未查證：server 換 project 後舊 session 是否還會列出 | 仍未查證 | 實作者明說沒有查證。S-1 的改動恰好影響這個情境的訊號 |
| 未查證：GitNexus 影響分析 | 實作者陳述已執行 | 由產物無從查核 |

## 已查核維度

### 驗收與證據

「重跑」指我在 2026-10-01 於 HEAD a09b2ad 親自執行；「僅閱讀」指只讀了實作者的紀錄（需要真實 opencode 容器，在本次審查的禁止範圍內）。

| 編號 | 實作 | 證據位置 | 查核方式 | 結論 |
|---|---|---|---|---|
| SCN-001 | `select_expired`＋`prune_opencode_sessions` | runtime `session_retention` 測試；計畫步驟 1、8、9 | 單元測試重跑；binary 對假 server：只有過期無主的 `ses_old_orphan` 收到 `DELETE`；schedulerd 複本同。訊息／片段／事件隨之消失是 opencode 的行為，**僅閱讀** | 通過 |
| SCN-002 | 受保護集合取自兩張表；以樹判定 | `referenced_session_ids_cover_both_session_tables`、`deletes_expired_orphans_…`、`a_scope_pointing_at_a_child_protects_the_whole_tree` | 重跑；假 server 上受保護的 `ses_old_kept` 未收到 `DELETE`。另查過 `agent_session_state` 沒有其他存放 session id 的欄位，web／telegram／scheduler 也沒有另存指向。「下一回合續接」沒有真實模型回合，實作者已揭露 | 通過；截斷邊界為 TBD-7 |
| SCN-003 | `RetentionPolicy::from_env` | `policy_defaults_…`、`only_a_server_backend_…` | 重跑；binary：`off` → 警告＋「已停用」、`0` → 「已停用」，皆結束碼 0 | 通過；compose 的空值見 N-1 |
| SCN-004 | 兩個 `?` 先於任何刪除；清單任何一筆看不懂就整份失敗 | `an_unreadable_scope_table_deletes_nothing`、`a_failed_listing_deletes_nothing`、gateway 的 `list_sessions_fails_…` | 重跑；schedulerd 複本在 server 消失後每輪記一行 `warning: opencode session retention failed: …list_sessions failed…` 並繼續 | 通過 |
| SCN-005 | `GET /session?limit=10000`；筆數達上限即標示 | `list_sessions_asks_for_every_session_with_an_explicit_limit`、`a_full_page_is_reported_as_truncated`、`truncation_is_carried_…` | 重跑；binary 對 10,000 筆的假清單：輸出「列表已達上限而被截斷」。真實 opencode 上的行為**僅閱讀**，且樣本最多 304 筆（見限制） | 通過 |
| SCN-006 | 單筆失敗記錄後繼續；整輪 5 分鐘上限 | `one_failed_delete_does_not_stop_the_rest`、`a_server_that_never_answers_gives_the_loop_back`、`a_failed_delete_exits_nonzero` | 重跑；另把 `RUN_BUDGET` 改成 2 秒的複本對「接受連線但不回應」的假 server：每輪記 `timed out after 2s`，排程掃描（`/global/health`）照常，SIGTERM 正常結束 | 通過 |
| SCN-007 | `opencode_db::vacuum`＋entrypoint | `opencode_db` 6 項；計畫步驟 6、8、9、10 | 單元測試重跑；binary：55,316,480 → 36,880,384 bytes，`integrity_check` ok、`journal_mode` 仍為 wal、`user_version` 與資料列不變、未留暫存檔；再跑一次 `below_threshold`；檔案不存在 `outcome=missing` 且不建立任何東西。entrypoint 片段（自 HEAD 原文擷取、`set -euo pipefail`、替身指令）：預設、覆寫、含空白、相對路徑、空值、回收失敗、舊 binary、沒有 `wukong` 八種情況都走到 server 啟動。真實容器的重啟**僅閱讀**，且是 a09b2ad 之前的 entrypoint（實作者已揭露） | 通過 |
| SCN-008 | `--new` 先刪再清，刪除失敗仍清 | `tests/new_session_flag.rs` 2 項 | 重跑；變異「刪除失敗就中止」轉紅 | server backend 通過；CLI backend 見 S-3 |
| SCN-009 | `dry_run` 在刪除迴圈前返回 | `preview_deletes_nothing_and_names_what_a_real_run_deletes` | 重跑；binary：預覽不送 `DELETE`，之後實刪的正是預覽列出的 id | 通過 |
| SCN-010 | `anchored` 判定；`prune` 印記憶庫位置；schedulerd 的 `warning:` 行 | runtime 測試、`tests/opencode_prune.rs`、schedulerd 的 `log_lines` 測試 | 重跑；binary：空記憶庫與指向別處的記憶庫，預覽與實刪都回「未清理…」、結束碼 1、沒有 `DELETE`、第一行是記憶庫位置；schedulerd 複本寫出原因與記憶庫位置 | 清單非空時兩條路徑都通過；清單為空的組合見 S-1 |

**規格與核准**：把三個版本的 Gherkin 區塊各自擷取後比對——cbd65ef 與 HEAD 完全相同；c613028 與 HEAD 的唯一差異是新增 SCN-010 的 7 行，SCN-001 至 SCN-009 逐字未變。核准表與現存 Scenario 集合相等。cbd65ef 之後 README 的變動是核准 commit 回填、核准來源補上第四個選項的名稱、TBD 與 Timeline。對話本身我無從查證。

**新測試的真偽（a09b2ad）**：
- `the_first_run_waits_a_full_interval`：暫停時鐘下，間隔前 1 秒逾時、之後 2 秒內觸發；改回 `interval(INTERVAL)` 轉紅。測的是 `main` 實際使用的 `ticker()`。
- `tests/opencode_prune.rs` 四項：執行真正的 binary；期望值是字面的請求字串、結束碼與傳入的資料庫位置。五個變異（拿掉記憶庫那一行、結束碼恆為 0、忽略刪除失敗、防護永不拒絕、空清單視為對不上）各自轉紅。
- `a_child_whose_parent_is_not_listed_…`、成環 fixture：前者在變異下轉紅。
- `an_empty_server_is_not_a_mismatch`：會為這個條件紅燈，但只涵蓋記憶庫也是空的（S-1）。
- schedulerd 的兩個 `log_lines` 測試：拿掉 `warning:` 行會轉紅；但測的是函式而非 daemon 的輸出（S-2）。第一行以 `summary_line()` 自己比對，摘要內容由 runtime 測試釘住。

**變異測試彙總**：19 個變異，16 個轉紅（上列之外：防護要求全數相符、邊界改 `<=`、樹的年齡只看根、列表帶回 `roots=true`、由新到舊刪、第一個失敗就停、摘要行改欄位名、`--new` 刪除失敗就中止）；3 個未被抓到，即 S-2。未發現假綠燈。

**全量檢查（重跑）**：`cargo test --workspace` 40 個套件、602 通過、0 失敗；`cargo clippy --all-targets -- -D warnings` 無警告；`cargo fmt --all -- --check` 通過；`bash scripts/test-docker-runtime.sh` 通過。與實作者步驟 10 的數字一致。

### 相關失敗面

| 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|
| 記憶庫是空的或完全指向別處，server 有 session | 整輪不刪並說明 | 測試＋binary＋schedulerd 複本 | 正確 |
| 記憶庫有指向，server 清單為空 | 依 SCN-010 字面應說明 | 無 | 不刪，但沒有說明，見 S-1 |
| 記憶庫與 server 都是空的 | 不算失敗 | 測試＋binary | 正確 |
| 兩份記憶庫共用 server、各有對應 | 明確揭露 | 無測試；文件與 TBD-5 已揭露 | 會刪，屬使用者決定後的不處理 |
| Wukong 與使用者自己的 opencode 共用 server | 同上 | TBD-5、文件 | 同上 |
| 進行中的回合所建立、尚未寫回對應的 session | 不被選中 | 保留期下限 1 天；讀程式碼 | 正確 |
| scope 指向子 session | 整棵樹保留 | 測試 | 正確 |
| 父子成環、父不在清單 | 不卡死、不單獨刪 | 測試（本輪新增） | 正確 |
| 截斷把受保護的子切在清單之外 | 根仍受保護 | 無；TBD-7 | 不成立，已揭露 |
| 批次中一筆刪除失敗 | 其餘照常、結束碼 1 | 測試＋binary 測試 | 正確 |
| server 接受連線但不回應 | 放棄這一輪，迴圈繼續 | 測試＋複本重現 | 正確 |
| server 消失 | 記錄後繼續 | 測試＋複本重現 | 正確 |
| schedulerd 在第一輪之前就重啟 | 仍會清理 | 無；TBD-8 | 不會，已揭露 |
| 保留天數無法解析 | 停用並警告 | 測試＋binary | 正確 |
| `.env` 把保留天數留空（compose） | 不確定就不刪 | 無 | 得到 30 天，見 N-1 |
| `--new` 時刪除失敗 | 仍開新 context | 測試 | 正確 |
| `--new`、CLI backend | 舊 session 被刪 | 無；TBD-6 | 不會刪，見 S-3 |
| vacuum：被鎖、磁碟不足、檔案不存在、舊 binary、沒有 binary | 不擋啟動、不動檔案 | 測試＋重現 | 正確 |
| vacuum：`WUKONG_OPENCODE_DB` 被覆寫 | 暫存檔落在磁碟檢查量過的檔案系統 | 字串檢查；`strace` 重現 | 正確：設定後暫存檔開在資料庫目錄，未設時開在 `/var/tmp`；`SQLITE_TMPDIR` 指到不存在的目錄時 SQLite 自行退回 `/var/tmp`，不會失敗 |
| `wukong opencode --help \| grep -q` 在 `pipefail` 下誤判 | 不因管線而略過回收 | 讀程式碼 | 說明文字一次寫完、且在 `if` 條件內，不成立 |
| 容器啟動時 vacuum 耗時 | 不卡住啟動 | busy timeout 5 秒；無總時限 | 可接受：實測 831 MB 為 2.59 秒（僅閱讀），失敗即降級 |

### 需求、架構、安全、品質

- **需求**：十個 Scenario 都有對應實作。diff 內的檔案都在 README 的涉及檔案範圍內；`session.rs`、`opencode-idle-restart.sh` 未被觸及；資料庫只做 `VACUUM`。
- **架構**：相依方向未被破壞——`wukong-runtime::session_retention` 只用 gateway 與 memory；schedulerd 與 cli 用 runtime 及其下層；`SessionSummary`／`SessionListing` 在 gateway。`AiBackend::list_sessions` 的預設實作回傳錯誤而非空清單。
- **安全／權限**：列表與刪除都走既有的 `authorize`；entrypoint 以 `gosu wukong` 降權執行 vacuum，root 只跑 `--help`；環境變數只加在那一個指令前，不會流到 `opencode serve` 或閒置重啟腳本。未發現注入或提權路徑。日誌可能帶出 session 標題，見 N-3。
- **相依**：`Cargo.lock` 只多三條相依邊。a09b2ad 新增的 `tokio` `test-util` 在 `[dev-dependencies]`；workspace 是 resolver 2，以 `cargo tree -e normal,features` 確認正式建置的 tokio 不含 `test-util`。
- **品質／重複／過度設計**：`log_lines` 把「要寫什麼」與「寫出去」分開，合理；間隔、上限、時間預算與門檻都是常數。重複只有 `--new` 與 `/new` 兩份寫法，以及現在四份的測試用 HTTP stub（`opencode_prune.rs` 與 `new_session_flag.rs` 幾乎相同，可抽到 `tests/common`，不急）。未發現過度設計。
- **文件**：`docs/docker.md`、`docs/cli-reference.md`、`CHANGELOG.md`、`.env.example`、`AGENTS.md` 的間隔、預設值、上限、無效值處置、結束碼、樹狀判定、三類不成立的情況，與程式及我的實測一致。不一致之處：空清單的例外沒有寫進任何一份文件（S-1）；計畫第 104 行（S-3）。
- **計畫的自我回報**：步驟 10 的各項宣稱逐一對照，未發現不實回報。「拿掉記憶庫那一行並讓結束碼恆為 0 後 4 項中 3 項失敗」與我分開做的兩個變異結果相符；測試數相符；「沒有在真實容器重跑」「沒有以真正的 schedulerd 觀察」兩項限制都有寫明。步驟 10 的 M-1 紅燈（函式尚未實作時測試失敗）是否為行為紅燈而非編譯失敗，由產物無從查核。

### 豁免、待確認與限制

- 沒有 gate 豁免。
- 待確認事項逐項判定：
  - TBD-1、TBD-2：已解決，數值與程式一致。
  - TBD-3、TBD-4：待確認，需要受影響主機的資料，不阻塞。
  - TBD-5：描述屬實（重現）。使用者在 SCN-010 的確認題中未選其他防護，屬合法的不處理。
  - TBD-6：描述屬實；狀態是「待確認」而非決定，見 S-3。
  - TBD-7：描述屬實（重現）。不採上一輪建議的最小修法，理由成立：清單被截斷時，被截掉的正是最舊的 session，其中本來就包含被棄置 scope 的受保護 session，所以「截斷且有受保護 id 不在清單」幾乎必然同時成立，那個修法會讓超過 10,000 個 session 的部署永遠不清理。觸發條件需要超過 10,000 個 session、對應是 CLI backend 留下的子 session id、其根在最新的 10,000 筆內且已過期；後果有 gateway 的 404 重建兜底。SCN-002 的字面不允許這個例外，我比照上一輪列為已揭露的限制。
  - TBD-8：描述屬實，文件已寫明處置。
- 實作者自行揭露的限制（SCN-002 沒有真實模型回合、樹狀保護沒有在真實 opencode 的舊資料上演練、容器用 gnu 而非 musl binary、a09b2ad 的 entrypoint 改動沒有在真實容器重跑、schedulerd 的日誌沒有以真正的 daemon 觀察）依規則不視為缺失；後兩項我以替身指令、`strace` 與匯出複本補做了。
- 本次審查的限制：
  - 沒有碰任何容器、volume 或 `~/.local/share/opencode`。「opencode 刪除 session 會連帶清掉訊息／片段／事件」「清單帶 `parentID`、依 `time.updated` 由新到舊」「真實容器重啟時檔案變小」「新 entrypoint 配 v0.21.11 binary」只讀了紀錄。
  - schedulerd 的定期清理我觀察的是只改了 `INTERVAL`（3 秒）、其中一次另改 `RUN_BUDGET`（2 秒）的匯出複本，不是 HEAD 的原始 binary。
  - 沒有連線 GitHub：PR #4 的說明與遠端 HEAD 未核對。本地的 `origin/issue-0003-opencode-session-retention` 指向 a09b2ad，但那是上次同步時的狀態。
  - 所有執行都指向拋棄式假 server、拋棄式 SQLite 檔與 repo 之外的匯出複本。過程中有兩個我啟動的 schedulerd 複本沒有隨外層 shell 結束，已以確切 PID 停止；它們接的是指向別處的記憶庫與已關閉的假 server，沒有送出任何刪除。repo 內除了本報告沒有其他變更。
- 未查證：
  - opencode 是否真的接受 `limit=10000`。實作者的量測最多只到 304 筆；若 server 對 `limit` 另有上限，回傳筆數永遠小於 10,000，`truncated` 就不會標示，SCN-005 的第二個 Then 與 TBD-7 的「截斷會被標示」都倚賴這一點。需要真實 opencode 才能確認。
  - server 的 project 改變後舊 session 是否還會被列出（上一輪留下、仍未查證）。
  - 修正提交是否依根目錄 `AGENTS.md` 跑過 GitNexus 影響分析與 `detect_changes`。

## 流程判定
PASS

理由：沒有 MUST FIX。review-46cd815 的兩項 MUST FIX 都已處理——M-1 在 daemon 實際執行的路徑上觀察到原因與記憶庫位置；M-2 依使用者既有的決定不加防護，文件已改成與實測一致。十個 Scenario 的成功路徑、失效方向與測試真偽查核通過，未發現會刪到仍在使用的 session、擋住容器啟動或卡住排程的路徑（已揭露的 TBD-5、TBD-7 除外）。S-1 與 S-3 各牽涉一個已核准劇本的字面範圍，建議合併前向使用者確認；S-2 是證據持久力。三者依規則不阻塞。
