# 審查報告
- 範圍：完整交付範圍（增量覆審）
- Reviewed BASE SHA：b26c7958b11e0f75859d53d5d9901c08b9b5c74e
- Reviewed HEAD SHA：b88a4c47b83b1bbcd1d9d9e993262dea54dce3a0
- Reviewed patch-id：bb5f5f35326e2f205fa25fccdea4cd5a9f72c9b5（2026-11-23 以 `git diff b26c795 b88a4c4 | git patch-id --stable` 重算，相符）
- 獨立 reviewer：Claude Code 隔離 subagent（模型 Opus 5.5，claude-opus-5-5），不是本 issue 的實作者
- Review artifact：docs/issues/issue-0030/review-b88a4c4.md
- 增量覆審：上一份報告 review-5f2c2d6.md，增量範圍 `5f2c2d6..b88a4c4`

## 問題與風險

- MUST FIX：無。

- SHOULD FIX 1：建議 1 的修正少了一半。LLM step 的耗時被直接寫成上游模型的耗時，沒有說明它也包含 opencode 自己的處理時間。
  - 位置：docs/issues/issue-0030/inventory.md:281（把 stream 到 `loop … step=1` 定義為 LLM step）、:308（「回合耗時主要在上游模型｜成立：……兩次 LLM step 合計 6.6～12.6 秒，占回合耗時的 66～78%」）、:320（根因排序第 1 名「上游模型：每回合兩次 LLM step……占 66～78%」）。implementation-plan.md:17 的摘要照抄同一組數字。
  - 證據：review-5f2c2d6 的 SHOULD FIX 1 建議寫成「包含上游回應與 opencode 處理串流的時間」，並註明「它是上游耗時的上限，不是純網路等待；這點要一起寫清楚」。README TBD-6 記載使用者照建議修正建議 1。b88a4c4 補上了數字，但沒有加這句限定。同時 :308 的判定從「大致成立」改成「成立」，:320 把整段 LLM step 歸給「上游模型」。同一節 :282 自己也寫了「其餘約 3 CPU 秒花在串流處理、Wukong 本身或程序結束，這次的量測分不出來」，所以 LLM step 裡有多少是本機處理，目前確實分不出來。
  - 影響：數字本身正確（見下方重算），根因排序與 no-go 也不受影響：就算把 3 CPU 秒全算進 LLM step，上游仍是第 1 名。真正的問題在之後的決策：讀者會把 66～78% 當成換模型或換供應商能省下的上限，但這個比例其實高估了上游的份量。
  - 建議：在 :281 的 LLM step 定義後補一句：「包含上游回應與 opencode 處理這一步的時間，是上游耗時的上限」。:308、:320 可以改寫成「LLM step（含上游回應）」，或在句末加「（上限）」。這是 inventory.md 的修改，不屬於記帳區段，改了就要重新審查（可以再做一次增量覆審）。不改的話，在 PR 揭露這個限定。

- NICE TO HAVE 2：v2-migration.sh 讀的是最新的 session，文件說的是最大的 session。
  - 位置：docs/issues/issue-0030/scripts/v2-migration.sh:50 用 `max(d, key=lambda s: s['time']['updated'])` 選 session，輸出標籤是 `v1 latest session`。docs/issues/issue-0030/inventory.md:217 卻寫「含 V1 讀回最大 session 的 message 與 part 數」，:242 寫「最大 session 523 則 message／2,114 個 part 都讀得到」。
  - 證據：我在一次性容器（`--network none`，副本 immutable 開啟）查了 copy-20261123/opencode.db 的 6 個 session，依 time_updated 排序，各自的 message 與 part 數依序是 523／2114、279／1161、201／767、14／62、37／145、2／5。這份副本裡最新的 session 剛好也是最大的，所以 2026-11-23 的輸出與文件相符。
  - 影響：換一份副本，兩者就不一定是同一個 session，重跑的人會對不上 :242 的說法。現有結論不受影響。
  - 建議：把 key 改成 message 數（例如先列 session，再逐一取 message 數取最大），或把文件改成「最新的 session（在這份副本裡也是最大的）」。

## 已查核維度

- 驗收與證據
  - 增量覆審的前提：我自己核對過。HEAD 是 b88a4c47b83b1bbcd1d9d9e993262dea54dce3a0；`git merge-base --is-ancestor 5f2c2d6 HEAD` 成立；`5f2c2d6..b88a4c4` 只有 4b63d65、b88a4c4 兩個提交，沒有 merge；reflog 沒有 reset 或 rebase，歷史沒有改寫；`git merge-base HEAD origin/main` 與 main、origin/main 都是 b26c7958b11e0f75859d53d5d9901c08b9b5c74e，等於上一份報告的 Reviewed BASE。上一份報告綁定 H1、記完整範圍、判定 PASS，符合 review-evidence.md「增量覆審」的三項條件。工作區乾淨。
  - 核准基線：增量沒有動到 Gherkin、核准紀錄與 SCN 狀態表。核准 commit 仍是 83d3670，SCN-001、SCN-002 仍是已核准，沿用上一份報告的推導。
  - SCN-001 Then「報告包含來源主機、日期、聚合數值與限制」：步驟 5 新增的 LLM step 數字，我從 bench 副本的 err-1～3.txt 逐列重算，與文件相符。plan／build 從 `message=stream` 到 `message=loop … step=1` 分別是 4.603／8.002、8.095／3.943、4.295／2.340 秒，合計 12.605、12.038、6.635 秒。對照 results2.jsonl 的 turn_ms（16,112／15,405／10,063），比例是 78.2%、78.1%、65.9%。我也確認每個程序都只有 step=0、step=1，沒有 step=2，而且都有 exiting loop，所以 step=1 確實是這一步的結束。判定有證據，限定的缺口見 SHOULD FIX 1。
  - SCN-002 Then「產出升級或暫緩的證據及回復方案」：no-go 的四點理由、五項前提與回復方案都沒有改動，仍然成立。理由中的「瓶頸在上游模型」與修正後的數字一致。判定有證據。
  - SCN-002 Then「不相容或測試失敗會明列為阻礙」：inventory.md:205 的 USERNAME 結論與 :242 的 523／2,114 現在都由腳本重現，見下方失敗面。判定有證據。
- 相關失敗面
  - 腳本宣稱重現、其實沒檢查（上一份報告的建議 4）：我用 scratchpad 的一致性副本與 V2 安裝，在自己的工作目錄（scratchpad/review-b88a4c4/mig）重跑 HEAD 版的 v2-migration.sh，rc 0，約 7 秒。輸出與實作者保存的 v2/migration-run2.txt 逐行相同（`diff` 無差異）。新增的兩項檢查：`v1 latest session: messages 523 parts 2114`；`custom:pw 401`、`opencode:pw 200`。後者用 401 與 200 的對比，證明自訂的使用者名稱不生效、固定的 opencode 才有效，不是只看命中數（地雷 W-2）。step 5 的容器沒加 `--print-logs`，日誌裡仍有 `server listening on`，等待迴圈有效。判定已覆蓋，殘留問題見 NICE TO HAVE 2。
  - 中途失敗留下容器：我傳空目錄當 V2_PREFIX，讓第一個 V2 容器啟動失敗。腳本以 rc 127 結束，trap 依 `name=^mig-PID-` 清掉了已建立的容器，`docker ps -a --filter name=mig-` 沒有殘留。正常執行結束後也沒有殘留。容器名稱帶 `$$`，同時跑兩份也不會衝突。判定已覆蓋。
  - 啟動成本量測卡死：我在一次性容器重跑 HEAD 版的 boot-cost.sh（上一輪 boot 目錄的複本、v0.25.1、1.5 CPU、`--network none`，3 次），得到 822～1,028 ms、1.11～1.38 CPU 秒，與文件的 0.89～0.99 秒、1.21～1.32 CPU 秒在雜訊範圍內。反向對照：把等待字串換成不會出現的字串，腳本在 35 秒後以 rc 1 結束，印出 `opencode did not report listening within 30s`，沒有卡住。判定已覆蓋。
  - v2-probe.sh 的重跑說明（上一份報告的建議 3）：inventory.md:224 補上了掛載 `-v "$PWD/docs/issues/issue-0030/scripts":/scripts:ro`，與 v2-probe.sh:3 的 `sh /scripts/v2-probe.sh` 一致。這支腳本會呼叫模型，我沒有執行，只讀了腳本，`sh -n` 通過。
  - 正式資料被寫入：本次審查完全沒有讀或掛載 ~/Documents/RunWuKong 與它的 volume，所有重跑都用 scratchpad 的副本。
  - 額度用完污染正式部署：本次審查沒有呼叫任何模型，也沒有跑 bench-turn.sh 或 v2-probe.sh。
- 需求、架構、安全、品質
  - 需求：增量仍然只有 issue 目錄下的文件與取數腳本，沒有產品程式碼，與「涉及檔案」相符。
  - 架構：不適用。
  - 安全與隱私：增量的新增行沒有 session id、token、email 或對話內容。v2-migration.sh 印出的只有計數與狀態碼；line 50 寫進容器 /tmp/latest 的 session id 只留在一次性容器裡，不會輸出。`probe-secret` 是只用在 `--network none` 一次性容器的固定測試密碼。判定通過。
  - 品質：三支腳本的 `bash -n`／`sh -n` 都通過。v2-migration.sh:67 的 `mkdir -p "$work/user-check"` 沒有掛進容器（step 5 用容器內的 HOME=/tmp/home），是用不到的目錄，不影響結果，不另列發現。
- 增量覆審
  - 上一份報告每項建議的處理結果（使用者的決定記在 README TBD-6）：
    - SHOULD FIX 1（決定修正）：部分修好。inventory.md:281、:308、:320 與 implementation-plan.md:17 的「拆不出比例」都改成實際數字，我重算過，相符。`git grep` 確認 HEAD 的 docs 裡已經沒有「拆不出」或「8～14」的舊說法。缺的是建議中要一起寫清楚的「LLM step 包含 opencode 處理、是上游耗時的上限」，見 SHOULD FIX 1。兩個程序之間約 0.6～0.8 秒、日誌以外約 0.6～0.7 秒，這兩個數字沒有寫進去；它們只是建議寫法裡的補充，不列為缺漏。
    - SHOULD FIX 2（決定把 TBD-5 改成追蹤：無並補上影響）：已修好。README.md:63 的狀態欄有結論、日期與使用者選項原話，追蹤寫「追蹤：無」，理由是「照每次 kit 發版的例行做法在 main 執行 init-project.py --update，不需另外追蹤」。這個理由具體、與交付範圍相關：main 上確實有一串 `chore: 升級 dev-rules-kit 至 vX`、`docs: 更新 dev-rules-kit 核心至 vX` 的提交（809f824～3c7630b），project.md 也規定規則檔類提交直接放 main，符合 docs/AGENTS.md:139 對「不需後續處理時」的要求。影響欄改成未確認時的影響（main 帶著未發版的流程契約 2.7，且與盤點文件併在同一個 squash），並保留關聯編號。dev-rules-kit #11 仍是 OPEN，最新正式版是 v5.2.0，上游還沒發版，所以「發版後若不同就 --update」的條件還沒觸發。這列沒有 `<`，行首不是粗體或 `[`。判定通過。
    - NICE TO HAVE 3（決定修正）：已修好。inventory.md:224 補上掛載與執行路徑；v2-probe.sh:18 的註解改成「只看狀態碼的 compose healthcheck 會誤判」；v2-probe.sh:3 的用法改成通用的容器名稱並註明要掛 /scripts。
    - NICE TO HAVE 4（決定修正）：已修好。USERNAME 與最大 session 的讀回都納入了腳本，inventory.md:217 的說明也跟著改；固定容器名稱與沒有清理的問題改成帶 PID 的名稱加 trap；boot-cost.sh 加上 30 秒逾時。以上都經過我重跑或反向對照驗證。殘留問題見 NICE TO HAVE 2（最新與最大的說法不一致）。
  - 增量範圍內每個變動檔案的結論：
    - docs/issues/issue-0030/README.md：4b63d65 把 `**現行 review**` 改成 review-5f2c2d6.md，位置仍在 `## 概述` 之後，符合 review-evidence.md 對保存報告提交的要求。b88a4c4 改了 TBD-5（見上），並新增 TBD-6。TBD-6 的狀態是 `已解決`，寫了結論、日期與使用者決定，沒有 `<`，行首不是粗體或 `[`。它的影響欄只寫 SCN-001；步驟 6 的根因排序在計畫裡屬於 SCN-002，嚴格說應寫 SCN-001、SCN-002。不過這項已解決，改不改都不影響判定，也不另列發現。`**狀態**` 維持「待審查」，只寫階段。判定通過。
    - docs/issues/issue-0030/implementation-plan.md：:17 的摘要與 inventory 步驟 5 一致，任務狀態不變。判定通過，附 SHOULD FIX 1（同一組數字的限定）。
    - docs/issues/issue-0030/inventory.md：:217、:224、:281、:308、:320 的修改都查核過，數字與我重算或重跑的結果相符。判定通過，附 SHOULD FIX 1 與 NICE TO HAVE 2。
    - docs/issues/issue-0030/review-5f2c2d6.md：4b63d65 新增，b88a4c4 沒有修改（`git diff 4b63d65 b88a4c4 --name-status` 不含它），保留作為歷史。判定通過。
    - docs/issues/issue-0030/scripts/boot-cost.sh：重跑結果相符，逾時的反向對照有效。判定通過。
    - docs/issues/issue-0030/scripts/v2-migration.sh：重跑的輸出與保存的紀錄逐行相同，失敗時的清理有效。判定通過，附 NICE TO HAVE 2。
    - docs/issues/issue-0030/scripts/v2-probe.sh：只改了註解，`sh -n` 通過，行為不變。判定通過。
  - 重新查核的項目與理由：
    - 步驟 6 的假設表、根因排序與 no-go 理由：建議 1 的修正改了第 1 名的依據，所以重新核對三者與步驟 5 的數字是否一致，以及排序是否改變。結果：一致，排序與 no-go 不變。
    - 啟動成本與 LLM step 加起來是否合理（未改動的 :281 前半與 :282）：以第 3 回合為例，啟動 1.94 秒加上 LLM step 6.64 秒，剩下約 1.5 秒，與上一份報告量到的程序之間 0.81 秒、日誌以外 0.66 秒吻合。兩組數字沒有互相矛盾。
    - inventory.md:205 與 :242 的既有結論：腳本新增的檢查第一次真正驗證了它們，所以用輸出與副本查詢重新核對。結果：相符。
    - 交接紀錄 docs/2026-11-11-runtime-performance-handover.md：增量沒有改它；它的追補段落只連到 README，沒有重述被修正的數字，不受影響。
    - TBD-5 的上游狀態：理由依賴 kit 發版的時點，所以查了 dev-rules-kit #11 與 release 清單（見上）。
  - 沿用的結論（增量沒有改動它們，也沒有改它們的前提）：
    - 步驟 1 的主機、映像 digest、版本矩陣、副本 sha256 與 WAL 自檢。
    - 步驟 2 的盤點數字、對照組與 sed 反向自檢、table_bytes 驗算。
    - 步驟 3 的查詢計畫、冷暖耗時與召回 p50／p95／max。
    - 步驟 4 的 API 差異矩陣、健康檢查（gateway 與 compose）的判讀、v2-probe 的探測結果與錯誤密碼的負向對照。
    - 步驟 5 的 CPU、throttling、記憶體、DB 增量表格、成功判準，以及 v0.25.1 tool 失敗是上游限流的判讀。
    - bd001e6 的七個 sha256 與 docs/_templates/ 的處理。
    - Gate 豁免紀錄，以及 TBD-1～4 的格式與語意。
    - 安全與架構的結論。
- 豁免、待確認與限制
  - Gate 豁免紀錄（README.md:66-70）：增量沒有改動，沿用上一份報告的判定，符合格式與語意。
  - 待確認事項：TBD-1～6 都已有結論，沒有 `待確認`，所以沒有會阻擋交付的未知。TBD-5 的「PR 說明與 squash 訊息寫明內含候選版」要到建立 PR 時才能驗證，本次審查無法確認。
  - 限制：沒有重跑 bench-turn.sh 與 v2-probe.sh，因為它們會呼叫模型。步驟 5 的 LLM step 數字是用實作者留在 scratchpad 的日誌重算的，那些日誌不在 repo 裡，保存不長久。我的重跑輸出放在 session scratchpad 的 review-b88a4c4 目錄（mig-run.txt、mig-fail.txt、boot-run.txt、boot-timeout.txt），同樣不在 repo 裡。

## 流程判定
PASS

理由：沒有 MUST FIX。上一份報告的四項建議，使用者決定要修的部分都經過我重算或重跑：建議 2、3、4 已修好，建議 1 的數字已補上，但少了「LLM step 是上游耗時的上限」這句限定（SHOULD FIX 1）。這個缺口不會改變根因排序與 no-go，屬於證據的精確度，不阻擋判定。NICE TO HAVE 2 只影響換一份副本重跑時的對照。兩項都要改 inventory.md 或腳本，不屬於記帳區段：修了就要重新審查，不修就在 PR 揭露 SHOULD FIX 1 的限定。

PASS 只適用於上列 Reviewed HEAD SHA。之後在這個分支提交 issue README 記帳區段與本報告以外的檔案（例如交接文件），PASS 會失效、需要重新審查（流程契約 2.3 以下只有單獨新增本報告的提交可沿用）；提交前先告知使用者，由使用者決定做法。
