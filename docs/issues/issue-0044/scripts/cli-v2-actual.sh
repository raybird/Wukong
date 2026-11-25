#!/usr/bin/env bash
# Task 2.6（SCN-010）外迴圈：wukong CLI backend 接真實 V2 與真實模型，兩個回合同一個 scope。
# 用法：cli-v2-actual.sh IMAGE WORK_DIR MODEL
#   IMAGE：內建 V2 的 Wukong 映像（例如本機建的 wukong-issue44:v2-local）
#   WORK_DIR：隔離目錄，事先放好 home/.local/share/opencode/auth.json（只含要用的 provider）
#   MODEL：provider/model，例如 nvidia/nvidia/nemotron-3.5-lightning-30b-a3b
# 在 repo 根目錄執行，先 `cargo build -p wukong-cli`。只送固定的合成提示詞。
#
# `--agent-cmd` 帶額外旗標才走 CLI backend（只有 `opencode run` 時走按需本機 backend）。
# 驗三件事：Wukong 傳給 V2 的旗標被接受並拿到回覆與 session；第二回合以 `-s` 接續第一回合的
# session；每個回合結束後容器裡沒有殘留的 opencode 程序（--standalone 起的私有 serve 跟著結束）。
# 模型由容器的 OPENCODE_CONFIG_CONTENT 指定，argv 與正式部署相同，不另加 --model。
set -uo pipefail
image="${1:?usage: cli-v2-actual.sh IMAGE WORK_DIR MODEL}"
work="$(realpath "${2:?}")"
model="${3:?}"
repo="$(pwd)"
box="cli26-$$"
mkdir -p "$work/workspace" "$work/out" "$work/bin"
cleanup() { docker rm -f "$box" > /dev/null 2>&1; }
trap cleanup EXIT
fails=0
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }

docker run -d --name "$box" --user "$(id -u):$(id -g)" -e HOME=/p/home \
    -e OPENCODE_CONFIG_CONTENT="{\"model\":\"$model\"}" \
    -v "$work":/p -w /p/workspace --entrypoint sleep "$image" infinity > /dev/null

# wrapper：把 Wukong 傳來的 argv 原樣交給容器裡的 V2；記下 argv（最後一個是提示詞，不記）
cat > "$work/bin/opencode" <<SH
#!/usr/bin/env bash
if (( \$# > 1 )); then printf '%s\n' "\${*:1:\$#-1}"; else printf '%s\n' "\$*"; fi >> "$work/out/argv.log"
exec docker exec -w /p/workspace "$box" opencode "\$@"
SH
chmod +x "$work/bin/opencode"

# 映像沒有 ps：從 /proc 找 opencode 程序，跳過這段 sh 自己（它的命令列含 opencode 字樣）
procs() {
    docker exec "$box" sh -c 'for d in /proc/[0-9]*; do [ "${d#/proc/}" = "$$" ] && continue; c=$(tr "\0" " " < $d/cmdline 2>/dev/null); case "$c" in *opencode*) echo "${d#/proc/} $c";; esac; done' \
        | tee -a "$work/out/procs.log" | grep -c .
}
turn() { # turn N PROMPT
    WUKONG_MEMORY_DB="sqlite://$work/memory.db" WUKONG_WORKSPACE="$work/workspace" HOME="$work" \
        WUKONG_AGENT_TIMEOUT_SECS=180 timeout 300 "$repo/target/debug/wukong" \
        --agent-cmd "$work/bin/opencode run --dangerously-skip-permissions" --scope project:issue44-cli --no-stream --no-thinking \
        "$2" > "$work/out/turn$1.out" 2> "$work/out/turn$1.err"
}

: > "$work/out/argv.log"
turn 1 "Reply with exactly the word OK."; rc1=$?
grep -q OK "$work/out/turn1.out"
check "turn 1 gets a reply with the V2 flags (rc=$rc1)" $(( rc1 || $? ))
sleep 2; left1="$(procs)"
check "no opencode process left after turn 1 ($left1)" $(( left1 != 0 ))
lines1="$(wc -l < "$work/out/argv.log")"

grep -q -- "--standalone --print-logs --log-level error" "$work/out/argv.log"
check "argv carries --standalone --print-logs --log-level error" $?
session="$(sqlite3 "file:$work/memory.db?mode=ro" "select session_id from agent_session_state where scope='project:issue44-cli'")"
[[ "$session" == ses_* ]]
check "turn 1 stored a V2 session for the scope (${session:0:12}…)" $?

turn 2 "Reply with exactly the word AGAIN."; rc2=$?
grep -q AGAIN "$work/out/turn2.out"
check "turn 2 gets a reply (rc=$rc2)" $(( rc2 || $? ))
tail -n +"$((lines1 + 1))" "$work/out/argv.log" | grep -q -- "-s $session"
check "turn 2 continues the session with -s" $?
sleep 2; left2="$(procs)"
check "no opencode process left after turn 2 ($left2)" $(( left2 != 0 ))

echo "argv per call (prompt omitted):"; sed 's/^/  /' "$work/out/argv.log"
echo "$fails failed"
exit $(( fails > 0 ))
