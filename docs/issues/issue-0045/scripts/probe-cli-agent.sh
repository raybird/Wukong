#!/usr/bin/env bash
# #45 Task 3.4（SCN-014）外迴圈：wukong CLI backend 接真實 V2 2.3.2 與真實模型。
# 用法：probe-cli-agent.sh IMAGE WORK_DIR AUTH_JSON MODEL
#   IMAGE：內建 V2 的 Wukong 映像（例如本機建的 wukong-issue44:v2-local）
#   AUTH_JSON：只含要用的 provider 的 opencode auth.json
#   MODEL：provider/model，例如 nvidia/nvidia/nemotron-3-ultra-550b-a55b
# 在 repo 根目錄執行。只送固定的合成提示詞。HOME、WUKONG_WORKSPACE、記憶庫、設定檔都在 WORK_DIR。
#
# 兩個情況各一回合：
#   A. 自訂命令沒有指定 agent：Wukong 加上 --agent wukong，V2 以 agent wukong 執行（定義由 wukong 啟動時寫入工作區）。
#   B. 自訂命令自己寫了 --agent build：Wukong 不再加 --agent，V2 以 build 執行。
# `--agent-cmd` 帶額外旗標才走 CLI backend（只有 `opencode run` 時走按需本機 backend）。
# 模型由容器的 OPENCODE_CONFIG_CONTENT 指定，與 #44 的 cli-v2-actual.sh 相同。
set -uo pipefail
image="${1:?usage: probe-cli-agent.sh IMAGE WORK_DIR AUTH_JSON MODEL}"
work="$(realpath "${2:?}")"
auth="${3:?}" model="${4:?}"
repo="$(pwd)"
box="probe45-cli-$$"
fails=0
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }
cleanup() { docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

mkdir -p "$work/home/.local/share/opencode" "$work/workspace" "$work/out" "$work/bin"
install -m 600 "$auth" "$work/home/.local/share/opencode/auth.json"
echo '{}' > "$work/settings.json"
cargo build -q -p wukong-cli || { echo "FAIL cargo build"; exit 1; }

docker run -d --rm --name "$box" --user "$(id -u):$(id -g)" -e HOME=/p/home \
    -e OPENCODE_CONFIG_CONTENT="{\"model\":\"$model\"}" \
    -v "$work":/p -w /p/workspace --entrypoint sleep "$image" infinity > /dev/null

# wrapper：把 Wukong 傳來的 argv 原樣交給容器裡的 V2；記下 argv（最後一個是提示詞，不記）
cat > "$work/bin/opencode" << SH
#!/usr/bin/env bash
if (( \$# > 1 )); then printf '%s\n' "\${*:1:\$#-1}"; else printf '%s\n' "\$*"; fi >> "$work/out/argv.log"
exec docker exec -w /p/workspace "$box" opencode "\$@"
SH
chmod +x "$work/bin/opencode"

turn() { # turn NAME SCOPE AGENT_CMD
    env -i PATH="$PATH" HOME="$work" WUKONG_WORKSPACE="$work/workspace" WUKONG_SETTINGS_FILE="$work/settings.json" \
        WUKONG_MEMORY_DB="sqlite://$work/memory.db" WUKONG_AGENT_TIMEOUT_SECS=180 \
        timeout 300 "$repo/target/debug/wukong" --agent-cmd "$3" --scope "$2" --no-stream --no-thinking \
        "Reply with exactly the word OK." > "$work/out/$1.out" 2> "$work/out/$1.err" < /dev/null
}
# 某個 scope 的 session 裡 assistant 訊息的 agent 與模型（opencode.db 唯讀）
agents_of() {
    local session
    session="$(sqlite3 "file:$work/memory.db?mode=ro" "select session_id from agent_session_state where scope='$1'")"
    /usr/bin/python3 - "$work/home/.local/share/opencode/opencode.db" "$session" << 'EOF'
import json, sqlite3, sys
c = sqlite3.connect(f"file:{sys.argv[1]}?mode=ro", uri=True)
seen = set()
for (data,) in c.execute("select data from session_message where session_id = ? and type = 'assistant'", (sys.argv[2],)):
    m = json.loads(data)
    model = m.get("model") or {}
    seen.add(f"{m.get('agent')} {model.get('providerID')}/{model.get('id')}")
print(",".join(sorted(seen)) or "none")
EOF
}

: > "$work/out/argv.log"
turn a project:probe45-cli-a "$work/bin/opencode run --dangerously-skip-permissions"; rc=$?
grep -q OK "$work/out/a.out"
check "A: the turn gets a reply (rc=$rc)" $(( rc || $? ))
grep -q -- "--agent wukong" "$work/out/argv.log"
check "A: argv carries --agent wukong" $?
got="$(agents_of project:probe45-cli-a)"
[[ "$got" == "wukong $model" ]]
check "A: V2 ran agent wukong on $model ($got)" $?
lines="$(wc -l < "$work/out/argv.log")"

turn b project:probe45-cli-b "$work/bin/opencode run --dangerously-skip-permissions --agent build"; rc=$?
grep -q OK "$work/out/b.out"
check "B: the turn gets a reply (rc=$rc)" $(( rc || $? ))
! tail -n +"$((lines + 1))" "$work/out/argv.log" | grep -q -- "--agent wukong"
check "B: the user's --agent build is not overridden" $?
got="$(agents_of project:probe45-cli-b)"
[[ "$got" == "build $model" ]]
check "B: V2 ran agent build on $model ($got)" $?

echo "argv per call (prompt omitted):"; sed 's/^/  /' "$work/out/argv.log"
echo "fails=$fails"
exit $(( fails > 0 ))
