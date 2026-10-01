# 審查報告
- 範圍：完整 PR（GitHub PR #4，`issue-0003-opencode-session-retention` → `main`；審查的是 BASE..HEAD 的完整 diff，29 個檔案、三個提交 6037672、cbd65ef、46cd815。依限制未連線 GitHub，PR 說明與遠端 HEAD 未核對，見「豁免、待確認與限制」）
- Reviewed BASE SHA：c613028e74127c633fecf408bacb93f0dceb915c
- Reviewed HEAD SHA：46cd8152658a5ed98752ae53f166cb8d8b2e1b8b
- Reviewed patch-id：94d93478543273a0fb8feb9292d82d2ff2c63d6c（2026-10-01 自行以 `git diff BASE HEAD | git patch-id --stable` 重算，相符；`git rev-parse HEAD` 相符；審查前後 `git status --short` 皆為空）
- 獨立 reviewer：Claude Code 隔離 subagent（claude-opus-5-5）；未參與本次實作，也不是 review-6037672 的作者
- Review artifact：docs/issues/issue-0003/review-46cd815.md
- 審查日期：2026-10-01
- 風險：High（不可逆刪除）

## 問題與風險

### MUST FIX

**M-1　SCN-010 的「輸出說明原因與所用的記憶庫」在 schedulerd 的定期清理上沒有實作**

- 位置：`crates/wukong-schedulerd/src/session_retention.rs:56`（只印 `summary_line`）、`crates/wukong-runtime/src/session_retention.rs:151-165`（摘要行）與 `:175-182`（原因說明只存在於 `render_report`，schedulerd 不呼叫它）、`crates/wukong-schedulerd/src/main.rs:139-145`；規格 `docs/issues/issue-0003/README.md:106-111`；`docs/docker.md:218`（「整輪不刪並輸出原因」）
- 規格：SCN-010 的 When 是「清理**或**預覽執行」，Then/And 是「不刪除任何 session…」與「輸出說明原因與所用的記憶庫」。整份 Feature 裡「清理執行」指的就是 schedulerd 的定期清理（SCN-003「到達清理時間」、SCN-006「進行中的聊天與排程回合照常完成」）。
- 重現（2026-10-01）：把 HEAD 匯出到 repo 之外，只把 `INTERVAL` 改成 3 秒後編譯 `wukong-schedulerd`（原本的 6 小時無法等），對拋棄式假 server、一份沒有任何對應的記憶庫執行。完整日誌：
  ```
  opencode session retention enabled retention_days=30 first_run_in_secs=3 interval_secs=3
  opencode_session_retention dry_run=false listed=3 truncated=false anchored=false protected=0 stale_protected=0 expired=0 deferred=0 deleted=0 failed=0
  opencode_session_retention dry_run=false listed=3 truncated=false anchored=false protected=0 ...
  ```
  「不刪」成立（server 只收到 `GET /session?limit=10000`，沒有 `DELETE`）。但輸出沒有任何一句原因，也沒有記憶庫位置；schedulerd 從啟動到結束都不會印出它用的是哪一份記憶庫。`anchored` 這個欄位只在 issue 的實作計畫裡解釋過，`docs/docker.md`、`docs/cli-reference.md`、`.env.example` 都沒有提到。
- 影響：SCN-010 存在的理由是「記憶庫接錯」。這個情況在 `wukong opencode prune` 上會被看到（有人在讀輸出、結束碼為 1）；在 schedulerd 上它變成每 6 小時一行、外觀與正常結果相同、等級不是 warning 的日誌，清理永遠不發生而沒有人知道。這同時就是「防護錯誤地永遠拒絕」時的唯一可觀察訊號。
- 證據面：計畫的測試策略表把 SCN-010 對到「步驟 9 的真實 opencode 驗證」與「runtime 測試」，兩者都只涵蓋 `prune` 指令與 `render_report`；schedulerd 這條路徑沒有任何證據。
- 判定理由：已核准驗收條件在兩個觸發路徑之一未實作，屬必要驗收缺口。
- 建議：schedulerd 在 `anchored=false` 時另印一行 warning，內容含原因與 `cfg.db_url`（可直接重用 `render_report` 的那段文字），並補一個斷言該輸出的測試；`docs/docker.md` 說明 `anchored=false` 的意義。若核准者的本意是 SCN-010 的 And 只適用於 `prune` 指令，那是規格修訂，要由使用者決定並記錄，不能由實作者或 reviewer 代為解讀。

**M-2　兩份記憶庫共用一個 server 且各自跑過回合時，防護會放行並刪掉另一份記憶庫仍在續接的 session；這個殘餘沒有被揭露，文件還把它列在「有防護」的那一類**

- 位置：`crates/wukong-runtime/src/session_retention.rs:255-261`（只要有任一個受保護 id 出現在清單就算對得上）；`docs/docker.md:218-219`；`docs/issues/issue-0003/README.md:165`（TBD-5）；`CHANGELOG.md:34-36`；`docs/issues/issue-0003/implementation-plan.md:72`
- 這是 review-6037672 的 M-2 輸入二原文列出的情況（「主機上的 `wukong` 指向容器的 server、兩套部署共用一個 server」）。修正只擋住「完全沒有交集」的子情況（空的記憶庫、完全指向別處的記憶庫）。
- 重現（2026-10-01，本機 debug binary 對拋棄式假 server，兩份拋棄式記憶庫）：
  1. server 上有 `h_recent`（1 天）、`c_chat`（45 天）、`orphan`（50 天）；「容器」記憶庫指向 `c_chat`，「主機」記憶庫指向 `h_recent`。以主機記憶庫執行 `wukong opencode prune`：輸出「已刪除 2 個… orphan、c_chat」、結束碼 0，`c_chat` 被刪。之後以容器記憶庫預覽，得到「未清理：…沒有任何一個出現在…」。
  2. 反方向（這一個方向是自動的，不需要任何人下指令）：server 上有 `c_recent`（1 天）、`h_chat`（45 天）；以容器記憶庫（schedulerd 用的那一份）執行，`h_chat` 被刪、結束碼 0，主機記憶庫仍指向已不存在的 `h_chat`。
- 文件面：`docs/docker.md:218` 把「主機上的 `wukong` 指向容器的 server」當成第一類（有防護）的例子。實際上只要主機的 `wukong` 在那個 server 上跑過一個回合——這正是把它指過去的目的——防護就放行。TBD-5、CHANGELOG 的已知限制與計畫第 72 行都只描述「使用者自己也在用的 `opencode serve`」，沒有涵蓋「第二份 Wukong 記憶庫」。
- 影響：不可逆地刪掉另一份記憶庫仍在續接的 opencode session。緩解事實與上一輪相同：gateway 遇到 404 會重建 session（`crates/wukong-gateway/src/opencode_server.rs:570-577`），scope 不會壞，Wukong 自己的記憶也還在，失去的是 opencode 端的對話歷史。compose 預設部署（四個服務共用同一份 `wukong-data`）不受影響。
- 判定理由：上一輪把這個失敗面判為 MUST FIX 的依據是「既無防護、也無測試、也無文件揭露」。使用者已選定防護的形式（SCN-010），所以這裡不要求更強的防護；但這個子情況目前沒有防護、沒有測試、沒有揭露，而且文件的說法與實測相反。依 review-evidence.md，重要失敗面需要證據或明確的豁免／揭露，兩者皆無。
- 建議（不需要新的使用者決策）：`docs/docker.md` 把第一類的敘述改成「記憶庫與 server **完全**沒有交集時才擋得住」，把「兩份記憶庫共用一個 server（例如主機 CLI 與 compose 各有一份）」移到第二類並給出處置（保留天數設 0，或不要讓第二份記憶庫接到這個 server）；README 的 TBD-5 與 CHANGELOG 已知限制補上這個輸入；`.env.example` 與 `docs/docker.md:41`（允許自行接 `opencode serve` 的那一段）加上指向。若想改用更強的判定（例如要求受保護 id 全數出現、或比例門檻），那會改變 SCN-010，需由使用者決定。

### SHOULD FIX

**S-1　`wukong --new` 在 CLI backend 下仍然不刪舊 session，CHANGELOG 的說法沒有限定**

- 位置：`CHANGELOG.md:29-30`（「`wukong --new` 不再留下舊 session」）；`crates/wukong-cli/src/main.rs:54-70`；`crates/wukong-gateway/src/backend.rs:82-84`（trait 的 `delete_session` 預設是空實作，`AgentCliBackend` 沒有覆寫）
- binary 模式預設走 `opencode run`（CLI backend），而 `--new` 是 CLI 旗標，最常在這個模式下使用。此時 `delete_session` 回 `Ok(())` 什麼都不做，行為與修正前相同；保留期清理在 CLI 模式也不啟用，所以那個 session 仍然永久留存。計畫的「現況查核」已記錄 CLI backend 的空實作，但步驟 7、9 與 CHANGELOG 都沒有把它寫成 `--new` 修正的適用範圍。測試 `tests/new_session_flag.rs` 只跑 server backend。
- 依限制我沒有以 CLI backend 實際執行（所有執行都必須指向假 server），這一項是讀程式碼得出的。
- SCN-008 的文字沒有限定 backend。我把它列為建議而非退回理由，是因為整個 issue 的範圍是 server 的 `opencode.db`、`/new` 既有的行為也相同；但對外文件不應宣稱沒做到的事。建議 CHANGELOG 與 README 註明「server backend」，是否要讓 CLI backend 以 `opencode session delete` 補齊由使用者決定。

**S-2　修正提交新增的四個行為沒有任何測試會為它們紅燈（證據持久力）**

把 HEAD 匯出到 repo 之外做變異，下列四個變異後相關測試全綠：

| 變異 | 被釘住的行為 | 位置 |
|---|---|---|
| `ticker()` 改回 `interval(INTERVAL)`（啟動當下就跑） | S-2 的修正 | `crates/wukong-schedulerd/src/session_retention.rs:17-19` |
| 拿掉 `println!("記憶庫：…")` | SCN-010「所用的記憶庫」 | `crates/wukong-cli/src/main.rs:94` |
| 拿掉 `prune` 的 `exit(1)` | N-4 的修正、`docs/cli-reference.md:89` 的結束碼說明 | `crates/wukong-cli/src/main.rs:99-101` |
| 父 session 不在清單裡的子 session 被當成根 | 截斷把樹切半時不單獨刪子 session | `crates/wukong-runtime/src/session_retention.rs:75-78` |

這四項在 HEAD 上的行為我都實測過是正確的（見下方），所以不構成假綠燈，本次驗收的判定不受影響。建議：`ticker()` 用 `tokio::time::pause` 釘住（我在匯出的複本上加了這樣的探針：HEAD 通過、套用變異後失敗）；`prune` 的第一行與結束碼比照 `tests/new_session_flag.rs` 以 binary 對假 server 測；挑選邏輯補一個「子 session 的父不在清單」與一個成環的 fixture。

另外，S-2 修正的實測證據是「啟動 12 秒內為 0 行」。這個觀測在「延後一個間隔」與「永遠不會跑」兩種情況下都成立；日誌裡的 `first_run_in_secs=21600` 是印出來的常數，不是從 ticker 讀到的。實作者已揭露迴圈接線只在步驟 8（修正前）觀察過，所以不算缺失；我以 3 秒間隔的匯出複本確認迴圈在一個間隔後確實會呼叫清理，並且之後持續觸發。

**S-3　清單被截斷時，樹狀保護對「被截掉的受保護子 session」失效**

- 位置：`crates/wukong-runtime/src/session_retention.rs:48-72`、`crates/wukong-gateway/src/opencode_server.rs:724-733`
- 列表不再帶 `roots=true`，10,000 筆的上限現在由根與子 session 共用，截斷也因此可能把一棵樹切成兩半。三種切法的結果：
  - 根被截掉、子留在清單：不會被刪（子不是根，根不在清單）。實測成立，安全。
  - 子被截掉、根留在清單，只看年齡：被截掉的是最舊的，不影響取最大值。安全。
  - 子被截掉、根留在清單，而 scope 指向的正是那個子：根看不到保護，過期就會被選中，連帶刪掉受保護的子，違反 SCN-002。實測（預覽）：清單恰為 10,000 筆、根 `root_r` 40 天、記憶庫指向不在清單裡的 `kid_p` → `root_r` 被列為將刪除。
- 觸發條件很窄：server 上超過 10,000 個 session，且對應是 CLI backend 留下的子 session id（server backend 自己建立的一定是根）。因此列為建議。最小的修法是「清單被截斷、且有受保護 id 不在清單裡」時整輪不刪；或在文件與 TBD 記為已知限制。

### NICE TO HAVE

- **N-1**　第一輪延後一個完整間隔、又沒有保存上次執行時間，所以**重啟間隔短於 6 小時的 schedulerd 永遠不會清理**（頻繁升級、crash loop）。唯一的訊號是日誌裡從未出現 `opencode_session_retention`。compose 內 schedulerd 不隨 `opencode-server` 的每日閒置重啟而重啟，正常部署不受影響。值得在 `docs/docker.md` 寫一句。（`crates/wukong-schedulerd/src/session_retention.rs:17-19`）
- **N-2**　全新部署（server 與記憶庫都是空的，`listed=0`）執行 `wukong opencode prune` 會回結束碼 1 並說「記憶庫與 server 對不上」，但其實只是沒有東西可清。可在 `listed == 0` 時視為成功。（`crates/wukong-cli/src/main.rs:99`、`crates/wukong-runtime/src/session_retention.rs:175-181`）
- **N-3**　`scripts/docker-entrypoint.sh:298` 的 `SQLITE_TMPDIR` 固定為 `$OPENCODE_STATE`，而 `WUKONG_OPENCODE_DB` 可以被覆寫到別的檔案系統；那時磁碟檢查又與暫存檔的位置不一致。用資料庫所在目錄較一致。預設部署不受影響。
- **N-4**　`docs/cli-reference.md:44` 的標題寫「需要 opencode server backend」，但其下的 `opencode vacuum` 不需要 backend。
- **N-5**　`--new` 的處理現在是 `SessionCommand::New`（`crates/wukong-cli/src/command.rs:46-52`）的另一份寫法，兩者在刪除失敗時行為不同（`/new` 回報錯誤並保留對應，`--new` 警告並清除對應）。差異是刻意的且有註解，可接受；日後若改其中一邊要記得另一邊。
- **N-6**　SCN-010 的核准來源寫「四個選項」，但只列出三個（選中的一個、未選的兩個）。補上第四個選項的名稱，紀錄才完整。

### 上一輪各項發現的處置

| 項目 | 處置 | 查核 |
|---|---|---|
| M-1 `--new` 刪除失敗時帶舊 context | 已解決 | `tests/new_session_flag.rs` 重跑通過；變異「刪除失敗就不清對應」「完全不刪」都轉紅 |
| M-2 未防護的前提 | 部分解決 | 空記憶庫與完全不相交的記憶庫已擋住（重跑）；共用 server 記為 TBD-5 並寫進文件（屬使用者決定後的合法不處理）；部分重疊的情況見本報告 M-2，schedulerd 的輸出見本報告 M-1 |
| S-1 無效天數被當成 30 天 | 已解決 | binary 實測 `off` → 警告＋「已停用」、結束碼 0、不發任何請求；變異轉紅 |
| S-2 啟動當下就清理 | 已解決，無測試 | 以暫停時鐘的探針與 3 秒間隔的複本確認；見本報告 S-2 |
| S-3 server 掛住時拖住排程迴圈 | 已解決 | 測試重跑；以 2 秒上限的複本確認逾時後迴圈繼續、SIGTERM 仍被處理；變異「逾時當成成功」轉紅 |
| S-4 `VACUUM` 暫存檔位置 | 已解決 | `strace` 重現：未設時開在 `/var/tmp/etilqs_*`，設 `SQLITE_TMPDIR` 後開在資料庫目錄 |
| N-1 時鐘健全性檢查 | 未採納，理由成立 | 損害上限是尚未寫回對應的進行中 session，且需要時鐘前跳超過保留期 |
| N-2 證據持久力 | 部分採納 | scope 對應不變的斷言已補；entrypoint 的字串檢查維持原樣並說明理由 |
| N-3 紀錄出入 | 已解決 | 計畫步驟 3 與測試策略表已更正 |
| N-4 `prune` 結束碼 | 已解決，無測試 | 我實測了實作者沒執行到的「有刪除失敗」：結束碼 1，其餘 session 照常刪除 |
| N-5 三份 HTTP stub | 未採納 | 上一輪已判定可接受 |
| 未查證：scope 指向子 session | 已解決，有一個邊界 | 改為以樹判定，兩個新測試在變異下轉紅；截斷時的邊界見本報告 S-3 |

## 已查核維度

### 驗收與證據

「重跑」指我在 2026-10-01 於 HEAD 46cd815 親自執行；「僅閱讀」指只讀了實作者的紀錄（需要真實 opencode 容器與 `~/.local/share/opencode` 的複本，兩者都在本次審查的禁止範圍內）。

| 編號 | 實作 | 證據位置 | 查核方式 | 結論 |
|---|---|---|---|---|
| SCN-001 | `select_expired`＋`prune_opencode_sessions` | runtime `session_retention` 測試；計畫步驟 1、8、9 | 單元測試重跑；binary 對假 server：只有過期無主的 id 收到 `DELETE`；3 秒間隔的 schedulerd 複本同樣只刪 `old_a`。訊息／片段／事件隨之消失是 opencode 的行為，**僅閱讀** | 通過 |
| SCN-002 | 受保護集合取自兩張表；以樹判定 | `referenced_session_ids_cover_both_session_tables`、`deletes_expired_orphans_...`、`a_scope_pointing_at_a_child_protects_the_whole_tree` | 重跑；假 server 上受保護 id 未收到 `DELETE`、清理後兩張表的對應不變；指向過期孫 session 時根被保留。「下一回合續接」沒有真實模型回合，實作者已揭露 | 通過；截斷時的邊界見 S-3 |
| SCN-003 | `RetentionPolicy::from_env` | `policy_defaults_to_thirty_days_and_zero_disables`、`only_a_server_backend_...` | 重跑；binary：未設 → 「保留期 30 天」，`0` 與 `off` → 「已停用」且不發請求 | 通過 |
| SCN-004 | 兩個 `?` 先於任何刪除；列表任何一筆看不懂就整份失敗 | `an_unreadable_scope_table_deletes_nothing`、`a_failed_listing_deletes_nothing`、gateway 的 `list_sessions_fails_...`、`..._surfaces_the_upstream_error_body` | 重跑；變異「讀取失敗當成空集合」轉紅 | 通過 |
| SCN-005 | `GET /session?limit=10000`；筆數達上限即標示截斷 | `list_sessions_asks_for_every_session_with_an_explicit_limit`、`a_full_page_is_reported_as_truncated`、`truncation_is_carried_...` | 重跑；binary 對 10,000 筆的假清單：輸出「列表已達上限而被截斷」。真實 opencode 上「第 100 筆之後仍看得到」**僅閱讀** | 通過 |
| SCN-006 | 單筆失敗記錄後繼續；整輪 5 分鐘上限 | `one_failed_delete_does_not_stop_the_rest`、`a_server_that_never_answers_gives_the_loop_back`、`an_unreachable_server_...` | 重跑；binary：一筆 `DELETE` 回 500 時另一筆照刪；複本上 server 掛住時每輪記一行逾時警告、迴圈繼續 | 通過 |
| SCN-007 | `opencode_db::vacuum`＋entrypoint | `opencode_db` 6 項；計畫步驟 6、8、9 | 單元測試重跑；binary 重現四種情況：檔案不存在（`outcome=missing`、不建立檔案）、回收（92,200,960 → 30,744,576 bytes，`integrity_check` ok、`journal_mode` 仍為 wal、資料列不變）、未達門檻、另一連線持有寫鎖（5 秒後 `database is locked`、結束碼 1、檔案不變）。entrypoint 片段在 `set -euo pipefail` 下以 stub 重現：失敗時印警告並繼續、舊 binary 不呼叫。真實容器的重啟驗證**僅閱讀** | 通過 |
| SCN-008 | `--new` 先刪再清，刪除失敗仍清 | `tests/new_session_flag.rs` 2 項 | 重跑（真正的 binary 對假 server） | server backend 通過；CLI backend 見 S-1 |
| SCN-009 | `dry_run` 在刪除迴圈前返回 | `preview_deletes_nothing_and_names_what_a_real_run_deletes`、`rendered_report_names_...` | 重跑；binary：預覽不送 `DELETE`，之後實刪的正是預覽列出的兩個 id | 通過 |
| SCN-010 | `anchored` 判定；`prune` 印記憶庫位置 | `a_memory_that_matches_nothing_on_the_server_deletes_nothing`；計畫步驟 9 | 重跑；binary：空記憶庫與指向別處的記憶庫，預覽與實刪都回「未清理…」、結束碼 1、沒有 `DELETE`，第一行是記憶庫位置。schedulerd 路徑以複本觀察 | 「不刪」兩條路徑都通過；「輸出說明原因與所用的記憶庫」在 schedulerd 路徑**未通過**，見 M-1 |

**規格與核准**：`git show c613028:…README.md` 與 HEAD 的 Gherkin 區塊相比，唯一差異是新增 SCN-010 的 7 行，SCN-001 至 SCN-009 逐字未變；cbd65ef 與 HEAD 的 Gherkin 區塊相同。核准表與現存 Scenario 集合相等，規格提交（cbd65ef）早於實作提交（46cd815）。SCN-010 的核准來源記錄了日期、觸發它的審查發現、被選中的選項原文與兩個未被選的選項，內容具體且與 TBD-5、文件的處置一致；依 acceptance.md「已接受具體方案」可作為核准來源。對話本身我無從查證；小瑕疵見 N-6。

**假綠燈查核**：新增與修改的測試都以字面 id、字面請求字串為期望值，沒有以實作重算期望值；`ListingBackend` 取代的是 opencode（外部系統），被驗收的判定邏輯是真的在跑。我把 HEAD 匯出到 repo 之外，套用 23 個變異並重跑對應測試：19 個轉紅（防護永不拒絕、防護只看記憶庫是否為空、子不保護根、樹的年齡只看根、`--new` 失敗不清對應、`--new` 不刪、無效天數退回 30、列表帶回 `roots=true`、丟掉 `parentID`、由新到舊刪、CLI backend 也清、邊界改 `<=`、預覽仍刪、漏掉 `agent_session_state`、滿頁不算截斷、逾時當成功、第一個失敗就停、未對上仍列出清單、讀取失敗當空集合）；4 個未被抓到，見 S-2。未發現假綠燈。

**全量檢查（重跑）**：`cargo test --workspace` 39 個套件、592 通過、0 失敗；`cargo clippy --all-targets -- -D warnings` 無警告；`cargo fmt --all -- --check` 通過；`bash scripts/test-docker-runtime.sh` 通過。數字與實作者的紀錄一致。

### 相關失敗面

| 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|
| 記憶庫是空的或完全指向別處 | 整輪不刪 | 測試＋binary 重現 | 正確 |
| 兩份記憶庫共用 server、各有對應 | 不刪另一份仍在續接的 session，或明確揭露 | 無測試、無揭露 | 會刪，見 M-2 |
| Wukong 與使用者自己的 opencode 共用 server | 同上 | TBD-5、`docs/docker.md:219`、CHANGELOG 已揭露；使用者選擇不加防護 | 合法的不處理 |
| 防護在 schedulerd 上持續拒絕 | 看得出原因 | 只有 `anchored=false` | 見 M-1 |
| 記憶庫暫時沒有任何對應（所有 scope 剛下過 `/new`） | 這一輪跳過，下一回合後恢復 | 讀程式碼 | 正確，會自行恢復 |
| scope 指向子 session | 整棵樹保留 | 測試＋binary 重現（指向過期孫 session） | 正確 |
| 父子關係成環、自己指向自己 | 不卡死、不刪 | 無測試；binary 重現：立即結束，環上的 session 都留著 | 正確 |
| 子 session 的父不在清單 | 不單獨刪子 | 無測試（S-2）；binary 重現：留著 | 正確 |
| 截斷把受保護的子切在清單之外 | 根仍受保護 | 無 | 不成立，見 S-3 |
| 根安靜、子最近有活動 | 以最新成員為準，不刪 | 測試＋binary 重現（`quiet_root`／`busy_kid`） | 正確 |
| 過期數超過單輪上限 | 由舊到新取前 500 個根，其餘標示 | `a_run_is_capped_and_takes_the_oldest_first`；排序鍵是（樹的最後活動, id） | 正確 |
| 清單中超過 10,000 個 session 都在保留期內 | 不刪並標示截斷 | 截斷標示有測試 | 符合 SCN-005；此時永遠看不到更舊的，屬規格內行為 |
| 批次中一筆刪除失敗 | 其餘照常、結束碼 1 | 測試＋binary 重現 | 正確 |
| server 接受連線但不回應 | 5 分鐘後放棄，迴圈繼續 | 測試＋複本重現 | 正確。逾時那一輪不會印摘要行，但已刪除的每一筆在 gateway 層各有一行 `session_deleted` |
| schedulerd 在第一輪之前就重啟 | 仍會清理 | 無 | 不會，見 N-1 |
| 保留天數無法解析 | 停用並警告 | 測試＋binary 重現 | 正確 |
| `--new` 時刪除失敗 | 仍開新 context | 測試 | 正確 |
| `--new`、CLI backend | 舊 session 被刪 | 無 | 不會刪，見 S-1 |
| vacuum：被鎖、磁碟不足、檔案不存在、舊 binary | 不擋啟動、不動檔案 | 測試＋重現 | 正確 |
| vacuum：`WUKONG_OPENCODE_DB` 被覆寫到別的檔案系統 | 磁碟檢查涵蓋暫存檔 | 無 | 不一致，見 N-3 |

### 需求、架構、安全、品質

- **需求**：十個 Scenario 都有對應實作；SCN-010 的 And 在 schedulerd 路徑缺漏（M-1）。diff 內的檔案都在 README 的涉及檔案範圍內（`crates/wukong-cli/tests/` 屬測試）。`session.rs`、`opencode-idle-restart.sh` 未被觸及；資料庫只做 `VACUUM`，沒有直接改寫資料列。
- **架構**：相依方向未被破壞——`wukong-runtime::session_retention` 只用 gateway 與 memory，schedulerd 與 cli 只用 runtime 及其下層；`SessionSummary`／`SessionListing` 放在 gateway 的 backend 模組。`AiBackend::list_sessions` 的預設實作回傳錯誤而非空清單，方向正確。無違規。
- **安全／權限**：列表與刪除都走既有的 `authorize`；entrypoint 以 `gosu wukong` 降權執行 vacuum，root 只跑了 `--help`。錯誤訊息只帶上游的錯誤回應，不含對話內容。未發現注入或提權路徑。資料安全面的問題是 M-2。
- **相依**：`Cargo.lock` 只多三條相依邊（`wukong-cli` → `sqlx`、`rustix`；`wukong-runtime` 的測試 → `sqlx`），沒有新增套件或版本。
- **品質／重複／過度設計**：挑選是純函式、執行與呈現分開；間隔、上限、時間預算與門檻都是常數，沒有多出來的設定項。重複只有 `--new` 與 `/new` 的兩份寫法（N-5）與三份測試 stub。未發現過度設計。
- **文件**：`.env.example`、`AGENTS.md`、`docs/cli-reference.md` 的間隔、預設值、無效值處置、結束碼、樹狀判定與程式一致；沒有找到修正前的殘留說法（`roots=true` 只留在計畫的歷史紀錄裡，設計章節已加修訂註）。與程式不符的三處：`docs/docker.md:218`（M-2）、`docs/docker.md:218` 的「並輸出原因」在 schedulerd 上不成立（M-1）、`CHANGELOG.md:29`（S-1）。
- **計畫的自我回報**：步驟 9 的各項宣稱我逐一對照，未發現不實回報；實作者主動揭露的限制都與我看到的一致。

### 豁免、待確認與限制

- 沒有 gate 豁免。TBD-3、TBD-4 仍待確認，README 已說明不阻塞。TBD-5 是使用者決定後的不處理，紀錄含結論、理由與日期。
- 實作者自行揭露的限制（SCN-002 沒有真實模型回合、樹狀保護沒有在真實 opencode 的舊資料上演練、容器用 gnu 而非 musl binary、`prune` 的「有刪除失敗」結束碼未實際執行）依規則不視為缺失；最後一項我已補做。
- 本次審查的限制：
  - 沒有碰任何容器、volume 或 `~/.local/share/opencode`，所以「opencode 刪除 session 會連帶清掉訊息／片段／事件」「不帶 `roots` 的列表會回傳子 session 且依 `time.updated` 由新到舊」「真實容器重啟時檔案變小」「新 entrypoint 配 v0.21.11 binary」只讀了紀錄。
  - 沒有連線 GitHub：PR #4 的說明（Proof of Test 是否每個驗收編號一列）與遠端 source HEAD 是否等於本地 HEAD 都未核對。
  - schedulerd 的定期清理要等 6 小時才觸發，我觀察的是只改了 `INTERVAL`（3 秒）與 `RUN_BUDGET`（2 秒）兩個常數的匯出複本，不是 HEAD 的原始 binary。
  - 依限制所有執行都指向假 server，CLI backend 的行為（S-1）是讀程式碼得出的。
  - 我的實驗用的是拋棄式假 HTTP server、拋棄式 SQLite 檔與匯出到 repo 之外的 HEAD 複本，啟動的行程都以 PID 結束；repo 內除了本報告沒有其他變更。
- 未查證：
  - 實作計畫步驟 1 記錄列表查的是 server 目錄所屬的 project。若 server 的工作目錄日後變成 git repo 而換了 project id，先前建立的 session 可能不再出現在清單裡——那會是安全方向（不刪），但那些 session 永遠清不到，受保護的若全在舊 project 也會讓防護持續拒絕。需要真實 opencode 才能確認。
  - 根目錄 `AGENTS.md` 要求改動符號前跑 GitNexus 影響分析、提交前跑 `detect_changes`；修正提交是否照做，我無從由產物查核。

## 流程判定
RETURN TO execute-task

理由：M-1 是已核准驗收條件（SCN-010）在 schedulerd 路徑上的缺口；M-2 是 High 風險下一個已辨識失敗面既無覆蓋也無揭露，且文件的說法與實測相反。兩者的修正範圍都很小（一行警告加測試；文件與 TBD 的更正）。上一輪的兩項 MUST FIX 中，M-1 已確實解決；其餘 Scenario 的成功路徑、失敗方向與測試真偽都查核通過。SHOULD FIX 與 NICE TO HAVE 不阻塞。
