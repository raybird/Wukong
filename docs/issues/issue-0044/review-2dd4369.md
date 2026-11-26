# 審查報告
- 範圍：完整交付範圍（TARGET main e6a0bf9；處理 review-d632fe3 建議後的增量覆審）
- Reviewed BASE SHA：e6a0bf921fd12d65e5f95b2418eb2916b847b4d8
- Reviewed HEAD SHA：2dd43697b956d595e3b869660b135826b5784bac
- Reviewed patch-id：91da8ae22a8ed68b5cfcfd93dfd31d6dcdc62d51
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，claude-opus-5-5）。不是實作者，也沒有參與修正
- Review artifact：docs/issues/issue-0044/review-2dd4369.md
- 增量覆審：上一份報告 review-d632fe3.md，增量範圍 d632fe3..2dd4369。這是處理建議後的增量覆審：b30ab56 只保存上一份報告並記帳，2dd4369 依使用者 2026-11-26 的決定處理 S-1、N-F、N-G
- 審查日期：2026-11-26
- 規格基線：沿用 review-c4d47d8.md（核准 commit 94aaa40）。增量沒有改動 Gherkin 劇本，README 的變動在 `## 現況`、`**現行 review**`、`## 待確認事項`
- 風險等級：High（沿用上一份報告）

## 增量覆審的前提

| 條件 | 核對 | 結果 |
|---|---|---|
| 上一份報告綁定 H1、記完整範圍 | review-d632fe3.md 綁定 d632fe3，範圍是完整交付，判定 PASS | 成立 |
| H1 是新 HEAD 的祖先，中間沒有改寫歷史 | `git log d632fe3..2dd4369` 只有 b30ab56、2dd4369，兩個都是一般提交 | 成立 |
| merge-base 沒有變 | `git merge-base main HEAD` = `git rev-parse main` = `origin/main` = e6a0bf9 = 上一份報告的 BASE | 成立 |
| patch-id | `git diff e6a0bf9 2dd4369 \| git patch-id --stable` = 91da8ae2…，與指定值相同 | 成立 |

## 問題與風險

### MUST FIX

**M-1. Docker `--rollback` 沒帶 `--version` 時拿「最新 release」比對 `previousVersion`；升級到最新版之後（正式發版後的正常情況）照 SCN-015 執行 `installer --rollback` 一定被拒絕，訊息還寫成 metadata 無效** `需決策`
- 位置：`scripts/install.sh:888`（`VERSION` 沒指定時填成 `releases/latest` 的 tag）、`:818-822`（`rollback_docker` 把 `$VERSION` 當成 requested，`assert not requested or requested == d["previousVersion"]`）
- 對照：binary 的 `rollback_binary`（`:555-562`）多傳了 `$VERSION_EXPLICIT`，只有明確指定 `--version` 時才比對；Docker 這邊沒有。兩者是同一個提交 5b6c45f（2026-07-12，v0.18.0 起）加的，所以這是既有缺陷，不是本分支引入的
- 本次在 scratchpad 實跑（HEAD 的測試工具加一個臨時案例）：安裝 v9.9.9，以 `FIXTURE_TAG=v9.9.8`（假 GitHub 的 latest 也是 v9.9.8，跟正式發版後的情況一樣）`--upgrade` 到 v9.9.8，再跑 `--rollback`：rc=1，`AssertionError`（第 4 行，就是 `:822`），「installer: rollback source is absent or metadata is invalid」；同一個部署加 `--version v9.9.9`：rc=0，「Wukong rollback completed: v9.9.9」
- 為什麼現有證據沒有抓到：
  - 測試工具的假 curl 把 `releases/latest` 回成 `${FIXTURE_TAG:-v9.9.9}`（`test-installer-upgrade.sh:85`），而 `run_installer` 預設 `FIXTURE_TAG=v9.9.9`。所有 rollback 案例都是「升級到 v9.9.8 或 v9.9.7、回到 v9.9.9」，latest 剛好等於 rollback 目標
  - rehearsal.md 第 7 節與 `scripts/rehearse-rc.sh` 都在 V2 版還沒發布時跑，GitHub 的 latest 仍是 v0.25.1，也剛好等於 rollback 目標
- 影響：SCN-015 的 When 是「執行 installer --rollback」。這一版正式發布、成為 latest 之後，從 v0.25.1 升上來的使用者照 `docs/docker.md:98` 與 CHANGELOG 執行 `--rollback` 會被拒絕，看到的是「metadata is invalid」，容易以為備份壞了。拒絕發生在任何改動之前，資料不會有事；加 `--version v0.25.1` 可以繞過，但沒有任何文件寫這個做法。V2 會單向遷移 `opencode.db`，rollback 是這次交付的安全網
- 為什麼列 MUST FIX：這是已核准劇本主要路徑上的邏輯錯誤，現有的測試與 rehearsal 都在「latest 等於 rollback 目標」的條件下取得，條件與正式環境不同（AGENTS.md「驗證紀律」：fixture 與真實狀況不符時測試會全綠）
- 待決問題：在這個 PR 修正，或記入 Gate 豁免紀錄、另開 issue
- 可選做法：
  - (a) 在這個 PR 修：`rollback_docker` 比照 binary 傳 `$VERSION_EXPLICIT`，只有明確指定 `--version` 時才比對；`test_docker_rollback` 或 `test_docker_state_rollback` 補一個 latest 不等於 rollback 目標的案例（上面的臨時案例在舊程式碼下會失敗）。改 `install.sh` 要再送增量覆審
  - (b) 記入 Gate 豁免紀錄並另開 issue；在那之前 `docs/docker.md:98` 與 CHANGELOG 寫明要加 `--version {前一版}`
- reviewer 建議：(a)。改動是一個參數加一行條件，與 binary 已有的寫法相同；不修的話，這一版發布後 SCN-015 的路徑對所有使用者都不成立

### SHOULD FIX
無

### NICE TO HAVE

**N-H. `docs/docker.md:90` 同一段先說 rollback 失敗「要先修好設定再執行」，接著又說「不要再跑一次 `--rollback`」**
- 位置：`docs/docker.md:90`「升級與 rollback 都會被判為失敗，要先修好設定再執行；…。rollback 時…修好設定後用 `docker compose -p <project> up -d` 啟動即可，不要再跑一次 `--rollback`」
- 前半句把升級與 rollback 寫在一起，讀起來像兩者都要重跑 installer，後半句才更正。意思最後是對的，只是要讀完整段才知道
- 建議：前半句改成只講升級（例如「升級會被判為失敗，要先修好設定再執行」），rollback 的做法留在後半句。只影響措辭，可以和 M-1 一起改；改這個檔案會讓下一份報告的 PASS 失效

## 已查核維度

### review-d632fe3 三項建議的處理確認

| 項目 | 使用者決定 | 本次查核 | 結論 |
|---|---|---|---|
| S-1 rollback 健康檢查失敗時 metadata 仍是新版 | (a) 先寫 rollback 後的 metadata 再等健康，失敗時仍非零結束、改訊息 | `write_json_atomically .wukong-release`（`:876`）移到 `activate_docker_services`（`:881`）之前，位置在 state 還原（`:868`）與檔案換回（`:871`）之後；寫出的欄位與成功 rollback 完全相同。失敗訊息改成「rolled back to {前一版}, but its services did not become healthy within 180s; fix the cause, then start them with 'docker compose -p {project} up -d'」，後面接 `docker_health_hint`。結束碼仍是非零（`abort`） | 已照決定修好 |
| N-F CHANGELOG 備份清單那條是中間狀態 | 併入 Added 的備份條目 | Changed 的那條刪除；Added「Docker 升級前會備份 `opencode-state`」底下多一個子項目「要備份的 volume 清單（release manifest 的 `affectedState`）讀不出來時，installer 在停服務之前中止，不會在沒有備份的情況下升級」，不再提「以前會截斷」 | 已照決定修好 |
| N-G 降級模式條件寫法不精確 | 只修增量內的三處 | CHANGELOG、`docs/docker.md:90`、`install.sh:591` 註解改成「沒設 token、又把 `WUKONG_WEB_ALLOW_INSECURE` 改成 1 以外的值」。`crates/wukong-web/src/main.rs:11-19`：token 空字串視同沒設，`allow_insecure` 只認 `"1"`；`lib.rs:263` 另看 host 是否非 loopback，compose 固定 `WUKONG_WEB_HOST=0.0.0.0`、`WUKONG_WEB_ALLOW_INSECURE` 預設 1（`docker-compose.release.yml:157`、`:159`），所以在 Docker 裡新寫法就是完整條件。CHANGELOG 的「或退出時」拿掉，與 TBD-10 一致。`docs/docker.md` 其他段落（`:126`、`:128`、`:223`）沒有動，符合決定 | 已照決定修好 |

### S-1 修正後各失敗路徑的 metadata 與實際部署

| 失敗點 | metadata | 檔案與 volume | 服務 | 結論 |
|---|---|---|---|---|
| 前置檢查（metadata、備份可讀、映像 digest）被拒 | 不變 | 不變 | 照常執行 | 一致（未改動） |
| 另存目前 state 失敗 | 不變 | 不變 | 重新 `start` | 一致（未改動） |
| 還原 state 失敗（`:868`） | 新版 | 檔案是新版；volume 可能只還原一部分 | 停止 | 與修正前相同。訊息列出備份與另存位置，要使用者手動還原；metadata 停在新版，重跑 `--rollback` 會再另存、再還原一次，不會把資料記錯版本 |
| 換檔案時 `cp` 失敗（`:871`，`set -e` 中止，沒有訊息） | 新版 | 檔案混合 | 有 state 備份時停止 | 與修正前相同；本機磁碟上 `cp -p` 失敗的機會極低，不列發現 |
| 寫 metadata 失敗（`:876`，`set -e` 中止） | 新版 | 都是前一版 | 有 state 備份時停止，沒有時仍是新版容器 | 修正前這一步在啟用之後，服務會在跑；修正後服務可能停著。只會在磁碟滿這類情況發生，列為限制 |
| 健康等待失敗（`:881`） | 前一版（`previousBackupPath` 指向另存目錄） | 都是前一版 | 已以前一版建立，只是沒變健康 | 一致；這就是 S-1 要修的情況 |

另外確認：`activate_docker_services` 在 `up` 之前的 `config --services` 失敗時也會印「did not become healthy」，這句不完全精確，但這行為在 d632fe3 就有，hint 仍然適用，不列發現。

### rollback 寫出的 metadata 之後再升級、再 rollback

在 scratchpad 以 HEAD 的測試工具加一個臨時案例實跑（`upgraded_with_state_backup` 之後）：

1. `FIXTURE_UNHEALTHY_TAG=v9.9.9` 跑 `--rollback`：rc=1，訊息依序是還原哪份備份、「the v9.9.7 state is kept in .wukong-backups/v9.9.9-rollback-T1/volumes」、新的失敗訊息。`opencode-state` 等於升級前、compose 檔等於升級前；metadata 是 `productTag` v9.9.9、`previousVersion` v9.9.7、`previousBackupPath` 指向 T1，沒有 `dataCompatibility` 與 `stateBackups`（與成功 rollback 寫的一樣）。T1 的 tar 解開等於 V2 的 state，T1 的 compose 檔等於 V2 的。上一份報告 S-1 說的「另存位置的訊息寫錯」「T1 沒有任何紀錄指向它」都不再發生
2. 再跑 `--rollback`：rc=1，被 `rollback_docker` 開頭的 metadata 檢查拒絕，log 只有 `compose version`、`up --help` 與 project 探測，沒有 stop、pull、up
3. 以同一個部署再 `--upgrade` 到 v9.9.7：新 metadata 的 `previousVersion` 是 v9.9.9（實際跑的版本）、`rollbackSafeTo` v9.9.9、`stateBackups` 指向新備份。上一份報告推導的「手動 `up -d` 後再升級，`previousVersion` 讀錯、之後的 rollback 被拒絕」已不成立
4. 再 `--rollback`：rc=0，`opencode-state` 與 compose 檔回到升級前。T1 留在磁碟上沒有紀錄指向它，這與任何成功 rollback 之後再升級的情況相同

「再跑一次 `--rollback` 會被拒絕」的原因：
- rollback 寫出的 metadata 沒有 `dataCompatibility`，`:824` 要求 `schemaVersion == 1` 與 `rollbackSafeTo == previousVersion`，一定不成立。以 latest 等於 v9.9.8（正式環境的情況）或明確加 `--version v9.9.8` 跑，都是在這一行（heredoc 第 6 行）被拒
- 測試裡的第二次 rollback 其實更早就被拒：latest 是 v9.9.9，與 `previousVersion` v9.9.8 不同，在 `:822` 被拒（就是 M-1）。不論哪一行，結果都是不改動任何東西就拒絕，所以這個斷言在 HEAD 成立；但它沒有區分是哪一個條件擋下的，M-1 修好後它會改由 `dataCompatibility` 那一行守住，仍然成立
- 訊息「rollback source is absent or metadata is invalid」對 rollback 之後的 metadata 並不精確，但成功 rollback 之後再跑 `--rollback` 一直都是這句（rollback 引入時就這樣），新的失敗訊息與 docs、CHANGELOG 也都寫明不要再跑；不另列發現。M-1 的情況也是這句，那邊的誤導較嚴重，已寫在 M-1

### 有 state 備份的 rollback（`test_docker_state_rollback`）

成功路徑只有「寫 metadata」與「啟用」對調，寫出的內容不變。HEAD 與換回 b30ab56 的 `install.sh` 都通過 `docker-state-rollback`；案例斷言的順序（pull → stop → 還原 → `-d --force-recreate`）、另存的 V2 state、0600、拒絕路徑都不受影響。健康等待失敗加上 state 備份的組合沒有正式案例，由上一節的臨時案例實跑確認。

### 紅燈與消融（scratchpad 副本，`git archive 2dd4369 scripts` 取出）

| 版本 | `docker-rollback` 結果 |
|---|---|
| 2dd4369 原樣 | 通過 |
| `install.sh` 換回 b30ab56 | 失敗：新的 metadata 斷言（`productTag` 仍是 v9.9.8），與 R10 的紅燈說法相同 |
| 舊 `install.sh`，測試拿掉 metadata 斷言 | 失敗：missing 'docker compose -p wukong up -d' |
| 舊 `install.sh`，測試再拿掉訊息斷言 | 失敗：a second rollback ran after the first one already went back |
| HEAD `install.sh` 只把啟用移回寫 metadata 之前（新訊息保留） | 失敗：metadata 斷言 |

三個新斷言（metadata、訊息、第二次 rollback 被拒）各自都會在舊順序下失敗，不是靠前一個斷言短路。`docker-state-rollback` 在 HEAD 與舊 `install.sh` 下都通過（成功路徑不變，見上節）。

### 測試改動是否仍測到原本的東西

- 原本的成功 rollback 保留：移到新案例前面，從乾淨的部署做「升級到 v9.9.8、rollback 回 v9.9.9」，斷言 `productTag`。修正前它是接在失敗的 rollback 之後跑，現在改成獨立的一段，涵蓋的行為一樣
- 不健康的 rollback 另起一個部署（`prepare` 重來），不再依賴前一段留下的狀態
- 上一份報告提到「斷言只看 productTag，沒有看另存位置」：新案例斷言了 `previousVersion`，沒有斷言 `previousBackupPath`；另存位置正確由上面的臨時案例確認，不列發現
- `bash -n scripts/install.sh`、`bash -n scripts/test-installer-upgrade.sh` 通過

### 增量範圍內每個變動檔案的結論

| 檔案 | 結論 |
|---|---|
| `scripts/install.sh` | S-1 照決定修好（見上面兩節）；N-G 的註解改對。M-1 在未改動的 `:822`、`:888`，不是本增量引入 |
| `scripts/test-installer-upgrade.sh` | 新斷言在舊順序下都會失敗，原本的成功 rollback 仍有覆蓋；`all` 通過。假 curl 的 latest 預設等於 rollback 目標，所以抓不到 M-1 |
| `CHANGELOG.md` | N-F、N-G 照決定修好。新的「rollback 後前一版的服務沒有變健康…不要再跑一次 `--rollback`」是 v0.25.1 使用者看得到的差異（v0.25.1 的 `ps` 失敗時被 `set -e` 停在寫 metadata 之前，沒有訊息）。「照樣先還原再啟動前一版」：v0.25.1 啟用失敗時就會換回前一版的檔案並啟動，「照樣」成立。Added 的 `--rollback` 子項目受 M-1 影響 |
| `docs/docker.md` | `:90` 的降級模式條件與 rollback 失敗後的做法與程式一致；同段前後句的張力見 N-H。`:98` 寫的 `install.sh --mode docker --rollback` 受 M-1 影響 |
| `docs/issues/issue-0044/README.md` | b30ab56：`**現行 review**` 改指 review-d632fe3.md、`## 現況` 補一句。2dd4369：`## 現況` 補「使用者決定三項都修正（TBD-12），修正後待增量覆審」、新增 TBD-12。TBD-12 是 `已解決` 附日期與結論，影響欄寫「無」，新增內容不含 `<`、行首粗體或行首 `[`，符合 docs/AGENTS.md「待確認事項」。`**狀態**: 實作完成，待審查` 與現況相符；Timeline 只記語意事件，這一輪沒有新增，符合規則 |
| `docs/issues/issue-0044/implementation-plan.md` | R10 與 diff、本次重跑的紅燈一致；「Docker 容器內一律綁非 loopback」查核成立（compose 固定 `WUKONG_WEB_HOST=0.0.0.0`） |
| `docs/issues/issue-0044/review-d632fe3.md` | b30ab56 只新增，之後沒有改動（`git diff --stat b30ab56 2dd4369` 不含它） |

### 重新查核的項目與理由

- **上一份報告 S-1 的「手動 `docker compose up -d` 後再升級」推導**
  - 理由：修正就是為了它
  - 已實跑（見「rollback 寫出的 metadata 之後再升級、再 rollback」），不再成立
- **上一份報告對 SCN-015 與 `docker-rollback`、`docker-state-rollback` 的結論**
  - 理由：rollback 的結尾順序改變，測試的前提（latest 的預設值）被本輪重新檢視
  - 成功路徑不受順序影響；但查出 M-1，上一份與更早的報告沿用的「SCN-015 有證據」只在 latest 等於 rollback 目標時成立
- **`activate_docker_services` 的其他呼叫點（升級 `:787`、升級失敗後重啟 `:800`）**
  - 理由：同一個函式
  - 函式本身沒改，兩個呼叫點也沒改，沿用上一份報告
- **沒有重跑 Rust 測試的理由**
  - `git diff --name-only d632fe3 2dd4369 -- '*.rs' 'Cargo.*'` 沒有輸出；N-G 只讀了 `main.rs`、`lib.rs`

### 其餘沿用 review-d632fe3.md

除了上面查核的項目，其餘都沿用 review-d632fe3.md（以及它沿用的 review-7df8450.md、review-c4d47d8.md）：P2-1、P2-2 的處理、邊界案例、真實 Compose 實測、需求、架構、安全、品質與 CI 三道的結論。

### 豁免、待確認與限制（本輪新增）

- 沒有 Gate 豁免紀錄；新的 `需決策` 是 M-1
- 限制：
  - 寫 metadata 失敗（磁碟滿這類情況）時，修正後服務可能停著；修正前會在跑。沒有實測
  - `docker-rollback` 的「第二次 rollback 被拒」在測試裡是被 latest 比對擋下，不是被 `dataCompatibility` 檢查擋下（見上文）；正式環境的條件由臨時案例確認
  - M-1 只以假 docker、假 curl 實跑；沒有對真實 GitHub 與真實映像跑 rollback
- 本次 reviewer 遵守的界線：沒有修改任何被追蹤的檔案（只建立本報告）；紅燈、消融與臨時案例都在 scratchpad 的副本；沒有送資料給任何模型；沒有起任何真實 Docker 容器或 project；正式容器 wukong-telegram、wukong-web、wukong-schedulerd 維持 Up 25 hours，沒有碰主機的 opencode service；`test-installer-upgrade.sh` 使用自己的假 docker；跑完以 `pgrep` 確認沒有殘留的測試程序

### 本次實際執行的命令與結果

- `git rev-parse HEAD` = 2dd4369；`git merge-base main HEAD` = `git rev-parse main origin/main` = e6a0bf9
- `git diff e6a0bf9 2dd4369 | git patch-id --stable` → `91da8ae22a8ed68b5cfcfd93dfd31d6dcdc62d51`，相符
- `git diff --stat d632fe3..2dd4369`：7 個檔案，226 行新增、18 行刪除（其中 review-d632fe3.md 173 行由 b30ab56 新增）
- `bash scripts/test-installer-upgrade.sh all`（repo 內，HEAD 2dd4369，TMPDIR 指到 scratchpad）：exit 0，「installer upgrade checks passed (all)」，99 秒；之後 `git status --porcelain` 沒有輸出
- `bash scripts/test-docker-runtime.sh`：「docker runtime persistence checks passed」
- `bash -n scripts/install.sh`、`bash -n scripts/test-installer-upgrade.sh`：通過
- scratchpad 副本：`docker-rollback`、`docker-state-rollback` 在 HEAD 與 b30ab56 的 `install.sh` 下各跑一次，以及三種消融，結果見紅燈與消融表
- scratchpad 臨時案例：不健康的 state rollback、再 rollback、再升級、再 rollback（結果見上文）；latest 等於升級目標時的 `--rollback` 與 `--rollback --version`（M-1）；latest 等於 v9.9.8 與加 `--version v9.9.8` 的第二次 rollback（都在 `dataCompatibility` 那一行被拒）
- `git log -S'assert not requested or requested == d["previousVersion"]' -- scripts/install.sh`、`git log -S'VERSION_EXPLICIT'`：都是 5b6c45f（2026-07-12），`git tag --contains` 最早 v0.18.0
- 沒有執行 `cargo test`，理由見「重新查核的項目與理由」

## 流程判定
RETURN TO execute-task

S-1、N-F、N-G 都已照使用者的決定處理好；退回的原因只有 M-1。M-1 是 v0.18.0 起就有的既有缺陷，不是本分支引入，但它讓 SCN-015 在這一版正式發布後的主要路徑上不成立。使用者決定 (a) 時修正後再送增量覆審（增量範圍 2dd4369..新 HEAD）；決定 (b) 時把 M-1 記入 Gate 豁免紀錄（日期、使用者原話、殘餘風險），依 review-evidence.md「審查強度與流程判定」，明列接受不修的 MUST FIX 不阻擋 PASS，但 (b) 需要改 docs 與 CHANGELOG，仍要再送增量覆審。
