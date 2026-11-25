# Task 6.1：隔離環境的端到端演練

> ← 回到 [README](README.md)｜[implementation-plan.md](implementation-plan.md) Task 6.1

- **日期**：2026-11-25 14:41～15:14（台北）
- **主機**：flashaim
- **環境**：獨立的 Docker daemon（`docker:27-dind`，容器 `wk44reh-dind`）。裡面除了演練部署和一個 `registry:2`，沒有其他容器
- **從**：v0.25.1，用 GitHub 上 v0.25.1 自己的 installer 全新安裝，內建 opencode 1.19.6
- **到**：本分支 `eef1bc5` 建的映像，標成假的發版 tag `v0.26.0-rc.44`，內建 @opencode/cli 2.3.2。升級與 rollback 都用本分支的 `scripts/install.sh`
- **模型**：使用者的 NVIDIA 模型 `nvidia/nvidia/nemotron-3-ultra-550b-a55b`（免費額度用完時改用，見 Task 2.6）
  - 憑證：只把 nvidia 一筆放進部署的 `opencode-state` volume，跟使用者在容器裡登入的結果相同
  - 模型設定：寫在 `opencode-config` 的 `user.json`
  - 一開始用 `nemotron-3.5-lightning-30b-a3b`，但第一個回合它沒照指示回答，跑去執行 writing-plan 技能。這是模型能力不足，不是 Wukong 的問題，所以換模型重來
- **資料**：全新的部署，只送固定的合成提示詞（暗號 ZEBRA-41、顏色 MAGENTA、水果 PAPAYA 等）。正式部署的資料一律不用
- **腳本**：
  - [scripts/rehearsal-env.sh](scripts/rehearsal-env.sh)：建 dind、準備映像與發版檔案
  - [scripts/rehearsal-webturn.sh](scripts/rehearsal-webturn.sh)：跑一個 Web 回合
  - [scripts/rehearsal-webquestion.sh](scripts/rehearsal-webquestion.sh)：跑一個會提問的 Web 回合，並回答或取消
  - [scripts/rehearsal-state.sh](scripts/rehearsal-state.sh)：唯讀查 `memory.db` 與 `opencode.db`

## 為什麼不在主機的 Docker 上演練

installer 依固定的容器名稱（`wukong-telegram`、`wukong-web` 等）找出既有部署的 Compose project（`resolve_docker_project`）。主機上正式部署的容器正用這些名稱，project 是 `runwukong`。在主機上演練，會出兩種問題：

- 演練目錄已有自己的 metadata：installer 判定兩邊衝突並中止。
- 演練目錄還沒有 metadata：installer 會認成正式部署的 project，之後的 `stop`、`up` 就作用在正式部署上。

所以演練放在 dind 裡。dind 裡沒有正式部署的任何容器。

## 與真實發版的差異

只有兩處不同，其餘照 release.yml 與 installer 原樣：

- **映像來源**：
  - 本版映像先推進 dind 裡的 registry，再拉回來，這樣才有真實的 registry digest（`sha256:076521ab…`），然後標成 `ghcr.io/raybird/wukong:v0.26.0-rc.44`。
  - installer 的 `compose pull` 只有遇到這個 tag 時不連 GHCR，由 `bin/docker` 跳過；之後的 digest 比對照常執行。
  - v0.25.1 的 pull 照常連 GHCR。
- **發版檔案來源**：
  - `bin/curl` 只把 `releases/download/v0.26.0-rc.44/` 的網址對應到本機檔案。
  - 這些檔案依 release.yml 的 Assemble public artifacts 產生：`SHA256SUMS`、`release-manifest.json`、Docker bundle。
  - 其他網址照常連網，例如 v0.25.1 的 Release 與 `raw/…/SOUL.md`。

另外，installer 在 dind 裡以 root 執行，建出的 `workspace/` 歸 root 所有，容器裡的 uid 1000 寫不進去。V1 第一個回合因此失敗，錯誤是 `failed to prepare runtime skill assets: Permission denied`。實際部署的目錄屬於使用者本人，所以演練把部署目錄的擁有者改成 1000 後再繼續。這是演練環境造成的，不是發現的問題。

## 結果總表

| # | 步驟 | 驗收編號 | 結果 |
|---|---|---|---|
| 1 | v0.25.1 全新安裝，三個入口在 V1 上跑回合 | 基準 | 通過 |
| 2 | 升級（本機模式）：停服務、備份 opencode-state、啟用 V2 | SCN-013 | 通過 |
| 3 | V1 時期的對話在 V2 上接續（同一個 session，內容含換版前的對話） | SCN-001 | 通過 |
| 4 | 本機模式：Web、CLI、排程的回合；回合結束後沒有殘留的 opencode 程序 | SCN-002 | 通過 |
| 5 | Web 提問的回答與取消 | SCN-006 | 通過 |
| 6 | 排程的提問 | SCN-006 | 回合正常結束，但模型沒有提問，自動拒絕的路徑沒有觸發 |
| 7 | rollback：V1 時期的對話可接續，V2 時期的對話不在 | SCN-015 | 通過（第一次接續碰到 NVIDIA 卡住，重跑通過） |
| 8 | V1 改成 server 模式、不設密碼，升級時 installer 備份 .env 並產生密碼 | SCN-017、SCN-013 | 通過 |
| 9 | server 模式：帶認證的 healthcheck、回合接續 | SCN-003 | 通過 |
| 10 | baseline：絕對路徑的遞迴刪除被拒、workspace 內允許、user.json 覆蓋 baseline | SCN-009 | 通過 |
| 11 | server 模式的工具進度、提問回覆、指定模型的排程 | SCN-002、SCN-006 | 通過；權限詢問沒有觸發 |
| 12 | session 維護：壓縮後接續、保留期清理預覽 | SCN-008 | 通過；沒有過期的 session 可刪 |
| 13 | 從 server 模式 rollback，V1 在 installer 產生的密碼下運作 | SCN-015 | 通過 |
| 14 | Telegram 入口 | — | 沒有跑回合，見「未驗證與限制」 |
| 15 | 取消執行中的回合 | SCN-005 | 入口沒有這項操作，見「未驗證與限制」 |

## 各步驟

### 1. v0.25.1 全新安裝與 V1 基準

```bash
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/v0.25.1/scripts/install.sh -o /reh/install-v0.25.1.sh
cd /reh/deploy && bash /reh/install-v0.25.1.sh --mode docker --version v0.25.1
```

- 第一次執行時，`release-assets.githubusercontent.com` 的 DNS 暫時查不到，installer 中止，部署沒有任何改動。重跑後成功。
- `.wukong-release`：`productTag` v0.25.1，`imageDigest` `sha256:898b73a1…`，`composeProject` wukong。
- V1 時期的回合（scope 對 session 的對應記在 `agent_session_state`）：
  - Web `user:reh-v1`：「記住 ZEBRA-41」回 `NOTED`，下一回合問暗號回 `ZEBRA-41`。
  - CLI（`docker compose -p wukong --profile cli run --rm wukong wukong --scope user:reh-cli …`）：「記住 MAGENTA」回 `OK`。
  - 排程：`wukong schedule add-turn --cron "*/2 * * * *" --scope user:reh-sched …`，由 schedulerd 依 cron 執行，`status=success`、`SCHEDULED-OK`。
- Telegram：容器在跑，log 每 5 秒印「等待設定：請在 Web /settings 填入 Telegram bot token」。
- 升級前的 opencode.db：20 張表，V1 的 `data_migration`、`session_context_epoch`、`session_input` 都在；`user:reh-v1` 對到 `ses_dee8c6af…`，有 4 則訊息。

### 2. 升級（本機模式）

```bash
cd /reh/deploy && PATH=/reh/bin:$PATH bash /repo/scripts/install.sh --mode docker --upgrade --version v0.26.0-rc.44
```

- 輸出：`backed up opencode-state to .wukong-backups/v0.25.1-1795589337/volumes/opencode-state.tar`，最後是 `Wukong upgrade completed: v0.26.0-rc.44`。
- 備份檔 1.48 MB。`.wukong-release` 的 `stateBackups` 指向這份備份，`previousVersion` 是 v0.25.1，`previousImageDigest` 是 `sha256:898b73a1…`。
- 三個服務都換成 v0.26.0-rc.44，`opencode --version` 為 `opencode v2.3.2`。
- 升級後、第一個回合之前，opencode.db 仍是 20 張表：本機模式要到回合才會啟動 V2。
- 失敗時會看到：沒有 `backed up opencode-state` 這行，或 `stateBackups` 為 `{}`。

### 3. V1 時期的對話在 V2 上接續（SCN-001）

Web `user:reh-v1` 問暗號，回 `ZEBRA-41`。

- `agent_session_state` 仍是 `ses_dee8c6af…`，`turn_count` 從 2 變 3，沒有另建 session。
- opencode.db 遷移成 29 張表，V1 的三張被刪，多了 `session_v2`。
- V2 的 `session_message` 裡，這個 session 依序是 user、assistant、user、assistant（V1 時期的兩回合），再接 user、assistant、idle（V2 的回合）。
- CLI `user:reh-cli` 問顏色，回 `MAGENTA`。
- 失敗時會看到：scope 換成新的 session id，或回答不出暗號，或 `session_message` 裡沒有 V1 時期的訊息。

### 4. 本機模式的一般回合（SCN-002）

- Web、CLI 的回合見步驟 3。SSE 有 `reasoning` 事件，思考的串流照常送到入口。
- 升級後 schedulerd 依 cron 繼續執行同一個排程，第 2、3、5、6 次都是 `success`。
- 回合結束後，`wukong-web`、`wukong-schedulerd`、`wukong-telegram` 裡的 opencode 程序都是 0 個。
- 失敗時會看到：容器裡留著 `opencode serve`，或排程在升級後不再執行。

### 5. Web 提問的回答與取消（SCN-006）

用 `rehearsal-webquestion.sh`：等 SSE 的 `question` 事件，再呼叫 `POST /api/questions/<id>/reply` 或 `/reject`。

| scope | 操作 | 結果 |
|---|---|---|
| `user:reh-q1` | 第 26 秒收到提問（RED、BLUE），回答 BLUE | HTTP 200，回合結束，回 `BLUE` |
| `user:reh-q2` | 第 25 秒收到提問，取消 | HTTP 200，回合照常結束，回 `NO-ANSWER` |

失敗時會看到：POST 回 502，或回合停在提問直到逾時。

### 6. 排程的提問（SCN-006 第二句）

建了一個要求模型提問的排程，再用 `wukong schedule trigger` 觸發。

- 回合在 42 秒內 `success`，沒有停在提問。
- 但模型照 Wukong 對無人值守回合的指示，根本沒呼叫 question 工具，直接自選了 RED。所以「排程遇到提問時自動拒絕」這條路，這次沒有實際觸發。
- 第一次 trigger 有一個插曲，屬於我的操作，不是部署的問題：
  - 我是用 `docker exec` 在容器裡執行 `wukong schedule trigger`。這個指令會在 exec 的程序裡直接跑回合。
  - `OPENCODE_CONFIG` 只由 entrypoint export 給主程序，exec 進去的程序沒有這個變數，所以沒套用 `user.json`，改用預設的免費模型。
  - 結果收到真實的 `provider.free-limit` 429，Wukong 分類為「上游限流」，排程記為 `failure`。
  - 加上 `-e OPENCODE_CONFIG=/home/wukong/.config/opencode/user.json` 重跑後，就是上面的結果。
  - 這是 V1 就有的行為（entrypoint 沒改），列在「觀察」。

### 7. rollback（SCN-015）

```bash
cd /reh/deploy && PATH=/reh/bin:$PATH bash /repo/scripts/install.sh --mode docker --rollback
```

- 輸出依序是：
  - `opencode-state: restoring the backup .wukong-backups/v0.25.1-1795589337/volumes/opencode-state.tar; conversations after it was taken are not kept`
  - 目前的 V2 state 另存到 `.wukong-backups/v0.25.1-rollback-1795589868/volumes`
  - `Wukong rollback completed: v0.25.1`
- 三個服務回到 v0.25.1，opencode 是 1.19.6。opencode.db 回到 20 張表，V1 的三張表都在。
- `user:reh-v1` 的 session 在 opencode.db 裡是 4 則訊息：V2 時期那一回合不在，跟備份時的內容相同。
- `memory.db` 在 `wukong-data`，不在備份範圍：
  - 只在 V2 時期建過 session 的 `user:reh-q1`，`agent_session_state` 仍指向 V1 資料庫沒有的 session。
  - 在 V1 上跑回合時，自動換成新的 session（`ses_dee7b344…`），回 `HELLO-AGAIN`。
- 接續 V1 時期的對話：
  - 第一次 `user:reh-v1` 的回合沒有任何 SSE 事件，300 秒後 curl 逾時。
  - web 的 log 顯示，卡住的是規劃那一棒，它用自己的暫時 session。NVIDIA 300 秒內沒有回應 header（`ProviderHeaderTimeoutError: Provider response headers timed out after 300000ms`）。同一段時間第 8 次排程也因此 `failure`，分類為「上游錯誤」。
  - 這不是 rollback 造成的。把等待上限放寬到 600 秒重跑，14 秒回 `ZEBRA-41`，仍是 `ses_dee8c6af…`，訊息從 4 則變 6 則。
- 之後第 9 次排程在 V1 上 `success`。
- 失敗時會看到：installer 拒絕 rollback，或 opencode.db 仍是 29 張表，或 V1 讀不到 session。

### 8. server 模式、沒有密碼的升級（SCN-017、SCN-013）

- 在 V1 部署的 `.env` 加上 `COMPOSE_PROFILES=server` 與 `WUKONG_AGENT_SERVER_URL=http://opencode-server:4096`，不設密碼，然後 `docker compose -p wukong up -d`。
- V1 的 opencode-server 判為 healthy。`user:reh-srv`「記住 PAPAYA」回 `NOTED`。
- 第二次升級的輸出：
  - `backed up opencode-state to .wukong-backups/v0.25.1-1795590386/volumes/opencode-state.tar`
  - `.env: generated WUKONG_AGENT_SERVER_PASSWORD for the shared opencode-server (OpenCode V2 requires one; it stays in .env)`
  - `.env: backup .wukong-backups/v0.25.1-1795590386/.env`
- `.env` 多了 48 字元的 `WUKONG_AGENT_SERVER_PASSWORD`。這組密碼在 installer 輸出裡出現 0 次。
- 失敗時會看到：沒有產生密碼，入口啟動就報錯；或者密碼被印在輸出裡。

### 9. server 模式的就緒與接續（SCN-003）

- opencode-server 的 healthcheck 是帶 basic auth 查 `/api/info`（`curl -H @-`）。啟動期間前兩次 exit 1，之後 exit 0，判為 healthy。
- `user:reh-srv` 問水果，回 `PAPAYA`，同一個 session。
- `user:reh-v1` 是在本機模式建的 session，也在 server 模式接上，回 `ZEBRA-41`。

### 10. baseline（SCN-009）

V2 的 `session_message` 記錄的工具結果：

| scope | 命令 | V2 的工具狀態 |
|---|---|---|
| `user:reh-base1` | `rm -rf /opt/wk44-nonexistent-dir` | `error`，`permission.rejected`：「Permission denied: shell」，`executed: false` |
| `user:reh-base2` | `mkdir -p /workspace/wk44-del && rm -rf /workspace/wk44-del && echo DEL-OK` | `completed`，exit 0，輸出 `DEL-OK` |
| `user:reh-tool` | `ls -d /tmp && echo TOOL-OK` | 完成，回 `TOOL-OK`，沒有詢問 |

- V2 的 server 與本機程序都照常啟動並執行回合；沒有另外逐行檢查 log 裡的設定警告，SCN-009 的設定驗證由 Task 3.1 的容器測試涵蓋。
- 回合實際用的是 `user.json` 指定的 NVIDIA 模型，所以 `user.json` 有覆蓋 baseline。
- 失敗時會看到：`rm -rf /opt/…` 的狀態是 `completed`；或者 V2 啟動就報設定錯誤。

### 11. server 模式的工具、提問與指定模型（SCN-002、SCN-006）

- 工具進度：步驟 10 的回合，SSE 都有 `tool` 事件。
- 提問：`user:reh-srvq` 第 29 秒收到提問，回答 RED，回 `RED`。
- 權限詢問：要求讀 `/etc/hostname`，結果沒有詢問就直接讀了，回 `READ-DONE`。所以權限回覆這條路在演練裡沒有觸發；它由 Task 2.5 接真實 V2 的測試涵蓋。
- 指定模型：
  - `wukong schedule add-turn --model nvidia/minimaxai/minimax-m3.1-flash …`，由 schedulerd 依 cron 執行，`success`，回 `MODEL-OK`。
  - V2 對這個 session 記錄 `model-switched`（`minimaxai/minimax-m3.1-flash`），助理訊息的模型也是它。

### 12. session 維護（SCN-008）

- Web 對 `user:reh-v1` 送 `/compact`，回「已送出壓縮指令」。
  - V2 的 `session_message` 多了 `compaction`。
  - 下一回合仍回 `ZEBRA-41`，同一個 session，`turn_count` 從 1 重新計算。
- `wukong opencode prune --dry-run`：
  - 跨分頁列出 17 個 session，其中 12 個仍被 scope 指向而受保護。
  - 將刪除 0 個，因為保留期 30 天，沒有過期的。
- 過期 session 實際被刪除，以及 7 天輪替，演練裡沒有觸發；由 Task 2.1 的測試涵蓋。

### 13. 從 server 模式 rollback

- 輸出同步驟 7，還原的是 `.wukong-backups/v0.25.1-1795590386` 這份備份。
- 4 個服務（含 opencode-server）回到 v0.25.1，opencode-server 判為 healthy。
- `.env` 保留 installer 產生的密碼，V1 在這組密碼下正常運作。
- `user:reh-srv` 問水果，回 `PAPAYA`。同一個 session，訊息從 2 則變 4 則。

## 未驗證與限制

- **Telegram 入口**：沒有跑回合。測試要有專用的 bot token；正式部署的 bot 不能借用，兩邊同時 polling 會搶走正式部署的訊息。演練只確認容器在 V1、V2 上都正常執行、沒有重啟，並在等 token。Telegram 的提問按鈕由 crate 測試涵蓋。
- **取消執行中的回合（SCN-005）**：Wukong 的取消是丟掉回合的 future，入口介面沒有提供取消執行中回合的操作。從程式看，Web 的回合跑在獨立的執行緒上，中途斷線不會中止回合（沒有實測）。演練只驗了提問的取消；回合中止送 `interrupt` 由 Task 2.3 的測試涵蓋。
- **排程遇到提問自動拒絕、權限回覆**：模型沒有觸發這兩種情況，見步驟 6、11。
- **新版啟用失敗時的還原（SCN-014）**：沒有在演練裡製造啟用失敗，由 Task 5.2 的 installer 測試涵蓋。
- **附件**：Web 入口沒有上傳附件的 API，演練沒跑附件回合；由 Task 2.2 的測試涵蓋。
- **多棒回合**：每個回合都有規劃那一棒（`--agent plan`）加主回合，但沒有特別設計要動用多個角色的提示詞。

## 觀察（不在本 issue 範圍）

- **`docker exec` 執行的 wukong 指令不套 `user.json`**：`OPENCODE_CONFIG` 只由 entrypoint export 給主程序，exec 進去的程序沒有。本機模式下，在 exec 裡 `schedule trigger` 的回合會用預設模型，不是使用者設定的模型。V1 的 entrypoint 也是這樣。
- **V2 對 free-limit 429 的回報要等一段時間**：步驟 6 第一次 trigger 時，主回合那一棒從 V2 開始跑到回報 `Free usage limit reached`，經過約 161 秒（06:53:52～06:56:33 UTC）。中間 V2 有沒有在重試、為什麼沒有觸發 Wukong 的 90 秒重試中止，這次沒查清楚。2026-11-25 之後實測（README TBD-5）：V2 對 free-limit 不重試，第一步就失敗，1～2 秒結束；額度的限流是斷續的，161 秒最可能是那一棒前幾步成功、之後才碰到 429。
- **NVIDIA 會卡住不回應**：上游 300 秒沒有回應 header 時，V1 以 `ProviderHeaderTimeoutError` 結束，分類為「上游錯誤」。
- **`.env` 的權限是 644**：installer 從 `.env.example` 複製出來就是這個權限，產生的 server 密碼也寫在裡面。這不是本 issue 新增的，但 server 模式現在一定有密碼。
- **`user.json` 被 V1 的 opencode 改寫**：加上了 `$schema`，修改時間在 V1 時期（06:45）。V2 沒有動過這個檔，rollback 不受影響。

## 清理

演練結束後，刪除 dind 容器與它的資料 volume（`wk44reh-dind-data`），以及演練目錄。它們都含有 nvidia 憑證：在 opencode-state volume 裡，也在 `.wukong-backups` 的備份檔裡。
