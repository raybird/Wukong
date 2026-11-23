# 審查報告
- 範圍：完整交付範圍
- Reviewed BASE SHA：b26c7958b11e0f75859d53d5d9901c08b9b5c74e
- Reviewed HEAD SHA：4be256ce637df8c09ade99713020cab424bcfb5d
- Reviewed patch-id：7db774b4b45f05e9f4e6fb25ee5ad9c8b2d67c2e（2026-11-23 以 `git diff b26c795 4be256c | git patch-id --stable` 重算，相符）
- 獨立 reviewer：Claude Code 隔離 subagent（模型 Opus 5.5，claude-opus-5-5），不是本 issue 的實作者
- Review artifact：docs/issues/issue-0030/review-4be256c.md

## 問題與風險

- MUST FIX 1：按需啟動 opencode 的成本少算一半，根因排序第 2 項與「成立但小」的判定跟引用的日誌對不上。
  - 位置：docs/issues/issue-0030/inventory.md:269、:294、:296、:307；docs/issues/issue-0030/implementation-plan.md:17、:19。
  - 證據：步驟 5 v0.25.1 short 的 opencode 日誌（bench 副本的 opencode/log/opencode.log）裡，每個回合有兩個不同的 `run=` 程序，plan 一個、build 一個，各自重新載入設定、建立 instance，從啟動到 `message=stream` 分別是 1.250／0.947 s、1.048／0.968 s、0.948／0.989 s。所以兩次呼叫上游各自付一次啟動成本，每回合只算 opencode 啟動就約 1.9～2.2 s。以第 3 回合（10,063 ms）來看約占 19%，還沒算 Wukong 在 spawn 之前的時間，也沒算 plan 程序結束到 build 啟動之間的空檔。同一張表的 CPU 每回合 5.3～5.6 CPU 秒，在 1.5 CPU 上限下等於至少 3.5 s 的 1.5 核計算量；文件沒有解釋這和「本機開銷約 1 秒」怎麼兜得起來。日誌裡也沒有 stream 結束的紀錄，「其餘時間都在等上游」從這份日誌推不出來。
  - 影響：步驟 6 是本 issue 的主要交付（根因排序）。第 1、2 名的順序大致不變，但「每回合約 1 秒」「成立但小」低估了按需啟動模式的成本，之後評估要不要改用常駐 server 時會被誤導。
  - 建議：用既有的 bench 日誌重算，不必再呼叫模型。改成「每回合啟動兩次 opencode（plan、build），各約 1 秒，合計約 2 秒」，並交代 CPU 秒數的落差；「其餘都在等上游」改成可以從日誌證明的範圍，或列為限制。同步改 plan 第 17、19 行的摘要。

- SHOULD FIX 2：「現有健康檢查會判成功（假綠燈）」不符合 Wukong 的程式碼。
  - 位置：docs/issues/issue-0030/inventory.md:202、:249、:316、:324；implementation-plan.md:15。
  - 證據：Wukong 的 `health_check` 走 `send_json`（crates/wukong-gateway/src/opencode_server.rs:178-182、:450-471），狀態 200 之後還會解析 JSON；收到 SPA 的 HTML 會回 `returned invalid JSON` 錯誤，不會判成功。真的只看狀態碼的是 compose 的 healthcheck `curl -fsS http://localhost:4096/global/health`（docker-compose.yml:100、docker-compose.release.yml:80，選用的 opencode-server 容器），而且它不帶 basic auth；探測腳本是帶認證打 V1 路徑（docs/issues/issue-0030/scripts/v2-probe.sh:19-20），所以 V2 的 SPA 在沒有認證時會不會回 200，現有證據也沒量到。
  - 影響：不相容的結論與 no-go 不變，但會把阻礙的性質寫錯：gateway 這一層會明確失敗，可能假綠燈的是 compose 容器的健康狀態。將來改 adapter 時，第 324 行的前提 2 會指錯地方。
  - 建議：改成「gateway 的健康檢查會因為回應不是 JSON 而失敗；compose healthcheck 只看狀態碼，在 server 模式下可能顯示 healthy（未帶認證的行為未量測）」，前提 2 補上 compose healthcheck。

- SHOULD FIX 3：遷移與回復的紀錄有一處跟探測自己的產物矛盾，另外有一部分沒有留下可重跑的命令。
  - 位置：docs/issues/issue-0030/inventory.md:187、:199-201、:229-232；docs/issues/issue-0030/scripts/v2-probe.sh:2-3。
  - 證據：我比對了一致性副本與遷移探測後的資料庫（v2/probe-a，依序跑過 V2→V1→V2→V1）。migration 從 38 筆變成 62 筆，與「24 個 migration」相符；但 V2 刪掉了 V1 的 data_migration、session_context_epoch、session_input 三張表（這份副本裡三張都是 0 筆），和第 229 行「V1 的表保留」不符。session_message 在 V1 1.19.6 的副本裡本來就有，不是 V2 新增的。
  - 證據：V2 容器怎麼啟動、遷移與回復的四個動作、啟動訊息、認證行為、/api/info、/openapi.json 的 142 條路徑，都只有文字記載、沒有命令，v2-probe.sh 也只寫「在跑著 opencode serve 的隔離容器裡執行」。verification.md 對觀測式證據要求記錄命令或查詢原文。
  - 影響：回復方案本來就是還原備份，不受影響。但「不還原、只換回 V1」的風險比文件寫的還大：V1 少了三張表，探測沒跑到的 V1 功能可能出錯。之後的升級 issue 也沒辦法照著重做這個實驗。
  - 建議：把「V1 的表保留」改成實際結果（刪掉三張表，這份副本裡都是空表），刪掉 session_message 的「新增」；把 V2 容器的啟動命令與遷移／回復的四步命令補進 inventory.md 或腳本。

- SHOULD FIX 4：「壓縮、換模型都可用」說過頭了。
  - 位置：docs/issues/issue-0030/inventory.md:207-208、:221、:249；docs/issues/issue-0030/scripts/v2-probe.sh:66-70。
  - 證據：換模型是切到 V2 原本就在用的同一個預設模型（opencode/big-pickle），回 204 也分不出到底有沒有切換；壓縮只看了 HTTP 200，沒有確認真的壓縮了（文件自己也寫「排入下一個 step 邊界」）。
  - 影響：不影響 no-go，但將來改 adapter 時會被當成已驗證的契約。
  - 建議：改成「端點存在且接受請求，實際切換與壓縮結果未驗證」，或下次有額度時改切到另一個模型、確認出現 compaction 事件。

- SHOULD FIX 5（`需決策`）：提交 bd001e6（dev-rules-kit 候選版 da632e9，文件版本 2.11→2.15、流程契約 2.5→2.7）會隨這個 PR 進 main。
  - 位置：docs/.dev-rules-kit.json、docs/AGENTS.md:149、:196-197、docs/agents/*.md。
  - 已確認的部分：七個檔的 sha256 都和清單相符；BASE 上這些檔都是未經本地改寫的上游版本，所以沒有把專案的客製蓋掉。docs/_templates/ 不再列入清單，但檔案還在，project.md 也沒有引用它，與上游 2.12 的說明一致。
  - 影響：project.md:15 規定 squash 合併，所以 kit 升級會和 #30 的盤點文件併成同一個 squash commit，標題是 issue-0030，之後很難單獨回退或找到它。main 也會帶著上游還沒發版的流程契約 2.7；如果其他 session 用的是已發版的技能（v5.2.0），對「`已完成` 在合併前寫好」的判讀會和 2.7 不一致。正式版若又改了內容，main 上會留一份只存在於候選版的規範。
  - 待決問題：候選版要不要跟著 #30 進 main？
  - 可選做法：
    - (a) 照現狀合併，在 PR 說明寫明內含 kit 候選版。代價是 kit 升級與 issue 混在同一個 squash。
    - (b) 從分支拿掉 bd001e6，等上游發版後依 project.md「規則檔類提交直接放 main」，用 `init-project.py --update` 單獨提交。代價是 HEAD 改變、要重新審查，而且這份報告依據的 2.7 規範也要改回 2.5 重審。
    - (c) 合併時不用 squash。這違反 project.md 的固定做法。
  - 建議：選 (a)，在 PR 說明與 squash 訊息裡明列 bd001e6 的內容，因為這是使用者刻意放的試用版。上游發版後如果和 da632e9 有差異，再到 main 跑一次 `--update`。若使用者希望 main 的歷史乾淨，就選 (b)。

- NICE TO HAVE 6：docs/issues/issue-0030/inventory.md:183 拿 raybird-desktop 的逾時任務（1,218～2,390 秒）當比較基準，還寫「差了五個數量級以上」。最慢的查詢約 16 ms，1,218 s 除以 16 ms 約 7.6 萬倍，大約 4.9 個數量級。這段沒有把那台的觀測當成 flashaim 的量測，但同一份報告裡就有 flashaim 自己的回合耗時（步驟 5，10～25 s），用那個當基準更直接，也不會跨主機比較。

- NICE TO HAVE 7：取數腳本的對照組沒有涵蓋全部輸出。
  - docs/issues/issue-0030/scripts/inventory.py:34-42 的 `table_bytes`（66%／32% 的來源）不在對照組的比對範圍內。我另外驗算過：各表 byte 加總 42,057,728，page_count×page_size 是 42,078,208；memory.db 加上 freelist 也對得上，數字可信。
  - docs/issues/issue-0030/scripts/wal-check.py:33-38 只印數字、不判 PASS／FAIL，結果要人自己看。
  - 三種改壞版本的反向自檢沒有留下腳本。
  - bench 的成功判準（rc=0、輸出內容、bench.txt）沒有寫進 inventory.md；我確認過 v0.23.0 tool 的 workspace 有 bench.txt，內容是 hello，三次輸出相同，所以 W-7 的情況沒有發生。
  - 建議：把這些判準寫成腳本的斷言，或在 inventory.md 記一行。

- NICE TO HAVE 8：docs/issues/issue-0030/README.md:57 的 TBD-3 影響欄沒有關聯驗收編號（建議寫 SCN-002）。這欄是 BASE 時就有的文字，不影響判定。

## 已查核維度

- 驗收與證據
  - 核准基線：核准 commit 83d3670 由使用者 2026-11-23 的逐題確認核准。`git diff 83d3670 4be256c -- docs/issues/issue-0030/README.md` 的 Gherkin 與核准來源行完全沒變，只有文件清單、涉及檔案、核准 commit 回填、待確認事項、Gate 豁免紀錄、Timeline 與狀態有改動，所以原核准有效。依核准來源行推導（`git log -1 -G` 那道命令）也得到 83d3670，一致。核准表與現存的 SCN-001、SCN-002 完全相等，都是已核准。
  - SCN-001 Then「報告包含來源主機、日期、聚合數值與限制」：inventory.md 步驟 1～3 都有主機、取樣時間、數值與限制小節，判定有證據。我重查了 `hostnamectl` 是 flashaim；docker inspect 確認映像 ID、digest、WUKONG_AGENT_SERVER_URL 為空、1.5 CPU／3 GiB／256 PID 都相符。`git diff v0.25.1 bd001e6 -- . ':!docs'` 為空，確認「產品程式碼與 v0.25.1 相同」。`git show v0.25.1:release/runtime-inputs.env` 是 1.17.18，`npm view opencode-ai version` 是 1.19.6（2026-11-17 發布），都與文件相符。
  - SCN-001 Then「無法取得的資料列為未知而非健康」：vectors.db 的 integrity 與內容、raybird-desktop 的資料規模、寫入競爭、v0.23.0 的分階段耗時都標成未知；inventory.py 查不到的表回 unknown，有對照組涵蓋。判定有證據。
  - SCN-001 Then「raybird-desktop 的觀測不當成 flashaim 的量測結果」：版本矩陣把兩台分欄；步驟 3 與步驟 6 都明說結論不推到那台；v0.23.0 的對照是在 flashaim 上跑的，標成「raybird-desktop 當時的版本」，沒有拿來推論那台的根因。判定有證據，只有 NICE TO HAVE 6 的比較基準可以改進。
  - SCN-002 Then「產出升級或暫緩的證據及回復方案」：步驟 6 的 no-go 有四點理由、五項前提，回復方案是還原備份。判定有證據；MUST FIX 1 只影響根因排序的數字，不影響 go/no-go。
  - SCN-002 Then「不相容或測試失敗會明列為阻礙」：步驟 4 的對照表與步驟 6 的理由都有列出。判定有證據，但其中一項阻礙的性質寫錯（SHOULD FIX 2）。
  - 計畫各步完成判準：
    - 步驟 1：主機、digest、binary 版本與原始碼分列；副本以 `:ro` 掛載、`mode=ro` 開啟並加 `--network none`，副本 sha256 與文件相符（opencode.db 是 2d2f3518…）。判定符合。
    - 步驟 2：我在 `docker run --rm --network none` 的一次性容器裡，以 `:ro` 掛載副本重跑 `inventory.py control`，結果 PASS、exit 0；重跑盤點的輸出與實作者保存的 inv-20261123.jsonl 逐位元組相同，表格數字（event 14,033 筆、27,701,248 byte、tool part 占 80.65% 等）全部對上。判定符合。
    - 步驟 3：同樣條件加上 `--cpus 1.5 --memory 3g` 重跑 queries.py，查詢計畫完全相同，耗時在雜訊範圍內（例如 events_of_session 冷 17.8 ms、暖 16.5 ms）。recall p50／p95／max 是 7／32／42，相符。判定符合。
    - 步驟 4：差異矩陣與 v2-probe 的兩次輸出（scratchpad 的 probe-run1／probe-run2）和表格相符；第一次的取消判定太寬，文件已揭露並修正。但有 SHOULD FIX 2～4。判定大致符合。
    - 步驟 5：bench 的 results2.jsonl 與表格逐欄相符（CPU、throttled、記憶體峰值、增量），平均 8.1 對 5.4 CPU 秒、少約 33% 都算得出來。缺漏的項目已記入 Gate 豁免紀錄。分階段耗時的解讀有 MUST FIX 1。
    - 步驟 6：產出齊備，但第 2 名的量化與「成立但小」要依 MUST FIX 1 修正。
- 相關失敗面
  - 副本沒有包含 WAL：若副本只抄到主檔，筆數會等於「只有主檔」的值。wal-check 的三欄（2,295／2,295／2,226、19,422／19,422／19,293）能區分這兩種情況。腳本沒有斷言，是人工判讀（NICE TO HAVE 7）。判定已覆蓋。
  - 取數空結果的假綠燈：查不到的表要回 unknown 而不是 0，前幾名不能少算第一名，「不屬於 session 的 event」條件不能寫反。對照組逐項比對，我照程式邏輯推演三種改壞版本都會 FAIL。`table_bytes` 不在對照組內，我用 page_count 交叉驗算過。判定已覆蓋。
  - 正式資料被寫入或遷移：所有讀取正式 volume 的命令都是 `:ro`，加上 `mode=ro` 或 `immutable=1`；bench 與 V2 只掛副本。docker inspect 顯示三個容器的 StartedAt 都是 2026-11-23T00:54:38Z，RestartCount 都是 0，早於取樣（02:37）。判定已覆蓋，正式部署維持唯讀。
  - V2 探測在請求失敗時假綠燈：權限項目要同時看到 permission.asked 與 execution.succeeded，取消要 interrupted=true 加上對應事件；錯誤密碼的負向對照已記載（輸出沒有保存在 repo）。換模型與壓縮只看狀態碼（SHOULD FIX 4）。判定部分覆蓋。
  - 回合成功是假的（W-7）：v0.23.0 tool 的 rc=0、輸出內容、workspace 的 bench.txt 都在；v0.25.1 tool 的失敗明確標出上游限流。判定已覆蓋。
  - 以回滾代替還原：三入口共用同一份 opencode-state，V2 就地單向遷移，回復要還原備份，文件已寫明；但「V1 的表保留」不對（SHOULD FIX 3）。判定回復方案正確、細節要修。
  - 額度用完污染正式部署：文件記錄發現額度用完後停掉所有會呼叫模型的量測，也查了正式 schedulerd 的日誌。本次審查沒有呼叫任何模型。判定已揭露。
- 需求、架構、安全、品質
  - 需求：交付只有文件與 scripts/ 下的取數腳本，沒有產品程式碼，與「涉及檔案」相符；交接紀錄只在檔尾加了「追補：2026-11-23」並連到 README，既有結論沒有改寫，符合 project.md 常青文件表最後一列與 AGENTS.md 工作流程第 6 項。
  - 架構：不適用，沒有 crate 或相依方向的變更。
  - 安全與隱私：我 grep 了新增內容，沒有 session id、token、密碼值、email 或對話內容。v2-probe.sh 的密碼由環境變數傳入，沒有寫進檔案；bench 的 prompt 是合成內容。主機名稱是規格要求的來源欄位。判定通過。
  - 品質：腳本都很小、各管一件事，沒有過度設計。README 待確認事項、plan 證據行與 inventory 步驟 6 的結論有摘要性重複；這是任務來源與報告之間的引用，沒有分歧，不構成問題（MUST FIX 1、SHOULD FIX 2 修正時要一起改 plan 的摘要）。
- 豁免、待確認與限制
  - Gate 豁免紀錄（README.md:60-64）：表格寫法，行首沒有粗體，沒有 `<`；有日期、明列的項目、使用者選項原話與殘餘風險。只豁免步驟 5 列出的四項量測；V2 經 Wukong 的對照依計畫記為不可比較，不需要豁免。判定符合格式與語意。
  - 待確認事項：TBD-1、TBD-2 是已解決，結論與日期寫在狀態欄。TBD-3、TBD-4 是不影響本次交付，都寫了「追蹤：無」，也附了與交付範圍有關的具體理由（V1 仍在維護、CI 不會自動升到改名後的 V2；那台機器連不到、改盤點 flashaim）。判定符合，只有 NICE TO HAVE 8。
  - `**狀態**` 是「待審查」，只寫階段，符合 AGENTS.md「有特殊意義的狀態值」。README 沒有 `## 現況`，流程契約 2.5 起是選用；保存本報告的提交依 review-evidence.md 建立。
  - 限制：v2-probe.sh 與 bench-turn.sh 會呼叫模型，依指示只讀程式碼、比對 scratchpad 裡實作者留下的輸出與日誌，沒有重跑。那些輸出在 scratchpad，不在 repo，持久力有限。

## 流程判定
RETURN TO execute-task

理由：MUST FIX 1。根因排序（計畫步驟 6 的交付）把每回合的按需啟動成本寫成約 1 秒、成立但小，但引用的日誌顯示每回合啟動兩次 opencode，合計約 2 秒，CPU 秒數的落差也沒有解釋。修正只要重讀既有日誌，不需要呼叫模型。SHOULD FIX 2～4 建議同一輪一起修；SHOULD FIX 5 由使用者決定。
