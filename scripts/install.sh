#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# Wukong one-liner installer
#   curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash
# ---------------------------------------------------------------------------

REPO="raybird/Wukong"
GITHUB="https://github.com"
API="https://api.github.com/repos"
INSTALL_DIR="${HOME}/.local/bin"
CONFIG_DIR="${HOME}/.wukong"
CONFIG_FILE="${CONFIG_DIR}/config.env"

# --- helpers ----------------------------------------------------------------

bold()  { printf '\033[1m%s\033[0m' "$*"; }
green() { printf '\033[32m%s\033[0m' "$*"; }
red()   { printf '\033[31m%s\033[0m' "$*"; }
dim()   { printf '\033[2m%s\033[0m' "$*"; }

abort() { printf '%s\n' "$(red "✗") $*" >&2; exit 1; }
info()  { printf '  %s %s\n' "$(green "✓")" "$*"; }
step()  { printf '\n%s\n' "$(bold "→ $*")"; }

# --- argument parsing -------------------------------------------------------

VERSION=""
FLAVOR="musl"    # Linux default: static musl

usage() {
  cat <<'USAGE'
Usage: install.sh [--version v0.12.0] [--flavor gnu|musl]

Options:
  --version <tag>   Install a specific version (default: latest)
  --flavor  <name>  Linux only: gnu (glibc) or musl (static, default)
  --help            Show this help
USAGE
  exit 0
}

while (($#)); do
  case "$1" in
    --version) VERSION="${2:?missing version}"; shift 2 ;;
    --flavor)  FLAVOR="${2:?missing flavor}"; shift 2 ;;
    --help)    usage ;;
    *)         abort "未知選項: $1" ;;
  esac
done

# --- detect platform --------------------------------------------------------

OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)
    case "$FLAVOR" in
      gnu)  TARGET="x86_64-unknown-linux-gnu" ;;
      musl) TARGET="x86_64-unknown-linux-musl" ;;
      *)    abort "Linux flavor 必須是 gnu 或 musl，收到: $FLAVOR" ;;
    esac
    HAS_SYSTEMD=true
    ;;
  Darwin)
    case "$ARCH" in
      arm64) TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) abort "macOS on $ARCH 尚不支援" ;;
    esac
    FLAVOR=""
    HAS_SYSTEMD=false
    ;;
  *) abort "$OS 尚不支援" ;;
esac

info "平台: ${OS} / ${ARCH}  →  ${TARGET}"

# --- prerequisites -----------------------------------------------------------

for cmd in curl tar grep; do
  command -v "$cmd" >/dev/null 2>&1 || abort "需要 $cmd，請先安裝"
done

if ! command -v uname >/dev/null 2>&1; then
  abort "需要 uname"
fi

mkdir -p "$INSTALL_DIR"

if [[ ":$PATH:" != *":$INSTALL_DIR:"* ]]; then
  info "${INSTALL_DIR} 不在 PATH，正在加到 ~/.bashrc"
  printf '\n# Wukong\nexport PATH="%s:$PATH"\n' "$INSTALL_DIR" >> "${HOME}/.bashrc"
  export PATH="$INSTALL_DIR:$PATH"
fi

# --- resolve version --------------------------------------------------------

if [[ -z "$VERSION" ]]; then
  step "查詢最新版本..."
  VERSION="$(curl -fsSL "${API}/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')"
  if [[ -z "$VERSION" ]]; then
    abort "無法取得最新版本，請用 --version 指定"
  fi
fi
info "版本: ${VERSION}"

# --- download & verify -------------------------------------------------------

BASE_URL="${GITHUB}/${REPO}/releases/download/${VERSION}"

download_and_verify() {
  local name="$1"
  local tarball="${name}-${TARGET}.tar.gz"

  step "下載 ${tarball} ..."
  curl -fsSL "${BASE_URL}/${tarball}" -o "/tmp/${tarball}"
  curl -fsSL "${BASE_URL}/checksums-${TARGET}.txt" -o "/tmp/checksums-${TARGET}.txt"

  info "驗證 checksum ..."
  local expected
  expected=$(grep "${tarball}" "/tmp/checksums-${TARGET}.txt" | awk '{print $1}')
  if [[ -z "$expected" ]]; then
    abort "在 checksums 找不到 ${tarball}"
  fi
  local actual
  if [[ "$OS" == "Darwin" ]]; then
    actual=$(shasum -a 256 "/tmp/${tarball}" | awk '{print $1}')
  else
    actual=$(sha256sum "/tmp/${tarball}" | awk '{print $1}')
  fi
  if [[ "$expected" != "$actual" ]]; then
    abort "checksum 不符！\n  預期: ${expected}\n  實際: ${actual}"
  fi
  info "checksum 正確"

  tar -xzf "/tmp/${tarball}" -C "$INSTALL_DIR" "$name"
  chmod +x "${INSTALL_DIR}/${name}"

  if [[ "$OS" == "Darwin" ]]; then
    xattr -d com.apple.quarantine "${INSTALL_DIR}/${name}" 2>/dev/null || true
  fi

  info "$(green "$name") 已安裝至 ${INSTALL_DIR}/${name}"
  rm -f "/tmp/${tarball}" "/tmp/checksums-${TARGET}.txt"
}

download_and_verify "wukong"

# --- interactive config ------------------------------------------------------

echo ""
echo "$(bold '🐵 Wukong 安裝設定')"
echo "$(dim '================================')"
echo ""

# component selection
echo "你需要安裝哪些元件？"
echo "  [1] CLI only（單機 wukong 指令）"
echo "  [2] CLI + Telegram Bot（後台自動回覆）"
echo "  [3] CLI + Web Console（瀏覽器介面）"
echo "  [4] 全裝（CLI + Telegram + Web）"
read -r -p "選擇 [1-4] (預設 1): " COMPONENT
COMPONENT="${COMPONENT:-1}"

if [[ ! "$COMPONENT" =~ ^[1-4]$ ]]; then
  abort "請輸入 1–4"
fi

NEED_TELEGRAM=false
NEED_WEB=false
case "$COMPONENT" in
  2) NEED_TELEGRAM=true ;;
  3) NEED_WEB=true ;;
  4) NEED_TELEGRAM=true; NEED_WEB=true ;;
esac

mkdir -p "$CONFIG_DIR"
echo "# Wukong 設定 — 由 install.sh 產生 @ $(date +%F)" > "$CONFIG_FILE"

# telegram
if $NEED_TELEGRAM; then
  download_and_verify "wukong-telegram"
  echo ""
  echo "$(bold 'Telegram Bot 設定')"
  read -r -p "  Bot Token（@BotFather 取得）: " TG_TOKEN
  read -r -p "  允許的 User ID（逗號分隔）: " TG_ALLOWED
  {
    echo "WUKONG_TG_TOKEN=\"${TG_TOKEN}\""
    echo "WUKONG_TG_ALLOWED=\"${TG_ALLOWED}\""
  } >> "$CONFIG_FILE"
fi

# web
if $NEED_WEB; then
  download_and_verify "wukong-web"
  echo ""
  echo "$(bold 'Web Console 設定')"
  read -r -p "  監聽 Host [127.0.0.1]: " WEB_HOST
  read -r -p "  監聽 Port [8787]: " WEB_PORT
  read -r -p "  存取 Token（選填，留空不啟用）: " WEB_TOKEN
  WEB_HOST="${WEB_HOST:-127.0.0.1}"
  WEB_PORT="${WEB_PORT:-8787}"
  {
    echo "WUKONG_WEB_HOST=\"${WEB_HOST}\""
    echo "WUKONG_WEB_PORT=\"${WEB_PORT}\""
    echo "WUKONG_WEB_TOKEN=\"${WEB_TOKEN}\""
  } >> "$CONFIG_FILE"
fi

# memory
echo ""
echo "$(bold '記憶設定')"
read -r -p "  記憶資料庫位置 [${HOME}/.wukong/memory.db]: " MEM_DB
read -r -p "  Agent 工作目錄 [${HOME}/.wukong/workspace]: " WS_DIR
read -r -p "  啟用語意搜尋？(y/N): " EMBED
read -r -p "  Markdown 鏡像目錄（選填，留空停用）: " MD_DIR

if [[ -n "$MEM_DB" ]]; then
  echo "WUKONG_MEMORY_DB=\"${MEM_DB}\"" >> "$CONFIG_FILE"
fi
WS_DIR="${WS_DIR:-${HOME}/.wukong/workspace}"
echo "WUKONG_WORKSPACE=\"${WS_DIR}\"" >> "$CONFIG_FILE"

# ── Initialize local workspace templates ──
mkdir -p "${WS_DIR}"
if [[ ! -f "${WS_DIR}/SOUL.md" ]]; then
  info "初始化本地 SOUL.md..."
  curl -fsSL "${GITHUB}/${REPO}/raw/${VERSION}/workspace/SOUL.md" -o "${WS_DIR}/SOUL.md" 2>/dev/null || true
fi
if [[ ! -f "${WS_DIR}/AGENTS.md" ]]; then
  info "初始化本地 AGENTS.md..."
  curl -fsSL "${GITHUB}/${REPO}/raw/${VERSION}/workspace/AGENTS.md" -o "${WS_DIR}/AGENTS.md" 2>/dev/null || true
fi
EMBED_LOWER="$(echo "${EMBED:-n}" | tr '[:upper:]' '[:lower:]')"
case "$EMBED_LOWER" in
  y|yes|1) echo "WUKONG_EMBED=1" >> "$CONFIG_FILE" ;;
  *)       echo "WUKONG_EMBED=0" >> "$CONFIG_FILE" ;;
esac
if [[ -n "$MD_DIR" ]]; then
  echo "WUKONG_MD_DIR=\"${MD_DIR}\"" >> "$CONFIG_FILE"
fi

# common defaults
{
  echo "WUKONG_THINKING=1"
} >> "$CONFIG_FILE"

info "設定已寫入 $(green "$CONFIG_FILE")"

# --- systemd services (linux only) -------------------------------------------

if $HAS_SYSTEMD && ($NEED_TELEGRAM || $NEED_WEB); then
  step "安裝 systemd user service..."

  SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"
  mkdir -p "$SYSTEMD_USER_DIR"

  if $NEED_TELEGRAM; then
    cat > "${SYSTEMD_USER_DIR}/wukong-telegram.service" <<UNIT
[Unit]
Description=Wukong Telegram Bot
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
EnvironmentFile=%h/.wukong/config.env
ExecStart=%h/.local/bin/wukong-telegram
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
UNIT
    info "已建立 wukong-telegram.service"
  fi

  if $NEED_WEB; then
    cat > "${SYSTEMD_USER_DIR}/wukong-web.service" <<UNIT
[Unit]
Description=Wukong Web Console
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
EnvironmentFile=%h/.wukong/config.env
ExecStart=%h/.local/bin/wukong-web
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

[Install]
WantedBy=default.target
UNIT
    info "已建立 wukong-web.service"
  fi

  systemctl --user daemon-reload

  if $NEED_TELEGRAM; then
    systemctl --user enable --now wukong-telegram
    info "wukong-telegram 已啟動並設為開機自啟"
  fi
  if $NEED_WEB; then
    systemctl --user enable --now wukong-web
    info "wukong-web 已啟動並設為開機自啟"
  fi

  echo ""
  LINGER="$(loginctl show-user "$(whoami)" --property=Linger 2>/dev/null | cut -d= -f2)"
  if [[ "$LINGER" != "yes" ]]; then
    echo "$(red '⚠')  user service 在登出後會停止。執行以下指令讓服務常駐："
    echo ""
    echo "    $(bold "loginctl enable-linger")"
    echo ""
  fi
fi

# --- done --------------------------------------------------------------------

cat <<DONE

$(bold '═══════════════════════════════════════')
$(bold '  🐵 Wukong 安裝完成！')
$(bold '═══════════════════════════════════════')

  執行檔位置: $(green "${INSTALL_DIR}/wukong")
  設定檔位置: $(green "${CONFIG_FILE}")

  立即試用:
    $(bold "wukong") "你好，我是孫悟空"

  管理服務:
    $(dim "systemctl --user status wukong-telegram")
    $(dim "systemctl --user status wukong-web")
    $(dim "journalctl --user -u wukong-telegram -f")

  若你是 CLI 使用者，請在 ~/.bashrc 加入:
    $(dim '[ -f ~/.wukong/config.env ] && source ~/.wukong/config.env')

DONE