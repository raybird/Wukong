#!/usr/bin/env bash
# #45 Task 1.3：V2 2.3.2 的 session instruction entries（experimental）能不能持續存在、
# 不進對話紀錄、模型讀得到；覆寫、刪除、server 重啟（本機按需模式每回合起新 server）後的行為。
# 用法：probe-instructions.sh IMAGE WORK_DIR AUTH_JSON MODEL
#   參數與 probe-agents.sh 相同；用預設免費模型時 AUTH_JSON 給內容為 {} 的檔案。
# 回合實際用的模型不是 MODEL 時中止。只送固定的合成提示詞與合成的 entry 內容。
set -uo pipefail
image="${1:?usage: probe-instructions.sh IMAGE WORK_DIR AUTH_JSON MODEL}"
work="$(realpath "${2:?}")"
auth="${3:?}"
model="${4:?}"
here="$(cd "$(dirname "$0")" && pwd)"
box="probe45-instr-$$"
pw="probe-$$"
fails=0
# check MESSAGE RC：RC 要在組訊息之前先存下來；訊息裡的 $(...) 會把 $? 重設成 0。
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }
cleanup() { docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

mkdir -p "$work/home/.local/share/opencode" "$work/ws" "$work/out"
cp "$auth" "$work/home/.local/share/opencode/auth.json" && chmod 600 "$work/home/.local/share/opencode/auth.json"
cp "$here/v2turn.py" "$work/v2turn.py"
db="$work/home/.local/share/opencode/opencode.db"
run_args=(--user "$(id -u):$(id -g)" -e HOME=/p/home -v "$work":/p -w /p/ws)

start() { # 起一個 server；兩次呼叫之間用同一個 HOME，等同本機按需模式下一回合的新 server
    docker run -d --rm --name "$box" "${run_args[@]}" -e OPENCODE_SERVER_PASSWORD="$pw" \
        -e "OPENCODE_CONFIG_CONTENT={\"model\":\"$model\"}" \
        --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
    for _ in $(seq 30); do
        docker exec "$box" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && return
        sleep 1
    done
}
stop() { docker kill "$box" > /dev/null 2>&1; for _ in $(seq 20); do docker inspect "$box" > /dev/null 2>&1 || return; sleep 0.5; done; }
# api METHOD PATH [BODY]：印出「狀態碼 回應內容」
api() {
    printf '%s' "${3:-}" | docker exec -i "$box" sh -c \
        "curl -s -o /tmp/r -w '%{http_code}' -u opencode:$pw -X $1 -H 'content-type: application/json' \
         $([[ -n "${3:-}" ]] && echo "--data-binary @-") 'http://127.0.0.1:4096$2'; printf ' '; cat /tmp/r"
}
new_session() { api POST /api/session "{\"title\":\"$1\"}" \
    | python3 -c 'import json,sys; print(json.loads(sys.stdin.read().split(" ", 1)[1])["data"]["id"])'; }
entries() { api GET "/api/experimental/session/$1/instructions/entries"; }
put() { api PUT "/api/experimental/session/$1/instructions/entries/$2" "{\"value\": \"$3\"}"; }
turn() { # turn NAME PROMPT [ARGS...]
    local name="$1" prompt="$2"; shift 2
    docker exec -e PW="$pw" "$box" python3 -I /p/v2turn.py "/p/out/$name" "$prompt" --max-secs 180 "$@" 2> /dev/null | tail -1
}
# 最後一則 assistant 訊息裡 type 為 text 的部分，以及它實際用的模型
reply() { python3 - "$work/out/$1.json" <<'EOF_REPLY'
import json, sys
msgs = (json.load(open(sys.argv[1])).get("messages") or {}).get("data") or []
for m in msgs:  # V2 由新到舊排列
    if m.get("type") == "assistant":
        text = "".join(c.get("text", "") for c in m.get("content", []) if c.get("type") == "text")
        model = m.get("model") or {}
        print(text.strip().replace("\n", " ")[:80] + f" [model={model.get('providerID')}/{model.get('id')}]")
        break
EOF_REPLY
}
model_guard() {
    [[ "$1" == *" [model=$model]" ]] && return
    echo "ABORT: turn used another model than $model: $1"; exit 2
}
# history NAME TEXT：該回合結束後 /message 裡含 TEXT 的位置。
# 標成「訊息類型(notice)」或「訊息類型.部分類型」；assistant 的 reasoning 會引用它看到的內容。
history() { python3 - "$work/out/$1.json" "$2" <<'EOF_HISTORY'
import json, sys
hits = []
for m in json.load(open(sys.argv[1]))["messages"]["data"]:
    parts = m.get("content") if isinstance(m.get("content"), list) else []
    if sys.argv[2] in json.dumps({k: v for k, v in m.items() if k != "content"}):
        hits.append(f"{m.get('type')}({(m.get('metadata') or {}).get('notice', '')})")
    hits += [f"{m.get('type')}.{c.get('type')}" for c in parts if sys.argv[2] in json.dumps(c)]
print(" ".join(hits) if hits else "(none)")
EOF_HISTORY
}
# where TEXT：opencode.db 裡含有 TEXT 的資料表，session_message 另外列出訊息類型（唯讀開啟）。
# 用系統的 python3：主機的 pyenv 可能沒有 sqlite3 模組；讀不到資料庫時結束碼非 0，檢查會失敗。
where() { /usr/bin/python3 -I - "$db" "$1" <<'EOF_WHERE'
import sqlite3, sys
con = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
hits = []
for (table,) in con.execute("select name from sqlite_master where type='table'").fetchall():
    cols = [r[1] for r in con.execute(f'pragma table_info("{table}")')]
    cond = " or ".join(f'cast("{c}" as text) like ?' for c in cols)
    rows = con.execute(f'select * from "{table}" where {cond}', [f"%{sys.argv[2]}%"] * len(cols)).fetchall()
    if table == "session_message":
        hits += [f"session_message:{r[cols.index('type')]}" for r in rows]
    elif rows:
        hits.append(f"{table}={len(rows)}")
print(" ".join(hits) if hits else "(none)")
EOF_WHERE
}
not_a_message() { [[ "$1" != *user* && "$1" != *system* && "$1" != *assistant.text* ]]; }

first="The session codename is QX-91."
second="The session codename is ZB-22."
# 寫入後不再改的 entry：重啟後才第一次問，回答只可能來自 entry 本身
color="The session color is teal-47."
ask="What is the session codename? Reply with only the codename."

echo "== 1. 寫入與讀取"
start
sid="$(new_session probe45-instr)"; echo "session $sid"
out="$(put "$sid" wukong.scope "$first")"; [[ "$out" == 204* ]]; rc=$?; check "PUT a new entry returns 204 ($out)" $rc
out="$(put "$sid" wukong.color "$color")"; [[ "$out" == 204* ]]; rc=$?; check "PUT a second key returns 204 ($out)" $rc
out="$(entries "$sid")"; echo "  $out"
[[ "$out" == 200* && "$out" == *"wukong.scope"* && "$out" == *"QX-91"* && "$out" == *"teal-47"* ]]; rc=$?
check "GET lists both entries with their values" $rc
out="$(put "$sid" Bad_Key "x")"; [[ "$out" == 400* ]]; rc=$?; check "a key outside [a-z0-9._-] is rejected with 400 (${out:0:3})" $rc

echo "== 2. 模型讀得到，內容不進對話紀錄"
turn instr-1 "$ask" --session "$sid"
r="$(reply instr-1)"; model_guard "$r"
[[ "$r" == *QX-91* ]]; rc=$?; check "the model answers from the entry ($r)" $rc
h="$(history instr-1 "$first")"; echo "  entry text in /message: $h"
not_a_message "$h"; rc=$?; check "the entry is not a message in /message (the model's reasoning may quote it)" $rc
grep -q 'session.instructions.updated' "$work/out/instr-1.ndjson"; check "the turn announces session.instructions.updated" $?
python3 - "$work/out/instr-1.ndjson" <<'EOF_EVENT'
import json, sys
for line in open(sys.argv[1]):
    e = json.loads(line)
    if e.get("type") == "session.instructions.updated":
        print("  session.instructions.updated delta keys:", sorted((e.get("data") or {}).get("delta", {})))
EOF_EVENT

echo "== 3. 覆寫"
out="$(put "$sid" wukong.scope "$second")"; [[ "$out" == 204* ]]; rc=$?; check "PUT on the same key replaces the entry ($out)" $rc
turn instr-2 "$ask" --session "$sid"
r="$(reply instr-2)"; model_guard "$r"
[[ "$r" == *ZB-22* && "$r" != *QX-91* ]]; rc=$?; check "the next turn sees the new value ($r)" $rc
h="$(history instr-2 "$second")"; echo "  new value in /message: $h"
[[ "$h" == *"system(instructions)"* ]]; rc=$?
check "changing an entry that was already announced adds a system notice to the history" $rc

echo "== 4. server 重啟（本機按需模式每回合起新 server）"
stop; start
out="$(entries "$sid")"; [[ "$out" == 200* && "$out" == *"ZB-22"* && "$out" == *"teal-47"* ]]; rc=$?
check "after restart both entries are still listed" $rc
turn instr-3 "What is the session color? Reply with only the color." --session "$sid"
r="$(reply instr-3)"; model_guard "$r"
[[ "$r" == *teal-47* ]]; rc=$?; check "after restart the model answers from an entry it was never asked about ($r)" $rc
h="$(history instr-3 "$color")"; echo "  unchanged entry in /message: $h"
not_a_message "$h"; rc=$?; check "an entry that never changes leaves no message in the history" $rc

echo "== 5. 每個 session 各自一份；刪除"
other="$(new_session probe45-other)"
out="$(entries "$other")"; [[ "$out" == '200 {"data":[]}' ]]; rc=$?; check "a new session has no entries ($out)" $rc
out="$(api DELETE "/api/experimental/session/$sid/instructions/entries/wukong.scope")"; [[ "$out" == 204* ]]; rc=$?
check "DELETE returns 204 ($out)" $rc
out="$(entries "$sid")"; [[ "$out" == 200* && "$out" != *wukong.scope* && "$out" == *wukong.color* ]]; rc=$?
check "after DELETE only the other key is left ($out)" $rc
stop

echo "== 6. opencode.db（唯讀）"
for text in "$first" "$second" "$color"; do
    w="$(where "$text")"; rc=$?; echo "  '$text' in: $w"
    [[ $rc == 0 && "$w" == *instruction_blob* ]]; rc=$?; check "the value is stored in instruction_blob" $rc
done
w="$(where "$color")"; rc=$?
[[ $rc == 0 && "$w" != *session_message:user* && "$w" != *session_message:system* && "$w" != *session_part* && "$w" != *session_inbox* ]]
rc=$?; check "the unchanged entry is in no user/system message, part or inbox row ($w)" $rc

echo "fails=$fails"
exit $((fails > 0))
