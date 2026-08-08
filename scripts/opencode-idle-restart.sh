#!/usr/bin/env bash
# 讓 `opencode serve` 取回 CLI 模式免費擁有的那個特性：週期性重置。
#
# `opencode run` 每回合生成一個程序、跑完就退出，所以 heap、快取、DB handle 與
# event loop 狀態全部隨程序消滅——累積在結構上不可能發生。`opencode serve` 常駐
# 不死，每回合的殘留全部留存，idle 成本於是變成「上次重啟以來累積工作量」的函數。
# 本腳本在離峰時段等到真正閒置時讓 server 自行退出，交給 compose 的
# `restart: unless-stopped` 拉起。
#
# 背景與量測見 docs/superpowers/specs/2026-08-08-opencode-server-residency-remediation-design.md
#
# ── 為什麼是 SIGINT 而不是 SIGTERM ──
# opencode 只註冊了 SIGINT 處理常式（SigCgt 0x10002 = SIGINT + SIGCHLD），沒有攔
# SIGTERM。而核心對 PID 1 有特殊規則：**只有裝了處理常式的訊號才會被送達**，採用
# 預設動作的訊號一律丟棄。容器內 opencode 正是 PID 1，所以送 SIGTERM 會石沉大海，
# 連 SIGKILL 也一樣（同 namespace 內殺不了 PID 1）。SIGINT 是唯一能用的那一個，
# 實測 0.1 秒乾淨退出。compose 的 stop_signal 因此也設成 SIGINT。
#
# ── 失效方向 ──
# 任何一項檢查出錯都當成「不閒置」。寧可漏一次重啟，也不要在回合進行中把 server
# 拉掉——漏掉的那次隔天窗口會再試。
set -uo pipefail

PORT="${WUKONG_OPENCODE_PORT:-4096}"
# 這裡刻意用 ${VAR-default} 而不是 ${VAR:-default}：後者會把「設成空字串」也視為
# 未設定而套用預設，於是文件宣稱的「設空字串即停用」會失效，反而啟用預設窗口。
WINDOW="${WUKONG_OPENCODE_RESTART_WINDOW-03:00-05:00}"
MIN_UPTIME="${WUKONG_OPENCODE_RESTART_MIN_UPTIME_SECS:-43200}"
QUIET_SECS="${WUKONG_OPENCODE_IDLE_QUIET_SECS:-300}"
POLL_SECS="${WUKONG_OPENCODE_RESTART_POLL_SECS:-60}"
DB="${WUKONG_OPENCODE_DB:-/home/wukong/.local/share/opencode/opencode.db}"
# 容器內 opencode 是 PID 1。可覆寫是為了讓「真的送出訊號」這條路徑能在容器外測試，
# 否則測試會把 SIGINT 送給開發機的 init。
TARGET_PID="${WUKONG_OPENCODE_TARGET_PID:-1}"

START_EPOCH=$(date +%s)
LAST_DB_CHANGE="$START_EPOCH"
LAST_DB_SIG=""

log() { printf '[wukong-idle-restart] %s %s\n' "$(date -Is)" "$*" >&2; }

if [[ -z "$WINDOW" ]]; then
    log "disabled (WUKONG_OPENCODE_RESTART_WINDOW is empty); server will run until restarted externally"
    exit 0
fi

if [[ ! "$WINDOW" =~ ^([0-9]{1,2}):([0-9]{2})-([0-9]{1,2}):([0-9]{2})$ ]]; then
    log "FATAL: malformed WUKONG_OPENCODE_RESTART_WINDOW '$WINDOW' (want HH:MM-HH:MM); giving up"
    exit 1
fi
WIN_START=$(( 10#${BASH_REMATCH[1]} * 60 + 10#${BASH_REMATCH[2]} ))
WIN_END=$(( 10#${BASH_REMATCH[3]} * 60 + 10#${BASH_REMATCH[4]} ))

# 窗口是以「容器的本地時間」判定的。compose 未設 TZ 時容器是 UTC，那 03:00-05:00
# 就會落在主機時間的中午——所以啟動時把解析結果印出來，讓設錯的人第一眼就看得到。
log "window=$WINDOW tz=$(date +%Z%z) now=$(date +%H:%M) min_uptime=${MIN_UPTIME}s quiet=${QUIET_SECS}s poll=${POLL_SECS}s"

in_window() {
    local now
    now=$(( 10#$(date +%H) * 60 + 10#$(date +%M) ))
    if (( WIN_START <= WIN_END )); then
        (( now >= WIN_START && now < WIN_END ))
    else
        # 跨午夜，例如 23:00-01:00
        (( now >= WIN_START || now < WIN_END ))
    fi
}

# 條件一：最近一次 session 更新已超過 QUIET_SECS。
# /session 依 time.updated 由新到舊排序，time 為毫秒 epoch。解析不出來就回傳「忙」。
sessions_idle() {
    local body
    body=$(curl -fsS --max-time 10 "http://127.0.0.1:${PORT}/session?limit=1" 2>/dev/null) || return 1
    python3 -c '
import json, sys, time
try:
    d = json.loads(sys.stdin.read())
except Exception:
    sys.exit(1)                      # 解析失敗 → 當成忙
if not isinstance(d, list):
    sys.exit(1)
if not d:
    sys.exit(0)                      # 完全沒有 session → 閒置
try:
    newest = max(s["time"]["updated"] for s in d) / 1000.0
except Exception:
    sys.exit(1)
sys.exit(0 if (time.time() - newest) >= float(sys.argv[1]) else 1)
' "$QUIET_SECS" <<<"$body"
}

# 條件二：對外埠沒有已建立的連線。
# 映像檔沒裝 iproute2／net-tools，所以直接讀 /proc/net/tcp{,6}：欄位 2 是本地位址
# HEX:PORT，欄位 4 是狀態。**只數 01（ESTABLISHED）**：進行中的回合會持續握著一條
# 連線，而 healthcheck 與本腳本自己的探測結束後會留下 TIME_WAIT（06），那些不算活動，
# 用「非 LISTEN」去數會把它們算進來，導致永遠判不出閒置。
no_connections() {
    local hex established
    hex=$(printf '%04X' "$PORT")
    established=$(cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | awk -v p=":$hex" '
        NR > 1 && index($2, p) && $4 == "01" { n++ } END { print n + 0 }')
    [[ "$established" == "0" ]]
}

# 條件三：opencode.db 已連續 QUIET_SECS 沒有寫入。
# 這一項是為了涵蓋 compaction 與任何背景寫入——不需要知道 opencode 內部怎麼運作，
# 就能避免在寫入途中把它拉掉。串流回合會持續寫這個檔，所以它同時也是活動偵測。
db_quiet() {
    local sig now
    now=$(date +%s)
    sig=$(stat -c '%s:%Y' "$DB" 2>/dev/null) || return 1
    if [[ "$sig" != "$LAST_DB_SIG" ]]; then
        LAST_DB_SIG="$sig"
        LAST_DB_CHANGE="$now"
        return 1
    fi
    (( now - LAST_DB_CHANGE >= QUIET_SECS ))
}

# 只對確實是 opencode 的目標程序送訊號。若 dispatch 改動導致 PID 1 變成別的東西，
# 寧可什麼都不做。
target_is_opencode() {
    tr '\0' ' ' < "/proc/$TARGET_PID/cmdline" 2>/dev/null | grep -q 'opencode'
}

while true; do
    # 每輪都更新 DB 指紋，否則窗口外的寫入不會被記錄，一進窗口就會誤判為已靜止。
    db_quiet >/dev/null 2>&1 || true

    if in_window; then
        uptime=$(( $(date +%s) - START_EPOCH ))
        # 順序刻意由便宜到昂貴：sessions_idle 會發 HTTP 請求，而那個請求本身就會建立
        # 一條連線，所以放到最後——只有前面都判定閒置時才問它。
        if (( uptime < MIN_UPTIME )); then
            log "in window but uptime ${uptime}s < ${MIN_UPTIME}s; skipping"
        elif ! db_quiet; then
            log "in window but opencode.db was written within ${QUIET_SECS}s; skipping"
        elif ! no_connections; then
            log "in window but port ${PORT} still has established connections; skipping"
        elif ! sessions_idle; then
            log "in window but a session was updated within ${QUIET_SECS}s; skipping"
        elif ! target_is_opencode; then
            log "ERROR: pid $TARGET_PID is not opencode; refusing to signal"
        else
            log "idle for ${QUIET_SECS}s after ${uptime}s uptime — sending SIGINT to pid $TARGET_PID for a clean restart"
            kill -INT "$TARGET_PID" 2>/dev/null || log "ERROR: failed to signal pid $TARGET_PID"
            # 送出後就交棒：PID 1 退出 → 容器結束 → restart: unless-stopped 拉起。
            exit 0
        fi
    fi

    sleep "$POLL_SECS"
done
