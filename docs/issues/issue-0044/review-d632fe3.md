# 審查報告
- 範圍：完整交付範圍（TARGET main e6a0bf9；PR #46 第一輪審查修正後的增量覆審）
- Reviewed BASE SHA：e6a0bf921fd12d65e5f95b2418eb2916b847b4d8
- Reviewed HEAD SHA：d632fe381d760f51db8f8938afe887c9676c5e04
- Reviewed patch-id：c451ca5ac7800b480110f9b989ec126410df7886
- 獨立 reviewer：Claude Code 獨立 subagent（general-purpose，claude-opus-5-5）。不是實作者，也沒有參與修正
- Review artifact：docs/issues/issue-0044/review-d632fe3.md
- 增量覆審：上一份報告 review-7df8450.md，增量範圍 7df8450..d632fe3。238df43 只保存上一份報告並記帳，d632fe3 是 PR #46 第一輪審查（issue comment 7179559848）的修正
- 審查日期：2026-11-26
- 規格基線：沿用 review-c4d47d8.md（核准 commit 94aaa40）。增量沒有改動 Gherkin 劇本，README 的變動在 `## 現況`、`**現行 review**`、`## 待確認事項`、`## Timeline` 與 `**狀態**`
- 風險等級：High（沿用上一份報告）

## 增量覆審的前提

| 條件 | 核對 | 結果 |
|---|---|---|
| 上一份報告綁定 H1、記完整範圍 | review-7df8450.md 綁定 7df8450，範圍是完整交付，判定 PASS | 成立 |
| H1 是新 HEAD 的祖先，中間沒有改寫歷史 | `git log 7df8450..d632fe3` 只有 238df43、d632fe3，兩個都是一般提交 | 成立 |
| merge-base 沒有變 | `git merge-base main HEAD` = `git rev-parse main` = `origin/main` = e6a0bf9 = 上一份報告的 BASE | 成立 |
| patch-id | `git diff e6a0bf9 d632fe3 \| git patch-id --stable` = c451ca5a…，與指定值相同 | 成立 |

## 問題與風險

### MUST FIX
無

### SHOULD FIX

**S-1. rollback 在「檔案與資料都已還原、服務卻沒變健康」時不寫 metadata，`.wukong-release` 仍記著新版；照文件「修好設定再執行」重跑 `--rollback`，會把錯的內容記成另存備份** `需決策`
- 位置：`scripts/install.sh:868-877`（`rollback_docker` 結尾）；文件 `docs/docker.md:90`「升級與 rollback 都會被判為失敗，要先修好設定再執行」
- 現況：`activate_docker_services || abort …` 發生在 state 已還原（`:868`）、release 檔案已換回前一版（`:871`）之後，`write_json_atomically .wukong-release`（`:873`）之前。所以這一步失敗時，部署的檔案與 `opencode-state` 都是前一版，metadata 卻還是新版（productTag、stateBackups、dataCompatibility 都是升級那次的）
- 本次在 scratchpad 實跑（以 HEAD 的測試工具加一個臨時案例：`upgraded_with_state_backup` 之後，`FIXTURE_UNHEALTHY_TAG=v9.9.9` 跑 `--rollback`，再正常跑一次 `--rollback`）：
  - 第一次：rc=1，訊息「rollback restored the v9.9.9 files, but its services did not become healthy within 180s; see 'docker compose -p wukong ps' …」；compose 檔與 `opencode-state` 都是升級前的內容；metadata 仍是 v9.9.7；V2 的 state 另存在 `v9.9.9-rollback-T1/volumes`
  - 第二次（模擬修好設定後重跑）：rc=0。但它把「目前」的內容另存到 `v9.9.9-rollback-T2`，裡面是 v9.9.9 的 compose 檔與升級前的 state；印出「the v9.9.7 state is kept in …-rollback-T2/volumes」，這句不實，V2 的 state 其實在 T1；最後的 metadata `previousBackupPath` 指向 T2，T1 沒有任何紀錄指向它
  - 若使用者改成手動 `docker compose up -d`、之後再 `--upgrade`：`previous_version` 會從 metadata 讀成 v9.9.7（實際跑的是 v9.9.9），新 metadata 的 `previousVersion` 與 `rollbackSafeTo` 對不上，之後的 `--rollback` 會被 `:821-824` 的 assert 拒絕（依程式推導，未實跑）
- 影響：資料沒有遺失（T1 仍在磁碟上、0600），重跑 `--rollback` 的執行結果也正確；錯的是另存位置的訊息與 metadata，以及手動路徑會讓之後的 rollback 失效
- 為什麼不是 MUST FIX：這個「先換檔案與資料、最後才寫 metadata」的順序從 rollback 引入時就是這樣，7df8450 的 `activate_docker_services` 與 `ps` 失敗時也會被 `set -e` 停在同一點（而且沒有訊息）。d632fe3 沒有引入它，但把觸發條件從「compose 命令失敗」擴大到「服務不健康」，包括使用者決定要算失敗的 Web 降級模式，並在文件裡指示使用者重跑
- 待決問題：rollback 的檔案與資料已經換回、只有健康檢查失敗時，metadata 要記成哪一版
- 可選做法：
  - (a) 先寫 rollback 後的 metadata（previousBackupPath 指向 T1），再檢查健康；不健康時仍以非零結束，訊息改成「檔案與資料已回到 {前一版}，但服務沒有變健康；修好設定後執行 `docker compose -p {project} up -d`，不要再跑 `--rollback`」
  - (b) 維持不寫 metadata，訊息改成要使用者修好後重跑 `--rollback`，並寫出 V2 的 state 在 T1；重跑時偵測檔案已是前一版、不再另存一份
  - (c) 接受現況，記入 README 待確認事項
- reviewer 建議：(a)。它讓 metadata 與實際部署一致，也符合「不健康算失敗」的決定（結束碼仍非零），改動集中在 `rollback_docker` 結尾幾行；需要在 `test_docker_rollback` 或 `test_docker_state_rollback` 補一個斷言 metadata 與另存位置的案例。改動 `install.sh` 會讓本報告的 PASS 失效，要再送增量覆審

### NICE TO HAVE

**N-F. CHANGELOG 第二條新條目描述的是分支上的中間狀態，v0.25.1 沒有這個行為**
- 位置：`CHANGELOG.md:43-44`「升級要備份的 volume 清單讀不出來時，installer 在停服務之前中止。以前 release manifest 列了不合法的名稱時，清單會被截斷，可能在沒有備份的情況下升級」
- 查核：`git show v0.25.1:scripts/install.sh | grep -n 'required_state_backups\|backup_docker_state\|stateBackups'` 沒有輸出，v0.25.1 根本不備份 volume；「以前會截斷」只在本分支 d632fe3 之前成立。與 review-c4d47d8 N-B 同一類
- 第一條新條目大致正確：v0.25.1 的確是 `up -d --force-recreate` 加 `ps`（`:671`、`:707`），「不再只看容器有沒有建立」與「Compose 必須支援 `up --wait-timeout`」都是 v0.25.1 使用者看得到的差異；只有「照樣先還原備份再啟動前一版」的「照樣」暗示以前就有，還原備份本身是 Added 那條的新功能
- 建議：刪掉第二條，在 Added「Docker 升級前會備份 `opencode-state`」底下補一句「要備份的清單讀不出來時，在停服務之前中止」。可以併進 TBD-8 發版時整理 CHANGELOG 的那次提交

**N-G. 降級模式的觸發條件寫成「Web 對外開放卻沒設 token」，在 Docker 裡不精確；CHANGELOG 對沒有 healthcheck 的服務說得太滿**
- 位置：`CHANGELOG.md:41-42`、`docs/docker.md:90`、`scripts/install.sh:591` 的註解
- 查核：`crates/wukong-web/src/lib.rs:263-266` 的 `should_refuse_insecure_start` 看的是容器內的 host、token 與 `WUKONG_WEB_ALLOW_INSECURE == "1"`。compose 固定 `WUKONG_WEB_HOST=0.0.0.0`，`WUKONG_WEB_ALLOW_INSECURE` 預設 `1`（`docker-compose.release.yml`，v0.25.1 也是），所以只設 `WUKONG_WEB_BIND=0.0.0.0`、沒設 token 時服務照常健康，不會讓升級失敗；真正會進入降級模式的是「`WUKONG_WEB_ALLOW_INSECURE` 不是 `1` 且沒設 token」，與 `WUKONG_WEB_BIND` 無關（綁 127.0.0.1 也會）
- 同一個不精確的說法早就在 `docs/docker.md:126`、`:128`、`:223`（2026-07-08 起），不在增量範圍；新條目把它帶進 CHANGELOG，使用者會拿它判斷自己受不受影響
- 另外 CHANGELOG 寫「新版啟動後才變 unhealthy 或退出時，照樣先還原」，但沒有 healthcheck 的服務啟動就退出時只是盡力而為（R8 實測 7 次 2 次判成功，README TBD-10）；`docs/docker.md:90` 有寫這個限制，CHANGELOG 沒有
- 建議：改成「`WUKONG_WEB_ALLOW_INSECURE` 不是 1 又沒設 `WUKONG_WEB_TOKEN`（刻意回報 unhealthy 的降級模式）」，「或退出時」限定為有 healthcheck 的服務。只影響措辭，可以與 N-F 一起在發版時改；改 CHANGELOG、docs 或 install.sh 會讓本報告的 PASS 失效

## 已查核維度

### PR #46 第一輪審查各項的處理確認

| 項目 | 使用者決定 | 本次查核 | 結論 |
|---|---|---|---|
| P2-1 健康驗證不足 | 修正：`up --wait --wait-timeout 180`，Compose 不支援就在改動前中止；降級模式也算失敗 | `activate_docker_services`（`:593-601`）改為 `up --wait --wait-timeout "$DOCKER_WAIT_SECS" -d --force-recreate`，兩處 `ps` 拿掉；三個呼叫點（升級 `:787`、升級失敗後重啟前一版 `:800`、rollback `:872`）都經過它。`--help` 檢查在 `install_docker` 開頭（`:738-739`），位於 `resolve_docker_project`（唯讀）、rollback 分支與 `prepare_release_metadata` 之前，全新安裝、升級、rollback 都在任何下載與改動之前中止。紅燈、消融與真實 Compose 實測見下方 | 已修好；rollback 失敗後的 metadata 見 S-1 |
| P2-2 備份清單解析失敗被忽略 | 修正：停服務前中止 | `state_list="$(required_state_backups …)" \|\| abort`（`:770`），`state_list` 在 `:746` 的 `local` 一起宣告，賦值的結束碼就是 python 的結束碼，不會被 `local` 吃掉。空字串不讀入（`:771`），單一名稱與多名稱由 here-string 逐行讀入。位置在 `stop`（`:775`）之前；之前只建立了 `.wukong-backups/{前一版}-{時間}` 並複製舊檔，部署本身沒變 | 已修好 |
| P2-2 建議的另兩種負向案例 | 實作者說已由 `validate_manifest_version` 擋下 | `:299-304` 在 `prepare_release_metadata` 呼叫它；它斷言 `dataCompatibility` 存在、`affectedState` 是 list，JSON 解析失敗也會 `SystemExit`。說法成立 | 不需另加測試 |
| P3 版本快取 | 記入 TBD-11，不修 | `opencode_version.rs:13-63`：只在確認 2.x 時寫入 `CONFIRMED`，執行不了、逾時、看不懂都不快取；TBD-11 的事實與程式一致 | 處理方式符合決定 |

### 邊界案例

| 案例 | 預期 | 查核 | 結論 |
|---|---|---|---|
| profile 未啟用的服務（`wukong` 是 profile cli、`opencode-server` 是 profile server） | `up --wait` 不建立也不等待 | 真實 Compose 2.35.1、alpine 拋棄式 project wk46rv1：`config --services` 只列 bg、web；`up --wait` 只建立這兩個，6 秒 rc=0；`ps -a` 沒有 cli、srv | 成立 |
| 使用者在 `COMPOSE_PROFILES` 放了 cli | 不在支援範圍 | 同一 project 加 `COMPOSE_PROFILES=cli,server`，一次性的 cli 容器結束（exit 0）就讓 `up --wait` rc=1。真正的 `wukong` 服務是帶 tty 的 REPL，不會立刻結束，文件也沒有建議把 cli 放進 `COMPOSE_PROFILES` | 不列發現 |
| Memoria overlay（`docker-compose.memoria.yml` 的 `memoria-runtime` 是跑完就結束的一次性服務，其他服務 `depends_on: service_completed_successfully`） | 不應每次升級都失敗 | 拋棄式 project wk46rv2 重現同樣結構，連跑 3 次 `up --wait --wait-timeout 60 -d --force-recreate`：都是 rc=0、7 秒，一次性容器顯示 Exited (0)。Compose 2.35.1 不把成功結束的依賴當失敗 | 成立；較舊但有 `--wait-timeout` 的 Compose 是否相同沒有驗證，列為限制 |
| wukong-web 的 healthcheck（interval 30s、start_period 10s、retries 3）健康時 | 遠低於 180 秒變 healthy | 同參數的拋棄式服務：6 秒 healthy（Engine 28.1.1 在 start period 內以預設間隔探測） | 成立 |
| 同參數但一直不健康（降級模式的情況） | 在逾時之前回報失敗 | 71 秒 rc=1，「container wk46rv1-web-1 is unhealthy」，早於 180 秒。opencode-server（start_period 60s、start_interval 2s、interval 30s、retries 3）依同樣規則約 120～150 秒判 unhealthy，也在 180 秒內 | 成立 |
| 升級失敗後重啟前一版也走 `--wait`，`\|\| true` | 前一版不健康時不擋住後續訊息 | `:800`：`[[ "$state_restored" == false ]] \|\| activate_docker_services \|\| true`。前一版也不健康（例如同一份 `.env` 觸發降級模式）時照樣走到 abort，訊息附 `docker_health_hint`；部署回到升級前的狀態（原本就不健康）。最壞情況兩次等待共約 6 分鐘 | 可接受；訊息沒有區分「前一版也不健康」，hint 足以查到 |
| `set -e` 與 `\|\|` | 失敗要傳回呼叫端 | `activate_docker_services` 在 `if !` 與 `\|\|` 之下執行，函式內 `set -e` 失效，但每個命令都有明確的 `\|\| return`，最後一行 `up` 的結束碼就是函式的結束碼 | 成立 |
| `docker compose up --help \| grep -Fq` 在 `pipefail` 下的 SIGPIPE | 支援時不能誤判為不支援 | `strace` 看到 compose 以單一 write 寫出整份 4269 bytes 說明，`grep -q` 不會在寫完前關閉管線；`pipefail` 下連跑 300 次 0 次誤判 | 成立（只驗證 2.35.1） |
| Compose 不支援 `--wait-timeout` | 在任何改動前中止 | `docker-compose-wait` 案例斷言 metadata、compose 檔不變，log 沒有 stop、pull、force-recreate | 成立 |
| 清單為空、單一名稱、多名稱 | 空清單不停服務；有名稱照常備份 | 拿掉 `:771` 的空字串防護後，`docker-state-backup` 失敗（「services stopped without a required backup」，空清單被讀成一個空名稱），表示現有測試會守住這個防護；單一名稱由 `docker-state-backup`、`docker-state-rollback` 覆蓋；多名稱沒有直接案例，here-string 逐行讀入的語意與 v0.25.1 以來 `mapfile` 的用法相同 | 成立 |

### 紅燈與消融（scratchpad 副本，`git archive d632fe3 scripts` 取出，各自只改 `install.sh`）

| 版本 | 失敗的案例（第一個 FAIL） | 通過的案例 |
|---|---|---|
| d632fe3 原樣 | 無 | 下列 15 個全部通過 |
| 換回 238df43 的 `install.sh` | `docker`、`forced-upgrade`、`docker-compose-wait`（缺 `up --wait --wait-timeout 180 -d --force-recreate`）；`docker-env-migration`（unhealthy Docker install succeeded）；`docker-state-restore`（unhealthy upgrade succeeded）；`docker-state-backup-list`（upgrade went ahead with an unreadable backup list: ["../escape", "opencode-state"]）；`docker-rollback`（rollback to an unhealthy release succeeded）；`docker-recovery`（unhealthy Docker upgrade succeeded） | 其餘 7 個 |
| 只拿掉 `--wait --wait-timeout` | 與上一列相同，只少了 `docker-state-backup-list` | 其餘 8 個 |
| 只拿掉 `--help` 檢查 | `docker-compose-wait`（upgrade went ahead without a way to wait for health） | 其餘 14 個 |
| 只把清單改回 process substitution | `docker-state-backup-list`（第一組） | 其餘 14 個 |
| 改回 process substitution，測試只留第三組 `["opencode-state", "../escape"]`（清單被截斷的情況） | `docker-state-backup-list` | — |
| d632fe3，測試只留第三組 | 無 | 通過 |
| 只拿掉空字串防護 | `docker-state-backup`（services stopped without a required backup） | 其餘 14 個 |

跑的 15 個案例：docker、docker-env-migration、docker-env-server-password、upgrade-noop、forced-upgrade、docker-state-backup、docker-state-restore、docker-state-rollback、docker-state-backup-list、docker-compose-wait、docker-rollback、docker-recovery、docker-project、docker-project-conflicts、docker-project-persistence。

實作者在 R8、R9 說的紅燈與消融都成立；第三組（截斷）在舊寫法下單獨也會失敗，不是靠第一組的短路。

### 測試改動是否仍測到原本的東西

- `FIXTURE_DOCKER_PS_EXIT` → `FIXTURE_UNHEALTHY_TAG`：舊 fixture 讓 `ps` 失敗，模擬的是「啟用後檢查失敗」；`ps` 拿掉之後，新 fixture 讓帶 `--wait` 的 `up` 在 compose 檔是指定版本時失敗，模擬的是同一件事，而且容器照樣建立、`FIXTURE_MIGRATING_TAG` 的遷移照樣發生（它比對的是 ` up ` 與 `-d --force-recreate` 結尾，有沒有 `--wait` 都會觸發）。所以 `docker-state-restore` 仍驗證「遷移過的 state 被還原」。用到舊 fixture 的四個案例（env-migration、state-restore 兩段、recovery）全部改用新 fixture，`git grep FIXTURE_DOCKER_PS_EXIT d632fe3` 沒有殘留
- 不是假綠燈：拿掉 `--wait` 時新 fixture 不會觸發，那四個案例全部失敗（見消融表），所以它們量的是 `--wait` 本身，不是 fixture 自己
- 比對啟用命令的方式：順序檢查由「`up -d --force-recreate` 結尾」改為「` -d --force-recreate` 結尾」，`grep -c` 由 `up -d --force-recreate` 改為 `force-recreate`。log 裡只有 `up` 會帶 `--force-recreate`，放寬後不會多算其他命令；`docker`、`forced-upgrade` 兩案另外斷言完整的 `up --wait --wait-timeout 180 -d --force-recreate`，參數順序或秒數被改掉都會被抓到
- `docker-rollback` 的新段落：斷言 metadata 不改寫、訊息有 hint；之後正常 rollback 的斷言只看 productTag，沒有看另存位置，所以沒有抓到 S-1
- `bash -n scripts/test-installer-upgrade.sh`、`bash -n scripts/install.sh` 通過

### 增量範圍內每個變動檔案的結論

| 檔案 | 結論 |
|---|---|
| `scripts/install.sh` | P2-1、P2-2 已修好（見上表）；rollback 失敗後的 metadata 見 S-1；註解的降級模式條件見 N-G |
| `scripts/test-installer-upgrade.sh` | 新 fixture 與兩個新案例有效，沒有變成假綠燈（見上節）；`all` 通過 |
| `CHANGELOG.md` | 第一條大致正確，第二條是中間狀態（N-F）；降級模式條件與沒有 healthcheck 的服務見 N-G |
| `docs/docker.md` | 健康等待、Compose 需求、hint、沒有 healthcheck 的限制與程式一致；降級模式條件見 N-G。`:148` 的手動 `up -d --force-recreate` 是使用者自己執行的命令，不需要改 |
| `docs/issues/issue-0044/README.md` | 238df43：`## 現況`、`**現行 review**`、TBD-6 狀態（N-D 已改成 `不影響本次交付`，附「追蹤：無」與理由）、TBD-8、`**狀態**: 已完成`。d632fe3：`## 現況` 補 PR #46、TBD-9～11、Timeline 一列、`**狀態**` 改回「實作完成，待審查」。新增內容都不含 `<`、行首粗體或行首 `[`。TBD-9 是 `已解決` 附日期與結論；TBD-10、TBD-11 是 `不影響本次交付`，「追蹤：無」後面接與交付範圍相關的具體理由，影響欄只寫影響與關聯 SCN，符合 docs/AGENTS.md「待確認事項」。TBD-10 的「那兩個服務不遷移 `opencode.db`」指的是啟動時；按需模式下它們的第一個回合會遷移，這點同一列前半已寫明，不另列發現 |
| `docs/issues/issue-0044/implementation-plan.md` | R8、R9 與 diff、本次重跑的紅燈、消融、真實 Compose 實測一致；R9「無效 JSON 與缺少 `dataCompatibility` 早就由 `validate_manifest_version` 擋下」經查核成立 |
| `docs/issues/issue-0044/review-7df8450.md` | 238df43 只新增，之後沒有改動（`git diff --stat 238df43 d632fe3` 不含它） |

### 重新查核的項目與理由

- **`activate_docker_services` 的所有呼叫點**
  - 理由：函式行為由「建立容器」改為「建立並等健康」
  - `git grep activate_docker_services d632fe3` 只有 `:593` 定義與 `:787`、`:800`、`:872` 三處呼叫，都已在上表查核
- **其他會執行 installer 或檢查它內容的腳本**
  - 理由：它們可能依賴 `up -d --force-recreate` 字樣或 `ps`
  - `scripts/test-docker-runtime.sh` 只檢查 `DOCKER_RELEASE_OWNED` 與沒有 `build`、`down`，本次重跑通過；`scripts/rehearse-rc.sh` 呼叫 installer 後自己跑 `docker compose ps wukong-web`，不依賴 installer 內部；`.github/workflows/release.yml` 只複製檔案
- **上一份報告失敗面表的「新版啟用或健康檢查失敗」一列**
  - 理由：判定方式從 `ps` 改為 `--wait`
  - 覆蓋改為 installer 斷言（舊版與拿掉 `--wait` 都會失敗）加真實 Compose 2.35.1 的拋棄式實測；沒有 healthcheck 的服務啟動就退出只是盡力而為，已記入 TBD-10
- **上一份報告的 N-C（新建 `.env` 為 0600）**
  - 理由：`test_docker` 有改動（斷言的命令字串）
  - `test_docker` 在 HEAD 通過，結尾的 0600 斷言沒有被改動
- **沒有重跑 Rust 測試的理由**
  - `git diff --name-only 7df8450 d632fe3 -- '*.rs' 'Cargo.*'` 沒有輸出；上一份報告沿用的 `cargo test`、fmt、clippy 結果仍適用於同一份 Rust 程式碼。TBD-11 只讀了 `opencode_version.rs`，沒有改動

### 其餘沿用 review-7df8450.md 與 review-c4d47d8.md

除了上面查核的項目，其餘都沿用：各項修正的確認、相關失敗面、需求、架構、安全、品質與 CI 三道的結論。

### 豁免、待確認與限制（本輪新增）

- 沒有 Gate 豁免紀錄；新的 `需決策` 是 S-1
- 限制：
  - `--wait` 的真實行為只在 Docker Compose 2.35.1、Engine 28.1.1 驗證；有 `--wait-timeout` 但較舊的 Compose 對「成功結束的一次性依賴」（Memoria overlay）是否同樣不算失敗，沒有驗證
  - 沒有用真實 Wukong 映像跑 `--wait`，健康與不健康的時間是以相同 healthcheck 參數的 alpine 服務量測
  - opencode-server 第一次啟動的遷移若超過約 120～150 秒會被判 unhealthy 而還原；正式規模（約 42 MB）的遷移時間沒有量過（review-c4d47d8 已列），失敗方向是安全的
  - S-1 手動 `docker compose up -d` 後再升級的推導沒有實跑
- 本次 reviewer 遵守的界線：沒有修改任何被追蹤的檔案（只建立本報告）；紅燈與消融都在 scratchpad 的副本；沒有送資料給任何模型；拋棄式 project 只有 wk46rv1、wk46rv2，結束時各自 `down`，之後 `docker ps -a --filter name=wk46rv` 與 `docker network ls --filter name=wk46rv` 都沒有輸出；正式容器 wukong-telegram、wukong-web、wukong-schedulerd 維持 Up 25 hours，沒有碰主機的 opencode service；`test-installer-upgrade.sh` 使用自己的假 docker；跑完以 `pgrep` 確認沒有殘留的測試程序

### 本次實際執行的命令與結果

- `git rev-parse HEAD` = d632fe3；`git merge-base main HEAD` = `git rev-parse main origin/main` = e6a0bf9
- `git diff e6a0bf9 d632fe3 | git patch-id --stable` → `c451ca5ac7800b480110f9b989ec126410df7886`，相符
- `git diff --stat 7df8450..d632fe3`：7 個檔案，270 行新增、28 行刪除
- `gh api repos/raybird/Wukong/issues/comments/7179559848 --jq .body`：讀取 PR #46 第一輪審查
- `bash scripts/test-installer-upgrade.sh all`（repo 內，HEAD d632fe3，TMPDIR 指到 scratchpad）：exit 0，「installer upgrade checks passed (all)」，1 分 38 秒；之後 `git status --porcelain` 沒有輸出
- `bash scripts/test-docker-runtime.sh`：exit 0，「docker runtime persistence checks passed」
- `bash -n scripts/test-installer-upgrade.sh`、`bash -n scripts/install.sh`：通過
- scratchpad 副本的 15 個案例 × 6 個版本，以及兩個只留第三組的版本：結果見紅燈與消融表
- scratchpad 副本的臨時案例（不健康的 rollback 後重跑 rollback）：結果見 S-1
- 真實 Compose：wk46rv1 健康（6 秒 rc=0）、不健康（71 秒 rc=1）、`COMPOSE_PROFILES=cli,server`（6 秒 rc=1，一次性容器 exited (0)）；wk46rv2 一次性依賴（3 次都 7 秒 rc=0）；兩個 project 都已 `down`
- `pipefail` 下 300 次 `docker compose up --help | grep -Fq -- '--wait-timeout'`：0 次失敗；`strace -e trace=write` 看到單一 4269 bytes 的 write
- `git show v0.25.1:scripts/install.sh | grep …`、`git show v0.25.1:docker-compose.release.yml | grep …`：N-F、N-G 的查核
- 沒有執行 `cargo test`，理由見「重新查核的項目與理由」

## 流程判定
PASS

PASS 只適用於上列 Reviewed HEAD SHA；之後提交 README 記帳區段與本報告以外的檔案會讓它失效，提交前先告知使用者（review-evidence.md「審查之後的提交」）。
