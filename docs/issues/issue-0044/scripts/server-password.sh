#!/usr/bin/env bash
# Task 3.2（SCN-003、SCN-017）：常駐 server 模式的密碼接線與 healthcheck，對真實 V2 驗證。
# 用法：server-password.sh IMAGE [V2_PREFIX]
#   IMAGE：Wukong 映像。用它的 opencode，或 V2_PREFIX 的 opencode
#   V2_PREFIX：`npm install -g --prefix V2_PREFIX @opencode/cli@2.3.2` 的目錄，映像還沒換 V2 時用
# 在 repo 根目錄執行，先 `cargo build -p wukong-cli`。
#
# 密碼、環境變數與 healthcheck 命令都從 `docker compose config`（release compose）取出，驗的是
# 實際出貨的設定；容器以 container_name 以外的名稱另外啟動，不碰正式部署。server 容器
# --network none，模型是容器內只綁 127.0.0.1 的假模型；主機上的 wukong 經 docker exec 的
# 轉接連進容器，不開任何對外的網路。
set -uo pipefail
image="${1:?usage: server-password.sh IMAGE [V2_PREFIX]}"
v2="${2:+$(cd "$2" && pwd)}"
repo="$(pwd)"
tag="pw-$$"
work="$(mktemp -d)"
relay_pid=""
cleanup() {
    [[ -z "$relay_pid" ]] || kill "$relay_pid" 2>/dev/null
    docker ps -aq --filter "name=^$tag-" | xargs -r docker rm -f > /dev/null
    rm -rf "$work"
}
trap cleanup EXIT
fails=0
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }

password="$(python3 -c 'import secrets; print(secrets.token_hex(24))')"
WUKONG_AGENT_SERVER_URL=http://opencode-server:4096 WUKONG_AGENT_SERVER_PASSWORD="$password" \
    docker compose --env-file /dev/null -f docker-compose.release.yml --profile server config --format json \
    > "$work/config.json"
python3 -I - "$work/config.json" "$password" "$work" <<'PY'
import json, sys
services = json.load(open(sys.argv[1]))["services"]
password, work = sys.argv[2], sys.argv[3]
server = services["opencode-server"]
assert server["environment"]["OPENCODE_SERVER_PASSWORD"] == password
for name in ("wukong-telegram", "wukong-web", "wukong-schedulerd"):
    assert services[name]["environment"]["WUKONG_AGENT_SERVER_PASSWORD"] == password, name
# `config` keeps compose's `$$` escape; the engine receives a single `$`.
test = server["healthcheck"]["test"]
assert test[0] == "CMD-SHELL", test
open(f"{work}/health.sh", "w").write(test[1].replace("$$", "$"))
print("compose hands one password to opencode-server and every entrance")
PY
check "compose wiring" $?
health="$(cat "$work/health.sh")"

cat > "$work/fake.py" <<'PY'
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
class H(BaseHTTPRequestHandler):
    def log_message(self, *a): pass
    def do_POST(self):
        self.rfile.read(int(self.headers.get("content-length", 0)))
        self.send_response(200); self.send_header("content-type", "text/event-stream"); self.end_headers()
        for delta, finish in (({"role": "assistant", "content": "PONG"}, None), ({}, "stop")):
            chunk = {"id": "c", "object": "chat.completion.chunk", "created": 0, "model": "m",
                     "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}
            self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
        self.wfile.write(b"data: [DONE]\n\n")
ThreadingHTTPServer(("127.0.0.1", 18080), H).serve_forever()
PY
provider='{"model":"fake/m","provider":{"fake":{"npm":"@ai-sdk/openai-compatible","name":"Fake","options":{"baseURL":"http://127.0.0.1:18080/v1","apiKey":"unused"},"models":{"m":{"name":"m"}}}}}'
v2_args=()
if [[ -n "$v2" ]]; then
    path="$(docker image inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$image" | sed -n 's/^PATH=//p')"
    v2_args=(-e PATH="/v2prefix/bin:$path" -v "$v2":/v2prefix:ro)
fi

server() { # server NAME PASSWORD：照 compose 的 command 與 healthcheck 起 V2 server
    docker run -d --name "$tag-$1" --network none -e USER_ID="$(id -u)" -e GROUP_ID="$(id -g)" \
        -e WUKONG_WORKSPACE=/workspace -e OPENCODE_SERVER_PASSWORD="$2" -e OPENCODE_CONFIG_CONTENT="$provider" \
        "${v2_args[@]}" -v "$work":/probe:ro \
        -v "$repo/scripts/docker-entrypoint.sh":/probe-entrypoint.sh:ro \
        --health-cmd "$health" --health-interval 2s --health-timeout 5s --health-retries 3 \
        --entrypoint bash "$image" /probe-entrypoint.sh opencode serve --hostname 0.0.0.0 --port 4096 > /dev/null
}
health_of() { docker inspect --format '{{.State.Health.Status}}' "$tag-$1"; }
wait_health() { # wait_health NAME WANT：最多 90 秒
    local i
    for i in $(seq 1 45); do [[ "$(health_of "$1")" == "$2" ]] && return 0; sleep 2; done
    return 1
}

# 1. 同一組密碼：healthcheck 判 healthy。
server ok "$password"
docker exec -d "$tag-ok" python3 -I /probe/fake.py
wait_health ok healthy; check "server with the compose password is healthy ($(health_of ok))" $?

# 2. 入口拿同一組密碼：就緒檢查通過、回合成功。主機的 wukong 經 docker exec 轉接連進容器。
python3 -I - "$tag-ok" > "$work/relay.port" <<'PY' &
import socket, subprocess, sys, threading
container = sys.argv[1]
listener = socket.socket(); listener.bind(("127.0.0.1", 0)); listener.listen(16)
print(listener.getsockname()[1], flush=True)
RELAY = ("import socket,sys,threading\n"
         "s=socket.create_connection(('127.0.0.1',4096))\n"
         "def up():\n"
         "    while (b:=sys.stdin.buffer.read1(65536)): s.sendall(b)\n"
         "    s.shutdown(socket.SHUT_WR)\n"
         "threading.Thread(target=up,daemon=True).start()\n"
         "while (b:=s.recv(65536)): sys.stdout.buffer.write(b); sys.stdout.buffer.flush()\n")
def serve(conn):
    proc = subprocess.Popen(["docker", "exec", "-i", container, "python3", "-I", "-c", RELAY],
                            stdin=subprocess.PIPE, stdout=subprocess.PIPE, bufsize=0)
    def down():
        while (b := proc.stdout.read(65536)):
            conn.sendall(b)
        conn.close()
    threading.Thread(target=down, daemon=True).start()
    while (b := conn.recv(65536)):
        proc.stdin.write(b)
    proc.stdin.close()
while True:
    conn, _ = listener.accept()
    threading.Thread(target=serve, args=(conn,), daemon=True).start()
PY
relay_pid=$!
for _ in $(seq 1 20); do [[ -s "$work/relay.port" ]] && break; sleep 0.5; done
port="$(head -1 "$work/relay.port")"
turn() { # turn PASSWORD：回傳 rc，輸出到 $work/turn.out／turn.err
    WUKONG_MEMORY_DB="sqlite://$work/memory.db" WUKONG_WORKSPACE="$work" \
        WUKONG_AGENT_SERVER_URL="http://127.0.0.1:$port" WUKONG_AGENT_SERVER_PASSWORD="$1" \
        WUKONG_AGENT_SERVER_FILE_MODE=inline WUKONG_AGENT_TIMEOUT_SECS=120 HOME="$work" \
        timeout 180 "$repo/target/debug/wukong" --scope project:issue44-pw --no-stream --no-thinking \
        "Reply with exactly the word PONG." > "$work/turn.out" 2> "$work/turn.err"
}
turn "$password"; rc=$?
grep -q PONG "$work/turn.out"; check "entrance with the same password finishes a turn (rc=$rc)" $(( rc || $? ))
turn wrong; rc=$?
grep -q "認證被拒" "$work/turn.err"; check "entrance with another password is refused by name (rc=$rc)" $(( !rc || $? ))

# 3. 反向：healthcheck 帶錯的密碼就失敗，證明它真的有驗證。
docker exec -e OPENCODE_SERVER_PASSWORD=wrong "$tag-ok" sh -c "$health"
check "healthcheck with a wrong password fails" $(( ! $? ))

# 4. .env 沒設密碼：compose 給 server 空字串，V2 自己產生隨機密碼，healthcheck 判 unhealthy。
#    舊的 healthcheck（/global/health，不帶認證）對同一個 server 仍然成功，抓不到。
server empty ""
wait_health empty unhealthy; check "server without a password is unhealthy ($(health_of empty))" $?
docker exec "$tag-empty" sh -c 'curl -fsS http://localhost:4096/global/health > /dev/null'
check "reverse: the old unauthenticated healthcheck still passes on it" $?

# 5. server 不可用：同一個命令在沒有 server 的容器裡失敗。
docker run -d --name "$tag-down" --network none -e OPENCODE_SERVER_PASSWORD="$password" \
    --entrypoint sleep "$image" infinity > /dev/null
docker exec "$tag-down" sh -c "$health" 2> /dev/null
check "healthcheck fails when no server answers" $(( ! $? ))

echo "$fails failed"
exit $(( fails > 0 ))
