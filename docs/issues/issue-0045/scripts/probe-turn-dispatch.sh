#!/usr/bin/env bash
# #45 Task 3.1 外迴圈：真實的 wukong 入口（常駐 server 模式）接 V2 2.3.2，跑一題會派給 fixer 的回合。
# 檢查回合等到 subagent 回報與 primary 的最終回覆才結束、只送出一次 prompt、指定的是 agent wukong。
# 用法：probe-turn-dispatch.sh IMAGE WORK_DIR
#   server 先啟動，wukong 之後才寫入 agent 定義並立刻送出 prompt（與既有部署升級後的第一個回合相同）。
#   模型用 server 的預設模型（沒有憑證時是免費的 opencode/big-pickle）。只送固定的合成提示詞。
# wukong 在本機執行，HOME、WUKONG_WORKSPACE、記憶庫、設定檔都在 WORK_DIR；工作區以相同路徑掛進容器，
# server 看到的目錄與 wukong 送出的 location 一致。
set -uo pipefail
image="${1:?usage: probe-turn-dispatch.sh IMAGE WORK_DIR}"
work="$(realpath "${2:?}")"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
box="probe45-dispatch-$$"
pw="probe-$$"
fails=0
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }
events_pid=""
cleanup() { [[ -n "$events_pid" ]] && kill "$events_pid" 2> /dev/null; docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

ws="$work/ws"
mkdir -p "$work/home/.local/share/opencode" "$ws" "$work/out"
echo '{}' > "$work/home/.local/share/opencode/auth.json"
(cd "$repo" && cargo build -q -p wukong-cli) || { echo "FAIL cargo build"; exit 1; }
wukong="$repo/target/debug/wukong"

docker run -d --rm --name "$box" --user "$(id -u):$(id -g)" -e HOME="$work/home" -v "$work":"$work" -w "$ws" \
    -p 127.0.0.1::4096 -e OPENCODE_SERVER_PASSWORD="$pw" \
    --entrypoint opencode "$image" serve --hostname 0.0.0.0 --port 4096 > /dev/null
port="$(docker port "$box" 4096/tcp | head -1 | sed 's/.*://')"
url="http://127.0.0.1:$port"
api() { curl -sf -u "opencode:$pw" "$url$1"; }
for _ in $(seq 30); do api /api/info > /dev/null && break; sleep 1; done

# 整個事件串流另存一份，回合結束時機不對時用來看 primary 與 subagent 的事件順序
curl -sN -u "opencode:$pw" "$url/api/event" > "$work/out/events.sse" 2> /dev/null &
events_pid=$!
prompt="請派給 fixer 處理：在工作區建立 hello.txt，內容只有一行：hello from wukong。完成後告訴我檔案內容。"
start=$(date +%s)
env -i PATH="$PATH" HOME="$work/home" WUKONG_WORKSPACE="$ws" WUKONG_SETTINGS_FILE="$work/home/settings.json" \
    WUKONG_MEMORY_DB="sqlite://$work/memory.db" \
    WUKONG_AGENT_SERVER_URL="$url" WUKONG_AGENT_SERVER_PASSWORD="$pw" WUKONG_AGENT_TIMEOUT_SECS=500 \
    timeout 600 "$wukong" --scope project:probe45 --no-stream "$prompt" \
    > "$work/out/stdout.txt" 2> "$work/out/stderr.txt" < /dev/null
rc=$?
echo "  wukong rc=$rc in $(( $(date +%s) - start ))s"
echo "  reply: $(tr '\n' ' ' < "$work/out/stdout.txt" | cut -c1-200)"
check "the wukong turn succeeded" "$rc"
! grep -qi 'agent not found' "$work/out/stdout.txt" "$work/out/stderr.txt"
check "the freshly written wukong agent was found by the running server" $?
grep -qx 'hello from wukong' "$ws/hello.txt" 2> /dev/null
check "hello.txt has the requested content" $?

sleep 2
api "/api/session?location%5Bdirectory%5D=$ws&limit=50" > "$work/out/sessions.json"
python3 - "$work/out/sessions.json" "$url" "$pw" "$work/out/stdout.txt" <<'EOF_CHECK'
import base64, json, sys, urllib.request
sessions = json.load(open(sys.argv[1]))["data"]
url, pw = sys.argv[2], sys.argv[3]
reply = open(sys.argv[4]).read()
def get(path):
    req = urllib.request.Request(url + path, headers={
        "Authorization": "Basic " + base64.b64encode(f"opencode:{pw}".encode()).decode()})
    return json.load(urllib.request.urlopen(req))
roots = [s for s in sessions if not s.get("parentID")]
kids = [s for s in sessions if s.get("parentID")]
bad = 0
def report(ok, text):
    global bad
    print(("PASS" if ok else "FAIL") + " " + text)
    bad += not ok
report(len(roots) == 1, f"one primary session ({len(roots)})")
msgs = get(f"/api/session/{roots[0]['id']}/message")["data"] if roots else []
users = [m for m in msgs if m.get("type") == "user"]
assistants = [m for m in msgs if m.get("type") == "assistant"]
synthetic = [m for m in msgs if m.get("type") == "synthetic"]
report(len(users) == 1, f"wukong sent exactly one prompt ({len(users)} user messages)")
agents = sorted({m.get("agent") for m in assistants})
report(agents == ["wukong"], f"the primary session ran as agent wukong ({agents})")
kid_agents = sorted({s.get("agent") for s in kids})
for s in kids:
    for m in get(f"/api/session/{s['id']}/message")["data"]:
        if m.get("type") == "assistant":
            kid_agents.append(m.get("agent"))
report("fixer" in kid_agents, f"the work was dispatched to the fixer subagent ({sorted(set(kid_agents))})")
report(len(synthetic) >= 1, f"fixer's report came back to the primary ({len(synthetic)} synthetic)")
# 最終回覆是 primary 最後一則 assistant 訊息的文字（V2 由新到舊排列）
final = next(("".join(c.get("text", "") for c in m.get("content", []) if c.get("type") == "text").strip()
              for m in msgs if m.get("type") == "assistant"), "")
report(bool(final) and final[:40] in reply, "wukong returned the primary's final reply, not the dispatch notice")
models = set()
for s in [*roots, *kids]:
    for m in get(f"/api/session/{s['id']}/message")["data"]:
        if m.get("type") == "assistant":
            mm = m.get("model") or {}
            models.add(f"{mm.get('providerID')}/{mm.get('id')}")
print(f"  models: {sorted(models)}")
sys.exit(bad)
EOF_CHECK
rc=$?; fails=$((fails + rc))

echo "fails=$fails"
exit $((fails > 0))
