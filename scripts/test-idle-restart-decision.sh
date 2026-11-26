#!/usr/bin/env bash
# opencode-idle-restart.sh 的決策邏輯行為測試。
#
# 這個腳本先前只有「字串出現在檔案裡」的檢查（test-docker-runtime.sh），而字串檢查
# 對它真正的缺陷是盲的：schedulerd 的長生命週期 keep-alive 讓「有連線就不重啟」永遠
# 成立，連續 87 次 connection_skips、重啟從未發生，而所有 grep 檢查全綠。所以這裡
# 實際把腳本跑起來，用真的 TCP 連線與真的 HTTP 端點驗證它「有沒有送出訊號」。
set -uo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
SCRIPT="$PWD/scripts/opencode-idle-restart.sh"

TMP=$(mktemp -d)
FIXTURE_PIDS=()

kill_fixture() {
    local pid
    for pid in "${FIXTURE_PIDS[@]:-}"; do
        [[ -n "$pid" ]] && kill -9 "$pid" 2>/dev/null
    done
    FIXTURE_PIDS=()
    wait 2>/dev/null
}
trap 'kill_fixture; rm -rf "$TMP"' EXIT

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

# 一條閒置但 ESTABLISHED 的連線——模擬 schedulerd。連上就不動，直到測試結束。
cat >"$TMP/hold_conn.py" <<'PY'
import socket
import sys
import time

s = socket.create_connection(("127.0.0.1", int(sys.argv[1])))
sys.stdout.write("held\n")
sys.stdout.flush()
while True:
    time.sleep(3600)
PY

# 產生一個 V2 形狀的 server（issue-0044）：`/api/session` 要 basic auth（使用者 opencode、密碼
# FAKE_PASSWORD），回 `{"data": [...], "cursor": {...}}`；其他路徑跟 V2 一樣回 200 的網頁。
# 必須是 threading server：測試會另外掛一條閒置連線不放，單執行緒的 http.server 會被它卡死，
# 連 curl 探測都進不來。
# $1 = 產生 `/api/session` response body 的 python 運算式。
FAKE_PASSWORD=test-pass
write_fake_server() {
    cat >"$TMP/fake_server.py" <<PY
import base64
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

EXPECTED = "Basic " + base64.b64encode(b"opencode:$FAKE_PASSWORD").decode()


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/api/"):
            if self.headers.get("Authorization") != EXPECTED:
                self.reply(401, "application/json", '{"error":"unauthorized"}')
                return
            self.reply(200, "application/json", $1)
        else:
            self.reply(200, "text/html", "<!doctype html><html></html>")

    def reply(self, code, content_type, text):
        body = text.encode()
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, *_args):
        pass


srv = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
with open("$TMP/port", "w") as fh:
    fh.write(str(srv.server_address[1]))
srv.serve_forever()
PY
}

wait_for_file() {
    local path="$1" i
    for i in $(seq 1 100); do
        [[ -s "$path" ]] && return 0
        sleep 0.1
    done
    return 1
}

start_fixture() {
    kill_fixture
    rm -f "$TMP/port" "$TMP/held"

    python3 "$TMP/fake_server.py" &
    FIXTURE_PIDS+=("$!")
    wait_for_file "$TMP/port" || fail "fake server did not report a port"
    PORT=$(cat "$TMP/port")

    python3 "$TMP/hold_conn.py" "$PORT" >"$TMP/held" 2>&1 &
    FIXTURE_PIDS+=("$!")
    wait_for_file "$TMP/held" || fail "could not establish the idle keep-alive connection"

    # 假的 opencode 目標程序：target_is_opencode 只看 /proc/PID/cmdline 有沒有
    # 'opencode'，用 exec -a 改 argv[0] 就夠，不需要真的 opencode。
    #
    # 這裡必須先開 job control（set -m）。非互動 shell 用 `&` 背景啟動的子程序，
    # POSIX 規定其 SIGINT／SIGQUIT 會被設成 SIG_IGN，而且「進入時即被忽略的訊號無法
    # 再被 trap」——假目標於是完全收不到 SIGINT，測試會誤判成「腳本沒送出訊號」。
    # 真實情境沒有這個問題：容器內 opencode 是 PID 1，自己註冊了 SIGINT handler。
    set -m
    bash -c 'exec -a fake-opencode-server sleep 600' &
    TARGET_PID=$!
    set +m
    FIXTURE_PIDS+=("$TARGET_PID")

    : >"$TMP/opencode.db"
}

# 一個必定涵蓋「現在」的窗口，跨午夜時也成立。
current_window() {
    python3 -c '
import time
t = time.localtime()
start = t.tm_hour * 60 + t.tm_min
end = (start + 3) % 1440
print(f"{start // 60:02d}:{start % 60:02d}-{end // 60:02d}:{end % 60:02d}")
'
}

# $1 = CONN_GRACE_SECS, $2 = 觀察秒數, $3 = 日誌輸出路徑, $4 = 交給 supervisor 的密碼（預設正確的那組）
run_supervisor() {
    local grace="$1" seconds="$2" out="$3" password="${4-$FAKE_PASSWORD}" sup
    OPENCODE_SERVER_PASSWORD="$password" \
    WUKONG_OPENCODE_PORT="$PORT" \
    WUKONG_OPENCODE_RESTART_WINDOW="$(current_window)" \
    WUKONG_OPENCODE_RESTART_MIN_UPTIME_SECS=0 \
    WUKONG_OPENCODE_IDLE_QUIET_SECS=1 \
    WUKONG_OPENCODE_RESTART_POLL_SECS=1 \
    WUKONG_OPENCODE_CONN_GRACE_SECS="$grace" \
    WUKONG_OPENCODE_DB="$TMP/opencode.db" \
    WUKONG_OPENCODE_TARGET_PID="$TARGET_PID" \
        bash "$SCRIPT" >"$out" 2>&1 &
    sup=$!
    sleep "$seconds"
    kill -9 "$sup" 2>/dev/null
    wait "$sup" 2>/dev/null
}

# ── 案例一：寬限期未到，閒置的 keep-alive 仍應擋下重啟 ──
# 這是舊行為唯一正確的部分，必須保住：真正進行中的回合握著連線時不能被拉掉。
write_fake_server "'{\"data\":[],\"cursor\":{\"next\":null}}'"
start_fixture
run_supervisor 3600 6 "$TMP/wait.log"

grep -q "established connection" "$TMP/wait.log" \
    || fail "supervisor should report the blocking connection; log:
$(cat "$TMP/wait.log")"
grep -q "waiting up to 3600s" "$TMP/wait.log" \
    || fail "supervisor should say the block is bounded, not permanent; log:
$(cat "$TMP/wait.log")"
kill -0 "$TARGET_PID" 2>/dev/null \
    || fail "supervisor killed opencode while inside the connection grace period"
echo "ok: an established connection still blocks the restart inside the grace period"

# ── 案例二：寬限期歸零，閒置的 keep-alive 不得永久擋住重啟 ──
# 這一項在修正前必定失敗——舊的 no_connections() 沒有寬限期，連線存在就永遠跳過。
run_supervisor 0 8 "$TMP/override.log"

grep -q "overriding" "$TMP/override.log" \
    || fail "supervisor should override an idle keep-alive once the grace expires; log:
$(cat "$TMP/override.log")"
grep -q "sending SIGINT" "$TMP/override.log" \
    || fail "supervisor should signal for restart; log:
$(cat "$TMP/override.log")"
kill -0 "$TARGET_PID" 2>/dev/null \
    && fail "supervisor logged the restart but the target survived the SIGINT"
echo "ok: an idle keep-alive no longer blocks the restart forever"

# ── 案例三：session 有活動時，連線寬限期怎麼設都不能重啟 ──
# 安全閘門不能因為放寬連線判斷而一起鬆掉。
write_fake_server "'{\"data\":[{\"id\":\"ses_1\",\"time\":{\"updated\":%d}}],\"cursor\":{}}' % int(time.time() * 1000)"
start_fixture
run_supervisor 0 6 "$TMP/busy.log"

grep -q "a session was updated within" "$TMP/busy.log" \
    || fail "supervisor should refuse while a session is active; log:
$(cat "$TMP/busy.log")"
grep -q "sending SIGINT" "$TMP/busy.log" \
    && fail "supervisor restarted opencode while a session was active"
kill -0 "$TARGET_PID" 2>/dev/null \
    || fail "supervisor killed opencode while a session was active"
echo "ok: an active session still blocks the restart regardless of the connection grace"

# ── 案例四、五（issue-0044）：問不到 V2 的 session 列表時，不能當成閒置 ──
# V1 的 `/session` 在 V2 回 200 的網頁；不帶或帶錯密碼回 401。兩者都判「忙」，而且 log 要說出原因，
# 不能跟真的有活動一樣印「a session was updated」（V1 腳本在 V2 上就是這樣每天無聲跳過）。
write_fake_server "'<!doctype html><html></html>'"
start_fixture
run_supervisor 0 6 "$TMP/html.log"
grep -q "sending SIGINT" "$TMP/html.log" \
    && fail "supervisor restarted opencode although the session list was not JSON"
grep -q "not the V2 session list" "$TMP/html.log" \
    || fail "supervisor should say the session list was unreadable; log:
$(cat "$TMP/html.log")"
kill -0 "$TARGET_PID" 2>/dev/null \
    || fail "supervisor killed opencode although the session list was not JSON"
echo "ok: an unreadable session list blocks the restart and says why"

write_fake_server "'{\"data\":[],\"cursor\":{}}'"
start_fixture
run_supervisor 0 6 "$TMP/auth.log" wrong-pass
grep -q "sending SIGINT" "$TMP/auth.log" \
    && fail "supervisor restarted opencode although the server rejected the password"
grep -q "rejected the password" "$TMP/auth.log" \
    || fail "supervisor should say the password was rejected; log:
$(cat "$TMP/auth.log")"
grep -q "wrong-pass" "$TMP/auth.log" \
    && fail "supervisor printed the password"
echo "ok: a rejected password blocks the restart and says why"

echo "idle-restart decision checks passed"
