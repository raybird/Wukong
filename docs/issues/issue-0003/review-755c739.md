# 審查報告
- 範圍：完整 PR（GitHub PR #4，`issue-0003-opencode-session-retention` → `main`；審查的是 BASE..HEAD 的完整 diff，35 個檔案、十個提交。心力集中在 8020271 之後的 755c739，其餘範圍依風險重新確認，哪些是重跑、哪些只讀紀錄見「驗收與證據」。依限制未連線 GitHub）
- Reviewed BASE SHA：c613028e74127c633fecf408bacb93f0dceb915c
- Reviewed HEAD SHA：755c739165df0f4e5838d3c364f68dd66f734424
- Reviewed patch-id：1daf4001bfc71205898b7d635b85c81cd93395c2（2026-10-02 自行以 `git diff BASE HEAD | git patch-id --stable` 重算，相符；`git rev-parse HEAD` 相符；`git merge-base main HEAD` 即 BASE；本地 `origin/issue-0003-opencode-session-retention` 也指向 755c739；審查前後 `git status --short` 皆為空）
- 獨立 reviewer：Claude Code 隔離 subagent（claude-opus-5-5）；未參與本次實作，也不是前四份審查報告的作者
- Review artifact：docs/issues/issue-0003/review-755c739.md
- 審查日期：2026-10-02
- 風險：High（不可逆刪除）

## 問題與風險

### MUST FIX

無。

### SHOULD FIX

**S-1　保留天數只傳進 `wukong-schedulerd`，在其他容器執行 `wukong opencode prune` 用的是程式預設的 30 天（即 review-8020271 的 N-1；三項刻意不處理的建議中，我認為只有這一項高於 NICE TO HAVE）**

- 位置：`docker-compose.yml:242`、`docker-compose.release.yml:184`（唯一的傳遞點）；`docs/docker.md:225`、`.env.example:106`（範例指定了 schedulerd 容器）
- 實測：以 `docker compose config` 渲染兩份 compose，這個變數只出現在 `wukong-schedulerd` 一個服務；`wukong-web`、`wukong-telegram` 同樣帶著 `WUKONG_AGENT_SERVER_URL` 與同一份記憶庫，卻沒有它。以 HEAD 的 `wukong` 對假 server 執行 `opencode prune --dry-run`：沒有這個變數時輸出「將刪除 1 個（保留期 30 天）」，設 `0` 時輸出「已停用」，設 `7` 時是「保留期 7 天」。
- 為什麼不只是 NICE TO HAVE：另外兩項（N-2、N-3）只影響日誌內容與測試的收尾，這一項影響的是**刪什麼**。`.env` 設 `0` 的操作者多半是照文件的指示，因為 server 與別人共用才停用；他在 web 或 telegram 容器裡下 `prune`，停用的設定沒有被帶到，會照 30 天刪。`.env` 設成 7 或 90 時，在那兩個容器看到的預覽與 schedulerd 實際會刪的集合不同。
- 為什麼不是 MUST FIX：要操作者自己換掉文件範例裡的容器才會遇到；輸出第二行會印出實際使用的保留期；會被刪的是沒有任何 scope 指向的 session。排程清理（SCN-003）與同一個容器內的預覽（SCN-009）都正確。
- 實作者把它留著在程序上是合法的（步驟 12 有列為未處理並寫了理由，使用者沒有要求），所以不阻塞。建議：把同一行加進另外兩個有 server 連線的服務，或在 `docs/docker.md` 與 `.env.example` 補一句「這個設定只有 schedulerd 容器拿得到，`prune` 請在該容器執行」。

### NICE TO HAVE

- **N-1　「測試鉤子不在 release 建置裡」這件事沒有任何自動化檢查守著。** 我把 `crates/wukong-schedulerd/src/session_retention.rs:18` 與 `:33` 的兩個 `#[cfg(debug_assertions)]` 拿掉後：debug 下 cfg 恆為真，程式展開後完全相同；`cargo test --release -p wukong-schedulerd` 仍是 21 項通過、daemon 測試 0 項（實際執行過）。這個變異建出來的 release binary 設 `1` 時啟動 1 秒後就開始刪、`strings` 找得到變數名。也就是說這個性質目前只靠實作者與我各自手動問了一次產物。CI（`.github/workflows/ci.yml:37`）只跑 debug 的測試，release workflow 不跑測試。屬證據持久力：在 `scripts/test-release-image.sh` 的 `check_smoke`（`:137` 起）加一行「映像檔內的 `wukong-schedulerd` 不含 `WUKONG_TEST_`」即可。即使哪天 cfg 被拿掉，1 到 21,599 的範圍限制仍在，回來的只有「提早清理」，不會再有啟動即 panic。
- **N-2　review-8020271 N-2 維持原狀（合法的不處理）。** `crates/wukong-gateway/src/opencode_server.rs:449`：回應不是合法 JSON 時共用的 `send_json` 仍寫出整個 body（重跑確認：錯誤訊息含 `"title":"quarterly salary review"`）。這是既有函式、不在本次 diff；清理每 6 小時才跑一輪，server backend 建立的 session 標題固定是 `Wukong`，影響有限。`:918`、`:919` 的兩句措辭與「測試資料沒有多位元組字元」也都沒動。
- **N-3　review-8020271 N-3 維持原狀（合法的不處理）。** `crates/wukong-schedulerd/tests/retention_daemon.rs:160-165` 的 `Drop` 在測試行程被訊號終止時不會執行，會留下 daemon；子行程沿用呼叫端的 proxy 設定；`main.rs:123` 改成不經過 `ticker()` 時測試仍然全綠（我重跑了這個變異：測試通過，`cargo clippy -D warnings` 以 `function ticker is never used` 擋下）。留下來的 daemon 指向已關閉的埠與已刪除的暫存記憶庫，刪不到任何東西。這些只影響測試本身。
- **N-4　`VACUUM` 本身沒有時間上限。** `scripts/docker-entrypoint.sh:296-302` 在 `opencode serve` 之前同步執行；資料庫被鎖時 5 秒後放棄（重跑確認），但真的開始重寫之後要跑完才會啟動 server。實作者量到 831 MB 全為存活資料時 2.59 秒（僅閱讀），而 `opencode-server` 的 healthcheck 在 `start_period: 60s` 之後還容許 3 次、每次間隔 30 秒（`docker-compose.yml:105-108`），所以要比量測慢數十倍才會在 `docker compose up` 時讓相依的服務等到逾時。不是缺失，只是 `docs/docker.md` 沒有提這段時間隨資料量成長。
- **N-5　步驟 12 的「未處理」清單是摘要。** `implementation-plan.md:206` 沒有逐字列出 review-8020271 N-2 的「多位元組測試資料」與 N-3 的「三份重複的假 server」，兩者確實都沒動。不影響判讀。

### review-8020271 各項發現的處置

| 項目 | 處置 | 查核 |
|---|---|---|
| S-1 測試用間隔進了正式 binary、極大值讓 daemon 啟動即 panic | 已確實解決 | 見下方「S-1 的修正」；兩種建置各 12 個以上的值實測、release binary 不含變數名、六個變異全部轉紅 |
| S-1 附帶的三處文字矛盾 | 已更正，與程式一致 | `session_retention.rs:9-11`（`INTERVAL` 的註解）、`README.md:166`（TBD-1）、`implementation-plan.md:92`（設定）與 `:134`（步驟 5）都補上「只存在於 debug 建置的測試鉤子」這個例外，我以產物確認這句話為真 |
| N-1 保留天數只傳進 schedulerd | 未處理，合法；我列為本報告 S-1 | 見上 |
| N-2 相鄰路徑仍寫出整個 body、兩句措辭 | 未處理，合法 | 本報告 N-2 |
| N-3 daemon 測試的收尾與覆蓋 | 未處理，合法 | 本報告 N-3 |
| N-4 紀錄上的四處出入 | **已處理**（不是未處理） | `README.md:131` 的狀態改回 `已核准`；`README.md:184` 的 Timeline 與步驟 11 一致；`implementation-plan.md:197` 補上與 `limit` 量測的區別；TBD-6（`README.md:170`）補上 compose `cli` profile 的情況 |

## 已查核維度

### 驗收與證據

「重跑」指我在 2026-10-02 於 HEAD 755c739 親自執行：全量檢查在 repo 內（只寫入 `target/`）與 repo 之外的匯出複本；黑箱實測用 repo 內由 HEAD 建出的 release binary 與匯出複本建出的 debug binary，一律指向拋棄式假 server 或已關閉的埠、拋棄式 SQLite 檔。「僅閱讀」指只讀了實作者的紀錄（需要真實 opencode 容器，在本次審查的禁止範圍內）。

| 編號 | 證據位置 | 查核方式 | 結論 |
|---|---|---|---|
| SCN-001 | runtime `session_retention` 測試；`tests/opencode_prune.rs`；`tests/retention_daemon.rs`；計畫步驟 1、8、9 | 測試重跑；`wukong` 與 debug daemon 對假 server：只有過期無主的 `ses_orphan` 收到 `DELETE`。變異「忽略受保護集合」「由新到舊刪」「保留期邊界含等號」轉紅。訊息／片段／事件隨之消失是 opencode 的行為，**僅閱讀** | 通過 |
| SCN-002 | `deletes_expired_orphans_…`、`a_scope_pointing_at_a_child_protects_the_whole_tree`、`a_root_is_only_as_old_…`、`referenced_session_ids_cover_both_session_tables` | 重跑；變異「不沿父子關係往上找」「漏讀 `agent_sessions`」「漏讀 `agent_session_state`」「父不在清單的子 session 當成根」轉紅。續接沒有真實模型回合，實作者已揭露 | 通過；截斷邊界為 TBD-7 |
| SCN-003 | `policy_defaults_…`、`only_a_server_backend_…`；compose 兩份 | 重跑；變異「無法解析時退回 30 天」「CLI backend 也啟用」「停用時仍啟用」轉紅；binary：`0` → 已停用且沒有任何請求，空字串 → 警告＋已停用；`docker compose config`：沒有變數 `"30"`、留空 `""`、`7`、`0` 各如其值 | 通過 |
| SCN-004 | `an_unreadable_scope_table_deletes_nothing`、`a_failed_listing_deletes_nothing`、gateway `list_sessions_fails_…` | 重跑；變異「列表失敗當成空清單」「讀不到指向表當成空集合」「缺時間的列當成 0」「跳過看不懂的列」「CLI backend 回空清單」轉紅；binary 對不合法 JSON 結束碼 1、沒有 `DELETE` | 通過 |
| SCN-005 | `list_sessions_asks_for_every_session_with_an_explicit_limit`、`a_full_page_is_reported_as_truncated` | 重跑；變異「不帶 `limit`」「帶 `roots=true`」「永不標示截斷」轉紅；binary 的請求確為 `GET /session?limit=10000`。真實 opencode 的行為**僅閱讀** | 通過 |
| SCN-006 | `one_failed_delete_does_not_stop_the_rest`、`a_server_that_never_answers_gives_the_loop_back`、`a_failed_delete_exits_nonzero` | 重跑；變異「拿掉整輪時間上限」讓測試停在 agent 的逾時不返回（我在約 10 分鐘後以 PID 停掉）；daemon 對已關閉的埠：每輪記一行 `warning: opencode session retention failed: …list_sessions failed…` 後繼續，`SIGTERM` 結束碼 0 | 通過 |
| SCN-007 | `opencode_db` 6 項；計畫步驟 6、8、9、10 | 測試重跑；變異「忽略門檻」「不檢查磁碟」「不檢查檔案存在」「不執行 VACUUM」轉紅。以替身 `gosu` 執行 entrypoint 的那一段（HEAD 的原文）六種情況：檔案不存在 `outcome=missing` 且不建立任何東西；9,224,192 → 57,344 bytes；另一連線持有寫鎖時 5.7 秒後 `database is locked`、印警告、繼續、檔案不變；舊 binary（help 沒有 `vacuum`）整段略過且沒有把字當 prompt 執行；PATH 上沒有 `wukong` 時略過；子命令以非零結束時印警告後繼續。真實容器重啟**僅閱讀** | 通過 |
| SCN-008 | `tests/new_session_flag.rs` 2 項 | 重跑；變異「不送刪除」兩項轉紅、「刪除失敗就中止」一項轉紅。CLI backend 依修訂後的 Given 不在範圍 | 通過 |
| SCN-009 | `preview_deletes_nothing_and_names_what_a_real_run_deletes` | 重跑；變異「dry-run 照刪」轉紅；binary：預覽只有一個 `GET`，列出的 `ses_orphan` 正是之後實刪的 | 通過 |
| SCN-010 | runtime 三項、`tests/opencode_prune.rs` 三項、`tests/retention_daemon.rs` 一項、schedulerd `log_lines` 一項 | 重跑；變異「拿掉 anchored 檢查」「不印記憶庫位置」「結束碼恆為 0」「daemon 傳空字串當記憶庫位置」轉紅；binary：空記憶庫對有 session 的 server、有指向對空 server 都是「未清理」、結束碼 1、沒有 `DELETE`；兩邊皆空結束碼 0 | 通過 |

**規格與核准**：自行擷取三個版本的 Gherkin 區塊比對。06b25d3 → HEAD 完全相同；c613028 → HEAD 的差異只有兩處：SCN-008 的 Given 一行，與新增的 SCN-010（7 行）。06b25d3 只動了 README。核准表（`README.md:122-133`）十列與現存 Scenario 集合相等，狀態全部是 `已核准`，沒有 `acceptance.md` 三個值以外的寫法。兩次修訂的核准來源各自寫明日期、觸發的審查項目、提供的選項與使用者的選擇，與 Timeline 一致；對話本身我無從查證。

**S-1 的修正（755c739）**——問產物，不問描述：

| 設定值 | release（repo 內 `cargo build --release --locked -p wukong-schedulerd -p wukong-cli` 的產物） | debug（匯出複本） |
|---|---|---|
| 未設定 | `first_run_in_secs=21600 interval_secs=21600` | 同左 |
| `1` | 21600；2 秒內只有一個 `GET /global/health`，沒有 `/session` 請求 | `first_run_in_secs=1`；1 秒後 `DELETE /session/ses_orphan`、`anchored=true deleted=1` |
| `+1` | 21600 | 1（Rust 的整數解析接受正號，無害） |
| `21599` | 21600 | 21599 |
| `0`、空字串、`abc`、`-5`、`" 1 "`、`1.5`、`21600` | 21600 | 21600 |
| `9223372036854775807`、`18446744073709551615`、`99999999999999999999999` | 21600，行程存活，`SIGTERM` 結束碼 0 | 同左，沒有 panic |
| `18446744073709551615` 且保留天數 `0`（上一輪 panic 的組合） | `retention disabled`，存活 | 同左 |

- 只選 `-p wukong-schedulerd` 建出的 release binary 是另一個產物（相依的 feature 組合不同、檔案大小不同），抽驗 `1` 與 `1.5`：同樣是 21600，同樣不含變數名。
- release binary 內 `WUKONG_TEST_OPENCODE_RETENTION_INTERVAL_SECS` 出現 0 次、任何 `WUKONG_TEST` 字樣 0 次；debug binary 1 次。判準本身的對照組：同一個 release binary 找得到 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS`（2 次），所以環境變數名在 strip 過的 binary 裡是看得到的；把 cfg 拿掉重建的 release binary 則出現 1 次並且真的讀它。
- **出貨的 binary 用哪個 profile**：`.github/workflows/release.yml:78` 是 `cargo build --release --locked --target …`，映像檔（`Dockerfile.release:32-35`）只複製那批 musl binary，`Dockerfile:22-29` 下載的是已發佈的 release 壓縮檔，`scripts/install.sh` 不編譯任何東西（整個檔案沒有 `cargo`，下載的是發佈的 docker bundle），`docs/installation.md:101` 的自行編譯是 `cargo build --release`。`Cargo.toml:27-31` 的 `[profile.release]` 只設了 `opt-level`、`lto`、`codegen-units`、`strip`，沒有 `debug-assertions`；repo 內沒有 `.cargo/config`，兩個 workflow 都沒有設 `RUSTFLAGS` 或 `CARGO_PROFILE_*`。沒有任何會出貨的建置開著 debug assertions。我實測的是 gnu 的 release binary，不是 musl；這個 cfg 與 target 無關。
- **debug 與 release 之間還有沒有別的行為差異**：整個 workspace 的 `cfg(debug_assertions)`／`debug_assert` 只有這次加的三處（`session_retention.rs:18`、`:33`、`:181`）與測試檔的 `#![cfg(debug_assertions)]`（`tests/retention_daemon.rs:10`）。沒有。
- **`cargo test --release -p wukong-schedulerd`**（重跑）：單元測試 21 項通過，`tests/retention_daemon.rs` 0 項，沒有警告；`cargo clippy --release --all-targets -p wukong-schedulerd -- -D warnings` 也通過。CI 與 `scripts/release.sh:117` 跑的都是 `cargo test --workspace --locked`（debug），所以那個被 cfg 包住的整合測試在 CI 會真的執行兩項，不是靜靜地什麼都沒測。
- **新的單元測試會不會紅**：六個變異全部轉紅——解析函式改回空實作（重現計畫記錄的紅燈：期望 `Some(1s)`，得到 `None`）、下界改 0、上界改為含 21600、不設上界、上界少 1、解析前先 `trim`。期望值都是寫死的字面值，不是由 `INTERVAL` 重算。另外兩個接線變異（`interval()` 無視鉤子、變數名寫錯）讓 daemon 的兩項整合測試各等滿 30 秒後失敗。

**變異測試彙總**（匯出複本，每次以原始複本還原後 `diff -rq` 確認相同）：44 個變異，42 個轉紅。兩個存活：拿掉 cfg（本報告 N-1）、`main` 不經過 `ticker()`（上一輪已知，由 clippy 擋下）。未發現假綠燈。

**全量檢查（重跑）**：`cargo test --workspace --locked` 41 個套件、608 通過、0 失敗（schedulerd 單元 22 項、daemon 兩項 1.07 秒、gateway 125 項，與計畫步驟 12 的數字全部相符）；`cargo clippy --all-targets --locked -- -D warnings` 無警告；`cargo fmt --all -- --check` 通過；`bash scripts/test-docker-runtime.sh` 通過。

### 相關失敗面

| 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
|---|---|---|---|
| 測試用間隔的各種值 × 兩種建置 | release 一律 6 小時；debug 只接受 1..21599 | 單元測試（debug）＋binary 實測 | 正確，見上表 |
| 上一輪 panic 的組合（極大值、保留天數 0） | daemon 照常啟動 | binary 實測 | 正確 |
| cfg 被拿掉 | 有檢查會紅 | 無 | 存活，N-1 |
| 記憶庫與清單：兩邊皆空／空記憶庫對有 session／有指向對空 server／相符 | 0／拒絕／拒絕／刪無主的 | 測試＋binary | 正確 |
| `.env` 沒有變數／留空／`7`／`0` | 30／停用／7／停用 | `test-docker-runtime.sh`；`docker compose config` 實際渲染 | 正確 |
| `.env` 寫了測試用的變數 | 到不了容器 | `docker compose config` | 正確：兩份 compose 渲染結果都沒有它；就算到了，release binary 也不讀 |
| 在 schedulerd 以外的容器執行 `prune` | 沿用 `.env` 的設定 | 無 | 用的是 30 天，S-1 |
| 清單不是合法 JSON、缺 id、缺時間、時間不是整數 | 整輪不刪 | 測試＋binary | 不刪；訊息內容見 N-2 |
| server 連不上／接受連線但不回應 | 放棄這一輪，迴圈繼續 | 測試；連不上以 daemon 實測 | 正確 |
| vacuum：檔案不存在、被鎖、未達門檻、磁碟不足、子命令失敗、舊 binary、沒有 binary | 不擋啟動 | 測試＋entrypoint 片段的替身執行 | 正確；耗時無上限見 N-4 |
| 進行中回合剛建立、尚未寫回對應的 session | 不被選中 | 保留期最小 1 天（`u32`，0 為停用）；讀程式碼 | 正確 |
| 兩份記憶庫共用 server；Wukong 與使用者共用 server | 明確揭露 | TBD-5、文件 | 會刪，屬使用者決定後的不處理；未重跑，由挑選邏輯直接推得 |
| 截斷把受保護的子 session 切在清單之外 | 根仍受保護 | 無；TBD-7 | 不成立，已揭露 |
| schedulerd 每次都在第一輪之前重啟 | 仍會清理 | 無；TBD-8 | 不會，已揭露 |

### 需求、架構、安全、品質

- **需求**：十個 Scenario 都有對應實作與證據。diff 內的檔案都在 README 的涉及檔案範圍內；`crates/wukong-runtime/src/session.rs` 與 `scripts/opencode-idle-restart.sh` 未被觸及；資料庫只做 `VACUUM`。
- **架構**：相依方向未被破壞。`tokio` 的 `test-util` 在 `[dev-dependencies]`，`cargo tree -p wukong-schedulerd -e normal,features -i tokio` 的輸出不含它。
- **安全／權限**：未發現注入或提權路徑。測試鉤子在出貨的 binary 裡已不存在。
- **會不會刪到還在用的 session、擋住啟動、卡住排程**：755c739 沒有新增或放寬任何刪除路徑——release 的行為與「從未有過這個鉤子」相同，debug 的鉤子只能把間隔縮短、不能讓啟動失敗。整個範圍內：刪除只走「保留期啟用 → 讀得到指向表 → 列得出清單 → 對得上 → 樹裡沒有任何受保護成員 → 過期」這一條路，每一關的變異都會轉紅；整輪清理有 5 分鐘上限；entrypoint 的回收在各種失敗下都繼續啟動。除已揭露的 TBD-5、TBD-7 與 S-1 外未發現。
- **品質／重複／過度設計**：修正是最小的——一個 cfg、一個範圍檢查、把解析抽成可測的純函式。沒有為此新增設定或抽象。重複的只有上一輪已指出的三份測試用 HTTP stub。
- **文件對照程式**：`docs/docker.md`、`docs/cli-reference.md`、`CHANGELOG.md`、`.env.example`、`AGENTS.md` 對預設值、留空、無效值、6 小時、每輪 500 個、5 分鐘、25%、2 倍、10,000、結束碼、兩邊皆空的例外的說法，與程式及我的實測一致；這五份文件都沒有提測試鉤子，與「它不是設定」一致。issue 文件裡上一輪指出的三處矛盾與四處紀錄出入都已更正。沒有發現文件宣稱而程式沒做的事。
- **計畫的自我回報**：步驟 12 的宣稱逐一對照產物，未發現不實回報——紅燈訊息、22 項、21 項＋0 項、1.07 秒、608 項與 41 個套件、兩種建置各個值的啟動日誌、`strings` 的結果都與我重跑的一致。

### 豁免、待確認與限制

- 沒有 gate 豁免。
- 待確認事項逐項判定：
  - TBD-1：已解決，數值與程式一致；新增的那句例外經產物確認為真。
  - TBD-2：已解決，與程式一致（25%、2 倍）。
  - TBD-3、TBD-4：待確認，需要受影響主機的資料；不影響安全範圍與必要驗證。
  - TBD-5：描述屬實，屬使用者決定後的不處理，文件已寫明處置。
  - TBD-6：已解決，以有核准來源的規格修訂結案；補上的 compose `cli` profile 一句，「之後會被 schedulerd 的清理收掉」是推論，沒有人實測過。
  - TBD-7：描述屬實，已寫進使用者文件。SCN-002 的字面不允許這個例外，比照前幾輪列為已揭露的限制。
  - TBD-8：描述屬實，文件已寫明處置。
- 實作者自行揭露的限制（SCN-002 沒有真實模型回合、樹狀保護沒有在真實 opencode 的舊資料上演練、容器用 gnu 而非 musl binary、步驟 11 與 12 的改動沒有在真實環境重跑）依規則不視為缺失。步驟 12 改到的路徑不經過真實 opencode 才有的行為，我同意不需要重跑。
- 本次審查的限制：
  - 沒有碰任何容器或 volume。「opencode 刪除 session 會連帶清掉訊息／片段／事件」「清單帶 `parentID`、依 `time.updated` 排序」「`limit=10000` 不被 server 另行截斷」「真實容器重啟時檔案變小」「`VACUUM` 的耗時」只讀了紀錄。
  - 沒有以 CLI backend 實際執行；沒有連線 GitHub，PR #4 的說明與遠端 HEAD 未核對；沒有建 musl 的 binary。
  - 沒有重現 review-8020271 N-3 的「測試行程被訊號終止會留下 daemon」，只確認相關程式碼自上一輪起沒有變。
- 審查過程中我自己的三個失誤，如實記錄：
  1. 一次黑箱執行用到了還留著變異的 debug `wukong`（變異還原後 binary 沒有重建），輸出少了第一行才發現。之後確認匯出複本與原始複本相同、重建，再重跑；本報告引用的 `wukong` 黑箱結果都來自重建後的 debug binary，或 repo 內從未被變異過的 release binary。S-1 表中 debug 那一欄是在任何變異之前跑的，重建後又抽驗了兩個值，結果相同。
  2. 一條寫壞的 shell 指令讓變數在主 shell 裡是空的，於是執行到的不是我的 `wukong`，而是 PATH 上真正的 `opencode`：`opencode prune --dry-run` 兩次（印出用法，結束碼 1）、`opencode prune` 一次（`Failed to change directory to ~/prune`）。這三次都沒有啟動 server、沒有建立 session、沒有送出任何 prompt。同一條指令在 `~` 建了 `out.txt` 與 `err.txt` 兩個檔案，我以建立時間確認是那一刻才產生的之後刪除。`~/.config/opencode`、`~/.cache/opencode`、`~/.local/state/opencode` 在 08:23:30 至 08:25:30 之間沒有任何檔案被修改。**`~/.local/share/opencode` 我依限制沒有去看，所以無法確認真正的 `opencode` 那三次啟動有沒有在裡面寫入日誌之類的檔案**——這一點請使用者知悉。之後的黑箱執行都改成開了 `set -u`、使用絕對路徑的腳本檔。
  3. 同一條指令留下一個我的假 server（Python）行程，已以確切 PID 停止。
- 結束前確認：沒有任何我啟動的行程留下；機器上運行中部署的 `wukong-schedulerd`（PID 5672）、`wukong-web`、`wukong-telegram` 與容器內的 `opencode serve` 我都沒有碰。repo 內除了本報告沒有其他變更；匯出複本與它們的建置輸出已刪除（repo 自己的 `target/` 多了這次的 release 建置）。
- 未查證：
  - server 的 project 改變後舊 session 是否還會被列出；真實 opencode 在超過 10,000 個 session 時的列表行為。
  - 修正提交是否依根目錄 `AGENTS.md` 跑過 GitNexus 影響分析與 `detect_changes`（由產物無從查核，依限制不重建索引）。

## 流程判定
PASS

理由：沒有 MUST FIX。review-8020271 的 S-1 已確實解決，而且是以產物確認的——出貨用的 release 建置不含那個變數、對任何值都以 6 小時啟動；debug 建置只接受 1 到 21,599，上一輪會 panic 的值現在都被忽略；新的單元測試對六個變異全部轉紅；三處相矛盾的文字已更正。十個 Scenario 的證據與失敗面重新查核通過，整個範圍內沒有發現會刪到仍在使用的 session、擋住容器啟動或卡住排程的路徑（已揭露的 TBD-5、TBD-7 除外）。本報告的 S-1 是上一輪就存在、實作者有揭露的設定傳遞缺口，要操作者偏離文件範例才會遇到，依規則不阻塞；其餘是證據持久力與可觀察性的建議。
