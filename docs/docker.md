# Docker 容器化部署

> ← 回到 [主 README](../README.md)｜相關文件:[安裝指南](installation.md)、[各進入點](entrypoints.md)

提供完整的 Docker / Docker Compose 配置，隔離 host 環境，同時滿足 opencode 工作空間掛載與設定隔離需求。

**特點：**
- **Host 工作目錄掛載**：opencode 工作空間透過 volume 掛載 host 路徑
- **opencode 設定與 session 隔離**：`~/.config/opencode` 與 `~/.local/share/opencode` 都存放在 Docker volume 中，不污染 host，且可跨容器升級保留 session
- **UID/GID 對齊**：runtime user 與 host 一致，避免檔案權限問題
- **預設 Web + Telegram + Scheduler**：`docker compose up -d` 會啟動 Web Console、Telegram Bot 與排程 daemon；CLI / REPL 透過被動 `run` 使用
- **按需執行與互動（2026-10-04）**：預設每次執行在入口容器內啟動 OpenCode 控制程序，監聽 loopback 可用埠，完成、錯誤、逾時或取消後回收。CLI／REPL、Web、Telegram 都支援問答，排程依 Reject／AllowOnce 處理權限，一般問題一律拒絕。
- **權限設定**：entrypoint 的 baseline 保留 destructive-rm denylist 與 `/tmp` 放行；自訂 allow／ask／deny 寫入 `user.json`。它是防呆護欄，隔離邊界仍是容器及 host 掛載目錄。

### opencode 設定分層（baseline / user）

`opencode-config` volume 內有兩份設定，opencode 會**深度合併**它們，後者的鍵勝出：

| 檔案 | 誰維護 | 行為 |
|------|--------|------|
| `opencode.json` | Wukong | **每次容器啟動都覆寫**成映像檔內的 baseline。不要手改 |
| `user.json` | 你 | 由 `OPENCODE_CONFIG` 指向；只在缺檔時建立，之後 Wukong 永不覆寫 |

這個分層是為了讓**新版預設能隨映像檔升級自動生效**。舊版邏輯只在缺檔時 seed，導致既有部署永遠拿不到新預設——v0.18.7 的 CPU guardrail 與 `external_directory` 規則都因此沒進到執行中的主機。

- **自訂設定**：寫進 `user.json`，例如把某個操作改成 `ask`、加自己的 deny 規則、換 model。
- **升級行為**：新版 baseline 新增的鍵自動生效；你在 `user.json` 設過的鍵維持不變。
- **首次升級遷移**：若既有 `opencode.json` 是舊版 seed 出來的（沒有 `.wukong-baseline` 標記），entrypoint 會備份成 `opencode.json.pre-baseline.bak`，並把內容複製到 `user.json`，避免手改過的規則消失。想回到純預設，刪掉 `user.json` 再重啟即可。
- **注意合併順序**：opencode 以「最後符合的規則勝出」解析權限，而兩個 rule 物件合併後，你的鍵落在順序中的哪個位置沒有保證。自訂規則請寫得具體、自足，不要依賴它與 baseline 規則的相對順序。

## 按需執行與選用共用 server（2026-10-04）

預設 `WUKONG_AGENT_CMD=opencode run`、`WUKONG_AGENT_SERVER_URL` 留空。Wukong 在需要執行時啟動本機 `opencode serve`，回合結束就停止；閒置時沒有 OpenCode 控制程序。session 與設定仍在既有 volumes 中，跨程序、容器升級都能續接。這個方案使用本機 HTTP 控制 API，並非純 `opencode run` 的 stdin 問答。

`docker compose run --rm wukong` 可使用 REPL；問題輸入選項編號或文字，多選以逗號分隔，`/cancel` 或 EOF 取消。Web／Telegram 維持原問答介面。各入口現在自行執行 agent，預設上限為 1.5 CPU、2 GiB、256 PIDs；同時執行多個入口會累加資源需求，每次啟動也會增加延遲。

需要較低啟動延遲時，可在 `.env` 明確開啟共用 server：

```dotenv
COMPOSE_PROFILES=server
WUKONG_AGENT_SERVER_URL=http://opencode-server:4096
```

再執行 `docker compose up -d`。啟用 profile 與 URL 是兩個必要設定；連接自行管理的遠端 server 則只設定 URL。共用 `/workspace` 時附件用 `shared`，沒有共享檔案系統的遠端可改用 `inline`（單檔 10 MiB）。本機程序固定使用入口的工作目錄與共享檔案，不套用遠端路徑映射。

升級到按需模式時，移除 `.env` 的 `COMPOSE_PROFILES=server` 與非空 URL，將舊的 `WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions` 改為 `opencode run`。額外 run 旗標會保留純 CLI 路徑，沒有問答回覆通道。installer 會停止同一 project 已停用的 `opencode-server`；手動更新 Compose 時先執行以下命令，因為 profile 停用與 `--remove-orphans` 不會停止舊的 profile 容器：

```bash
docker compose --profile server stop opencode-server
docker compose up -d
```

本機模式不啟用週期性 server 重啟、session 保留期刪除或啟動前 vacuum；這些功能僅適用於明確啟用的共用 server。輔助回合與 `/new` 的 session 清理仍執行。

**快速開始：**

若你不是從 Git repository 使用，而是在空目錄部署，建議直接使用 installer：

```bash
mkdir wukong-docker && cd wukong-docker
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode docker
```

installer 會從 GitHub Release 下載並驗證 `SHA256SUMS`、`release-manifest.json` 與 Docker bundle，再從 GHCR pull manifest 指定的 immutable image digest，因此不需要 Rust、原始碼或本機 Docker build。

**升級既有 installer Docker 部署：**

若你當初是用 `install.sh --mode docker` 在空目錄產生部署檔案，請在同一個部署目錄重新下載新版 Docker bundle。`.env`、workspace、Compose override 與其他自訂檔會保留；`--upgrade` 只會覆蓋 bundle 擁有的 `docker-compose.yml`、`.env.example`、`LICENSE`、`scripts/install.sh`，然後 pull 並重建服務。

```bash
# 請在既有 Wukong Docker 部署目錄執行
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode docker --upgrade
```

`--upgrade` 會先比對目標 release、`.wukong-release` 與目前的 Compose image 設定；已是相同版本時會直接結束，不呼叫 Docker。需要重新部署相同版本時可加上 `--force`。

installer 會把 Docker Compose project 記錄在 `.wukong-release`。既有部署若尚未記錄，會從現有 Wukong container 的 `com.docker.compose.project` label 判斷並沿用，避免升級時切換到另一組 volumes。只有全新安裝可用 `COMPOSE_PROJECT_NAME=<name>` 選擇 project；既有 metadata、container labels 與手動指定值不一致，或現有 containers 的 ownership 不明確時，installer 會在覆寫檔案或重建服務前中止。這個選項不會遷移或複製 volumes。

installer 不會呼叫 `docker compose build`、`down` 或 `down -v`；升級時也請不要手動使用 `docker compose down -v`，避免刪除 `wukong-data`、`opencode-config`、`opencode-state` 等持久化 volume。若你是從舊版升級且容器還在，想盡量保留尚未持久化的 opencode session，可先備份：

在同一部署目錄執行 `install.sh --mode docker --rollback` 可回復最近一個已驗證 release；`.env`、Compose override、workspace 和 volumes 保持使用者擁有。compatibility metadata 缺少或拒絕目標版本時，installer 不會變更部署。

```bash
docker cp wukong-telegram:/home/wukong/.local/share/opencode ./opencode-session-backup
```

從 v0.14.1 起，Docker bundle 會額外持久化 `/home/wukong/.local/share/opencode`，讓 Wukong 的 `/data/memory.db` 中 `agent_sessions` 與 opencode 本身的 session 檔案一起跨容器保留；entrypoint 也會在降權前建立並修正 `/home/wukong/.local`、`/home/wukong/.local/share/opencode`、`/home/wukong/.local/state` 權限。

若你已經在 Git repository 中，也可以直接使用隨附的 compose 檔案：

```bash
# 1. 複製環境範例（Telegram token 可稍後透過 Web 設定）
cp .env.example .env
# 可選：編輯 .env 調整 USER_ID/GROUP_ID、Web port 等

# 2. 建置並啟動 Web Console + Telegram Bot + Scheduler
docker compose up -d

# 3. 開啟 Web Console，必要時在設定區填入 Telegram bot token / allowed IDs
open http://localhost:8787/

# CLI / opencode 只在需要時被動執行
docker compose run --rm wukong opencode
docker compose run --rm wukong wukong
```

Web Console 預設**只綁定本機 `127.0.0.1`**，開箱即用、不需任何設定，且不會對區網或公網開放。若要讓同網段其他裝置存取，請在 `.env` 設 `WUKONG_WEB_BIND=0.0.0.0` **並**設定 `WUKONG_WEB_TOKEN=<secret>`（對外開放卻無 token 時，`wukong-web` 會進入「設定錯誤」降級模式以防未認證外洩；詳見下方環境變數說明）。

> **設定錯誤降級模式**：當偵測到不安全綁定（對外開放但無 token）時，`wukong-web`
> 不會直接崩潰重啟，而是照常綁定並對**所有請求（含 `/healthz`）回應 `503`** 與一頁
> 修正說明。因此你會在瀏覽器直接看到原因與解法，`docker compose ps` 也會顯示
> `unhealthy`（而非不斷 `Restarting`）。設好 token 或 `WUKONG_WEB_ALLOW_INSECURE=1`
> 後重啟即恢復正常。
>
> 若 `localhost:8787` 連不上或顯示 503，查看服務狀態與日誌：
> ```bash
> docker compose ps wukong-web
> docker compose logs wukong-web
> ```

**啟用網路資訊檢索（Agent Reach + GitHub CLI）：**

Docker image 會預裝 `agent-reach` CLI 與 `gh`，但不會在 build 或 daemon 啟動時自動執行登入、Cookie 或 MCP 設定。若要讓 opencode/Wukong 具備更強的網路資訊檢索能力，請先用互動式 CLI runtime 完成一次性初始化：

```bash
docker compose run --rm wukong agent-reach install --env=auto
docker compose run --rm wukong agent-reach doctor
docker compose run --rm wukong gh auth login
docker compose up -d --force-recreate
```

請從 `wukong` CLI service 執行初始化，不要從 `wukong-web`、`wukong-telegram` 或 `wukong-schedulerd` 這類常駐服務執行互動式設定。初始化後，Agent Reach 狀態會保存在 `agent-reach-state` volume，GitHub CLI 認證會保存在 `gh-config` volume，Web、Telegram 與 Scheduler 會共用這些狀態。

部分 Agent Reach channel 需要 Cookie、Token 或平台登入態。請只在你信任的部署環境中提供這些憑證；不要把 Cookie 或 Token 寫進 `.env`，除非你明確接受該風險。若 Agent Reach 安裝流程改動了 opencode MCP 設定，請重啟相關 Docker 服務，因為 opencode 啟動後不會熱載入設定。

第一次啟動時，`wukong-telegram` 會保持待命而不是因缺少 token 重啟。開啟 Web Console 的設定區，填入 Telegram bot token 與允許的 chat/user ID 後，Telegram 服務會自動套用設定並開始 long-poll。

**自訂建構版本（可選）：**

```bash
# 指定版本（預設 v0.14.1）
docker-compose build --build-arg VERSION=v0.14.1

# 指定 target（預設 musl 靜態編譯，跨 distro 相容）
docker-compose build --build-arg TARGET=x86_64-unknown-linux-gnu  # glibc 動態連結
```

或在 `docker-compose.yml` 永久設定：
```yaml
services:
  wukong:
    build:
      args:
        VERSION: v0.14.1
        TARGET: x86_64-unknown-linux-musl
```

**AI Agent (OpenCode) 授權與多 Provider 設定：**

由於 Wukong 底層是由 `opencode` 驅動，您可以透過以下兩種方式在 Docker 環境中處理 AI 模型的授權與 Provider 設定：

*   **方法 A：互動式認證與 TUI 設定（推薦多 Provider 混合使用或 OAuth 帳號）**
    對於需要帳號驗證的服務（如 `opencode go` 雲端、GitHub Copilot）或想透過互動引導來新增 Provider（如 OpenAI、NVIDIA 等 API Key）：

    *   **方式一：進入 TUI 進行連線設定**
        執行以下指令開啟 `opencode` 互動視窗：
        ```bash
        docker compose run --rm wukong opencode
        ```
        進入介面後，輸入 `/connect` 並按 Enter，即可根據畫面 UI 提示選擇您的 Provider（如 NVIDIA NIM, OpenAI, Anthropic）並貼入 API Key。

    *   **方式二：進行 OAuth 帳號登入**
        如果使用的是官方雲端服務：
        ```bash
        docker compose run --rm wukong opencode auth login
        ```
        畫面上會顯示驗證網址與驗證碼，請在主機瀏覽器中開啟並完成登入。

    > [!NOTE]
    > 以上互動式設定都會自動保存至 `opencode-config` 持久化 Volume 中。之後背景啟動 `wukong-web` 或 `wukong-telegram` 時會自動共享此授權狀態，不需重複設定。

*   **方法 B：直接透過環境變數注入（適用於 OpenAI / Anthropic / NVIDIA 等單一 API Key）**
    如果不希望手動在 UI 輸入，可以直接將 API Key 透過環境變數注入容器：
    1. 在 `.env` 中加入您的金鑰（例如 `OPENAI_API_KEY=sk-...` 或 `NVIDIA_API_KEY=nvapi-...`）。
    2. 編輯 `docker-compose.yml`，在您需要啟動的服務（如 `wukong-web`、`wukong` 等）的 `environment` 區段中加上對應的環境變數名稱（例如 `- OPENAI_API_KEY` 或 `- NVIDIA_API_KEY`），Docker Compose 即會自動載入。

**環境變數說明（.env）：**

| 變數 | 說明 | 預設 |
| :--- | :--- | :--- |
| `USER_ID` / `GROUP_ID` | 與 host 對齊的 UID/GID，避免 volume 權限問題 | `1000` |
| `WUKONG_HOST_WORKSPACE` | Host 工作目錄路徑（opencode workspace） | `./workspace` |
| `WUKONG_AGENT_CMD` | 預設選按需本機控制程序；額外 run 旗標與任意命令走純 CLI | `opencode run` |
| `WUKONG_AGENT_SERVER_URL` | 非空時選既有共用 server adapter；空值使用本機執行 | 空值 |
| `WUKONG_AGENT_SERVER_FILE_MODE` | server backend 附件模式：共享工作區 `shared`、Base64 `inline`、停用 `disabled` | `shared` |
| `WUKONG_AGENT_SERVER_WORKSPACE` | `shared` 模式中 OpenCode server 看見的 workspace 絕對路徑 | `/workspace` |
| `WUKONG_TG_TOKEN` | Telegram Bot Token（選用；可由 Web `/settings` 設定，env 優先） | — |
| `WUKONG_TG_ALLOWED` | 允許的 Telegram chat ID（選用；可由 Web `/settings` 設定，env 優先） | — |
| `WUKONG_WEB_BIND` | Web Console 的 **host 端**綁定位址。預設僅本機可達；設 `0.0.0.0` 才對區網／公網開放（對外時請務必搭配 `WUKONG_WEB_TOKEN`） | `127.0.0.1` |
| `WUKONG_WEB_PORT` | Web Console 的 **host 端**對外埠（容器內固定聽 `8787`，此值只改 host 端映射） | `8787` |
| `WUKONG_WEB_TOKEN` | Web Console 存取密鑰。對外開放（`WUKONG_WEB_BIND=0.0.0.0`）卻未設此值時，服務進入「設定錯誤」降級模式（所有請求回 `503` 說明頁、healthcheck 標記 unhealthy）以防未認證外洩。可用 `Authorization: Bearer <token>` 標頭或 `?token=` 查詢字串提供 | — |
| `WUKONG_WEB_ALLOW_INSECURE` | 設為 `1` 時允許在無 token 下對外綁定（僅限可信內網）。**Docker Compose 預設為 `1`**（容器內必綁 `0.0.0.0`，安全邊界改由 host 端 `WUKONG_WEB_BIND` 控制）；對外開放建議改設 token 而非依賴此旗標 | `1`（compose） |
| `WUKONG_MEMORY_HOST` | `wukong-memoryd` 綁定位址（預設僅本機，避免記憶未認證外洩） | `127.0.0.1` |
| `WUKONG_MEMORY_TOKEN` | `wukong-memoryd` 存取密鑰（選用；設定後除 `/v1/health` 外皆需 `Authorization: Bearer <token>`） | — |
| `WUKONG_THINKING` | 啟用思考過程顯示 | `1` |
| `WUKONG_EMBED` | 啟用語意向量召回 | `0` |
| `WUKONG_SESSION_COMPACT_EVERY_TURNS` | 每 scope 成功回合數達門檻後，在下一個 final turn 前執行 session compact；設 `0` 停用 | `20` |
| `WUKONG_SESSION_LEASE_SECS` | session lifecycle lease 秒數，避免同一 scope 的回合互相覆寫 | `900` |
| `WUKONG_MEMORY_AUTO_MAINTENANCE` | schedulerd 是否啟用安全的 all-scope consolidation（只刪除已折疊來源） | `1` |
| `WUKONG_MEMORY_MAINTENANCE_INTERVAL_SECS` | schedulerd 自動 memory maintenance 間隔秒數 | `900` |
| `WUKONG_MEMORY_CONSOLIDATE_THRESHOLD` | 單一 scope 觸發自動 consolidation 的候選數 | `40` |
| `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` | opencode session 保留天數。schedulerd 啟動 6 小時後第一次、之後每 6 小時，刪除超過此天數、且沒有任何 scope 指向的 session；設 `0` 停用；在 `.env` 留空或寫了無法解析的值也視為停用 | `30` |
| `WUKONG_BIN` | 注入排程能力提示詞時使用的 `wukong` 指令路徑（agent 自行建排程時用） | `wukong` |
| `WUKONG_SCHED_NOTIFY` | schedulerd 是否把排程結果回送 Telegram（`0` 關閉） | `1` |
| `WUKONG_SCHED_PERMISSION` | 無人值守排程遇到 opencode 權限詢問時的處置；`allow` 才自動允許一次，其餘一律拒絕 | `reject` |
| `TZ` | 容器時區。**`opencode-server` 的重啟窗口是本地時間**，由此決定實際落點；不在 +08:00 才需要改 | `Asia/Taipei` |
| `WUKONG_OPENCODE_RESTART_WINDOW` | `opencode-server` 的離峰重啟窗口（`HH:MM-HH:MM`，可跨午夜）。**設為空字串完全停用** | `03:00-05:00` |
| `WUKONG_OPENCODE_RESTART_MIN_UPTIME_SECS` | 已執行未滿此秒數就不重啟，避免部署或故障後接連重啟 | `43200`（12h） |
| `WUKONG_OPENCODE_IDLE_QUIET_SECS` | 「閒置」須持續多久才動手（無 session 更新、`opencode.db` 無寫入） | `300` |
| `WUKONG_OPENCODE_CONN_GRACE_SECS` | 對外埠仍有 `ESTABLISHED` 連線時，最多再等多久才視為閒置的 keep-alive 並放行。`0` 表示不等待 | `1800`（30m） |
| `WUKONG_OPENCODE_CPUS` / `_MEM` / `_PIDS` | `opencode-server` 與 `cli` profile 的 cgroup 上限（agent 實際幹活的容器）。溫度壓不下來就調降 CPU；回合明顯變慢且溫度尚可再往上加 | `1.5` / `2g` / `256` |
| `WUKONG_SVC_CPUS` / `_MEM` / `_PIDS` | `wukong-web`／`wukong-telegram`／`wukong-schedulerd` 的 cgroup 上限。2026-10-04：預設也執行本機 agent；開 embedding 時另留模型空間 | `1.5` / `2g` / `256` |

**關於 opencode session 的保留期清理：** opencode 把每個 session 的訊息、片段與事件歷史存在 `opencode.db`，不會自己刪。Wukong 只在輔助棒跑完、session 輪替與 `/new` 時刪除 session，其餘（回合失敗留下的、人工探測建立的）會一直留著。`wukong-schedulerd` 因此在啟動 6 小時後第一次、之後每 6 小時，刪除超過 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` 天、且沒有任何 scope 指向的 session，每輪最多 500 個、最長 5 分鐘。**仍被 scope 指向的 session 不論多舊都保留**（scope 指向子 session 時，整棵 session 樹都保留），所以對話延續不受影響；Wukong 自己另存對話，不讀舊的 opencode session。讀不到 scope 對應或列不出 session 時整輪不刪。只在顯式共用 server backend 生效：按需本機／純 CLI 模式的 `opencode.db` 與你自己的 opencode 使用共用，不自動清掃。

**這項清理假設 server 上的 session 都屬於這一套 Wukong、而且只有一份記憶庫在用它。** compose 部署成立，因為 `opencode-state` 與 `wukong-data` 兩個 volume 都是專屬的。三種情況不成立：

- **記憶庫與 server 完全對不上**（`WUKONG_MEMORY_DB` 打錯而開出空的記憶庫、或指到另一套部署的記憶庫）。這時 server 上還有人接著的 session 會被看成無主。防護：記憶庫指向的 session 若沒有任何一個出現在 server 的清單裡，整輪不刪，並在輸出與 schedulerd 日誌寫出原因與所用的記憶庫。全新的記憶庫因此要先跑過一個回合才會開始清理。唯一的例外是兩邊都是空的全新部署：那不算對不上，只是沒東西可清。
- **一個 server 被兩份記憶庫共用**（主機上的 `wukong` 與容器內的服務各用各的記憶庫卻指向同一個 server，或兩套部署共用一個 server）。**上面那道防護擋不住這種情況**：只要兩邊都在這個 server 上跑過回合，各自都「對得上」，而每一份記憶庫都會把對方還在續接的舊 session 看成無主——容器的 schedulerd 會自動刪掉主機那份指向的，主機上手動 `prune` 也會刪掉容器那份指向的。**一個 opencode server 只能對應一份記憶庫**；做不到時把 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` 設為 `0`，主機上也不要對它執行 `wukong opencode prune`。
- **把 Wukong 接到你自己也在用的 `opencode serve`**。只要 Wukong 在上面跑過回合，上述防護就會放行，而你自己超過保留期的 session 在 Wukong 看來就是無主的，會被刪除。**這種用法請把 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` 設為 `0`。**

套用前可先預覽，`exec` 預設是 root，要指定使用者：

```bash
docker compose exec -u wukong wukong-schedulerd wukong opencode prune --dry-run
```

輸出第一行是所用的記憶庫位置，接著列出將被刪除與受保護的 session。升級後 schedulerd 不會在啟動當下清理，有 6 小時可以先看這份預覽。反過來說，上次清理的時間沒有被保存：schedulerd 若每次都在 6 小時內重啟，清理就永遠不會執行，這時請手動執行 `wukong opencode prune`。受保護清單中「已超過保留期」的數量是被棄置 scope 的規模：它們不會被清理。同樣的數字每輪也會出現在 schedulerd 日誌的 `opencode_session_retention` 那一行。該行的 `anchored=false` 表示記憶庫與 server 對不上、這一輪沒有清理，下一行 `warning:` 會寫出原因與所用的記憶庫；`truncated=true` 表示 session 超過一次取回的上限（10,000 個），最舊的沒被看到。這時有一個已知的邊界：若某個 scope 指向的是一個被截掉的子 session，它的根 session 可能被當成無主而刪除。server backend 自己建立的對應一定指向根 session，所以只有 CLI backend 留下的對應會遇到。

刪除只把空間還給 SQLite 重用，檔案不會變小。`opencode-server` 容器每次啟動、在 server 開啟資料庫之前，會在可回收空間達 25% 且剩餘磁碟不小於資料庫 2 倍時執行 `VACUUM`；既有的離峰重啟讓這大約每天發生一次。`VACUUM` 的暫存複本寫在資料庫旁邊（entrypoint 設了 `SQLITE_TMPDIR`），不佔容器的根檔案系統。回收失敗（例如資料庫被鎖）只記一行警告，不影響 server 啟動。日誌關鍵字是 `opencode_db_vacuum`。

這項清理刪不到**長壽的 scope session**——一個用了幾個月的聊天 scope，它的 session 會持續累積歷史，而它正是被保護的對象。如果 `opencode.db` 仍然很大，先用預覽看可刪的佔多少。

**關於選用 opencode server 的週期性重啟（2026-10-04）：** `opencode serve` 常駐不死，每回合的殘留（heap、快取、DB handle）全部留存，idle CPU 會隨累積工作量上升；CLI 模式沒有這個問題，因為 `opencode run` 每回合退出，等於免費獲得重置。容器內因此常駐一個 supervisor，在 `WUKONG_OPENCODE_RESTART_WINDOW` 的窗口內、且判定閒置時讓 server 自行退出，由 `restart: unless-stopped` 拉起。閒置的判準是：無近期 session 更新、`opencode.db` 已停止寫入（後者用來涵蓋 compaction 等背景工作）。

對外埠的 `ESTABLISHED` 連線**不是**否決條件，而是一段有上限的等待（`WUKONG_OPENCODE_CONN_GRACE_SECS`，預設 30 分鐘）。原因是連線數不等於有工作進行中：`wukong-schedulerd` 對 server 保有長生命週期的 HTTP 連線、閒置時也不斷開，早期版本把它當成活躍工作，於是條件永遠湊不齊、重啟從未發生。但這個訊號也不能丟掉——一個安靜超過 `WUKONG_OPENCODE_IDLE_QUIET_SECS` 的長工具呼叫期間，session 與 `opencode.db` 都可能毫無寫入，那時連線是唯一還在說「有人接著」的東西。預設的 30 分鐘刻意大於 `WUKONG_AGENT_TIMEOUT_SECS`（1200 秒）：撐過那個時間的回合，gateway 自己也已經放棄了。

窗口內若始終不閒置就**跳過、等隔天，不會強制中斷進行中的回合**。代價是：若排程任務集中在凌晨，server 可能長期湊不齊條件而從不重啟——那時該換窗口，而不是調短門檻。是否真的重啟過，看 `docker logs wukong-opencode-server | grep wukong-idle-restart`；每次跳過都會寫明是哪一項條件沒過。

**關於資源上限：** 上限一律**改在 `.env`，不要直接編輯 `docker-compose.yml`**——後者由 release bundle 擁有，`install.sh --upgrade` 會覆寫它，手改會無聲消失；`.env` 則會保留。另外要知道設了 `mem_limit` 就多出一種原本不存在的失敗模式：容器可能被 OOM kill 再由 `restart` 拉起，進行中的回合會遺失。調低之前先用 `scripts/collect-opencode-baseline.sh` 確認 cgroup `memory.events` 的 `oom` 仍為 `0`。

**Volume 架構：**

```
┌─────────────────────────────────────────────────────────┐
│  Host                      │  Container                │
├─────────────────────────────────────────────────────────┤
│  ./workspace               →  /workspace                │  (opencode 工作空間)
│  Docker Volume:            →  /home/wukong/.config/   │  (opencode 設定隔離)
│    opencode-config            opencode/                 │
│  Docker Volume:            →  /home/wukong/.local/    │  (opencode session)
│    opencode-state             share/opencode/           │
│  Docker Volume:              →  /data/                  │  (wukong 記憶資料庫與設定)
│    wukong-data                                           │
└─────────────────────────────────────────────────────────┘
```
