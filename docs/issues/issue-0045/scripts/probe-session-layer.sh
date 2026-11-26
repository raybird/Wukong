#!/usr/bin/env bash
# #45 Task 4.1（SCN-007）與 4.2（SCN-006、SCN-008）外迴圈：真實的 wukong 入口（常駐 server 模式）接 V2 2.3.2。
# 用法：probe-session-layer.sh IMAGE WORK_DIR AUTH_JSON MODEL
#   AUTH_JSON：只含要用的 provider 的 opencode auth.json；MODEL：provider/model，寫進隔離設定檔的 agent.default_model。
# 在 repo 根目錄執行。只送固定的合成提示詞。HOME、WUKONG_WORKSPACE、記憶庫、設定檔都在 WORK_DIR，
# 以相同路徑掛進容器。
#
# 三個回合：
#   1. 要求每天提醒：排程建在本回合的 scope；session 的指令層有 wukong.scope；user message 只有使用者原文。
#   2. 把 session 開始時間改成 8 天前，同一個 scope 再要求一次：換了新 session，新 session 也有 wukong.scope，
#      第二個排程也建在同一個 scope。
#   3. 另一個 scope、WUKONG_MEMORIA_HINT=1：agent 定義改寫成含長期記憶能力，問模型有沒有被告知 memoria，應答「有」。
#   4. 關閉開關、另一個記憶庫，問同一題，應答「沒有」。兩回合的 user message 都只有原文。
# agent 的 bash 在容器裡執行：排程指令用映像內建的 wukong（主機編出的需要較新的 glibc），寫進同一個記憶庫。
set -uo pipefail
image="${1:?usage: probe-session-layer.sh IMAGE WORK_DIR AUTH_JSON MODEL}"
work="$(realpath "${2:?}")"
auth="${3:?}" model="${4:?}"
repo="$(pwd)"
box="probe45-layer-$$"
pw="probe-$$"
fails=0
# 訊息裡有 $(…) 時 $? 會被它覆寫：條件的結果先存進變數再呼叫 check
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }
cleanup() { docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

ws="$work/ws"
mkdir -p "$work/home/.local/share/opencode" "$ws" "$work/out"
install -m 600 "$auth" "$work/home/.local/share/opencode/auth.json"
printf '{"agent":{"default_model":"%s"}}\n' "$model" > "$work/home/settings.json"
cargo build -q -p wukong-cli || { echo "FAIL cargo build"; exit 1; }
wukong="$repo/target/debug/wukong"
db="$work/memory.db"
ocdb="$work/home/.local/share/opencode/opencode.db"

docker run -d --rm --name "$box" --user "$(id -u):$(id -g)" -e HOME="$work/home" -v "$work":"$work" -w "$ws" \
    -e WUKONG_MEMORY_DB="sqlite://$db" -e WUKONG_WORKSPACE="$ws" -e WUKONG_SETTINGS_FILE="$work/home/settings.json" \
    -p 127.0.0.1::4096 -e OPENCODE_SERVER_PASSWORD="$pw" \
    --entrypoint opencode "$image" serve --hostname 0.0.0.0 --port 4096 > /dev/null
port="$(docker port "$box" 4096/tcp | head -1 | sed 's/.*://')"
url="http://127.0.0.1:$port"
api() { curl -sf -u "opencode:$pw" "$url$1"; }
for _ in $(seq 30); do api /api/info > /dev/null && break; sleep 1; done

turn() { # turn NAME SCOPE PROMPT [ENV...]
    local name="$1" scope="$2" prompt="$3"; shift 3
    env -i PATH="$PATH" HOME="$work/home" WUKONG_WORKSPACE="$ws" WUKONG_SETTINGS_FILE="$work/home/settings.json" \
        WUKONG_MEMORY_DB="sqlite://$db" WUKONG_AGENT_SERVER_URL="$url" WUKONG_AGENT_SERVER_PASSWORD="$pw" \
        WUKONG_AGENT_TIMEOUT_SECS=500 "$@" \
        timeout 600 "$wukong" --scope "$scope" --no-stream "$prompt" \
        > "$work/out/$name.out" 2> "$work/out/$name.err" < /dev/null
}
session_of() { sqlite3 "file:${2:-$db}?mode=ro" "select session_id from agent_session_state where scope='$1'"; }
jobs_of() { sqlite3 "file:$db?mode=ro" "select count(*) from scheduler_jobs where json_extract(config_json, '$.Turn.scope') = '$1'"; }
scope_entry() { api "/api/experimental/session/$1/instructions/entries" \
    | python3 -c 'import json,sys; print(next((e["value"] for e in json.load(sys.stdin)["data"] if e["key"]=="wukong.scope"), ""))'; }
# session 的 user message 是否都只有使用者原文（不含任何固定區塊的標頭）
clean_user_messages() { # clean_user_messages SESSION PROMPT
    /usr/bin/python3 - "$ocdb" "$1" "$2" << 'EOF'
import json, sqlite3, sys
c = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
texts = [json.loads(d).get("text", "") for (d,) in
         c.execute("select data from session_message where session_id = ? and type = 'user'", (sys.argv[2],))]
headers = ["[排程能力]", "[目前的 scope]", "[輸出要求]", "[長期記憶能力]", "[檔案互動規則]", "[產出目錄]"]
found = sorted({h for t in texts for h in headers if h in t})
# SCN-006：召回的相關記憶照舊附加（[相關記憶] … [使用者輸入] 原文），固定區塊不可出現
ok = texts and not found and all(t == sys.argv[3] or t.endswith("[使用者輸入]\n" + sys.argv[3]) for t in texts)
recalled = sum(t.startswith("[相關記憶]") for t in texts)
print(f"{len(texts)} user messages, {recalled} with recalled memories, fixed headers {found}")
sys.exit(0 if ok else 1)
EOF
}
definition_has() { grep -qF -- "$1" "$ws/.opencode/agents/wukong.md"; }

# --- 回合 1 ---
s="project:probe45-sched"
p1="每天早上 8 點提醒我喝水。"
turn t1 "$s" "$p1"; rc=$?
check "turn 1 succeeded (rc=$rc)" "$rc"
[[ "$(jobs_of "$s")" == 1 ]]; r=$?
check "turn 1 created a schedule owned by $s ($(jobs_of "$s"))" $r
s1="$(session_of "$s")"
v="$(scope_entry "$s1")"
[[ "$v" == *"\`$s\`"* ]]
check "session ${s1:0:12}… carries wukong.scope for $s" $?
m="$(clean_user_messages "$s1" "$p1")"
check "turn 1 user message is the user's text only ($m)" $?
definition_has "[排程能力]" && definition_has "[輸出要求]" && definition_has "[檔案互動規則]" && ! definition_has "[長期記憶能力]"
check "agent wukong carries scheduling, output and file rules, no memoria (hint off)" $?

# --- 回合 2：session 已開始 8 天 ---
sqlite3 "$db" "update agent_session_state set session_started_at = strftime('%s','now') - 8*86400 where scope='$s'"
p2="每週一早上 9 點提醒我開週會。"
turn t2 "$s" "$p2"; rc=$?
check "turn 2 succeeded (rc=$rc)" "$rc"
grep -q session_rotation_due "$work/out/t2.err"
check "turn 2 rotated the 8-day-old session" $?
s2="$(session_of "$s")"
[[ -n "$s2" && "$s2" != "$s1" ]]
check "turn 2 runs on a new session (${s2:0:12}…)" $?
v="$(scope_entry "$s2")"
[[ "$v" == *"\`$s\`"* ]]
check "the new session carries wukong.scope for $s" $?
[[ "$(jobs_of "$s")" == 2 ]]; r=$?
check "turn 2 created a second schedule owned by $s ($(jobs_of "$s"))" $r
m="$(clean_user_messages "$s2" "$p2")"
check "turn 2 user message is the user's text only ($m)" $?

# --- 回合 3：開啟長期記憶能力 ---
s3="project:probe45-memoria"
p3="你這個 agent 的系統提示裡，有沒有提到一個叫 memoria 的 CLI？只回答「有」或「沒有」，不要執行任何工具。"
turn t3 "$s3" "$p3" WUKONG_MEMORIA_HINT=1; rc=$?
check "turn 3 succeeded (rc=$rc)" "$rc"
definition_has "[長期記憶能力]"
check "with WUKONG_MEMORIA_HINT=1 agent wukong carries the memoria capability" $?
grep -q '有' "$work/out/t3.out" && ! grep -q '沒有' "$work/out/t3.out"; r=$?
check "with the hint on the model says it was told about memoria ($(tr '\n' ' ' < "$work/out/t3.out" | cut -c1-40))" $r
m="$(clean_user_messages "$(session_of "$s3")" "$p3")"
check "turn 3 user message is the user's text only ($m)" $?

# --- 回合 4：關閉長期記憶能力，問同一題（另一個記憶庫，召回不到回合 3 的答案） ---
s4="project:probe45-nomemoria"
turn t4 "$s4" "$p3" WUKONG_MEMORY_DB="sqlite://$work/memory-b.db"; rc=$?
check "turn 4 succeeded (rc=$rc)" "$rc"
! definition_has "[長期記憶能力]"
check "with the hint off agent wukong no longer carries the memoria capability" $?
grep -q '沒有' "$work/out/t4.out"; r=$?
check "with the hint off the model says it was not told about memoria ($(tr '\n' ' ' < "$work/out/t4.out" | cut -c1-40))" $r
m="$(clean_user_messages "$(session_of "$s4" "$work/memory-b.db")" "$p3")"
check "turn 4 user message is the user's text only ($m)" $?

echo "schedules:"; sqlite3 "file:$db?mode=ro" "select name, cron, json_extract(config_json, '$.Turn.scope') from scheduler_jobs" | sed 's/^/  /'
echo "fails=$fails"
exit $(( fails > 0 ))
