# 審查報告
- 範圍：完整交付範圍（增量覆審）
- Reviewed BASE SHA：b26c7958b11e0f75859d53d5d9901c08b9b5c74e
- Reviewed HEAD SHA：b708576dcff0cc5f4d051cb76fb5c66b8232ecef
- Reviewed patch-id：5f17f1d9c46bd664406d5fe45e118321bcb4f128（2026-11-23 以 `git diff b26c795 b708576 | git patch-id --stable` 重算，相符）
- 獨立 reviewer：Claude Code 隔離 subagent（模型 Opus 5.5，claude-opus-5-5），不是本 issue 的實作者
- Review artifact：docs/issues/issue-0030/review-b708576.md
- 增量覆審：上一份報告 review-b88a4c4.md，增量範圍 `b88a4c4..b708576`

## 問題與風險

- MUST FIX：無。
- SHOULD FIX：無。
- NICE TO HAVE：無。

## 已查核維度

- 驗收與證據
  - 增量覆審的前提：我自己核對過。HEAD 是 b708576dcff0cc5f4d051cb76fb5c66b8232ecef；`git merge-base --is-ancestor b88a4c4 HEAD` 成立；`b88a4c4..b708576` 只有 0d9afb3、b708576 兩個提交，沒有 merge；reflog 從 b26c795 起只有一連串 commit，沒有 reset 或 rebase，歷史沒有改寫；`git merge-base HEAD origin/main` 與 main、origin/main 都是 b26c7958b11e0f75859d53d5d9901c08b9b5c74e，等於上一份報告的 Reviewed BASE。上一份報告綁定 H1、記完整範圍、判定 PASS，符合 review-evidence.md「增量覆審」的三項條件。工作區乾淨。
  - 核准基線：增量沒有動到 Gherkin、核准紀錄與 SCN 狀態表。核准 commit 仍是 83d3670，SCN-001、SCN-002 仍是已核准，沿用上一份報告的推導。
  - SCN-001 Then「報告包含來源主機、日期、聚合數值與限制」：步驟 5 的 LLM step 數字沒有改，只補上「含 opencode 處理串流的時間，是上游耗時的上限」的限定。這正是上一份報告要的限制說明，判定有證據。
  - SCN-002 Then「產出升級或暫緩的證據及回復方案」：no-go 的理由、前提與回復方案沒有改動。inventory.md:332「瓶頸在上游模型」依賴的是步驟 5 的比例，加上「上限」之後仍然成立，理由見下方「重新查核」。判定有證據。
  - SCN-002 Then「不相容或測試失敗會明列為阻礙」：inventory.md:242 的「最大 session 523 則 message／2,114 個 part 都讀得到」現在由腳本依 part 數取最大的 session 重現，見下方失敗面。判定有證據。
- 相關失敗面
  - 腳本選錯 session（上一份報告的 NICE TO HAVE 2）：
    - 正常輸入：我在自己的工作目錄（scratchpad/review-b708576/mig）用 copy-20261123 與 v2/prefix 重跑 HEAD 版的 v2-migration.sh，rc 0，約 7 秒，輸出與實作者保存的 v2/migration-run3.txt 逐行相同；和 migration-run2.txt 只差第 13 行的標籤（`v1 latest session` 改成 `v1 largest session`，數字都是 523／2114）。重跑前後副本的 sha256 都是 2d2f3518…f594，沒有被改到。
    - 反例：我另做一份副本（scratchpad/review-b708576/alt-snap），把 part 最少的 session（2 則 message／5 個 part）的 time_updated 改成最新，讓最新與最大不是同一個 session。HEAD 版輸出 `v1 largest session: messages 523 parts 2114`，b88a4c4 版輸出 `v1 latest session: messages 2 parts 5`。新腳本確實取最大的 session，不是碰巧與舊腳本結果相同。
    - 讀取失敗：Python 迴圈裡任一個 session 的 urlopen 失敗都會讓 python3 以非零結束，`set -e` 會中止腳本，不會印出 0 當成結果。
    - 判定已覆蓋。
  - 中途失敗留下容器：增量沒有改 trap 與容器命名；三次重跑（正常、反例的新舊兩版）結束後 `docker ps -a --filter name=mig-` 都沒有殘留。沿用上一份報告的失敗路徑結論。
  - 正式資料被寫入：本次審查完全沒有讀或掛載 ~/Documents/RunWuKong 與它的 volume，所有重跑都用 scratchpad 的副本或它的再複製。
  - 額度用完污染正式部署：本次審查沒有呼叫任何模型，也沒有跑 bench-turn.sh 或 v2-probe.sh。
- 需求、架構、安全、品質
  - 需求：增量只有 issue 目錄下的文件與取數腳本，沒有產品程式碼，與「涉及檔案」相符。
  - 架構：不適用。
  - 安全與隱私：增量的新增行沒有 session id、token、email 或對話內容。新腳本把 session id 只用在容器內的 HTTP 請求，不寫檔也不輸出，比舊版寫進容器 /tmp/latest 更少留痕。判定通過。
  - 品質：`bash -n` 通過。巢狀引號（bash 雙引號包 sh 雙引號包 Python，URL 用單引號）實際執行正確，見上方重跑。
- 增量覆審
  - 上一份報告每項建議的處理結果（使用者的決定記在 README TBD-7：兩項都修正）：
    - SHOULD FIX 1（決定修正）：已修好。inventory.md:281 在 LLM step 的定義後補上「也包含 opencode 自己處理串流的時間，所以這是上游耗時的上限，不是純粹等待上游的時間」；:308 的判定從「成立」改回「大致成立」，並加註「含 opencode 處理串流的時間，是上游耗時的上限」；:320 根因排序第 1 名加上同樣的限定；implementation-plan.md:17 的摘要加上「（上游耗時的上限）」。`git grep` 確認 HEAD 的 issue 目錄裡，所有寫到 66～78% 的地方（implementation-plan.md:17、inventory.md:281、:308、:320）都帶著這個限定。
    - NICE TO HAVE 2（決定修正）：已修好。腳本改成逐一讀各 session 的訊息，取 part 最多的那個（同數時比 message 數），標籤改為 `v1 largest session`，與 inventory.md:217「讀回最大 session」、:242「最大 session 523 則 message／2,114 個 part」以及 :124 的盤點（part 最多的 session 2,114 個，message 數最多也是同一個的 523 則）一致。反例驗證見上方失敗面。
  - 增量範圍內每個變動檔案的結論：
    - docs/issues/issue-0030/README.md：0d9afb3 把 `**現行 review**` 改成 review-b88a4c4.md，位置仍在 `## 概述` 之後，符合保存報告提交的要求。b708576 新增 TBD-7，狀態是 `已解決`，寫了結論、日期與使用者選項原話；「重跑輸出除標籤外相同」我比對過，屬實。新增行沒有 `<`，行首不是粗體或 `[`。它的影響欄只寫 SCN-001，但步驟 6 與 v2-migration.sh（步驟 4）屬於 SCN-002，嚴格說應寫 SCN-001、SCN-002（TBD-6 上一輪就照這個意見改過）。這項已解決，改不改都不影響判定；這列在記帳區段，要改的話不會讓 PASS 失效，所以不另列發現。`**狀態**` 維持「待審查」，只寫階段。判定通過。
    - docs/issues/issue-0030/implementation-plan.md：只在 :17 加上「（上游耗時的上限）」，任務狀態不變，與 inventory 步驟 5 一致。判定通過。
    - docs/issues/issue-0030/inventory.md：:281、:308、:320 的修改都查核過，數字沒有變動，限定的寫法前後一致。判定通過。
    - docs/issues/issue-0030/review-b88a4c4.md：0d9afb3 新增，b708576 沒有修改（`git diff --name-status 0d9afb3 b708576` 不含它），保留作為歷史。判定通過。
    - docs/issues/issue-0030/scripts/v2-migration.sh：只改了步驟 2 讀回 session 的那一段，重跑輸出與保存的紀錄相同，反例證明選的是最大的 session。判定通過。
  - 重新查核的項目與理由：
    - 步驟 6 的根因排序與 no-go 理由（inventory.md:320、:332，後者未改動）：判定改成「大致成立、是上限」，所以重新確認排序與「瓶頸在上游模型」是否還站得住。就算把每回合 opencode 啟動以外的約 3 CPU 秒（inventory.md:282）全部算進 LLM step，在 1.5 CPU 上限下最多約 2 秒牆鐘時間，LLM step 扣掉後仍有約 4.6～10.6 秒，比第 2 名的啟動成本（約 2 秒）大。排序與 no-go 不變。
    - inventory.md:294「同版 short 任務與 v0.23.0 tool 任務都顯示耗時主要在上游」（未改動）：用語是「主要」，與「大致成立」一致，不受影響。
    - inventory.md:217、:242、:124 的既有說法：腳本的標籤與選法改了，所以重新核對三處是否與新輸出一致。結果：一致。
    - 交接紀錄 docs/2026-11-11-runtime-performance-handover.md：增量沒有改它；它只連到 README，沒有重述被修正的數字或判定，不受影響。
  - 沿用的結論（增量沒有改動它們，也沒有改它們的前提）：
    - 步驟 1 的主機、映像 digest、版本矩陣、副本 sha256 與 WAL 自檢。
    - 步驟 2 的盤點數字、對照組與 sed 反向自檢、table_bytes 驗算。
    - 步驟 3 的查詢計畫、冷暖耗時與召回 p50／p95／max。
    - 步驟 4 的 API 差異矩陣、健康檢查的判讀、v2-probe 的探測結果與錯誤密碼的負向對照、v2-migration.sh 其餘步驟（USERNAME 的 401／200 對比、遷移與回復的 session 數、表的差異）；後者這次重跑也相同。
    - 步驟 5 的 LLM step 數字（上一份報告從 err-1～3.txt 逐列重算）、啟動成本、CPU、throttling、記憶體、DB 增量表格、成功判準，以及 v0.25.1 tool 失敗是上游限流的判讀。
    - boot-cost.sh 與 v2-probe.sh 的結論：增量沒有改這兩支腳本。
    - bd001e6 的七個 sha256 與 docs/_templates/ 的處理。
    - Gate 豁免紀錄，以及 TBD-1～6 的格式與語意。
    - 安全與架構的結論。
- 豁免、待確認與限制
  - Gate 豁免紀錄（README.md:67-71）：增量沒有改動，沿用上一份報告的判定，符合格式與語意。
  - 待確認事項：TBD-1～7 都已有結論，沒有 `待確認`，所以沒有會阻擋交付的未知。TBD-5 的「PR 說明與 squash 訊息寫明內含候選版」要到建立 PR 時才能驗證，本次審查無法確認。
  - 限制：沒有重跑 bench-turn.sh 與 v2-probe.sh，因為它們會呼叫模型。步驟 5 的 LLM step 數字沿用上一份報告的重算，所依據的日誌在 scratchpad，不在 repo 裡。我的重跑輸出與反例副本放在 session scratchpad 的 review-b708576 目錄（mig-run.txt、alt-head.txt、alt-old.txt、alt-snap/），同樣不在 repo 裡，保存不長久。

## 流程判定
PASS

理由：沒有 MUST FIX。上一份報告的兩項建議，使用者決定都修，兩項都已修好：SHOULD FIX 1 的「上游耗時的上限」限定已補在所有寫到 66～78% 的地方，判定改回「大致成立」，根因排序與 no-go 經重新查核不變；NICE TO HAVE 2 的腳本改取 part 最多的 session，重跑輸出與保存的紀錄相同，反例副本證明它選的是最大而不是最新的 session。本輪沒有新的發現。

PASS 只適用於上列 Reviewed HEAD SHA。之後在這個分支提交 issue README 記帳區段與本報告以外的檔案（例如交接文件），PASS 會失效、需要重新審查（流程契約 2.3 以下只有單獨新增本報告的提交可沿用）；提交前先告知使用者，由使用者決定做法。
