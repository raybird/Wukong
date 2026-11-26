# 審查報告
- 範圍：完整交付範圍（TARGET main e6a0bf9；處理 review-e48c402 M-2 後的增量覆審）
- Reviewed BASE SHA：e6a0bf921fd12d65e5f95b2418eb2916b847b4d8
- Reviewed HEAD SHA：a9b0a615a832caf0ef1f61873b3e1db4a3677883
- Reviewed patch-id：145613b4ab30eb85840ba6892bf0abd94a2262ef
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，claude-opus-5-5）。不是實作者，也沒有參與修正
- Review artifact：docs/issues/issue-0044/review-a9b0a61.md
- 增量覆審：上一份報告 review-e48c402.md，增量範圍 e48c402..a9b0a61。1d3ddc5 只保存上一份報告並記帳，a9b0a61 修 M-2（MUST FIX，不需使用者決策）
- 審查日期：2026-11-26
- 規格基線：沿用 review-c4d47d8.md（核准 commit 94aaa40）。增量沒有改動 Gherkin 劇本，README 的變動在 `## 現況`、`**現行 review**`、`## 待確認事項`
- 風險等級：High（沿用上一份報告）

## 增量覆審的前提

| 條件 | 核對 | 結果 |
|---|---|---|
| 上一份報告綁定 H1、記完整範圍 | review-e48c402.md 綁定 e48c402，範圍是完整交付，判定 RETURN TO execute-task | 成立 |
| H1 是新 HEAD 的祖先，中間沒有改寫歷史 | `git merge-base --is-ancestor e48c402 a9b0a61` 成立；`git rev-list --parents e48c402..a9b0a61` 只有 1d3ddc5（父 e48c402）、a9b0a61（父 1d3ddc5），都是一般提交 | 成立 |
| merge-base 沒有變 | `git merge-base main HEAD` = `git rev-parse main` = `origin/main` = e6a0bf9 = 上一份報告的 BASE | 成立 |
| patch-id | `git diff e6a0bf9 a9b0a61 \| git patch-id --stable` = 145613b4…，與指定值相同 | 成立 |

## 問題與風險

### MUST FIX
無

### SHOULD FIX
無

### NICE TO HAVE
無

## 已查核維度

### review-e48c402 M-2 的處理確認

| 項目 | 本次查核 | 結論 |
|---|---|---|
| M-2 不帶 `--version` 的 Docker rollback 成功後印出最新 release | `rollback_docker` 在 metadata heredoc 成功（「metadata is invalid」那個 abort 之後）、檢查備份目錄之前設 `VERSION="$version"`（`scripts/install.sh:831-832`），位置與上一份報告建議的相同，寫法與 `rollback_binary:578` 相同。`docker-rollback` 在「latest 是剛升級到的 v9.9.8」那段加上 `assert_contains "$TMP/out" 'Wukong rollback completed: v9.9.9'`（`scripts/test-installer-upgrade.sh:814-815`）。下表實跑各情況都符合預期 | 已修好 |

### 修正後各情況的實際行為（scratchpad 臨時案例，a9b0a61 的 `scripts` 加臨時案例）

安裝 v9.9.9、以 `FIXTURE_SAFE_TO=v9.9.9` 升級到 v9.9.8，再跑 rollback。「1d3ddc5」欄是同一組案例換成 1d3ddc5 的 `install.sh` 的結果，用來確認差異只在完成訊息：

| 情況 | rc | 最後一行（a9b0a61） | metadata（productTag／previousVersion） | 1d3ddc5 |
|---|---|---|---|---|
| 不帶 `--version`，latest = 升級目標 v9.9.8 | 0 | `Wukong rollback completed: v9.9.9` | v9.9.9／v9.9.8 | 最後一行是 `completed: v9.9.8`，其餘相同 |
| 不帶 `--version`，latest = v9.10.0（比升級目標新） | 0 | `completed: v9.9.9` | v9.9.9／v9.9.8 | 最後一行是 `completed: v9.10.0`，其餘相同 |
| 不帶 `--version`，latest = rollback 目標 v9.9.9 | 0 | `completed: v9.9.9` | v9.9.9／v9.9.8 | 相同 |
| 上面三種之後各再跑一次（不帶 `--version`，同一個 latest） | 1 | `rollback source is absent or metadata is invalid`，沒有 completed | 不變 | 相同 |
| `--version v9.9.9`（正確），latest = v9.9.8 | 0 | `completed: v9.9.9` | v9.9.9／v9.9.8 | 相同 |
| `--version v9.9.8`（等於目前版本） | 1 | `metadata is invalid`，沒有 completed | v9.9.8／v9.9.9，不變 | 相同 |
| `--version v9.9.7`（不相干的版本） | 1 | 同上 | 不變 | 相同 |
| 不健康（`FIXTURE_UNHEALTHY_TAG=v9.9.9`），latest = v9.9.8 | 1 | `rolled back to v9.9.9, but its services did not become healthy within 180s; …`，沒有 completed | v9.9.9／v9.9.8 | 相同 |
| metadata 無效（把 `rollbackSafeTo` 改成 v1.0.0） | 1 | `metadata is invalid`，沒有 completed | 不變 | 相同 |
| 備份目錄被刪（在設 `VERSION` 之後的第一個 abort） | 1 | `Docker rollback backup is unavailable`，沒有 completed | 不變 | 相同 |
| `--dry-run`，latest = v9.9.8 | 0 | `dry-run: mode=docker action=rollback version=v9.9.8` | 不變 | 相同（dry-run 不進 `rollback_docker`） |
| 有 state 備份（`upgraded_with_state_backup`），latest = 升級目標 v9.9.7 | 0 | `completed: v9.9.9`；`opencode-state` 等於升級前 | v9.9.9／v9.9.7 | 最後一行是 `completed: v9.9.7`，其餘相同 |

`diff` 兩組輸出只有上表三列的完成訊息不同；失敗路徑的訊息與 metadata 都不受 `VERSION` 改動影響。

### `VERSION` 改動後在 rollback 路徑之後的使用者

| 位置 | 是否用到 `VERSION` | 結論 |
|---|---|---|
| `rollback_docker` 設定之後的本體（`:833-886`） | 沒有，全部用區域變數 `version`、`current_version` | 不受影響 |
| 它呼叫的函式：`state_volume`、`state_backup_entries`、`make_backup_dir`、`backup_docker_state`、`restore_docker_state`、`write_json_atomically`、`activate_docker_services`、`docker_health_hint`、`abort`、`info` | `grep -n VERSION` 在 `:307-313`、`:592-733` 都沒有命中；`abort`、`info` 只印參數 | 不受影響 |
| `install_docker` 裡 `rollback_docker` 之後 | `return`（`:742-743`），不執行 | 不受影響 |
| `trap cleanup EXIT`（`:73`） | 只刪暫存目錄 | 不受影響 |
| `BASE_URL`（`:894`） | 在 `install_docker` 之前就算好，rollback 不下載 | 不受影響 |
| `:898` 完成訊息 | 是 | 這是要修的地方，現在印 rollback 目標 |

R12 寫「之後在這個函式裡沒有其他地方用到 `VERSION`」，與上表一致。

### 新斷言是否在舊程式下失敗（scratchpad 副本，`git archive` 取出 a9b0a61、1d3ddc5 的 `scripts`）

| 組合 | `docker-rollback` 結果 |
|---|---|
| a9b0a61 原樣 | 通過 |
| a9b0a61 的測試 + 1d3ddc5 的 `install.sh` | 失敗：missing 'Wukong rollback completed: v9.9.9'，與 R12 的紅燈說法相同 |
| 1d3ddc5 原樣 | 通過（舊測試抓不到 M-2，與上一份報告相同） |

1d3ddc5 與 a9b0a61 的 `install.sh` 只差這兩行（`diff` 確認），所以紅燈只能來自這個修正。

### 增量範圍內每個變動檔案的結論

| 檔案 | 結論 |
|---|---|
| `scripts/install.sh` | a9b0a61：加註解與 `VERSION="$version"` 兩行，M-2 修好；註解說的「without --version is the latest release」與 `:893` 一致。沒有其他改動 |
| `scripts/test-installer-upgrade.sh` | a9b0a61：加註解與一個完成訊息斷言，在 1d3ddc5 的 `install.sh` 下失敗、在 a9b0a61 下通過；`all` 通過 |
| `docs/issues/issue-0044/review-e48c402.md` | 1d3ddc5 只新增，內容與上一份報告相同（`git show 1d3ddc5:…` 與工作樹 `diff` 無差異）；a9b0a61 沒有改動 |
| `docs/issues/issue-0044/README.md` | 1d3ddc5：`**現行 review**` 改指 review-e48c402.md，`## 現況` 補 review-e48c402 的判定。a9b0a61：`## 現況` 句尾補「已修正，待增量覆審」，新增 TBD-14，狀態 `已解決` 附日期、「MUST FIX，不需使用者決策」與結論，影響欄寫「無」，與 TBD-12、TBD-13 的寫法一致。新增內容不含 `<`、行首粗體或行首 `[`，符合 docs/AGENTS.md「待確認事項」。1d3ddc5 的 `## 現況` 先提到 TBD-14、a9b0a61 才補上那一列，只是兩個提交之間的順序，HEAD 上一致。`**狀態**: 實作完成，待審查` 與現況相符；Timeline 沒有新增，符合只記語意事件的規則 |
| `docs/issues/issue-0044/implementation-plan.md` | a9b0a61 新增 R12：問題、修正、測試、紅燈都與本次實跑一致。「這一輪的整體檢查（2026-11-26）」沒有為 R12 另記，R12 只動 `install.sh` 的 rollback 路徑，本次已重跑 `all`，不列發現 |

### 重新查核的項目與理由

- **上一份報告對 SCN-015 的結論**
  - 理由：M-2 在 SCN-015 的路徑上，本增量改了它的輸出
  - 行為（映像、`opencode-state`、metadata 回到升級前）在 latest 為任何值時都成立，installer 的完成訊息現在也是 rollback 目標。rehearsal.md 第 145 行記的預期輸出 `Wukong rollback completed: v0.25.1`，在正式發版、latest 變成新版之後也會成立
- **失敗路徑的訊息與 metadata**（S-1 修正後的不健康 rollback、metadata 無效、指定錯誤版本、備份目錄不在）
  - 理由：`VERSION` 在這些路徑之前或途中被改
  - 都在 `:898` 之前以 `abort` 結束，訊息用的是區域變數，與 1d3ddc5 逐字相同，見上表
- **`docker-rollback` 第二次 rollback 被拒的斷言**、R10 一節的結論
  - 理由：同一個測試函式被改
  - 改動只在第一段加斷言；第二次 rollback 仍在 heredoc 的 `dataCompatibility` 那一行被拒（上表三種 latest 都是 rc=1、metadata 不變），沿用上一份報告
- **CHANGELOG 的 Fixed 條目**
  - 理由：上一份報告說 M-2 修好後這條不必改
  - M-2 是本 PR 內引入又修掉的，沒有發布過，CHANGELOG 不需另記；條目內容沒有提到完成訊息，仍正確
- **`rollback_binary`**：本增量沒改，沿用
- **沒有重跑 Rust 測試與 `test-docker-runtime.sh` 的理由**：`git diff --name-only e48c402 a9b0a61` 只有 `install.sh`、`test-installer-upgrade.sh` 與 issue 文件，沒有 `*.rs`、`Cargo.*`、entrypoint 或 compose 檔

### 其餘沿用 review-e48c402.md

除了上面查核的項目，其餘都沿用 review-e48c402.md（以及它沿用的 review-2dd4369.md、review-d632fe3.md、review-7df8450.md、review-c4d47d8.md）：M-1 與 N-H 的處理、R10「再跑一次 `--rollback` 會被拒絕」的條件、`VERSION` 在 rollback 路徑其他用途的分析、S-1 修正後各失敗路徑的 metadata、有 state 備份的 rollback、P2-1、P2-2 的處理、邊界案例、真實 Compose 實測、需求、架構、安全、品質與 CI 三道的結論。

### 豁免、待確認與限制（本輪新增）

- 沒有 Gate 豁免紀錄；本輪沒有 `需決策` 的發現
- 限制：
  - 明確指定正確 `--version` 的 Docker rollback 仍沒有正式案例，本輪以臨時案例確認完成訊息與 metadata（上一份報告已列，本輪的結果見上表）
  - 本輪所有實跑都用測試工具的假 docker、假 curl；沒有對真實 GitHub 與真實映像跑 rollback
- 本次 reviewer 遵守的界線：沒有修改任何被追蹤的檔案（只建立本報告）；紅燈與臨時案例都在 scratchpad 的副本；沒有送資料給任何模型；沒有起任何真實 Docker 容器或 project；正式容器 wukong-telegram、wukong-web、wukong-schedulerd 維持 Up 26 hours，沒有碰主機的 opencode service；跑完以 `pgrep` 確認沒有殘留的測試程序

### 本次實際執行的命令與結果

- `git rev-parse HEAD` = a9b0a61；`git merge-base main HEAD` = `git rev-parse main origin/main` = e6a0bf9
- `git diff e6a0bf9 a9b0a61 | git patch-id --stable` → `145613b4ab30eb85840ba6892bf0abd94a2262ef`，相符
- `git diff --stat e48c402..a9b0a61`：5 個檔案，183 行新增、2 行刪除（其中 review-e48c402.md 169 行由 1d3ddc5 新增）
- `bash scripts/test-installer-upgrade.sh all`（repo 內，HEAD a9b0a61，TMPDIR 指到 scratchpad）：exit 0，「installer upgrade checks passed (all)」，128 秒；之後 `git status --porcelain` 沒有輸出
- `bash -n scripts/install.sh`、`bash -n scripts/test-installer-upgrade.sh`：通過
- scratchpad 副本：`docker-rollback` 在三種組合下的結果見紅燈表
- scratchpad 臨時案例：上表 12 種情況，a9b0a61 與 1d3ddc5 的 `install.sh` 各跑一次，`diff` 兩份輸出只差三列完成訊息
- 沒有執行 `cargo test`，理由見「重新查核的項目與理由」

## 流程判定
PASS

M-2 已照上一份報告的建議修好：不帶 `--version` 的 Docker rollback 不論 latest 是哪一版，完成訊息都是 rollback 目標，metadata 正確；失敗路徑的訊息與 metadata 不受影響；新斷言在舊程式下失敗。增量範圍內沒有新的問題。

PASS 只適用於上列 Reviewed HEAD SHA；之後提交 README 記帳區段與本報告以外的檔案會讓它失效，提交前先告知使用者（review-evidence.md「審查之後的提交」）。
