# 審查報告
- 範圍：完整交付範圍（TARGET main e6a0bf9；處理建議後的增量覆審）
- Reviewed BASE SHA：e6a0bf921fd12d65e5f95b2418eb2916b847b4d8
- Reviewed HEAD SHA：7df8450cd4f844d25714b401a67009b128b27075
- Reviewed patch-id：93242f5a0ba7316e8d74f7edf21c5f08617ee748
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，claude-opus-5-5）。不是實作者，也沒有參與修正
- Review artifact：docs/issues/issue-0044/review-7df8450.md
- 增量覆審：上一份報告 review-c4d47d8.md，增量範圍 c4d47d8..7df8450。這是處理上一份 PASS 所附建議之後的增量覆審
- 審查日期：2026-11-25
- 規格基線：沿用 review-c4d47d8.md（核准 commit 94aaa40）。增量沒有改動 Gherkin 劇本，README 的變動只在 `## 現況` 與 `## 待確認事項`
- 風險等級：High（沿用上一份報告）

## 增量覆審的前提

| 條件 | 核對 | 結果 |
|---|---|---|
| 上一份報告綁定 H1、記完整範圍 | review-c4d47d8.md 綁定 c4d47d8，範圍是完整交付，判定 PASS | 成立 |
| H1 是新 HEAD 的祖先，中間沒有改寫歷史 | `git log c4d47d8..7df8450` 只有 452dc22、7df8450，兩個都是一般提交 | 成立 |
| merge-base 沒有變 | `git merge-base main HEAD` = `git rev-parse main` = e6a0bf9 = 上一份報告的 BASE | 成立 |
| patch-id | `git diff e6a0bf9 7df8450 \| git patch-id --stable` = 93242f5a…，與指定值相同 | 成立 |

## 問題與風險

### MUST FIX
無

### SHOULD FIX
無

### NICE TO HAVE

**N-D. TBD-6 的狀態欄不是三個合法狀態值之一**
- 位置：`docs/issues/issue-0044/README.md` `## 待確認事項` TBD-6
- 現況：狀態欄寫「接受，不修（2026-11-25 使用者選「記入待確認」）：…」。docs/AGENTS.md「待確認事項」只允許 `待確認`、`已解決`、`不影響本次交付`；`不影響本次交付` 還要寫後續去向，或不需追蹤的理由
- 內容本身已經寫了不需追蹤的理由（只在尚未遷移的 V1 DB 上發生、重新整理就正常、資料完整），只是狀態值不合格式，懸置掃描可能把它當成未處理
- 建議：改成「不影響本次交付（2026-11-25 使用者選「記入待確認」）：追蹤：無，…」，後面接原本的理由
- 這一節是記帳區段，只改這一列不會讓本報告的 PASS 失效。新內容不要用 `<`、行首粗體或行首 `[`

**N-E. CHANGELOG 閒置重啟條目舉的例子，有兩個在 v0.25.1 不會發生**
- 位置：`CHANGELOG.md:35-37`
- 內容：「例如沒有密碼、密碼被拒、回應格式不符；以前這些情況跟「有 session 剛更新」印成同一句」
- 查核：`git grep -E 'OPENCODE_SERVER_PASSWORD|AGENT_SERVER_PASSWORD' v0.25.1 -- docker-compose*.yml scripts/docker-entrypoint.sh .env.example scripts/opencode-idle-restart.sh` 沒有輸出，v0.25.1 的 server 沒有密碼，supervisor 也不帶認證。所以「沒有密碼」「密碼被拒」是 V2 才有的情況；以前就會印成同一句的，只有回應看不懂與請求失敗（v0.25.1 `opencode-idle-restart.sh:79`、`:173`）
- 標題「問不到 session 列表時，log 會寫出原因」與「重啟的條件不變」都正確（`git diff v0.25.1 7df8450 -- scripts/opencode-idle-restart.sh` 只改了 `sessions_idle` 與那一行 log，判斷順序與條件沒有變）
- 建議：改成「例如回應看不懂、請求失敗，以及 V2 的密碼沒設或被拒」這類寫法。改 CHANGELOG 會讓本報告的 PASS 失效，可以併進發版時本來就要整理 CHANGELOG 的提交，或接受重審；影響只在措辭，不改也可以

## 已查核維度

### 上一份報告建議的處理確認

| 項目 | 使用者決定 | 本次查核 | 結論 |
|---|---|---|---|
| N-A 兩個 `--standalone` 同時第一次開啟 V1 DB | 記入待確認，不修 | README TBD-6 的事實與 review-c4d47d8.md 一致：錯誤訊息、`tokio::join!` 同時跑 `auth list` 與 `models`、只在尚未遷移的 V1 DB 上發生、遷移照常完成、session 都在、integrity ok、已遷移、全新 HOME、與 serve 同時啟動都正常。上一份報告另提到 CLI `run --standalone` 與 Web 診斷之間的同類競爭，TBD-6 沒有寫；那條條件更窄，不另列發現 | 處理方式符合決定；狀態值格式見 N-D |
| N-B CHANGELOG 寫成中間狀態 | 修正 | 兩條改寫成與 v0.25.1 的差異：`/providers` 改列 `auth list` 的輸出、格式不同（v0.25.1 `command.rs:111` 是 `providers list`）；閒置重啟問不到時 log 寫出原因。「命令直接失敗」「每天跳過重啟」這兩句中間狀態已經拿掉 | 已修好；例子的措辭見 N-E |
| N-C 新建 `.env` 的 0600 沒有斷言 | 修正 | `test_docker` 結尾補一段：`prepare` 之後不放 `.env`，裝完斷言 `.env` 存在且為 600。四種版本各跑一次 `test-installer-upgrade.sh docker`（見下方），斷言抓得到 `install.sh:771` 的 umask，而且不靠改寫路徑的 `os.chmod` | 已修好 |
| N-C 附帶：實作計畫的紅燈案例編號 | 修正 | `implementation-plan.md:540` 改成「案例一（…原本誤寫為案例二）」。本次把 0405603 的 `opencode-idle-restart.sh` 放在 7df8450 的測試底下重跑：rc=1，第一個失敗是案例一的「supervisor should report the blocking connection」，log 一直是「a session was updated within 1s; skipping」，與更正後的文字相符 | 已修好 |

N-C 的消融（7df8450 的 scripts 樹取出到 scratchpad，umask 0002）：

| 版本 | `install.sh` 的差異 | `test-installer-upgrade.sh docker` |
|---|---|---|
| 7df8450 原樣 | 無 | rc=0，「installer upgrade checks passed (docker)」 |
| 換回 0405603 的 `install.sh` | 沒有任何權限處理 | rc=1，「FAIL: a new .env is not 0600」（實作者的說法成立） |
| 只拿掉 `:771` 的 `umask 077` | 新建 `.env` 改用一般 `cp` | rc=1，「FAIL: a new .env is not 0600」 |
| 只拿掉 `:651` 的 `os.chmod(path, 0o600)` | 改寫路徑不設權限 | rc=0 |

最後一列說明測試的 `.env.example`（`EXAMPLE=1`）不會觸發遷移改寫，新斷言只量新建的路徑，與 `docker-env-server-password` 的改寫斷言互不重疊。

### 增量範圍內每個變動檔案的結論

| 檔案 | 結論 |
|---|---|
| `CHANGELOG.md` | N-B 已處理；`/providers` 的描述與上一份報告實測的 `auth list` 輸出一致，不印密鑰。閒置重啟條目的例子見 N-E |
| `docs/issues/issue-0044/README.md` | 452dc22 改 `## 現況` 與 `**現行 review**`，7df8450 改 `## 現況` 並新增 TBD-6、TBD-7，都在記帳區段。內容不含 `<`、行首粗體或行首 `[`。TBD-7 與實際處理一致。TBD-6 見 N-D |
| `docs/issues/issue-0044/implementation-plan.md` | R2 的案例編號更正經本次重跑確認；新增的 R7 與 diff、本次消融結果一致 |
| `docs/issues/issue-0044/review-c4d47d8.md` | 452dc22 只新增，之後沒有改動（工作樹內容與 452dc22 相同） |
| `scripts/test-installer-upgrade.sh` | 新斷言有效，見 N-C 的消融；`bash -n` 通過，`all` 通過 |

### 重新查核的項目與理由

- **`test_docker` 其他斷言的前提**
  - 理由：新段落在同一個函式結尾重新 `prepare`，會清掉前面建立的部署目錄
  - 新段落放在所有既有斷言之後，前面的斷言在換回 0405603 的版本上也照常通過（失敗訊息是新段落的那一句），沒有被新段落影響
- **`install.sh` 新建 `.env` 的路徑**
  - 理由：上一份報告只用等價命令實測，現在改由測試覆蓋
  - 增量沒有改 `install.sh`；上表的消融確認測試量的是 `:771`
- **沒有重跑 Rust 測試的理由**
  - `git diff --name-only c4d47d8 7df8450 -- '*.rs' 'Cargo.*'` 沒有輸出，增量只改文件與一支 shell 測試，上一份報告的 `cargo test`、fmt、clippy 結果仍適用於同一份 Rust 程式碼

### 其餘沿用 review-c4d47d8.md

除了上面查核的項目，其餘都沿用 review-c4d47d8.md：各項修正的確認、重新查核的項目、相關失敗面、需求、架構、安全、品質與 CI 三道的結論。失敗面表「新建 `.env`」一列的覆蓋，由「只有等價命令實測」改為「installer 斷言（舊版與拿掉 umask 都會失敗）」。

### 豁免、待確認與限制（本輪新增）

- 沒有 Gate 豁免紀錄；沒有新的 `需決策`
- 限制：新斷言在呼叫端 umask 已是 077 時會恆真；常見的 0002、0022 都能分辨，本次以 0002 實跑
- 本次 reviewer 遵守的界線：沒有修改任何被追蹤的檔案；消融都在 scratchpad 的副本進行；沒有送資料給任何模型；沒有碰正式容器與主機的 opencode service；`test-installer-upgrade.sh` 使用自己的假 docker，沒有動到主機的 docker 部署；跑完以 `pgrep` 確認沒有殘留的測試程序

### 本次實際執行的命令與結果

- `git rev-parse HEAD` = 7df8450；`git merge-base main HEAD` = `git rev-parse main` = e6a0bf9
- `git diff e6a0bf9 7df8450 | git patch-id --stable` → `93242f5a0ba7316e8d74f7edf21c5f08617ee748`，相符
- `git diff --stat c4d47d8..7df8450`：5 個檔案，306 行新增、10 行刪除
- `bash scripts/test-installer-upgrade.sh all`（repo 內，HEAD 7df8450）：exit 0，「installer upgrade checks passed (all)」；之後 `git status --porcelain` 沒有輸出
- `bash -n scripts/test-installer-upgrade.sh`：通過
- scratchpad 副本的 `bash scripts/test-installer-upgrade.sh docker`，四個版本的結果見 N-C 的消融表
- scratchpad 副本換回 0405603 的 `opencode-idle-restart.sh` 跑 `bash scripts/test-idle-restart-decision.sh`：rc=1，案例一失敗，log 為「a session was updated within 1s; skipping」
- `git diff v0.25.1 7df8450 -- scripts/opencode-idle-restart.sh` 與 `git grep … v0.25.1`：N-E 的查核
- 沒有執行 `cargo test`，理由見「重新查核的項目與理由」

## 流程判定
PASS

PASS 只適用於上列 Reviewed HEAD SHA；之後提交 README 記帳區段與本報告以外的檔案會讓它失效，提交前先告知使用者（review-evidence.md「審查之後的提交」）。
