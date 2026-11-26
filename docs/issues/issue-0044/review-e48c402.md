# 審查報告
- 範圍：完整交付範圍（TARGET main e6a0bf9；處理 review-2dd4369 M-1、N-H 後的增量覆審）
- Reviewed BASE SHA：e6a0bf921fd12d65e5f95b2418eb2916b847b4d8
- Reviewed HEAD SHA：e48c4029357fc29b92fb981c8e3b93b1a31af0d2
- Reviewed patch-id：4b99d77d46bb91d7c2fb4381c7fdf03636a65a70
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，claude-opus-5-5）。不是實作者，也沒有參與修正
- Review artifact：docs/issues/issue-0044/review-e48c402.md
- 增量覆審：上一份報告 review-2dd4369.md，增量範圍 2dd4369..e48c402。a267749 只保存上一份報告並記帳，e48c402 依使用者 2026-11-26 的決定修 M-1（在這個 PR 修）與 N-H（修正）
- 審查日期：2026-11-26
- 規格基線：沿用 review-c4d47d8.md（核准 commit 94aaa40）。增量沒有改動 Gherkin 劇本，README 的變動在 `## 現況`、`**現行 review**`、`## 待確認事項`
- 風險等級：High（沿用上一份報告）

## 增量覆審的前提

| 條件 | 核對 | 結果 |
|---|---|---|
| 上一份報告綁定 H1、記完整範圍 | review-2dd4369.md 綁定 2dd4369，範圍是完整交付，判定 RETURN TO execute-task | 成立 |
| H1 是新 HEAD 的祖先，中間沒有改寫歷史 | `git log 2dd4369..e48c402` 只有 a267749、e48c402，兩個都是一般提交 | 成立 |
| merge-base 沒有變 | `git merge-base main HEAD` = `git rev-parse main` = `origin/main` = e6a0bf9 = 上一份報告的 BASE | 成立 |
| patch-id | `git diff e6a0bf9 e48c402 \| git patch-id --stable` = 4b99d77d…，與指定值相同 | 成立 |

## 問題與風險

### MUST FIX

**M-2. Docker rollback 不帶 `--version` 成功後，最後一行印的是 GitHub 最新的 release，不是 rollback 的目標；正式發版後照 SCN-015 執行，installer 會印出剛退出的那一版**
- 位置：`scripts/install.sh:891`（沒有 `--version` 時 `VERSION` 填成 `releases/latest`）、`:896`（`info "Wukong ${ACTION} completed: ${VERSION}"`）。`rollback_docker`（`:815-885`）整段都用區域變數 `version`，從來沒有把 `VERSION` 改成 rollback 目標；binary 的 `rollback_binary` 有（`:578` `VERSION="$version"`）
- 本增量引入：修正前，不帶 `--version` 的 rollback 只在 latest 等於 `previousVersion` 時才成功，所以成功時印出的版本一定對；修正後放行了 latest 不等於目標的情況，印出的版本就跟著錯。v0.25.1 的 `install.sh`（`git show v0.25.1:scripts/install.sh` 第 692-696、724 行）同樣是比對加 `completed: ${VERSION}`，不會出現這個錯
- 本次在 scratchpad 實跑（HEAD 的測試工具加臨時案例，安裝 v9.9.9、升級到 v9.9.8，再不帶 `--version` rollback）：

  | latest | rc | 最後一行 | 實際 metadata |
  |---|---|---|---|
  | v9.9.8（等於升級目標，正式發版後的情況） | 0 | `Wukong rollback completed: v9.9.8` | `productTag` v9.9.9 |
  | v9.10.0（比升級目標更新） | 0 | `Wukong rollback completed: v9.10.0` | `productTag` v9.9.9 |
  | v9.9.9（等於 rollback 目標） | 0 | `Wukong rollback completed: v9.9.9` | `productTag` v9.9.9 |

  有 state 備份的 rollback（`upgraded_with_state_backup` 之後，latest 等於升級目標 v9.9.7）也一樣：`opencode-state` 正確回到升級前，最後一行卻是 `Wukong rollback completed: v9.9.7`
- 為什麼測試沒抓到：新的不帶 `--version` 案例把輸出導到 `$TMP/out`，只在失敗時印出，成功時只斷言 metadata 的 `productTag`。在 HEAD 的測試加上 `assert_contains "$TMP/out" 'Wukong rollback completed: v9.9.9'`，HEAD 的 `install.sh` 下 `docker-rollback` 失敗（missing 'Wukong rollback completed: v9.9.9'）
- 影響：這一版發布、成為 latest 之後，從 v0.25.1 升上來的使用者執行 `--rollback`，部署確實回到 v0.25.1，但 installer 最後印的是新版的版本號。rehearsal.md 第 145 行記的預期輸出是 `Wukong rollback completed: v0.25.1`，那是在 latest 仍是 v0.25.1 時跑的，條件跟上一份報告 M-1 說的一樣。使用者看到新版的版本號，容易以為 rollback 沒有生效；這時再跑一次 `--rollback` 會被拒絕，訊息又是「metadata is invalid」。資料與部署不受影響，但 rollback 是這次交付的安全網，成功訊息說錯版本，等於在 SCN-015 的主要路徑上回報了錯的結果
- 為什麼列 MUST FIX：這是本增量在剛修好的路徑上引入的邏輯錯誤，使用者看得到的輸出跟實際行為矛盾；修法跟 binary 一樣只要一行，不需要使用者決定需求邊界
- 建議修法：在 `rollback_docker` 讀完 metadata、確認備份目錄存在之後設 `VERSION="$version"`，跟 `rollback_binary:578` 相同；`docker-rollback` 的不帶 `--version` 案例加斷言 `Wukong rollback completed: v9.9.9`。本次在 scratchpad 試過這兩處改動：`docker-rollback`、`docker-state-rollback`、`docker-project-persistence` 都通過；`VERSION` 在 rollback 路徑之後只剩 `:896` 這一個用途（見下方「`VERSION` 在 rollback 路徑的其他用途」），所以不影響其他輸出。失敗訊息（`:884`）用的是 `$version`，不受影響

### SHOULD FIX
無

### NICE TO HAVE
無

## 已查核維度

### review-2dd4369 兩項的處理確認

| 項目 | 使用者決定 | 本次查核 | 結論 |
|---|---|---|---|
| M-1 不帶 `--version` 的 Docker rollback 拿 latest 比對 | (a) 在這個 PR 修 | `rollback_docker` 的 heredoc 多傳 `$VERSION_EXPLICIT`，`requested=sys.argv[2] if sys.argv[3] == "true" else ""`，寫法與 `rollback_binary:555-561` 相同。`VERSION_EXPLICIT` 只在 `parse_args` 的 `--version` 設成 true（`:93`），預設 false（`:42`），沒有其他地方改它。下表實跑各情況都符合預期 | 比對已修好；成功訊息見 M-2 |
| N-H `docs/docker.md:90` rollback 失敗的說法前後不一致 | 修正 | 前半句只說「升級與 rollback 都會被判為失敗」，接著分開寫：「升級失敗時部署已還原成前一版，修好設定後重新執行升級」、「rollback 則是…等不到時 `.wukong-release` 已經記成前一版…用 `docker compose -p <project> up -d` 啟動即可，不要再跑一次 `--rollback`」。對照程式見下節 | 已照決定修好 |

### M-1 修正後各情況的實際行為（scratchpad 臨時案例）

安裝 v9.9.9、以 `FIXTURE_SAFE_TO=v9.9.9` 升級到 v9.9.8（`previousVersion` v9.9.9），再跑 rollback：

| 情況 | rc | 擋下的位置／結果 |
|---|---|---|
| 不帶 `--version`，latest = 升級目標 v9.9.8 | 0 | 回到 v9.9.9（`productTag` v9.9.9、`previousVersion` v9.9.8）；訊息錯，見 M-2 |
| 不帶 `--version`，latest = v9.10.0（比升級目標新） | 0 | 同上；訊息錯 |
| 不帶 `--version`，latest = rollback 目標 v9.9.9 | 0 | 同上；訊息正確（原本的覆蓋） |
| `--version v9.9.9`（正確），latest = v9.9.8 | 0 | 回到 v9.9.9，訊息 `completed: v9.9.9` |
| `--version v9.9.8`（等於目前版本），latest = v9.9.8 | 1 | heredoc 第 4 行（`assert not requested or requested == previousVersion`），metadata 不變 |
| `--version v9.9.7`（不相干的版本） | 1 | 同上；log 只有 `compose version`、`up --help` 與 project 探測，沒有 pull、stop、up |
| 有 state 備份，不帶 `--version`，latest = 升級目標 v9.9.7 | 0 | `opencode-state` 等於升級前；訊息錯（M-2） |

### R10「再跑一次 `--rollback` 會被拒絕」現在由哪個條件擋下

- 成功 rollback 之後，以 latest v9.9.9、latest v9.9.8、`--version v9.9.8` 各再跑一次：三種都是 rc=1，都在 heredoc 第 6 行（`dataCompatibility` 的 `schemaVersion == 1` 與 `rollbackSafeTo == previousVersion`）被拒。rollback 寫出的 metadata 沒有 `dataCompatibility`，所以這一行一定不成立
- 不健康的 rollback 之後（`FIXTURE_UNHEALTHY_TAG=v9.9.9`，latest v9.9.8）：第一次 rc=1，訊息是 S-1 修正後的那句，metadata 是 v9.9.9／v9.9.8；再跑一次，latest v9.9.8 與 v9.9.9 都在第 6 行被拒
- 上一份報告的限制「測試裡第二次 rollback 是被 latest 比對擋下」已不存在：`docker-rollback` 的第二次 rollback（latest 預設 v9.9.9、不帶 `--version`）現在不做版本比對，由第 6 行擋下，斷言照樣成立
- 訊息仍是「rollback source is absent or metadata is invalid」。上一份報告已評估過這句對 rollback 之後的 metadata 不精確但不另列；M-1 修好後只剩這一種情況會看到它，維持不列

### `VERSION` 在 rollback 路徑的其他用途

| 用途 | rollback 時是否執行 | 結論 |
|---|---|---|
| `:891` 沒有 `--version` 時向 GitHub 取 latest | 是 | 結果只剩 `:896` 用到；取不到時 rollback 會中止（「could not resolve latest release」）。v0.25.1 與 binary 都一樣，不是本增量引入，列為限制 |
| `:892` `BASE_URL` | 設定但不使用 | rollback 不下載任何檔案（`rollback_docker` 的 pull 用備份目錄的 compose 檔），不受影響 |
| `:894` dry-run 訊息 | 印 latest 後就結束，不讀 metadata | 與修正前、binary 相同，不受本增量影響 |
| `skip_current_upgrade` | 只在 `ACTION == upgrade` | 不受影響 |
| `install_docker` 裡 `rollback_docker` 之後的程式 | `return`，不執行 | 不受影響 |
| `:896` 完成訊息 | 是 | M-2 |

### N-H 的措辭與實際行為

- 「升級失敗時部署已還原成前一版」：`install_docker:787-805`，啟用失敗時依序還原 state（有備份時）、`.env`（有改寫時）、前一版檔案，再以前一版啟動（`|| true`），metadata 沒寫，最後非零結束。檔案、state 與 metadata 都是前一版；state 還原失敗時不啟動，這由下一段（`:92`）另外說明。原因在 `.env`（例如 `WUKONG_WEB_ALLOW_INSECURE`）時，前一版也可能不健康，但「部署已還原成前一版」仍成立。之後「重新執行升級」：檔案已換回，`skip_current_upgrade` 比對 compose 的映像不會跳過。`docker-recovery` 斷言失敗後 metadata 與 compose 檔不變
- 「rollback…等不到時 `.wukong-release` 已經記成前一版」：上面的不健康 rollback 實跑，metadata 是 `productTag` v9.9.9，compose 檔與 state 都是前一版
- 結論：與程式一致

### 測試是否真的會在舊程式下失敗（scratchpad 副本，`git archive` 取出 e48c402、a267749 的 `scripts`）

| 組合 | `docker-rollback` 結果 |
|---|---|
| e48c402 原樣 | 通過 |
| e48c402 的測試 + a267749 的 `install.sh` | 失敗：rollback without --version was refused（AssertionError 第 4 行，「rollback source is absent or metadata is invalid」），與 R11 的紅燈說法相同 |
| a267749 的測試 + e48c402 的 `install.sh` | 通過（原本的覆蓋在新程式下仍成立） |
| e48c402，heredoc 改成完全不比對（`requested=""`；sed 同時改到 binary 那行，但這個案例只跑 Docker） | 失敗：rollback to a version that is not the previous one ran，與 R11 的消融說法相同 |
| e48c402 的 `install.sh` + 加上完成訊息斷言的測試 | 失敗：missing 'Wukong rollback completed: v9.9.9'（M-2） |
| 加 `VERSION="$version"` 的 `install.sh` + 加上完成訊息斷言的測試 | 通過；`docker-state-rollback`、`docker-project-persistence` 也通過 |

R11 寫「a267749 的 `install.sh` 在第一個新斷言失敗」：照順序第一個新斷言是指定錯誤版本被拒，它在舊程式下也通過；失敗的是第二個（不帶 `--version` 的成功）。只是措辭，不列發現。

### 原本的 rollback 覆蓋是否保留

- 修正前 `docker-rollback` 第一段是 latest 等於 rollback 目標、不帶 `--version` 的成功 rollback；這段改成 latest 等於升級目標。latest 等於 rollback 目標、不帶 `--version` 的成功路徑仍由 `docker-state-rollback`、`docker-project-persistence`（latest 都是預設 v9.9.9）與上面的臨時案例覆蓋
- 不健康的 rollback 那段沒有改動
- 明確指定正確 `--version` 的 Docker rollback 沒有正式案例（修正前也沒有），本次以臨時案例確認，列為限制

### 增量範圍內每個變動檔案的結論

| 檔案 | 結論 |
|---|---|
| `scripts/install.sh` | M-1 的比對照決定修好，寫法與 binary 相同；但成功訊息仍用 `VERSION`，見 M-2 |
| `scripts/test-installer-upgrade.sh` | 兩個新案例都會在對應的錯誤程式下失敗（見紅燈表）；原本的覆蓋仍在其他案例；沒有斷言完成訊息，所以抓不到 M-2。`all` 通過 |
| `CHANGELOG.md` | Fixed 新增的條目描述 v0.25.1 使用者看得到的差異：v0.25.1 的 `rollback_docker` 同樣拿 `$VERSION` 比對（`git show v0.25.1:scripts/install.sh` 第 692-696 行），「v0.18.0 起」與上一份報告查到的 5b6c45f 一致；「與 binary 模式的 rollback 相同」成立。v0.25.1 時 rollback 也會被 `rollbackSafeTo` 擋下，這由前一條說明，兩條合起來正確。M-2 修好後這條不必改 |
| `docs/docker.md` | N-H 照決定修好（見上節）；`:98` 的 `install.sh --mode docker --rollback` 現在成立 |
| `docs/issues/issue-0044/README.md` | a267749：`**現行 review**` 改指 review-2dd4369.md、`## 現況` 改寫最後一句。e48c402：新增 TBD-13，狀態 `已解決` 附日期、使用者的選擇與結論，影響欄寫「無」，內容不含 `<`、行首粗體或行首 `[`，符合 docs/AGENTS.md「待確認事項」。`**狀態**: 實作完成，待審查` 與現況相符；Timeline 這一輪沒有新增，符合只記語意事件的規則。M-2 修好時 TBD-13 的結論可以補一句完成訊息，不是必要 |
| `docs/issues/issue-0044/implementation-plan.md` | R11 的問題、修正、測試、紅燈、消融與 R10 說明都與本次實跑一致（「第一個新斷言」的措辭見上）；沒有提到完成訊息 |
| `docs/issues/issue-0044/review-2dd4369.md` | a267749 只新增，e48c402 沒有改動（`git diff --stat a267749 e48c402` 不含它） |

### 重新查核的項目與理由

- **上一份報告對 SCN-015 的結論**
  - 理由：M-1 就在 SCN-015 的路徑上，本增量改了這條路徑的前提
  - 行為（映像、`opencode-state`、metadata 回到升級前）在 latest 為任何值時都成立；installer 的輸出不成立，見 M-2
- **`docker-rollback` 第二次 rollback 被拒的斷言**
  - 理由：上一份報告記它被 latest 比對擋下，本增量拿掉那個條件
  - 現在由 `dataCompatibility` 那一行擋下，見 R10 一節
- **`rollback_binary`**
  - 理由：同樣讀 `VERSION_EXPLICIT`
  - 本增量沒改，沿用；它在 `:578` 設 `VERSION`，完成訊息正確，這也是 M-2 建議修法的依據
- **沒有重跑 Rust 測試與 `test-docker-runtime.sh` 的理由**
  - `git diff --name-only 2dd4369 e48c402` 只有 `install.sh`、`test-installer-upgrade.sh` 與文件，沒有 `*.rs`、`Cargo.*`、entrypoint 或 compose 檔

### 其餘沿用 review-2dd4369.md

除了上面查核的項目，其餘都沿用 review-2dd4369.md（以及它沿用的 review-d632fe3.md、review-7df8450.md、review-c4d47d8.md）：S-1、N-F、N-G 的處理，S-1 修正後各失敗路徑的 metadata，有 state 備份的 rollback，P2-1、P2-2 的處理、邊界案例、真實 Compose 實測、需求、架構、安全、品質與 CI 三道的結論。

### 豁免、待確認與限制（本輪新增）

- 沒有 Gate 豁免紀錄；本輪沒有 `需決策` 的發現
- 限制：
  - Docker rollback 仍要先向 GitHub 取 latest（`:891`），取不到就中止；修正後 Docker rollback 只有完成訊息用到它。v0.25.1 與 binary 都一樣，不是本增量引入，不列發現
  - 明確指定正確 `--version` 的 Docker rollback 沒有正式案例，只以臨時案例確認
  - 本輪所有實跑都用測試工具的假 docker、假 curl；沒有對真實 GitHub 與真實映像跑 rollback
- 本次 reviewer 遵守的界線：沒有修改任何被追蹤的檔案（只建立本報告）；紅燈、消融、臨時案例與 M-2 的試修都在 scratchpad 的副本；沒有送資料給任何模型；沒有起任何真實 Docker 容器或 project；正式容器 wukong-telegram、wukong-web、wukong-schedulerd 維持 Up 25 hours，沒有碰主機的 opencode service；`test-installer-upgrade.sh` 使用自己的假 docker；跑完以 `pgrep` 確認沒有殘留的測試程序

### 本次實際執行的命令與結果

- `git rev-parse HEAD` = e48c402；`git merge-base main HEAD` = `git rev-parse main origin/main` = e6a0bf9
- `git diff e6a0bf9 e48c402 | git patch-id --stable` → `4b99d77d46bb91d7c2fb4381c7fdf03636a65a70`，相符
- `git diff --stat 2dd4369..e48c402`：7 個檔案，199 行新增、6 行刪除（其中 review-2dd4369.md 166 行由 a267749 新增）
- `bash scripts/test-installer-upgrade.sh all`（repo 內，HEAD e48c402，TMPDIR 指到 scratchpad）：exit 0，「installer upgrade checks passed (all)」，103 秒；之後 `git status --porcelain` 沒有輸出
- `bash -n scripts/install.sh`、`bash -n scripts/test-installer-upgrade.sh`：通過
- scratchpad 副本：`docker-rollback` 在六種組合下的結果見紅燈表；試修後另跑 `docker-state-rollback`、`docker-project-persistence`，通過
- scratchpad 臨時案例：latest 為 v9.9.8、v9.10.0、v9.9.9 時不帶 `--version` 的 rollback 與之後三種第二次 rollback；正確與錯誤的 `--version`；不健康的 rollback 之後再 rollback；有 state 備份的 rollback（結果見上文各表）
- `git show v0.25.1:scripts/install.sh`：`rollback_docker` 同樣拿 `$VERSION` 比對，完成訊息同樣是 `completed: ${VERSION}`
- 沒有執行 `cargo test`，理由見「重新查核的項目與理由」

## 流程判定
RETURN TO execute-task

M-1 的比對與 N-H 都已照使用者的決定修好，紅燈與消融成立。退回的原因只有 M-2：不帶 `--version` 的 Docker rollback 現在可以在 latest 不等於目標時成功，但完成訊息仍印 latest，正式發版後照 SCN-015 執行會印出剛退出的那一版。建議比照 `rollback_binary` 加一行 `VERSION="$version"`，並在 `docker-rollback` 斷言完成訊息；修正後再送增量覆審（增量範圍 e48c402..新 HEAD）。
