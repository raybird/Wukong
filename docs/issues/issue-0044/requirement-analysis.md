# 需求分析

2026-11-24 讀 main feeead8。V2 介面差異的實測見 [#30 inventory.md 步驟 4](../issue-0030/inventory.md#步驟-4opencode-v2232最小契約與遷移回復探測)，本檔不重抄。

## 需求描述

把 Wukong 對 OpenCode 的依賴從 1.x 換成 V2（@opencode/cli 2.3.2），三個入口、CLI、排程與 installer 的對外行為維持 V1 時的樣子。來源與決策見 [README](README.md) 的核准紀錄。

## 現況分析

### gateway 的三條路徑

`crates/wukong-gateway/src/backend.rs` 的 `build_backend_from_env` 依環境選 backend：

| 條件 | backend | 與 OpenCode 的介面 |
|---|---|---|
| `WUKONG_AGENT_SERVER_URL` 非空 | `OpencodeServerBackend`（`opencode_server.rs`） | 常駐 server 的 V1 HTTP API；選用 `WUKONG_AGENT_SERVER_USERNAME`／`PASSWORD` 做 basic auth |
| 命令剛好是 `opencode run`（去掉 `--model` 後只剩兩個字） | `LocalProcessBackend`（`local_process.rs`） | 每次操作起一個 `opencode serve --port 0`，等 stdout 的 `opencode server listening on`，再用 `OpencodeServerBackend` 打 HTTP；啟動前移除 `OPENCODE_SERVER_PASSWORD`，改由 `WUKONG_AGENT_SERVER_*` 帶入 |
| 其他命令 | `AgentCliBackend` | 以 `stdin=null` 執行命令；是 opencode 時加 `--print-logs --log-level ERROR`、`-s`、`--thinking`、`--model`、`--agent`、`--file`、`--format json`，解析 NDJSON（`stream.rs`）；不是 opencode 時（`printf fixer` 等假 agent）不加這些旗標 |

compose 與 `.env.example` 預設 `WUKONG_AGENT_CMD=opencode run`、URL 空值，所以預設是本機按需模式；flashaim 的 RunWuKong 也是（#30 步驟 1）。`opencode-server` 容器在 `server` profile，compose healthcheck 為 `curl -fsS http://localhost:4096/global/health`。

提問與權限回覆（`backend.rs` 的 `question_reply_route`）只有 server 類 backend 支援，permission 以 question 形式浮出、回到 `/permission/{id}/reply`。上游錯誤分類在 `upstream_error.rs`，主判依據是 opencode 事件裡具名錯誤的 `name` 與 `data.statusCode`（地雷 W-7）。

### opencode 設定 baseline

`scripts/docker-entrypoint.sh` 每次啟動重寫 `~/.config/opencode/opencode.json`（snapshot、compaction、tool_output、watcher、`permission.external_directory` 放行 /tmp、`permission.bash` 的 rm 黑名單），`OPENCODE_CONFIG` 指向使用者的 `user.json` 覆蓋。V2 對設定做嚴格驗證、欄位錯就拒絕啟動，權限動作名稱也改了（例如 bash 改為 shell，見 V2 遷移文件）。

### 打包與版本釘選

`release/runtime-inputs.env`、`release/package.json`、`release/package-lock.json` 釘選 opencode-ai 的版本與 integrity；release CI 先跑 `scripts/resolve-opencode-version.sh`，把 npm latest 寫進三個檔（`OPENCODE_VERSION_PIN` 可固定版本），`scripts/test-release-image.sh` 檢查一致。`Dockerfile.release` 以 `/opt/wukong-release/node_modules/.bin/opencode` 連到 `/usr/local/bin/opencode`。V2 套件改名為 @opencode/cli，原生執行檔在平台子套件（linux-x64 約 253 MB）。

### installer 的備份與回復

- Docker 升級（`install_docker`）：備份 `DOCKER_RELEASE_OWNED` 的 release 檔案與 `.env` 到 `.wukong-backups/`，拉映像、驗 digest、啟用；啟用失敗只還原 release 檔案與 `.env`。**沒有備份任何 volume。**
- `release/data-compatibility.json` 有 `affectedState`、`backupRequired`、`irreversibleMigration`、`rollbackSafeTo`；installer 只驗型別，`backupRequired` 沒有實際作用。
- Docker `--rollback`（`rollback_docker`）：要求新版的 `irreversibleMigration` 為 false 且 `rollbackSafeTo` 等於前一版，還原 release 檔案後拉前一版映像啟用。**不碰 volume。**
- binary 模式：installer 只管理 Wukong 的執行檔與 systemd unit，不安裝 opencode；使用者 PATH 上的 opencode 由使用者自己裝。

## 問題點總結

1. gateway 三條路徑用到的每一條 V1 路徑、事件名稱、回應形狀、啟動訊息與認證在 V2 都變了（#30 步驟 4），不改的話本機模式每回合卡到逾時。
2. V2 的 `opencode run` 預設連背景 service，`--print-logs` 的 server 日誌要 `--standalone` 才有；照現在的 argv 執行，CLI backend 會留下常駐程序，上游錯誤的 stderr 也可能拿不到。
3. V2 沒設密碼時自動產生隨機密碼，本機模式「移除密碼、不帶認證」的做法會讓每個請求回 401；server 模式的入口也無從得知密碼（TBD-2）。
4. `/global/health` 在 V2 回網頁 HTML 與 200：gateway 的健康檢查會失敗，compose healthcheck 卻是假綠燈。
5. baseline 若不改寫成 V2 格式，V2 可能拒絕啟動，或權限黑名單失效。
6. V2 就地、單向遷移 opencode.db，而 installer 升級與 rollback 都不備份 volume；rollback 的資料相容性檢查目前只能二選一（宣告不可逆而拒絕 rollback，或宣告可逆卻沒有資料備份）。
7. release CI 會跟 npm latest，V2 小版本的變動會直接進映像；套件改名後解析對象也錯了。

## 目標

- gateway 三條路徑改用 V2 介面，SCN-002～SCN-011 的行為與 V1 時相同。
- release 映像固定 @opencode/cli 2.3.2（SCN-012）。
- Docker installer 從 V1 版本升級時備份 opencode-state，失敗與 rollback 時一起還原（SCN-013～SCN-015）；binary 模式在 docs 說明使用者自行升級與備份。
- 換版前先證明 V1 session 在 V2 可接續（SCN-001），不成立就修訂規格。
