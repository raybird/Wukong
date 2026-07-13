# Docker 容器化部署

> ← 回到 [主 README](../README.md)｜相關文件:[安裝指南](installation.md)、[各進入點](entrypoints.md)

提供完整的 Docker / Docker Compose 配置，隔離 host 環境，同時滿足 opencode 工作空間掛載與設定隔離需求。

**特點：**
- **Host 工作目錄掛載**：opencode 工作空間透過 volume 掛載 host 路徑
- **opencode 設定與 session 隔離**：`~/.config/opencode` 與 `~/.local/share/opencode` 都存放在 Docker volume 中，不污染 host，且可跨容器升級保留 session
- **UID/GID 對齊**：runtime user 與 host 一致，避免檔案權限問題
- **預設 Web + Telegram + Scheduler**：`docker compose up -d` 會啟動 Web Console、Telegram Bot 與排程 daemon；CLI / REPL 透過被動 `run` 使用
- **非互動權限處理**：Wukong 驅動 `opencode run` 時 stdin 永遠為空（CLI/Web/Telegram/Scheduler 皆然），opencode 無法回應互動式權限詢問。因此容器內 `WUKONG_AGENT_CMD` 預設帶 `--dangerously-skip-permissions`（自動核准詢問），並由 entrypoint 在缺檔時 seed 一份 `~/.config/opencode/opencode.json`：該旗標仍尊重 `deny` 規則，故內含一組黑名單擋下對絕對路徑的毀滅性遞迴刪除（`rm -rf /…`、`sudo rm`、家目錄等變形），同時放行 `/workspace` 內的刪除。這是 **防呆/防幻覺護欄而非資安牆**（glob 字串比對擋不住 `find -delete`、變數展開等繞法），真正的隔離邊界仍是 container 本身與 host 掛載目錄的範圍。要自訂規則，直接把你的 `opencode.json` 放進 `opencode-config` volume 即可覆蓋（缺檔才會 seed）。

## Docker 低延遲 opencode serve 模式

Docker 常駐服務預設會啟動 `opencode-server`，並讓 `wukong-web`、`wukong-telegram`、`wukong-schedulerd` 透過 `WUKONG_AGENT_SERVER_URL=http://opencode-server:4096` 呼叫同一個長壽命 `opencode serve` process。

這個模式保留 Wukong 的 scope-level session 管理，但避免每次回合都重新啟動 `opencode run`，可降低 Web、Telegram、Scheduler 等常駐入口的延遲感。

Binary 模式第一版不自動啟動或管理 `opencode serve`。在一般本機 CLI 使用情境，Wukong 仍預設透過 `opencode run` 執行，以避免背景 daemon、port、跨專案工作目錄與清理策略帶來額外複雜度。進階使用者若自行啟動 `opencode serve`，可手動設定 `WUKONG_AGENT_SERVER_URL` 使用同一 backend。

若要回到舊的 Docker CLI subprocess 模式，移除服務環境中的 `WUKONG_AGENT_SERVER_URL`，Wukong 會使用 `WUKONG_AGENT_CMD`，預設為 `opencode run --dangerously-skip-permissions`。

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
cd /path/to/wukong-docker

curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh \
  | bash -s -- --upgrade

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
| `WUKONG_AGENT_CMD` | AI agent 指令（容器內預設帶 `--dangerously-skip-permissions`，見上方說明） | `opencode run --dangerously-skip-permissions` |
| `WUKONG_AGENT_SERVER_URL` | opencode serve backend URL；Docker 常駐服務預設使用，未設定時回到 `WUKONG_AGENT_CMD` | `http://opencode-server:4096` |
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
| `WUKONG_BIN` | 注入排程能力提示詞時使用的 `wukong` 指令路徑（agent 自行建排程時用） | `wukong` |
| `WUKONG_SCHED_NOTIFY` | schedulerd 是否把排程結果回送 Telegram（`0` 關閉） | `1` |

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
