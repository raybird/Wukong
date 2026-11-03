# 審查報告
- 範圍：完整 PR
- Reviewed BASE SHA：915dcd29ac14aa67dcece7d0b6090691ac343e22
- Reviewed HEAD SHA：e63beeb41fb1ae0beaaaf107af5c2afad57cf984
- Reviewed patch-id：e8262140206109ed2e67b1b435099712d6b5a332（2026-11-03 以 `git diff 915dcd2 e63beeb | git patch-id --stable` 重算，相符）
- 獨立 reviewer：Claude subagent（general-purpose，未參與實作）
- Review artifact：docs/issues/issue-0028/review-e63beeb.md
- 審查日期：2026-11-03

## 問題與風險

### MUST FIX

1. **主機上手動 `wukong opencode prune` 現在會刪使用者自己的 opencode session，`active_policy` 的閘門沒有套用到 CLI**（`crates/wukong-cli/src/main.rs:89-109`，起因在 `crates/wukong-gateway/src/local_process.rs:274` 新增的 `LocalProcessBackend::list_sessions`）。「需決策」
   - 現況：CLI 的 prune 直接拿 `RetentionPolicy::from_env()` 呼叫 `prune_opencode_sessions`。它不看 backend 種類，也不看 `retention_days_configured()`。變數沒設定時，`from_value(None)` 是 30 天。
   - 影響：主機預設是按需本機 backend（`build_backend_from_env`：`WUKONG_AGENT_SERVER_URL` 留空、指令是 `opencode run`）。在 BASE，本機 backend 沒有 `list_sessions`，這個指令在列表那一步就會安全失敗（步驟 1 探測 (3b) 拿到的就是這個錯誤）。到了 HEAD，列表會成功。主機上的 `opencode.db` 和使用者自己的 opencode 共用。記憶庫只要指向其中任何一個 session，anchored 防護就會放行，結果是使用者自己 30 天沒動、Wukong 沒有指向的 session 全部被刪，而且無法復原。這正是本 PR 在 schedulerd 刻意擋掉的情況（`session_retention.rs` 的註解寫著「主機上沒設定就視為共用、不清」，AGENTS.md 也這樣寫）。PR 一邊把這條限制寫進 schedulerd，一邊從 CLI 開出同一個刪除路徑。`docs/docker.md` 列出的「三種情況不成立」也沒有提到主機本機模式。
   - 建議：CLI prune 遇到 `AgentBackend::Local` 時，同樣要求 `retention_days_configured()`。最直接的做法是共用 schedulerd `active_policy` 的判斷（可把它搬到 runtime），不符合時印出原因、結束碼 1。另補一個單元測試或 CLI 測試：本機 backend、沒有設定時不刪。
   - 待決問題：在本機 backend、沒有設定的情況下，`--dry-run` 要不要放行？預覽不會刪資料，但輸出會把使用者自己的 session 列成「將刪除」，可能造成誤解。建議放行，並在輸出第一行註明「此設定下 schedulerd 不會清理」。

### SHOULD FIX

1. **`docs/cli-reference.md:93` 已經不正確**：它寫 `opencode prune`「只支援 opencode server backend」，本 PR 之後本機 backend 也支援。這個檔案不在本 PR 的修改範圍內，所以 W-1 對照時漏掉了。依 MUST FIX 1 的決策結果改寫：本機模式要明確設定才清，或者寫明主機上會刪到自己的 session。

### NICE TO HAVE

1. **本機模式一輪清理在預算用完前會卡住排程迴圈約 5 分鐘**（`crates/wukong-schedulerd/src/main.rs:158`）。`run_once` 是在 `select!` 的分支裡同步 await 的。TBD-4 已揭露「一輪只刪得完約 400 個」，但沒有提到：既有部署累積較多時，前幾輪每輪都會用滿 5 分鐘，這段期間到期的排程最多延後 5 分鐘。另外，逾時那一輪 `run_within` 會回 `Err`，不會印出 `opencode_session_retention` 摘要行，維運人員看不到這輪實際刪了幾個。建議在 PR 揭露排程延遲。若要改，可在逾時時仍記下已刪的筆數，但這是既有結構，不強求。
2. **純 CLI backend 每到 compaction 時點都會記一行 `warning: session_age_unknown`**（`crates/wukong-runtime/src/session.rs:127`）。CLI backend 不支援這個 API 是預期中的事，不是異常，每 20 回合一行 warning 是雜訊，也會稀釋 server／本機模式真正查詢失敗時的訊號。可以考慮讓 trait 預設回應「不支援」時降級成 info，或者不記。
3. **SCN-007 的 turn 測試沒有直接證明走的是輪替路徑**（`crates/wukong-runtime/src/turn.rs:1404`）。它只斷言了「回合失敗、`ses_old` 未刪、指向不變」。如果輪替被拿掉，失敗的第 1 次呼叫會變成 `/compact`，後續剛好因為 mock 回空字串而讓回合成功，測試才紅。這是間接的區辨力。建議補兩個斷言：`prompts[1] != "/compact"`、`session_ids[1] == None`。資料層另有 `deciding_to_rotate_leaves_the_pointer_for_the_next_turn` 覆蓋，所以只列 NICE TO HAVE。
4. **既有的輪替刪除路徑沒有防呆**（`crates/wukong-runtime/src/turn.rs:407-420`，既有程式碼）。輪替回合的 `captured_session` 是 `None` 時，`finish_agent_session_turn` 的 `COALESCE(?3, session_id)` 會保留舊指向，但 `captured != Some(old)` 成立，舊 session 照樣會被刪，scope 就指向一個已刪除的 session。我查過 server／本機 backend 的 `run_streaming` 一定回 `Some(session_id)`（`opencode_server.rs:623`、`:690`），CLI backend 也不會依年齡輪替，所以目前到不了這條路。不過本 PR 讓輪替從罕見路徑變成每 7 天一次的常態路徑，可以考慮把條件改成 `if let Some(new_id) = captured_session`，成本很低。

## 已查核維度

- **驗收與證據**：
  - SCN-001～003：`which_backends_and_settings_turn_retention_on` 用手寫期望值涵蓋三種 backend、未設定、明確設定與 0 天，並斷言停用原因字串；紅燈與 M1／M2 變異紀錄齊全。SCN-001 的刪除與保留、SCN-002 的「經過間隔仍不刪」，有真實 opencode 1.19.4 加 debug schedulerd 的沙盒紀錄（日誌原文、`deleted=1`、`protected=1`、沒有殘留行程）。`lists_sessions_through_an_on_demand_server` 驗證列表帶 `limit=10000`，結束後行程已停止。判定：足夠。
  - SCN-004、005、010：`session.rs` 的四支測試（年齡 8 天輪替、6 天壓縮、未到時點不查、查不到時壓縮）分別斷言 `rotated_from`、`session_id`、查詢與壓縮次數，在錯誤行為下不會是綠的。`turn.rs` 的兩支測試斷言新的指向是 `ses_new`、沒有 `/compact`、最後一棒 `session_id=None`、prompt 含上一回合、`ses_old` 被刪；反向測試斷言 prompt 不含上一回合，舊的沒刪。M1～M5 變異都紅在對應的測試上，紀錄也誠實寫出第一次變異執行時 cargo 參數錯誤、測試其實沒有跑的經過。真實端到端：`session_rotation_due` → `session_rotated` → `session_deleted`，回答仍是「藍鯨」。判定：足夠。
  - SCN-006：決策有測試。「日誌記錄一行警告」沒有斷言，只有程式碼（`session.rs:127`）。eprintln 的斷言成本高，可以接受。
  - SCN-007：見 NICE TO HAVE 3。資料層測試已證明決定輪替時不改指向、下一回合會再輪替。判定：可接受。
  - SCN-008、009：用真的 SQLite 加 `ChatHistoryStore`，字面值包含 86,401／86,400／10 秒前三個邊界，記憶維護關閉，backend 是一個不能執行的命令。關閉之後如果仍跑了摘要，就會失敗。三種變異（7 天、`<=`、被 enabled 擋住）都會紅。另有真實 daemon 的 `deleted=1` 紀錄。判定：足夠。
  - 我在 HEAD 重跑：`cargo test -p wukong-runtime --lib session`（40 passed）、`turn::tests::a_`（2 passed）、schedulerd 的 `maintenance_prunes`／`which_backends`（2 passed）、gateway 的 `session_created`／`lists_sessions_through`（3 passed）。工作樹乾淨，HEAD 是 `e63beeb`。

- **相關失敗面**：

  | 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
  |---|---|---|---|
  | 年齡查詢期間的 lease | 查詢期間續租，失敗時仍持有 lease 再去壓縮 | `hold_session_lease` 包住查詢（`session.rs:383-390`）；續租機制有 issue-0013 的既有測試 | 正確 |
  | ms／s 單位 | `now_unix()`（秒）×1000，再和 `time.created`（毫秒）比較 | 步驟 1 探測確認是毫秒；M2 變異會紅 | 正確 |
  | 時鐘偏差、`created` 在未來 | 年齡為負，不輪替 | 沒有專屬測試，以讀碼確認 | 安全方向，可接受 |
  | 輪替回合失敗 | 舊的不刪、指向不變、下一回合再輪替（`turn_count` 未消耗） | SCN-007 的兩支測試 | 正確；新 session 成為孤兒，30 天後由保留期清理（若有啟用） |
  | 同一 scope 的並行回合 | session lease 互斥 | 既有機制 | 正確 |
  | 本機 `CreatedAt`／`List` 啟動 serve 後出錯 | 程序一定回收 | `execute` 的 `return` 只離開內層 async 區塊，之後 `process.stop()` 一定會執行；外層 future 被丟棄（例如 5 分鐘預算到期）時，`Process::drop` 會 kill 整個 process group，`kill_on_drop` 也有設；兩支測試有 `assert_stopped` | 沒有洩漏 |
  | 本機模式的 anchored 防護 | 與 server 模式相同（`prune_opencode_sessions` 沒改） | issue-0003 的既有測試，加本 PR 的沙盒實測 | 正確；主機情境見 MUST FIX 1 |
  | 純 CLI backend | 永遠不輪替 | trait 預設回 Err，走壓縮 | 正確；log 雜訊見 NICE TO HAVE 2 |
  | 輪替後多一次壓縮（TBD-5） | 下一回合對新 session 壓縮一次，成功後 `mark_agent_session_compacted` 把 `turn_count` 歸零（`store/mod.rs:702`），不會連續輪替 | 端到端觀察，PR 有揭露 | 可接受 |
  | live event 清理是否刪到 Web 正在讀的 | 不會 | 表是 `INTEGER PRIMARY KEY AUTOINCREMENT`（`wukong-chat-history/src/lib.rs:205`），刪掉最新的列也不會重用 id，Web 的游標不會錯過新事件。Web 起點是 `latest_live_event_id`，24 小時的視窗遠大於 500 ms 的輪詢間隔；Telegram、Web、schedulerd 用同一個 `WUKONG_MEMORY_DB` | 安全 |
  | `active_policy` 遇到空字串 | `var_os` 回 `Some("")`，`configured=true`；`from_value("")` 是 0，結果停用並附原因 | 讀碼確認；compose 的 `${VAR-30}` 遇到 `.env` 留空也會停用，和 `.env.example` 的說明一致 | 安全方向 |
  | `created_at` 沒有索引 | 每 900 秒全表掃描一次 | 無 | 首次之後表只剩 24 小時的量，可忽略 |

- **需求、架構、安全、品質**：
  - 架構：gateway 只多一個 trait 方法加兩個 backend 實作；runtime 負責決策；schedulerd 負責啟用條件與 tick。依賴方向沒變，沒有改 schema。`Outcome` 列舉加三處 `unreachable!` 稍嫌笨重，但限縮在私有的 `execute`，可以接受。三個部分之間沒有重複實作；B 沿用既有的 `rotated_from` 與 `with_previous_turn`，沒有新增抽象。
  - 安全／資料遺失：schedulerd 這邊，CLI backend 一律不清，本機 backend 未設定時不清，anchored 防護保留，輪替只在指向切換之後才刪舊 session。我沒有找到會刪掉仍被指向的 session 的路徑（NICE TO HAVE 4 的既有路徑目前到不了）。唯一的新資料遺失路徑是 MUST FIX 1。
  - 文件：AGENTS.md、`docs/docker.md`、`docs/entrypoints.md`、`CHANGELOG.md`、`.env.example` 的敘述，我逐項對過程式碼，都相符：本機模式不 vacuum（entrypoint 只在 `opencode serve` 分支呼叫）、Web 從最新的 id 開始讀、約 0.7 秒、24 小時、`turn_count` 不歸零。不符的是 `docs/cli-reference.md:93`（SHOULD FIX 1），以及 `docs/docker.md` 的「三種情況不成立」沒有涵蓋主機本機模式下手動 prune 的情況（MUST FIX 1）。

- **豁免、待確認與限制**：
  - TBD-1（7 天是暫定值，沒有正式主機的量測）：checklist 寫「在 PR 揭露」，交付前要確認 PR 說明確實有揭露。
  - TBD-4、TBD-5：結論是不影響交付、由 PR 揭露。我同意，並建議 TBD-4 一併揭露排程延遲（NICE TO HAVE 1）。
  - 我沒有自己重跑真實 opencode 的端到端與沙盒 daemon，這部分依 implementation-plan 的紀錄判讀。
  - MUST FIX 1 是讀碼得出的結論（CLI 的 prune 分支 → `prune_opencode_sessions` → `LocalProcessBackend::list_sessions`／`delete_session`，中間沒有任何 backend 閘門），我沒有在主機上實測。另外有一點沒有驗證：opencode 的 `GET /session` 可能只列出 serve 工作目錄所屬 project 的 session。若是這樣，影響範圍會縮小到「和 `WUKONG_WORKSPACE` 同一個 project 的自有 session」，但仍然屬於不可逆的刪除。

## 流程判定
RETURN TO execute-task
