#!/usr/bin/env bash
# #45 Task 2.1 外迴圈：Wukong 寫出的 agent 定義（crates/wukong-runtime/src/agent_assets.rs），
# 在 V2 2.3.2 的三種模式下都列得到 wukong 與五個 subagent。全部在 --network none 下執行，不需要模型。
# 用法：probe-managed-agents.sh IMAGE WORK_DIR
#   WORK_DIR：空目錄。腳本先以 ignored 測試 write_definitions_for_probe 把定義寫到 WORK_DIR/ws。
set -uo pipefail
image="${1:?usage: probe-managed-agents.sh IMAGE WORK_DIR}"
work="$(realpath "${2:?}")"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
box="probe45-managed-$$"
pw="probe-$$"
fails=0
# check MESSAGE RC：RC 要在組訊息之前先存下來；訊息裡的 $(...) 會把 $? 重設成 0。
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }
cleanup() { docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

mkdir -p "$work/home/.local/share/opencode" "$work/ws" "$work/out"
echo '{}' > "$work/home/.local/share/opencode/auth.json"
cp "$here/v2turn.py" "$work/v2turn.py"
(cd "$repo" && WUKONG_AGENT_PROBE_DIR="$work/ws" cargo test -q -p wukong-runtime --lib \
    agent_assets::tests::write_definitions_for_probe -- --ignored > "$work/out/write.log" 2>&1)
check "the ignored writer test wrote the definitions" $?
echo "  written: $(ls "$work/ws/.opencode/agents" | tr '\n' ' ')"
want="wukong:primary explorer:subagent oracle:subagent librarian:subagent fixer:subagent designer:subagent"
run_args=(--network none --user "$(id -u):$(id -g)" -e HOME=/p/home -v "$work":/p -w /p/ws)

echo "== 1. opencode debug agents（CLI 的探索）"
docker run --rm "${run_args[@]}" --entrypoint sh "$image" -c 'timeout 60 opencode debug agents' \
    > "$work/out/debug.txt" 2> /dev/null
listed="$(python3 - "$work/out/debug.txt" <<'EOF_LIST'
import json, sys
text = open(sys.argv[1]).read()
docs = json.JSONDecoder().raw_decode(text, text.index("["))[0]
print(" ".join(sorted(a["id"] + ":" + a["mode"] for a in docs)))
EOF_LIST
)"
echo "  $listed"
for w in $want; do [[ " $listed " == *" $w "* ]]; check "debug agents lists $w" $?; done

echo "== 2. 常駐 server：/api/agent 與 wukong 的 prompt"
docker run -d --rm --name "$box" "${run_args[@]}" -e OPENCODE_SERVER_PASSWORD="$pw" \
    --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
for _ in $(seq 30); do
    docker exec "$box" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && break
    sleep 1
done
for _ in $(seq 20); do
    docker exec "$box" sh -c "curl -s -u opencode:$pw 'http://127.0.0.1:4096/api/agent?location%5Bdirectory%5D=/p/ws'" \
        > "$work/out/agents.json"
    grep -q '"wukong"' "$work/out/agents.json" && break
    sleep 1
done
python3 - "$work/out/agents.json" "$want" <<'EOF_API'
import json, sys
agents = {a["id"]: a for a in json.load(open(sys.argv[1]))["data"]}
bad = 0
for item in sys.argv[2].split():
    name, mode = item.split(":")
    a = agents.get(name)
    ok = bool(a) and a.get("mode") == mode and bool(a.get("description"))
    print(("PASS" if ok else "FAIL") + f" /api/agent lists {name} as {mode} with a description")
    bad += not ok
prompt = json.dumps(agents.get("wukong", {}), ensure_ascii=False)
for text in ("孫悟空", "[派工]", "派給 fixer", "[輸出要求]"):
    ok = text in prompt
    print(("PASS" if ok else "FAIL") + f" the wukong agent carries {text}")
    bad += not ok
ok = "由 Wukong 管理" not in prompt
print(("PASS" if ok else "FAIL") + " the frontmatter comment stays out of the agent")
bad += not ok
sys.exit(bad)
EOF_API
rc=$?; fails=$((fails + rc))
docker kill "$box" > /dev/null 2>&1

echo "== 3. 本機按需模式：剛啟動的 server，第一個請求就是指定 wukong 的 prompt"
for i in 1 2; do
    docker run -d --rm --name "$box" "${run_args[@]}" -e OPENCODE_SERVER_PASSWORD="$pw" \
        --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
    for _ in $(seq 50); do
        docker exec "$box" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && break
        sleep 0.2
    done
    docker exec -e PW="$pw" "$box" python3 -I /p/v2turn.py "/p/out/fresh$i" "Say hello." --agent wukong --max-secs 20 \
        > /dev/null 2>&1
    docker kill "$box" > /dev/null 2>&1
    for _ in $(seq 20); do docker inspect "$box" > /dev/null 2>&1 || break; sleep 0.5; done
    ! grep -q 'Agent not found' "$work/out/fresh$i.ndjson" && grep -q '"agent": "wukong"' "$work/out/fresh$i.ndjson"
    rc=$?; check "fresh server $i: the first prompt runs as wukong (no 'Agent not found')" $rc
done

echo "== 4. CLI：opencode run --agent（沒有網路，只看 agent 有沒有解析到）"
cli() { docker run --rm "${run_args[@]}" --entrypoint sh "$image" -c \
    "timeout 40 opencode run --standalone --print-logs --log-level error --agent $1 'Say hello.' 2>&1"; }
cli nosuchagent > "$work/out/cli-nosuch.txt"
grep -qi 'not found' "$work/out/cli-nosuch.txt"; check "control: an unknown agent reports not found" $?
cli wukong > "$work/out/cli-wukong.txt"
! grep -qi 'agent.*not found' "$work/out/cli-wukong.txt"; rc=$?
check "cli: --agent wukong resolves the agent ($(tr '\n' ' ' < "$work/out/cli-wukong.txt" | cut -c1-120))" $rc

echo "fails=$fails"
exit $((fails > 0))
