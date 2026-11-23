# 審查報告
- 範圍：完整交付範圍（增量覆審）
- Reviewed BASE SHA：b26c7958b11e0f75859d53d5d9901c08b9b5c74e
- Reviewed HEAD SHA：5f2c2d6ab3bceff7eecb916eb0a6ae2777f6fc87
- Reviewed patch-id：60862491ac031cb70671fc0126bcbe8893bfb841（2026-11-23 以 `git diff b26c795 5f2c2d6 | git patch-id --stable` 重算，相符）
- 獨立 reviewer：Claude Code 隔離 subagent（模型 Opus 5.5，claude-opus-5-5），不是本 issue 的實作者
- Review artifact：docs/issues/issue-0030/review-5f2c2d6.md
- 增量覆審：上一份報告 review-4be256c.md，增量範圍 `4be256c..5f2c2d6`

## 問題與風險

- MUST FIX：無。

- SHOULD FIX 1：日誌其實記得到每段 LLM step 的結束時間，「拆不出上游花了多久」這個限制寫錯了。上一份報告 MUST FIX 1 也寫錯（「日誌裡也沒有 stream 結束的紀錄」），這次的修正照著寫了進去。
  - 位置：docs/issues/issue-0030/inventory.md:281（「日誌沒有記錄串流結束的時間，上游各自花了多久無法直接拆出」）、:308（「日誌拆不出比例」）、:320（「日誌拆不出精確比例」）。
  - 證據：bench 副本的 err-1～3.txt 裡，每個 opencode 程序在 `message=stream` 之後都有 `message=loop ... step=1`，接著是 `exiting loop`。short 任務只有一個 step，所以 step=1 出現的時間就是這一段 LLM 呼叫串流結束的時間。從 stream 到 step=1，plan／build 分別是 4.60／8.00、8.10／3.94、4.30／2.34 秒。每回合合計 12.6、12.0、6.6 秒，占回合耗時（16,112／15,405／10,063 ms）的 78%、78%、66%。兩個程序之間（plan 的 `event disconnected` 到 build 的第一行）隔 0.66／0.64／0.81 秒。日誌以外的時間（turn_ms 減去第一行到最後一行）約 0.63／0.70／0.66 秒，是 Wukong spawn 之前與結束之後的時間。
  - 影響：結論的方向不變，反而更有根據：根因排序第 1、2 名的順序與 no-go 都不受影響。但文件把可以量到的東西寫成量不到，之後評估常駐 server 或換模型時，讀者會以為還需要另做量測。LLM step 的時間裡也包含 opencode 自己處理串流的 CPU，所以它是上游耗時的上限，不是純網路等待；這點要一起寫清楚。
  - 建議：用既有日誌改寫第 281 行，不需要呼叫模型。寫成「LLM step（stream 到 loop step=1）每回合合計 6.6～12.6 秒，占 66～78%，包含上游回應與 opencode 處理串流的時間；兩個程序之間約 0.6～0.8 秒；日誌以外約 0.6～0.7 秒」。第 308、320 行的「拆不出比例」改成引用這組數字；implementation-plan.md:17 的摘要可以照樣補上。

- SHOULD FIX 2：TBD-5 的追蹤去向不符合 docs/AGENTS.md「待確認事項」的寫法。
  - 位置：docs/issues/issue-0030/README.md:63。
  - 證據：docs/AGENTS.md:139 規定 `不影響本次交付` 的去向如果寫 issue 編號，要是「可確認存在的 issue 編號或連結（該 issue 回連此項）」。dev-rules-kit #11 存在，也還開著（`gh issue view 11 --repo raybird/dev-rules-kit` 回 OPEN）。但它的 README（docs/issues/issue-0011/README.md）、GitHub 內文與留言都沒有提到 Wukong 或這一項，下游專案一律匿名寫成「下游專案 A」。#11 的範圍是狀態寫法，「正式版發布後到 Wukong main 執行 `init-project.py --update`」這件事只有 Wukong 這邊知道，#11 發版時不會有人提醒。另外，影響欄寫「不影響 SCN-001、SCN-002」，沒有寫出未確認時的影響。規則要求影響欄寫「未確認時的影響與關聯驗收編號」，例如 kit 升級併進同一個 squash、main 帶著未發版的流程契約 2.7。
  - 結論與語意：狀態欄有結論、日期與使用者選項原話（「照現狀合併並在 PR 明列」），與上一份報告建議的 (a) 一致。狀態值用 `不影響本次交付` 是合理的：要後續處理的是發版後的 `--update`，不在本次交付內。這列沒有 `<`，行首不是粗體或 `[`。PR 說明與 squash 訊息要寫明內含 bd001e6，這是建立 PR 時要做的事，本次審查沒辦法驗證。
  - 影響：不影響 SCN-001、SCN-002 的證據。只影響後續追蹤：上游發版後，可能沒有人回來對齊 main 上的候選版規則檔。
  - 建議：二選一。(a) 在 dev-rules-kit #11 留言回連 Wukong #30 的 TBD-5；這會讓 kit 的 issue 出現下游專案的名稱。(b) 改成在 Wukong 開一個追蹤 issue（例如「dev-rules-kit 正式版發布後在 main 執行 `--update`」），TBD-5 寫那個編號；還沒開好之前，依規則維持 `待確認`。影響欄一併改成未確認時的影響。這兩種都只改待確認事項的表格列，屬於記帳區段，不會讓 PASS 失效。

- NICE TO HAVE 3：v2-probe.sh 的重跑說明還缺一步，腳本裡也留著一句舊結論。
  - docs/issues/issue-0030/inventory.md:224 寫「啟動方式同 v2-migration.sh 的 start（不加 `--network none`）」。但 docs/issues/issue-0030/scripts/v2-probe.sh:3 的用法是 `sh /scripts/v2-probe.sh`，而 v2-migration.sh:19-21 的 start 沒有掛 /scripts，照著做會找不到腳本。建議補上 `-v .../scripts:/scripts:ro`。
  - v2-probe.sh:18 的註解「回 200 代表 SPA 吞掉了路徑（健康檢查會誤判）」沒有跟著 SHOULD FIX 2 的修正改。這個檔不在增量範圍內，只是註解，不影響判定；建議改成「compose healthcheck 會誤判」。

- NICE TO HAVE 4：v2-migration.sh 宣稱可以重現的範圍，比它實際檢查的多一點。
  - inventory.md:217 寫「啟動訊息、認證、V1 路徑、/api/info、openapi 與遷移／回復都由 v2-migration.sh 重現」。但 :242 的「最大 session 523 則 message／2,114 個 part 都讀得到」與 :205 的「OPENCODE_SERVER_USERNAME 不再支援」，腳本都沒有檢查：它只數 session，也只用 opencode 這個使用者。建議在腳本補這兩項，或在說明裡把它們排除。
  - v2-migration.sh 用固定的容器名稱（m-v2a 等），`set -e` 中途失敗時沒有 trap 清理，留下的容器會讓下一次執行因為名稱衝突失敗。boot-cost.sh:16 的 `until grep -q "listening on"` 沒有逾時，opencode 起不來時會卡住。這兩個都只影響重跑，不影響現有的結果。

## 已查核維度

- 驗收與證據
  - 增量覆審的前提：我自己核對過。`git rev-parse` 確認 4be256c 是 4be256ce637df8c09ade99713020cab424bcfb5d，HEAD 是 5f2c2d6ab3bceff7eecb916eb0a6ae2777f6fc87；`git merge-base --is-ancestor 4be256c HEAD` 成立；`4be256c..5f2c2d6` 只有 70cc78b、5f2c2d6 兩個提交，沒有 merge；merge-base 與 origin/main 都是 b26c7958b11e0f75859d53d5d9901c08b9b5c74e。上一份報告綁定 H1、記完整範圍、判定 RETURN TO execute-task，符合 review-evidence.md「增量覆審」三項條件。工作區乾淨。
  - 核准基線：`git diff 83d3670 5f2c2d6 -- docs/issues/issue-0030/README.md` 只動到現況、文件清單、涉及檔案、核准 commit 回填、待確認事項、Gate 豁免紀錄、Timeline 與狀態，Gherkin 與核准來源行都沒變。`git log -1 -G '核准來源'` 推導出 83d3670，與核准 commit 一致。SCN-001、SCN-002 都是已核准，集合與 Gherkin 相等。
  - SCN-001：步驟 1～3 的證據不在增量範圍內，沿用上一份報告的結論。增量只改了步驟 1 的 WAL 自檢與步驟 3 的比較基準，見下方「增量覆審」。
  - SCN-002 Then「產出升級或暫緩的證據及回復方案」：no-go 的四點理由與五項前提仍然成立。修正後的前提 2（inventory.md:338）指對了地方，回復方案（還原備份）不變，「只換回 V1」的風險補上了被刪掉的三張表（:249）。判定有證據。
  - SCN-002 Then「不相容或測試失敗會明列為阻礙」：阻礙的性質已改正（gateway 會失敗，compose healthcheck 才會假綠燈），:206、:261、:330 一致。判定有證據。
  - 計畫步驟 4～6 的證據行（implementation-plan.md:15、:17、:19）與 inventory 的修正一致。只剩 SHOULD FIX 1 提到的「拆不出比例」可以補數字。
- 相關失敗面
  - 副本沒有包含 WAL：wal-check.py 改成自己判 PASS／FAIL（:35）。2026-11-23 03:45 UTC，我用 snapshot.py 從正式 volume（`:ro`、`--network none`）重取副本，放在我自己的暫存目錄，緊接著跑 wal-check.py：memory.db `PASS snapshot 2298 main+wal 2298 main-only 2226`，opencode.db `PASS snapshot 19430 main+wal 19430 main-only 19422`。反向對照：把「只有主檔」的檔案當成副本餵進去，兩者都判 FAIL（`snapshot 2226 main+wal 2298`、`snapshot 19422 main+wal 19430`）。結束碼不論 PASS 或 FAIL 都是 0，結果要看輸出，屬於揭露過的限制。判定已覆蓋。
  - 取數空結果的假綠燈：inventory.md:94-96 新記的三條 sed，我逐一套在 inventory.py 的複本上，在一次性容器裡跑 `inventory.py control`。原版 PASS（rc 0）；`offset 1` 版 FAIL，top_sessions_by_parts／by_messages 不符；`return 0` 版 FAIL，missing table 拿到 0；條件寫反版 FAIL，events_outside_sessions 拿到 2。都是 rc 1，與表格完全相符。table_bytes 的交叉驗算我在副本上用 dbstat 重算：總和 42,078,208 等於 page_count×page_size，freelist 為 0，差額 20,480 byte 剛好是 sqlite_schema，與 :98 相符。判定已覆蓋。
  - V2 探測只看狀態碼：壓縮與換模型已改成「端點接受請求，效果沒有驗證」（:212、:215、:233、:261）。判定已揭露。
  - 回合成功是假的（W-7）：成功判準已寫進 :283。對「v0.23.0 tool 三次都留下 bench.txt」，我另外查了 bench 副本的 opencode.db（含 WAL）：同一個 session 有三組 write（content hello、/workspace/bench.txt）加 read，狀態都是 completed；三次輸出也相同。這比只看 workspace 的檔案更能證明三次都有寫。判定已覆蓋。
  - 以回滾代替還原：我用一致性副本與 scratchpad 的 V2 安裝，在我自己的工作目錄重跑 v2-migration.sh（全程 `--network none`，rc 0）。輸出與實作者保存的 migration-run.txt 逐行相同：24 個 migration；沒帶認證時 /global/health 回 200 text/html、/api/info 回 401；openapi 142 條路徑；session 數 6→6→6→7；刪掉 data_migration、session_context_epoch、session_input（都是 0 筆）；新增 12 張表。這與 inventory.md:241-245 相符。判定已覆蓋，只剩 NICE TO HAVE 4。
  - 正式資料被寫入或遷移：我對正式 volume 只做了一次 `:ro` 掛載、`mode=ro` 開啟、`--network none` 的副本與 wal-check。docker inspect 顯示三個容器的 StartedAt 仍是 2026-11-23T00:54:38Z，RestartCount 都是 0，沒有被重啟。判定正式部署維持唯讀。
  - 額度用完污染正式部署：本次審查沒有呼叫任何模型，也沒有跑 bench-turn.sh 或 v2-probe.sh。
- 需求、架構、安全、品質
  - 需求：增量仍然只有文件與 scripts/ 下的取數腳本，沒有產品程式碼，與「涉及檔案」相符。
  - 架構：不適用。
  - 安全與隱私：我 grep 了增量的新增行，沒有 session id、message id、token、email 或對話內容。v2-migration.sh 的 `probe-secret` 是只用在 `--network none` 一次性容器裡的固定測試密碼，不是正式憑證。判定通過。
  - 品質：兩支新腳本都很小、只做一件事；`bash -n` 與 py_compile 都通過（py_compile 產生的 __pycache__ 已刪除，工作區維持乾淨）。
- 增量覆審
  - 上一份報告每項發現的處理結果：
    - MUST FIX 1：已修好。我從 err-1～3.txt 重算：每回合兩個 `run=`，啟動到 stream 1.25／0.95、1.05／0.97、0.95／0.99 秒，合計 1.94～2.20 秒，占 13.6%、13.1%、19.2%，與「13～19%」相符。plan 的 stream 到 build 的第一行是 5.27、8.74、5.12 秒，與「5.1～8.7」相符。turn_ms 減去啟動是 8.1～13.9 秒，與「8～14 秒」相符。我在一次性容器重跑 boot-cost.sh（bench/boot 的複本、v0.25.1、1.5 CPU、`--network none`，5 次），得到 0.84～1.04 秒、1.12～1.42 CPU 秒；實作者的是 0.89～0.99 秒、1.21～1.32 CPU 秒，在雜訊範圍內。兩次啟動約 2.3～2.8 CPU 秒，「約 2.5 CPU 秒、一半左右」成立。CPU 落差已交代（:282），「其餘都在等上游」已拿掉，根因排序第 2 項改成「成立，不可忽略」，plan 摘要也同步了。殘留問題見 SHOULD FIX 1（限制寫得比實際保守）。
    - SHOULD FIX 2：已修好。我重查了 crates/wukong-gateway/src/opencode_server.rs:178-182、:450-471：health_check 走 send_json，200 之後解析 JSON，失敗會回 `returned invalid JSON`。docker-compose.yml:100 與 docker-compose.release.yml:80 是不帶認證的 `curl -fsS`。v2-migration.sh 的輸出也量到了「沒帶認證也是 200 與 text/html」這個上次沒量到的前提。:206、:261、:330、:338 都已改正。
    - SHOULD FIX 3：已修好。「V1 的表保留」改成刪掉三張空表、其餘保留；session_message 不再寫成新增（我在副本確認 V1 本來就有它的索引）。遷移與回復的四步、啟動訊息、認證、/api/info、openapi 都有可重跑的 v2-migration.sh，重跑結果相同。v2-probe 的執行方式補了說明，但少了掛載 /scripts（NICE TO HAVE 3）；523／2,114 與 USERNAME 沒有納入腳本（NICE TO HAVE 4）。錯誤密碼的負向對照沒有保存輸出，文件已揭露。
    - SHOULD FIX 4：已修好（inventory.md:212、:215、:232-233、:261）。
    - SHOULD FIX 5（`需決策`）：使用者選 (a)，記在 README TBD-5，不需要改程式。格式與語意的查核結果見 SHOULD FIX 2：結論、日期、原話都有，但追蹤的 issue 沒有回連，影響欄也沒寫影響。bd001e6 不在增量範圍內，上一份報告對七個檔 sha256 的查核沿用。
    - NICE TO HAVE 6：已修好。16 ms 對 10～25 秒是 625～1,560 倍，「約三個數量級」正確，也不再跨主機比較。
    - NICE TO HAVE 7：已修好：table_bytes 的限制與驗算、wal-check 的判定、sed 反向自檢、bench 的成功判準都已補上，我也都重跑驗證過（見上）。
    - NICE TO HAVE 8：已修好（README.md:61 的影響欄補了 SCN-002）。
  - 增量範圍內每個變動檔案的結論：
    - docs/issues/issue-0030/README.md：`## 現況` 在 `## 概述` 之後（:8-10），位置正確；`**現行 review**` 只有一行檔名，符合 document-types.md。保存本報告的提交要把它改成 review-5f2c2d6.md。TBD-3 補了 SCN-002。TBD-5 見 SHOULD FIX 2。`**狀態**` 在 70cc78b 曾改成「修正中（review-4be256c）」，5f2c2d6 改回「待審查」，都只寫階段，符合規則。新增內容沒有 `<`，行首不是粗體或 `[`。判定通過，附 SHOULD FIX 2。
    - docs/issues/issue-0030/implementation-plan.md：:15、:17、:19 的證據行與 inventory 的修正一致，任務狀態不變。判定通過，可依 SHOULD FIX 1 補數字。
    - docs/issues/issue-0030/inventory.md：步驟 1 的 WAL 自檢、步驟 2 的 sed 與驗算、步驟 3 的比較基準、步驟 4 的健康檢查／遷移／壓縮／換模型、步驟 5 的啟動成本、步驟 6 的排序與前提都查核過，數字都與我重算或重跑的結果相符。判定通過，附 SHOULD FIX 1、NICE TO HAVE 3、4。
    - docs/issues/issue-0030/review-4be256c.md：70cc78b 新增，之後沒有修改，內容與上一份報告相同，保留作為歷史。判定通過。
    - docs/issues/issue-0030/scripts/boot-cost.sh：重跑成功，結果與文件相符。判定通過，附 NICE TO HAVE 4（沒有逾時）。
    - docs/issues/issue-0030/scripts/v2-migration.sh：重跑成功，輸出與保存的紀錄逐行相同。判定通過，附 NICE TO HAVE 4。
    - docs/issues/issue-0030/scripts/wal-check.py：重取副本後 PASS，反向對照 FAIL。判定通過。
  - 重新查核的項目與理由：
    - gateway 與 compose 的健康檢查（不在增量範圍內）：SHOULD FIX 2 的修正改寫了它們的行為描述，所以重讀原始碼確認。
    - bench 日誌與 results2.jsonl：MUST FIX 1 的修正重新解讀了它們，所以逐列重算。重算時發現 SHOULD FIX 1。
    - bench-turn.sh 與 bench 的 opencode.db：成功判準與 W-7 的結論依賴它們，所以重讀腳本並查 part 表。
    - v2-probe.sh（不在增量範圍內）：inventory 新增了它的執行方式，壓縮與換模型的結論也改了，所以重讀，結果是 NICE TO HAVE 3。
    - 交接紀錄 docs/2026-11-11-runtime-performance-handover.md：它的追補段落只連到 README，沒有重述被修正的數字，不受影響。
    - README TBD-2 的結論（「API、事件、啟動與認證都變了」「就地單向遷移」「回復要還原備份」）：與修正後的步驟 4 一致，不受影響。
    - 正式部署的唯讀狀態：本次又讀了一次正式 volume，所以重查容器的啟動時間與重啟次數。
  - 沿用的結論（增量沒有改動它們，也沒有改它們的前提）：
    - 步驟 1 的主機、映像 digest、版本矩陣、副本 sha256。
    - 步驟 2 的盤點數字與對照組 PASS，以及 inv-20261123.jsonl 逐位元組相同。
    - 步驟 3 的查詢計畫、冷暖耗時與 recall p50／p95／max。
    - 步驟 5 的 CPU、throttling、記憶體、DB 增量表格。
    - v0.25.1 tool 失敗是上游限流的判讀。
    - bd001e6 的七個 sha256 與 docs/_templates/ 的處理。
    - Gate 豁免紀錄的格式與語意，以及 TBD-1、TBD-2、TBD-4 的格式。
    - 安全與架構的結論。
- 豁免、待確認與限制
  - Gate 豁免紀錄（README.md:65-69）：增量沒有改動，沿用上一份報告的判定，符合格式與語意。
  - 待確認事項：TBD-1～4 符合規則；TBD-5 見 SHOULD FIX 2。這一節沒有 `待確認` 的項目，所以沒有會阻擋交付的未知。
  - 限制：沒有重跑 bench-turn.sh 與 v2-probe.sh，因為它們會呼叫模型、額度已用完。它們的結論只靠讀程式碼，以及 scratchpad 裡實作者留下的日誌與輸出；那些檔案不在 repo 裡，保存不長久。boot-cost.sh 與 v2-migration.sh 的重跑輸出、我取的副本，都放在 session scratchpad 的 review-5f2c2d6 目錄，同樣不在 repo 裡。

## 流程判定
PASS

理由：沒有 MUST FIX。上一份報告的 MUST FIX 1 與要修的 SHOULD FIX 2～4、NICE TO HAVE 6～8 都已修好，數字都經過我重算或重跑。SHOULD FIX 5 依使用者的決定記為 TBD-5。SHOULD FIX 1 是限制寫得比資料保守，根因排序與 no-go 不受影響；SHOULD FIX 2 只影響後續追蹤；兩者都不阻擋判定。SHOULD FIX 2 的修正只改待確認事項的表格列，屬於記帳區段，不會讓 PASS 失效。SHOULD FIX 1 要改 inventory.md 與 implementation-plan.md，這兩個檔不是記帳區段：修了就要重新審查（可以再做一次增量覆審），不修就在 PR 揭露。

PASS 只適用於上列 Reviewed HEAD SHA。之後在這個分支提交 issue README 記帳區段與本報告以外的檔案（例如交接文件），PASS 會失效、需要重新審查（流程契約 2.3 以下只有單獨新增本報告的提交可沿用）；提交前先告知使用者，由使用者決定做法。
