# 審查報告
- 範圍：完整交付範圍（增量覆審）
- Reviewed BASE SHA：b26c7958b11e0f75859d53d5d9901c08b9b5c74e
- Reviewed HEAD SHA：286df07c4257e6b6c01d7729d7b8c23f1ef8efb5
- Reviewed patch-id：56e1b5d7c3c0316943be9e1625c95b293335be1c（2026-11-23 以 `git diff b26c795 286df07 | git patch-id --stable` 重算，相符）
- 獨立 reviewer：Claude Code 隔離 subagent（模型 Opus 5.5，claude-opus-5-5），不是本 issue 的實作者
- Review artifact：docs/issues/issue-0030/review-286df07.md
- 增量覆審：上一份報告 review-ace1a3a.md，增量範圍 `ace1a3a..286df07`

## 問題與風險

- MUST FIX：無。
- SHOULD FIX：無。
- NICE TO HAVE：無。

## 已查核維度

- 驗收與證據
  - 增量覆審的前提：我自己核對過。HEAD 是 286df07c4257e6b6c01d7729d7b8c23f1ef8efb5；`git merge-base --is-ancestor ace1a3a HEAD` 成立；`ace1a3a..286df07` 只有 fe3cacf、286df07 兩個提交，各只有一個 parent，沒有 merge；reflog 從 ace1a3a 起只有 commit，沒有 reset 或 rebase；`git merge-base main HEAD` 與 main、origin/main 都是 b26c7958b11e0f75859d53d5d9901c08b9b5c74e，等於上一份報告的 Reviewed BASE。上一份報告綁定 H1 = ace1a3a、記完整範圍、判定 PASS，符合 review-evidence.md「增量覆審」的三項條件。工作區乾淨。
  - PR 版本（註記，不屬於本次判定）：`gh pr view 43` 的 headRefOid 與 `origin/issue-0030-runtime-inventory` 都還是 f35f895，fe3cacf、286df07 尚未推送。這份報告審的是本地的 286df07；推送後要核對 PR 的 source HEAD 等於 286df07（或之後只有記帳提交），才能拿本報告當 PR 的審查證據。
  - 核准基線：增量沒有動到 Gherkin、核准紀錄與 SCN 狀態表。核准 commit 仍是 83d3670，SCN-001、SCN-002 仍是已核准，沿用上一份報告的推導。
  - SCN-001 Then「報告包含來源主機、日期、聚合數值與限制」：步驟 5 的資料庫證據改正後與我直接查得的結果一致（見下方「相關失敗面」），步驟 6 那一列補上量測範圍的限定，數字都沒有變。判定有證據。
  - SCN-002 Then「產出升級或暫緩的證據及回復方案」：增量沒有改 go/no-go 與回復方案；no-go 的效能理由是上游模型，不依賴資料庫成本或 bench.txt 的判準，不受影響。判定有證據。
- 相關失敗面
  - 上一份報告的三項建議（使用者決定記在 README TBD-9：三項都修正，再審一輪），逐項確認：
    - SHOULD FIX 1（DB 證據的回合對應與「session 輪替刪除」）：已修好。inventory.md:283 改成「三個回合都在同一個 session，build 在 11:08:35、11:08:50、11:09:08（台北時間）各有一次對 /workspace/bench.txt 的 write 與 read，都是 completed，每個回合一次」，刪掉了輪替刪除的說法，並註明 bench 副本帶著未 checkpoint 的 -wal，要連同 -wal 複製後再開，用 immutable=1 只開主檔會漏掉最後一個回合。我自己的查核見下一點，每一項說法都相符。
    - SHOULD FIX 2（bench-turn.sh 無法逐回合判定）：已修好。bench-turn.sh:35-38 在回合結束、印 JSON 之前讀 /workspace/bench.txt，內容是 hello 時 bench_file=ok，否則 missing，short 任務固定為 n/a；:39 的 JSON 多了 `bench_file` 字串欄位；:7 的註解與 inventory.md:283 的成功判準同步寫明這個欄位。檢查在 t1 之後，不計入 turn_ms；回合前的 `rm -f` 保留，所以上一回合或先前留下的檔案不會讓沒寫入的回合判成 ok。驗證方式見下方「bench-turn.sh 的逐回合判定」。
    - NICE TO HAVE 3（步驟 6 長期 session 那列的限定）：已修好。inventory.md:309 改成「在目前規模與這組測試查詢下，讀取最大 session 的 SQL 約 15 ms，不構成瓶頸；讀出後的反序列化與事件處理沒有單獨量」，與 :187、:314、implementation-plan.md:13 一致。
  - DB 證據的查核方法與結果：只讀 bench 副本 scratchpad/bench/v0.23.0-tool/opencode/，把 opencode.db 與 opencode.db-wal（1,310,192 byte）一起複製到我的暫存目錄再開，integrity_check 為 ok；另複製一份只有主檔的用 immutable=1 開，作為對照。查核前後 bench 副本的 sha256 不變（opencode.db b2ec0fb4…c0cb，-wal 9f85ec4b…8f27，與上一份報告記錄的相同）。
    - 連同 WAL：bench 期間只有一個 session（ses_df99…，directory /workspace），建立於 11:08:30、time_updated 11:09:16；三則 user 訊息在 11:08:31、11:08:46、11:09:05，agent 都是 build；build 的 write 在 11:08:35、11:08:50、11:09:08，內容都是 hello，各接一次 read（11:08:37、11:08:52、11:09:12），六筆 tool part 都是 completed。
    - 只開主檔（immutable=1）：time_updated 停在 11:08:56，只有前兩則 user 訊息與前兩組 write／read，第三個回合查不到。inventory.md:283「用 immutable=1 只開主檔會漏掉最後一個回合」屬實。
    - 回合邊界：logs/out-1～3.txt 的修改時間是 11:08:39.9、11:08:56.3、11:09:16.3，減去 results2.jsonl 的 turn_ms（15,032、16,363、19,994）得到三個回合約 11:08:24.9～39.9、11:08:39.9～56.3、11:08:56.3～09:16.3，三次寫入各落在一個回合裡，離邊界至少 4 秒。「每個回合一次」成立。
    - 旁證：workspace/bench.txt 的修改時間 11:09:08、內容 hello，與第三次寫入相符。
  - bench-turn.sh 的逐回合判定：不呼叫模型，用模擬驗證。我用 Python 從 HEAD 的 bench-turn.sh 擷取 `bash -c '…'` 裡的原文（沒有改寫），在 `--network none`、與腳本相同限制（1.5 CPU、3 GiB、256 PID）的一次性容器裡執行；映像用本機已有的 ghcr.io/raybird/wukong:v0.25.1，但 docker-entrypoint.sh 以空殼掛載覆蓋，PATH 最前面放 stub 版的 gosu（直接 exec 後面的命令）與 wukong（依情境決定這一回合寫不寫 bench.txt、回傳什麼 rc），所以真的 wukong 與 opencode 都沒有執行。每種情境 3 回合，每行輸出都用 json.loads 解析成功，13 個欄位：
    - 三回合都寫 hello：ok、ok、ok。
    - 第 2 回合不寫：ok、missing、ok。上一份報告的模擬裡，這個情境與「三回合都寫」的輸出完全相同，現在分得出來。
    - workspace 事先放好內容為 hello 的 bench.txt、第 1 回合不寫：missing、ok、ok。回合前的刪檔仍有效。
    - 第 2 回合寫成 hello world：ok、missing、ok。
    - 第 1 回合寫 hello 但 rc=1、第 2 回合不寫且 rc=3：rc 1／3／0，bench_file ok／missing／ok；rc 與 bench_file 各自記錄，判準是兩者都要成立，與 inventory.md:283 一致。
    - short 任務（workspace 事先有 bench.txt）：三回合都是 n/a。
    - 這組情境的期望值是依情境設定手推的，不是重算腳本的判斷式，有可能與腳本不一致。
  - 增量揭露的事實對其他內容的影響：「帶 WAL 的檔案用 immutable=1 會漏資料」上一份報告已在整個 HEAD 查過（inventory.py、queries.py 讀的 copy-20261123 沒有 -wal 檔，這次再確認一次仍然沒有；wal-check.py 刻意只開主檔；v2-migration.sh 讀副本的再複製），這次的增量沒有改這些腳本，結論沿用。「session 輪替刪除」的錯誤說法已從 inventory.md 刪掉；HEAD 裡其他提到輪替的地方（docs/entrypoints.md:10、docs/docker.md:207、:224、:246、交接文件 :18）講的是 Wukong 的長期 session 輪替功能，不是這次 bench 的說法，不受影響。
  - 正式資料被寫入：本次審查沒有讀或掛載 ~/Documents/RunWuKong 與它的 volume，只讀 scratchpad 的 bench 副本，查詢都在它的複本上做。
  - 額度用完污染正式部署：本次審查沒有呼叫任何模型，也沒有跑 bench-turn.sh 本體或 v2-probe.sh；模擬容器沒有網路，wukong 是 stub。
- 需求、架構、安全、品質
  - 需求：增量只有 issue 目錄下的文件與一支取數腳本，沒有產品程式碼，與「涉及檔案」相符。
  - 架構：不適用。
  - 安全與隱私：增量的新增行沒有 token、email 或對話內容；inventory.md 沒有寫入 session id。本報告引用的 session id 前綴屬於 bench 副本的合成回合，prompt 是固定的合成內容。判定通過。
  - 品質：bench-turn.sh `bash -n` 通過；新加的 `bench_file` 值只有 ok、missing、n/a，不含空白或引號，不加引號傳給 printf 也不會破壞 JSON。模擬時每回合 stderr 有一行 memory.peak 唯讀的錯誤訊息，那是 :27 的既有寫法（`2>/dev/null` 寫在 `>` 之後，攔不到重導向失敗），不是這次的改動，inventory.md:280 也已寫明 cgroup 唯讀，不列為問題。
- 增量覆審
  - 增量範圍內每個變動檔案的結論：
    - docs/issues/issue-0030/README.md：fe3cacf 把 `**現行 review**` 改成 review-ace1a3a.md，位置仍在 `## 概述` 之後的 `## 現況`；286df07 新增 TBD-9。TBD-9 的狀態是「已解決」，有日期與使用者的選擇；新增行沒有 `<`，以 `|` 開頭；影響欄寫「影響步驟 5 的證據與步驟 6 的判定措辭；SCN-001」，與實際改動的範圍相符（inventory.md:283、:309）。`**狀態**` 仍是「待審查」，只寫階段，增量沒有改。判定通過。
    - docs/issues/issue-0030/inventory.md：:283 的 DB 證據與 bench_file 判準、:309 的限定，查核結果見上方，判定通過。
    - docs/issues/issue-0030/review-ace1a3a.md：fe3cacf 新增，286df07 沒有修改（`git diff --name-status fe3cacf 286df07` 不含它），保留作為歷史。判定通過。
    - docs/issues/issue-0030/scripts/bench-turn.sh：逐回合檢查與 JSON 欄位正確，其他量測欄位與計算沒有改動。判定通過。
  - 重新查核的項目與理由：
    - 步驟 5「v0.23.0 tool 3 次成功」與成功回合的耗時（未改動）：DB 證據的說法改了，所以重新從資料庫確認三個回合各寫入並讀回一次。判定維持成功，步驟 5 的數字與根因排序不受影響。
    - 步驟 5 的表格與 results2.jsonl：bench-turn.sh 的 JSON 格式改了，所以確認已記錄的數字不依賴新欄位。results2.jsonl 是改版前產生的，沒有 bench_file 欄位；inventory.md:283 寫明「2026-11-23 起」才有，沒有宣稱舊資料有這個欄位。HEAD 裡沒有其他程式或文件解析這份 JSON 的欄位順序（boot-cost.sh 只沿用 RUN_DIR 結構）。
    - 步驟 6 與資料庫成本相關的各列（:297、:309、:314、:322）：:309 改了措辭，重新核對同一個量的其他寫法。:297 在「沒有量測的項目」表裡，殘餘風險欄已寫明上游處理長上下文的耗時未知；:314、:322 上一份報告已確認。no-go（:332）不受影響。
    - 「immutable=1」與「輪替」的其他出處：理由與結果見上方「增量揭露的事實對其他內容的影響」。
  - 沿用的結論（增量沒有改動它們，也沒有改它們的前提）：
    - 步驟 1 的主機、映像 digest、版本矩陣、副本 sha256，WAL 涵蓋性自檢的數字、措辭與一致性的依據（Backup API 與 integrity_check）。
    - 步驟 2 的盤點數字、對照組與 sed 反向自檢、table_bytes 驗算。
    - 步驟 3 的查詢計畫、冷暖耗時、召回 p50／p95／max，以及 :187 的限定。
    - 步驟 4 的 API 差異矩陣、健康檢查的判讀、v2-probe 與 v2-migration.sh 的結果。
    - 步驟 5 的 LLM step 數字與「上游耗時的上限」限定、啟動成本、CPU、throttling、記憶體、DB 增量，以及 v0.25.1 tool 失敗是上游限流的判讀。
    - boot-cost.sh、v2-probe.sh、v2-migration.sh、inventory.py、queries.py、wal-check.py、snapshot.py：增量沒有改這些腳本。
    - bd001e6 的七個 sha256 與 docs/_templates/ 的處理。
    - implementation-plan.md、交接文件 docs/2026-11-11-runtime-performance-handover.md：增量沒有改。
    - Gate 豁免紀錄，以及 TBD-1～8 的格式與語意。
    - 安全與架構的結論。
- 豁免、待確認與限制
  - Gate 豁免紀錄（README.md:69-73）：增量沒有改動，沿用上一份報告的判定。
  - 待確認事項：TBD-1～9 都有結論，沒有 `待確認`，沒有會阻擋交付的未知。TBD-5 的「PR 說明與 squash 訊息寫明內含候選版」要到合併時才能驗證，本次審查無法確認。
  - 限制：沒有重跑 bench-turn.sh 與 v2-probe.sh，因為它們會呼叫模型；bench-turn.sh 的新判定只用 stub 模擬驗證，真實回合裡 opencode 寫檔的行為沒有在新腳本下重測。回合邊界是用 out-N.txt 的修改時間與 turn_ms 推算的，誤差在 1 秒以內，不影響對應。我的資料庫複本與模擬腳本放在 session scratchpad 的 review-286df07 目錄（withwal/、mainonly/、sim/），不在 repo 裡，保存不長久。

## 流程判定
PASS

理由：沒有 MUST FIX，也沒有新的建議。上一份報告的三項建議，使用者決定都修，三項都已修好：DB 證據改成連同 WAL 查詢的結果，回合對應與我直接查得的完全一致，錯誤的「session 輪替刪除」已刪除；bench-turn.sh 每回合結束後檢查 bench.txt 並寫進 `bench_file` 欄位，模擬顯示能分辨中間某一回合沒寫入、寫錯內容與舊檔殘留，JSON 格式正確；步驟 6 長期 session 那列補上了量測範圍的限定。步驟 5 的數字與 no-go 都不受影響。

PASS 只適用於上列 Reviewed HEAD SHA。之後在這個分支提交 issue README 記帳區段與本報告以外的檔案（例如交接文件），PASS 會失效、需要重新審查（流程契約 2.3 以下只有單獨新增本報告的提交可沿用）；提交前先告知使用者，由使用者決定做法。
