# 安裝指南

> ← 回到 [主 README](../README.md)｜相關文件:[Docker 部署](docker.md)、[CLI 參考](cli-reference.md)

## 先決條件

Release installer 需要 `curl`、`tar` 與 **Python 3**；Python 用來嚴格驗證 release manifest 與 archive 路徑。Binary mode 在 Linux 另外使用 `systemctl --user` 管理可選服務。

- **Rust**（stable，≥ 1.96）。若未安裝：
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  . "$HOME/.cargo/env"
  ```
- 一個可用的 **AI agent CLI**（預設 `opencode run`；可用 `--agent-cmd` 換成其他）。

## 依情境選擇安裝模式

Wukong 提供 **Docker** 與 **Binary** 兩種安裝模式，沒有絕對的「最佳解」——請依**使用情境**選擇，因為兩者對底層 agent（opencode）的「工作範圍」與「scope 自動隔離」行為截然不同：

| 你的情境 | 建議模式 | 為什麼 |
| :--- | :--- | :--- |
| 想當作 **CLI coding 夥伴**，在各個 git 專案目錄間切換使用 | **Binary** | `wukong` 在任意目錄即開即用，opencode 直接對**真實專案檔案**動工，記憶 scope 依工作目錄自動隔離（`project:<資料夾名>`）——這是 Wukong 的核心賣點 |
| 想跑**常駐後台服務**（Telegram Bot / Web Console / Scheduler），掛在固定 workspace 上 | **Docker** | opencode config/state 隔離於 volume、不污染 host、auto-restart、UID/GID 對齊、多服務共用同一份授權 |
| 想兩者兼得 | **Binary 為主、Docker 跑常駐服務** | CLI 互動用 Binary，後台機器人用 Docker，各取所長 |

> ⚠️ 重點：**Docker 模式下 opencode 只能存取掛載進去的單一 `/workspace`**，容器內 cwd 恆為 `/workspace`，因此「依工作目錄自動分 scope」會退化成單一 scope。若你主要是在本機多個專案間做互動式開發，請選 Binary。

## 快速安裝

```bash
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash
```

腳本會自動偵測版本，並依上表詢問你要使用哪種模式：

- **Docker mode**：驗證 `SHA256SUMS`、`release-manifest.json` 與 release bundle 後，只寫入 `docker-compose.yml`、`.env.example`、`LICENSE`、`scripts/install.sh`。它從 GHCR pull 已驗證 digest 的 image，不在本機 build。`.env`、workspace、Compose override 與 volume 均由使用者擁有。**適合常駐服務部署。**
- **Binary mode**：下載最新預編譯 binary 到 `~/.local/bin`，並以互動問答設定 Telegram / Web / 記憶等選項。**適合本機 CLI 互動開發。**

Docker `--upgrade` 會先檢查本機 release metadata 與 Compose image；版本相同時直接結束，`--force` 可要求重新部署。installer 會將 Compose project 寫入 `.wukong-release`，並在 legacy metadata 缺少此欄位時從既有 container labels 判斷。全新安裝可用 `COMPOSE_PROJECT_NAME=<name>` 指定 project；既有部署不得用它切換 ownership，任何 metadata、labels 或手動值衝突都會在 mutation 前中止，且 installer 不會自動遷移 volumes。

既有 Docker 部署升級（請在 Runtime／部署目錄執行）：

```bash
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode docker --upgrade
```

手動選項：

```bash
# 指定 Docker 模式部署到目前目錄
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode docker --version v0.14.1

# 指定 Binary 模式安裝到 ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode binary --version v0.14.1

# Linux binary mode 可選 linking flavor：
curl -fsSL ... | bash -s -- --mode binary --flavor gnu   # glibc (動態)
curl -fsSL ... | bash -s -- --mode binary --flavor musl  # musl  (靜態，預設，跨 distro)

# 保留已選元件與設定的 Binary 升級；不會重新提問
curl -fsSL ... | bash -s -- --mode binary --upgrade

# 新安裝或升級時明確加入 Linux Scheduler service
curl -fsSL ... | bash -s -- --mode binary --upgrade --with-schedulerd

# 回復最後一個已驗證的 Binary transaction
curl -fsSL ... | bash -s -- --mode binary --rollback
```

## 安裝 prerelease / RC 版本

預設 installer 會查詢 GitHub Releases 的 latest stable 版本；不指定 `--version` 時，不會自動安裝 prerelease 或 RC 版本。若你要協助測試尚未正式發布的版本，請明確指定 tag。

```bash
# Docker prerelease 安裝
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh \
  | bash -s -- --mode docker --version v0.16.15-rc.1

# 既有 Docker 部署升級到 prerelease
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh \
  | bash -s -- --mode docker --upgrade --version v0.16.15-rc.1

# Binary prerelease 安裝
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh \
  | bash -s -- --mode binary --version v0.16.15-rc.1
```

Prerelease 適合驗證新功能或修補，例如 runtime skill assets、Docker entrypoint、binary 安裝行為等。正式部署仍建議使用 latest stable。指定 prerelease tag 時，該 GitHub Release 必須包含完整 assets、全域 `SHA256SUMS`、`release-manifest.json` 與 Docker bundle。Binary 安裝資訊儲於 `~/.wukong/install.json`（權限 `0600`）；設定與 workspace 不會在升級時被覆寫。

`wukong-schedulerd` 的 managed unit 只在 Linux 上可用；macOS Binary mode 會安裝 binary，但不會建立或啟動 systemd service。

## Rollback

Binary rollback 只使用 `~/.wukong/install.json` 指向的已驗證上一個 transaction，成功後會旋轉 current/previous metadata。第一次從 legacy 安裝升級時會建立 hash-checked backup；config、data 與 workspace 永遠不包含在 backup。缺少、損毀或宣告不可逆 migration 的 compatibility metadata 都會在 mutation 前拒絕 rollback；先備份受影響 state，再依 manifest 的 recovery instructions 處理。

## Binary 模式換用 OpenCode V2

2026-11-25 起（issue-0044），Wukong 只支援 OpenCode V2（npm 套件 `@opencode/cli`，驗證過的版本是 2.3.2）。Docker 映像已內建 V2，installer 會自動備份與還原；**binary 模式的 opencode 由你自己安裝，installer 不會動它**，所以升級 Wukong 時要自己換裝 V2 並備份資料。

先知道兩件事：

- **新版 Wukong 遇到 V1 會直接拒絕**。PATH 上的 `opencode` 回報 1.x 時，第一個回合就失敗並顯示：`需要 OpenCode V2（2.x），opencode 回報的版本是 1.19.6。請改裝 V2，例如 npm install -g @opencode/cli@2.3.2。`（不會等到 agent timeout）。反過來，舊版 Wukong 也不能接 V2，兩者要一起換。
- **V2 的資料遷移是單向的**。V2 第一次啟動時就地改寫 `~/.local/share/opencode/opencode.db`（有設 `XDG_DATA_HOME` 時為 `$XDG_DATA_HOME/opencode/`）：新增 V2 的表、刪掉 V1 的 `data_migration`、`session_context_epoch`、`session_input` 三張表。原本的對話在 V2 裡可以接續；但要退回 V1，只能還原升級前的備份，**備份之後的對話不會保留**。登入資訊 `auth.json` 也在這個目錄。

### 升級

```bash
# 1. 停掉會開 opencode.db 的程序：Wukong 的 user service（有裝的才需要；macOS 沒有），以及你自己開著的 opencode
systemctl --user stop wukong-telegram wukong-web wukong-schedulerd

# 2. 備份 opencode 的資料、設定與目前的版本
backup="$HOME/opencode-v1-backup-$(date +%Y%m%d%H%M)"
mkdir -p "$backup"
opencode --version > "$backup/opencode-version"
cp -a "${XDG_DATA_HOME:-$HOME/.local/share}/opencode" "$backup/data"
[ ! -d "${XDG_CONFIG_HOME:-$HOME/.config}/opencode" ] || cp -a "${XDG_CONFIG_HOME:-$HOME/.config}/opencode" "$backup/config"

# 3. 換裝 V2。套件改名了，兩個套件都提供 opencode 指令，所以先移除 V1
npm uninstall -g opencode-ai
npm install -g @opencode/cli@2.3.2
opencode --version    # 應顯示 opencode v2.3.2

# 4. 升級 Wukong；已啟用的 service 會由 installer 重新啟動
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode binary --upgrade
```

V1 的設定檔不用改：V2 會自動轉換 `permission`、`bash`、`snapshot` 等舊鍵；轉換不了的（例如 `compaction.prune`、`tail_turns`）會被忽略。

### 退回 V1

```bash
systemctl --user stop wukong-telegram wukong-web wukong-schedulerd
backup="$HOME/opencode-v1-backup-<升級時的時間>"

npm uninstall -g @opencode/cli
npm install -g "opencode-ai@$(cat "$backup/opencode-version")"

data="${XDG_DATA_HOME:-$HOME/.local/share}/opencode"
rm -rf "$data" && cp -a "$backup/data" "$data"
if [ -d "$backup/config" ]; then
    config="${XDG_CONFIG_HOME:-$HOME/.config}/opencode"
    rm -rf "$config" && cp -a "$backup/config" "$config"
fi

curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode binary --rollback
```

## 從原始碼建置

如果不想用預編譯 binary，也可以直接編譯：

```bash
cargo build --release  # 編譯整個 workspace（含 wukong + wukong-telegram + wukong-web + wukong-schedulerd）
cargo test             # 全部測試
cargo clippy --all-targets -- -D warnings
```
