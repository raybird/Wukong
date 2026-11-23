# 審查報告
- 範圍：完整交付範圍（增量覆審）
- Reviewed BASE SHA：b26c7958b11e0f75859d53d5d9901c08b9b5c74e
- Reviewed HEAD SHA：ace1a3a4179519b2d1f32d07747d2dcd8db2cfb9
- Reviewed patch-id：38f2966e20c811567fc79987f22ea34dcd8a6451（2026-11-23 以 `git diff b26c795 ace1a3a | git patch-id --stable` 重算，相符）
- 獨立 reviewer：Claude Code 隔離 subagent（模型 Opus 5.5，claude-opus-5-5），不是本 issue 的實作者
- Review artifact：docs/issues/issue-0030/review-ace1a3a.md
- 增量覆審：上一份報告 review-b708576.md，增量範圍 `b708576..ace1a3a`

## 問題與風險

- MUST FIX：無。

- SHOULD FIX 1：P3 新增的資料庫證據寫錯了回合對應，「第 1 回合的 session 被輪替刪除」也不是事實。結論（三個回合都確實寫入）仍然成立，而且證據比文件寫的更完整。
  - 位置：docs/issues/issue-0030/inventory.md:283（「第 2、3 回合的 build 各有一次對 /workspace/bench.txt 的 write 與 read……第 1 回合……它的 session 在下一回合被輪替刪除，DB 裡查不到」）；README TBD-8 的「補上 v0.23.0 tool 回合確實寫入的 DB 證據」引用的就是這段。
  - 證據：bench 副本 scratchpad/bench/v0.23.0-tool/opencode/ 的 opencode.db 旁邊還有 1,310,192 byte 的 opencode.db-wal。
    - 照指示用 immutable=1 唯讀開主檔：bench 期間只有一個 session ses_df99…，建立於 11:08:30、time_updated 11:08:56；build 的 write／read bench.txt 只有兩組，時間是 11:08:35／11:08:37 與 11:08:50／11:08:52，都是 completed。immutable=1 不讀 WAL，所以這是只看主檔的結果。
    - 把 opencode.db 與 -wal 一起複製到我的暫存目錄再開（integrity_check ok）：同一個 session 的 time_updated 變成 11:09:16，多出第三組 write／read（11:09:08／11:09:12）。這個 session 有三則 user 訊息（11:08:31、11:08:46、11:09:05），每則之後各有一次 write 與 read。
    - 回合的時間範圍由 logs/out-N.txt 的修改時間（回合結束）減去 results2.jsonl 的 turn_ms 推得：第 1 回合約 11:08:25～11:08:39.9，第 2 回合約 11:08:39.9～11:08:56.3，第 3 回合約 11:08:56.3～11:09:16.3。三組寫入剛好各落在一個回合裡。
    - 旁證：workspace 目錄的修改時間是 11:08:35（第 1 回合建立 bench.txt），bench.txt 的修改時間是 11:09:08（第 3 回合寫入），內容是 hello。表格裡第 1 回合的 opencode.db 增量 1,239 KB，也與在第 1 回合新建 session 相符。
    - 所以第 1 回合的 session 沒有被刪除，三個回合都在同一個 session 裡，三次寫入都查得到。文件把只看主檔時的兩組寫入誤配成第 2、3 回合，再用「輪替刪除」解釋第 1 回合為什麼不見。
  - 影響：成功判準與步驟 5 的數字都不受影響，因為三個回合確實都寫入了。但這段文字對 v0.23.0 的 session 行為寫了錯誤的說法（每回合輪替刪除 session），之後讀到的人可能拿它去推論 session 的成長方式。另外，對帶著 -wal 的 bench 副本用 immutable=1 會漏掉最後一段資料，之後重查也可能踩到同一個坑。
  - 建議：改寫成「三個回合在同一個 session（scope project:bench-tool），build 各有一次對 /workspace/bench.txt 的 write 與 read，都是 completed（11:08:35、11:08:50、11:09:08）；第 1 回合之前 workspace 沒有 bench.txt」，並註明 bench 副本有 WAL，要把主檔與 -wal 一起複製後再查，不能用 immutable=1。這是 inventory.md 的修改，不屬於記帳區段，改了要重新審查（可以再做增量覆審）。

- SHOULD FIX 2：bench-turn.sh 只在回合開始前刪掉 bench.txt，還是沒辦法逐回合判定 tool 任務是否成功；P3 要解決的問題只解決了最後一個回合。
  - 位置：docs/issues/issue-0030/scripts/bench-turn.sh:28（新增的 `rm -f /workspace/bench.txt`）、:35（每回合輸出的 JSON 沒有 bench.txt 的狀態）；inventory.md:283（「2026-11-23 起 bench-turn.sh 每回合開始前先刪掉 bench.txt」）。
  - 證據：N 個回合在同一個容器裡連續跑完，中間沒有人看 workspace，每一行 JSON 也只記 rc 與 out_bytes。下一回合開始時就把上一回合的 bench.txt 刪了，容器結束後留下的 bench.txt 只代表最後一個回合。我在暫存目錄用同樣的迴圈結構模擬（回合開始先 rm，假回合依設定決定寫不寫）：三回合都寫入，與第 2 回合沒有寫入，兩種情況的 JSON 輸出與結束後的 bench.txt 完全相同。第三方審查的建議原文是「每回合改用唯一檔名，或回合前清除既有檔案並驗證當次 CLI 輸出」，這次只做了前半。
  - 影響：只影響之後的重測，不影響已記錄的結果（v0.23.0 tool 三個回合的寫入已由 SHOULD FIX 1 的資料庫查核確認）。但腳本註解與 inventory.md 的寫法會讓人以為重測時可以逐回合判定，實際上只有最後一個回合是可信的。
  - 建議：在回合結束後、印 JSON 之前檢查 bench.txt，例如 `ok=0; [ "$(cat /workspace/bench.txt 2>/dev/null)" = hello ] && ok=1`，把 ok 加進 JSON 欄位；inventory.md:283 的成功判準改為看這個欄位。這同樣是記帳區段以外的修改，要重新審查。

- NICE TO HAVE 3：inventory.md:309 步驟 6 假設表的「長期 session 的事件與工具輸出讓資料量變大」那一列，仍寫「在目前規模下，讀取最大 session 約 15 ms，不構成瓶頸」，沒有跟著 :187、:314 與 implementation-plan.md:13 加上「這組測試查詢、反序列化與事件處理沒有單獨量」的限定。這一列講的正是長期 session 的載入成本，也就是第三方擔心被誤讀的地方；:297「改用步驟 3 的本機載入時間（最大 session 約 15 ms）代替」也是同一個量。數字本身沒錯，:309 已經限定在目前規模，所以列為建議；要改的話，在句末補「（SQL 查詢時間；反序列化與事件處理沒有單獨量）」即可。

## 已查核維度

- 驗收與證據
  - 增量覆審的前提：我自己核對過。HEAD 是 ace1a3a4179519b2d1f32d07747d2dcd8db2cfb9；`git merge-base --is-ancestor b708576 HEAD` 成立；`b708576..ace1a3a` 只有 f35f895、ace1a3a 兩個提交，沒有 merge；reflog 從 b26c795 起只有一連串 commit，沒有 reset 或 rebase；`git merge-base HEAD origin/main` 與 main、origin/main 都是 b26c7958b11e0f75859d53d5d9901c08b9b5c74e，等於上一份報告的 Reviewed BASE。上一份報告綁定 H1、記完整範圍、判定 PASS，符合 review-evidence.md「增量覆審」的三項條件。工作區乾淨。
  - PR 版本：`gh pr view 43` 的 headRefOid 是 f35f895，`origin/issue-0030-runtime-inventory` 也是 f35f895，ace1a3a 還沒推送。這份報告審的是本地的 ace1a3a；推送後要核對 PR 的 source HEAD 等於 ace1a3a（或之後只有記帳提交），才能拿本報告當 PR 的審查證據。
  - 核准基線：增量沒有動到 Gherkin、核准紀錄與 SCN 狀態表。核准 commit 仍是 83d3670，SCN-001、SCN-002 仍是已核准，沿用上一份報告的推導。
  - SCN-001 Then「報告包含來源主機、日期、聚合數值與限制」：步驟 1 的 WAL 自檢與步驟 3 的資料庫結論都加上了適用範圍，數字沒有變；步驟 5 新增的資料庫證據細節有誤（SHOULD FIX 1），但它支持的結論經我直接查核屬實。判定有證據。
  - SCN-002 Then「產出升級或暫緩的證據及回復方案」：no-go 的四項理由沒有改動。:314 從「不成立」改成「在本次資料規模與測試查詢下不成立」，no-go 不依賴資料庫成本（:332 的效能理由是上游模型），不受影響。判定有證據。
- 相關失敗面
  - 第三方的三項建議（原文以 `gh api repos/raybird/Wukong/pulls/43/reviews/6018454389 --jq .body` 讀取，使用者的決定記在 README TBD-8：全選建議答案）：
    - P2 WAL 自檢的證明範圍：已修好。inventory.md:60 改稱「WAL 涵蓋性的反向自檢」；:67 寫明只比對總筆數、「主檔＋WAL」是分開複製的兩個檔案、來源持續寫入時時間點不一定一致、不證明每筆內容相同，一致性來自 Backup API 與 integrity_check。我核對 scripts/snapshot.py 確實用 `source.backup(target)` 並對副本跑 `pragma integrity_check`，步驟 1 的表格（:38-43）也有 integrity_check 欄，:67 的依據屬實。wal-check.py 的 docstring 只說「證明副本確實含 WAL 內容」，沒有宣稱一致性，與新寫法一致；implementation-plan.md:9「WAL 反向自檢通過（只有主檔時筆數較少）」也沒有誇大。
    - P2 資料庫結論的限定：大致修好。:187 改成「在這個資料規模、這組測試查詢與負載下，看不到明顯的 SQL 查詢延遲」，並寫明 opencode 的查詢形狀是依索引推測、不是 SQL trace，反序列化與事件處理沒有單獨量；:314 的判定與 implementation-plan.md:13 的摘要同步加上限定。:163 原本就寫了查詢形狀是推測的，與新增的說法一致。:309 漏改，見 NICE TO HAVE 3。
    - P3 bench 的輸出檔：部分修好。腳本加了回合前刪檔（`bash -n` 通過，rm 在容器內以 root 執行，對 bind mount 的檔案有權限），也補了資料庫證據。但逐回合仍判定不了（SHOULD FIX 2），補上的資料庫證據寫錯回合對應（SHOULD FIX 1）。
  - P3 的資料庫證據，我自己查核的方法：只讀 bench 副本，先用 immutable=1 開主檔，再把主檔與 -wal 複製到我的暫存目錄開複本；沒有寫入 bench 副本本身（查核前後 opencode.db 的 sha256 為 b2ec0fb4…c0cb，-wal 為 9f85ec4b…8f27）。結果見 SHOULD FIX 1。
  - immutable=1 的影響範圍：增量揭露了「對帶 WAL 的檔案用 immutable=1 會漏資料」，所以我在 HEAD 找了所有用 immutable=1 的地方。inventory.py、queries.py 讀的是 Backup API 的副本，copy-20261123 目錄裡沒有任何 -wal 檔；wal-check.py 對「只有主檔」刻意用 immutable=1，正是要排除 WAL；v2-migration.sh 讀的是副本的再複製。這些結論都不受影響。
  - 正式資料被寫入：本次審查沒有讀或掛載 ~/Documents/RunWuKong 與它的 volume，只讀 scratchpad 的 bench 副本，查詢都在它的複本上做。
  - 額度用完污染正式部署：本次審查沒有呼叫任何模型，也沒有跑 bench-turn.sh 或 v2-probe.sh；SHOULD FIX 2 的模擬只是一段不呼叫任何程式的 shell 迴圈。
- 需求、架構、安全、品質
  - 需求：增量只有 issue 目錄下的文件與一支取數腳本，沒有產品程式碼，與「涉及檔案」相符。
  - 架構：不適用。
  - 安全與隱私：增量的新增行沒有 token、email 或對話內容；inventory.md 沒有寫入 session id。本報告引用的 session id 前綴屬於 bench 副本的合成回合，prompt 是固定的合成內容。判定通過。
  - 品質：bench-turn.sh `bash -n` 通過。
- 增量覆審
  - 增量範圍內每個變動檔案的結論：
    - docs/issues/issue-0030/README.md：f35f895 把 `**現行 review**` 改成 review-b708576.md，位置仍在 `## 概述` 之後；TBD-7 的影響欄補上 SCN-002（上一份報告提過的意見）；`**狀態**` 改成「已完成」。ace1a3a 新增 TBD-8，並把 `**狀態**` 改回「待審查」。TBD-8 的狀態是「已解決」，有日期與使用者的選擇；新增行沒有 `<`，行首不是粗體或 `[`；影響欄寫「步驟 1、3、5 的結論措辭與根因排序的依據；SCN-001」，與實際改動的範圍相符。狀態欄的「補上 v0.23.0 tool 回合確實寫入的 DB 證據」所指的證據細節有誤，修 SHOULD FIX 1 時不需要改這一列。`**狀態**` 只寫階段「待審查」，符合 docs/AGENTS.md「有特殊意義的狀態值」；從 b708576 到 HEAD 的淨差異裡這個欄位沒有變。判定通過。
    - docs/issues/issue-0030/implementation-plan.md：只改 :13 的步驟 3 摘要，加上規模、測試查詢與反序列化的限定，與 inventory.md:187 一致；任務狀態不變。判定通過。
    - docs/issues/issue-0030/inventory.md：:60、:67（WAL 涵蓋性）與 :187、:314（資料庫結論）查核過，判定通過；:283 的資料庫證據有誤，見 SHOULD FIX 1、2。
    - docs/issues/issue-0030/review-b708576.md：f35f895 新增，ace1a3a 沒有修改（`git diff --name-status f35f895 ace1a3a` 不含它），保留作為歷史。判定通過。
    - docs/issues/issue-0030/scripts/bench-turn.sh：只加一行回合前刪檔，語法正確，不影響其他量測欄位；但沒達成逐回合判定，見 SHOULD FIX 2。
  - 重新查核的項目與理由：
    - 步驟 5 表格「v0.23.0 tool 3 次成功」與成功回合的耗時（inventory.md:276 等，未改動）：P3 質疑的正是這個判準，所以我直接查資料庫確認三個回合都寫入並讀回 bench.txt，三份 out-N.txt 的輸出也都回報建立並讀到 hello。判定維持成功，步驟 5 的數字與根因排序不受影響。
    - 步驟 6 假設表與根因排序（:309、:314、:322）：資料庫結論加了限定，所以重新核對其他寫到資料庫成本的地方。:314 已改；:322「目前不影響查詢」講的是查詢，用語精確；:309 漏改，見 NICE TO HAVE 3。no-go（:332）不依賴資料庫成本，不受影響。
    - 步驟 1 的副本可信度：自檢改稱涵蓋性檢查之後，一致性改由 Backup API 與 integrity_check 支撐，所以我核對了 snapshot.py 的實作與 :38-43 的 integrity_check 欄，兩者都在。
    - 所有用 immutable=1 的腳本：理由與結果見上方「immutable=1 的影響範圍」。
    - 交接紀錄 docs/2026-11-11-runtime-performance-handover.md：增量沒有改它；它沒有重述資料庫或 WAL 的結論，不受影響。
  - 沿用的結論（增量沒有改動它們，也沒有改它們的前提）：
    - 步驟 1 的主機、映像 digest、版本矩陣、副本 sha256，以及 WAL 自檢的數字與 PASS 判定。
    - 步驟 2 的盤點數字、對照組與 sed 反向自檢、table_bytes 驗算。
    - 步驟 3 的查詢計畫、冷暖耗時與召回 p50／p95／max（只改了結論的措辭）。
    - 步驟 4 的 API 差異矩陣、健康檢查的判讀、v2-probe 與 v2-migration.sh 的結果。
    - 步驟 5 的 LLM step 數字與「上游耗時的上限」限定、啟動成本、CPU、throttling、記憶體、DB 增量，以及 v0.25.1 tool 失敗是上游限流的判讀。
    - boot-cost.sh、v2-probe.sh、v2-migration.sh、inventory.py、queries.py、wal-check.py、snapshot.py：增量沒有改這些腳本。
    - bd001e6 的七個 sha256 與 docs/_templates/ 的處理。
    - Gate 豁免紀錄，以及 TBD-1～7 的格式與語意。
    - 安全與架構的結論。
- 豁免、待確認與限制
  - Gate 豁免紀錄（README.md:68-72，表格列在 :72）：增量沒有改動，沿用上一份報告的判定。
  - 待確認事項：TBD-1～8 都有結論，沒有 `待確認`，沒有會阻擋交付的未知。TBD-5 的「PR 說明與 squash 訊息寫明內含候選版」要到合併時才能驗證，本次審查無法確認。
  - 限制：沒有重跑 bench-turn.sh 與 v2-probe.sh，因為它們會呼叫模型。回合的時間範圍是用 out-N.txt 的修改時間與 turn_ms 推算的，誤差在 1 秒以內，三組寫入與回合邊界至少隔 4 秒，不影響對應。我的資料庫複本與模擬腳本放在 session scratchpad 的 review-ace1a3a 目錄（withwal/、sim/），不在 repo 裡，保存不長久。

## 流程判定
PASS

理由：沒有 MUST FIX。第三方的三項建議，使用者決定都修：兩項 P2 已修好（WAL 自檢改稱涵蓋性檢查並寫明一致性的依據；資料庫結論加上規模、測試查詢與反序列化的限定，只漏了 :309 一處）。P3 部分修好：補上的資料庫證據把回合對應寫錯，還多了一句不存在的「session 輪替刪除」，但我直接查資料庫確認三個回合都寫入了，它支持的結論成立，步驟 5 的數字與 no-go 都不受影響；腳本的回合前刪檔沒辦法逐回合判定，只影響之後的重測。這兩項列為 SHOULD FIX。

PASS 只適用於上列 Reviewed HEAD SHA。之後在這個分支提交 issue README 記帳區段與本報告以外的檔案（例如交接文件），PASS 會失效、需要重新審查（流程契約 2.3 以下只有單獨新增本報告的提交可沿用）；提交前先告知使用者，由使用者決定做法。
