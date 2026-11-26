#!/usr/bin/env bash
# #45 Task 1.1：V2 2.3.2 從哪裡載入 agent 定義、怎麼指定 agent，三種執行模式都試。
# 用法：probe-agents.sh IMAGE WORK_DIR AUTH_JSON MODEL
#   IMAGE：內建 V2 的 Wukong 映像（例如 #44 建的 wukong-issue44:v2-local）
#   WORK_DIR：空目錄（隔離的 HOME 與工作區放這裡）
#   AUTH_JSON：只含要用的 provider 的 opencode auth.json（例如只有 nvidia）；用預設免費模型時給內容為 {} 的檔案
#   MODEL：provider/model，例如 nvidia/nvidia/nemotron-3-ultra-550b-a55b 或 opencode/big-pickle
# 回合實際用的模型不是 MODEL 時中止（2026-11-26 曾因模型設定沒生效而誤用免費模型）。
# 只送固定的合成提示詞。不需要模型的檢查（列出 agent）在 --network none 的容器裡做。
set -uo pipefail
image="${1:?usage: probe-agents.sh IMAGE WORK_DIR AUTH_JSON MODEL}"
work="$(realpath "${2:?}")"
auth="${3:?}"
model="${4:?}"
here="$(cd "$(dirname "$0")" && pwd)"
box="probe45-agents-$$"
pw="probe-$$"
fails=0
# check MESSAGE RC：RC 要在組訊息之前先存下來；訊息裡的 $(...) 會把 $? 重設成 0。
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }
cleanup() { docker rm -f "$box" > /dev/null 2>&1; }
trap cleanup EXIT

mkdir -p "$work/home/.config/opencode/agents" "$work/home/.local/share/opencode" \
    "$work/ws/.opencode/agents" "$work/ws/.opencode/agent" "$work/out"
cp "$auth" "$work/home/.local/share/opencode/auth.json" && chmod 600 "$work/home/.local/share/opencode/auth.json"
cp "$here/v2turn.py" "$work/v2turn.py"
agent() { # agent FILE MODE TOKEN
    printf -- '---\ndescription: probe agent %s\nmode: %s\n---\nEvery reply MUST begin with the exact token %s followed by a space.\n' \
        "$(basename "$1" .md)" "$2" "$3" > "$1"
}
agent "$work/ws/.opencode/agents/wkprimary.md" primary PRIMARY-OK      # 專案目錄 agents/
agent "$work/ws/.opencode/agents/wkfixer.md" subagent FIXER-OK         # 專案目錄 agents/，subagent
agent "$work/ws/.opencode/agent/wkold.md" subagent OLD-OK              # 專案目錄 agent/（單數）
agent "$work/home/.config/opencode/agents/wkgprimary.md" primary GLOBAL-OK  # 全域設定目錄
agent "$work/home/.config/opencode/agents/wkglobal.md" subagent GSUB-OK
run_args=(--user "$(id -u):$(id -g)" -e HOME=/p/home -v "$work":/p -w /p/ws)

echo "== 1. opencode debug agents / config（--network none）"
docker run --rm --network none "${run_args[@]}" --entrypoint sh "$image" -c \
    'timeout 60 opencode debug config; timeout 60 opencode debug agents' > "$work/out/debug.txt" 2> /dev/null
listed="$(python3 - "$work/out/debug.txt" <<'PY'
import json, sys
text = open(sys.argv[1]).read()
dec = json.JSONDecoder(); i = 0; docs = []
while i < len(text):
    if text[i] in "[{":
        obj, j = dec.raw_decode(text, i); docs.append(obj); i = j
    else:
        i += 1
print(" ".join(sorted(a["id"] + ":" + a["mode"] for a in docs[-1])))
print("config sources: " + ", ".join(s["path"] for s in docs[0]), file=sys.stderr)
PY
)"
echo "$listed"
for want in wkprimary:primary wkfixer:subagent wkold:subagent wkgprimary:primary wkglobal:subagent; do
    [[ " $listed " == *" $want "* ]]; check "debug agents lists $want" $?
done

echo "== 2. 常駐 server（cwd 是工作區，與 compose 的 opencode-server 相同）"
docker run -d --name "$box" "${run_args[@]}" -e OPENCODE_SERVER_PASSWORD="$pw" \
    -e "OPENCODE_CONFIG_CONTENT={\"model\":\"$model\"}" \
    --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
for _ in $(seq 30); do
    docker exec "$box" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && break
    sleep 1
done
ids() { docker exec "$box" sh -c "curl -s -u opencode:$pw 'http://127.0.0.1:4096/api/agent$1'" \
    | python3 -c 'import json,sys; print(" ".join(sorted(a["id"] for a in json.load(sys.stdin)["data"])))'; }
# 實測（2026-11-26）：server 剛就緒時的第一個清單請求只有內建 agent，幾秒後才含自訂定義；
# 第一個 prompt 則會等定義載入（見 probe-agents.md）。這裡記下第一次的結果，再輪詢到穩定。
first="$(ids '?location%5Bdirectory%5D=/p/ws')"
echo "first GET /api/agent?location[directory]=/p/ws: $first"
located=""
for _ in $(seq 20); do
    located="$(ids '?location%5Bdirectory%5D=/p/ws')"
    [[ " $located " == *" wkprimary "* ]] && break
    sleep 1
done
echo "settled GET /api/agent?location[directory]=/p/ws: $located"
[[ " $located " == *" wkprimary "* && " $located " == *" wkgprimary "* && " $located " == *" wkold "* ]]
check "/api/agent lists project (agents/ and agent/) and global agents once loaded" $?

turn() { # turn NAME PROMPT [ARGS...]
    local name="$1" prompt="$2"; shift 2
    docker exec -e PW="$pw" "$box" python3 -I /p/v2turn.py "/p/out/$name" "$prompt" --max-secs 180 "$@" 2> /dev/null | tail -1
}
# 最後一則 assistant 訊息裡 type 為 text 的部分（不取 reasoning），以及它實際用的模型
reply() { python3 - "$work/out/$1.json" <<'PY2'
import json, sys
msgs = (json.load(open(sys.argv[1])).get("messages") or {}).get("data") or []
for m in msgs:  # V2 由新到舊排列
    if m.get("type") == "assistant":
        text = "".join(c.get("text", "") for c in m.get("content", []) if c.get("type") == "text")
        model = m.get("model") or {}
        print(text.strip().replace("\n", " ")[:80] + f" [model={model.get('providerID')}/{model.get('id')}]")
        break
PY2
}
# 回合用的模型必須是 MODEL；不是時中止，不繼續送回合
model_guard() {
    [[ "$1" == *" [model=$model]" ]] && return
    echo "ABORT: turn used another model than $model: $1"; exit 2
}
ended() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["end"])' "$work/out/$1.json"; }
q="Say hello in five words."

turn srv-primary "$q" --agent wkprimary
r="$(reply srv-primary)"; model_guard "$r"; [[ "$r" == PRIMARY-OK* ]]; rc=$?; check "server: selecting the project primary agent is used ($r)" $rc
turn srv-global "$q" --agent wkgprimary
r="$(reply srv-global)"; model_guard "$r"; [[ "$r" == GLOBAL-OK* ]]; rc=$?; check "server: selecting the global primary agent is used ($r)" $rc
turn srv-default "$q"
r="$(reply srv-default)"; model_guard "$r"
grep -q '"agent": "build"' "$work/out/srv-default.ndjson" && [[ "$(ended srv-default)" == session.execution.succeeded && -n "$r" ]]
rc=$?; check "server: without selecting, the turn runs as build ($r)" $rc
turn srv-nosuch "$q" --agent nosuchagent
[[ "$(ended srv-nosuch)" == session.execution.failed ]] && grep -q 'Agent not found' "$work/out/srv-nosuch.ndjson"
rc=$?; check "server: an unknown agent fails the turn with 'Agent not found' (select still returns 204)" $rc
agent "$work/ws/.opencode/agents/wklate.md" primary LATE-OK
sleep 2
turn srv-late "$q" --agent wklate
r="$(reply srv-late)"; model_guard "$r"; [[ "$r" == LATE-OK* ]]; rc=$?; check "server: a definition added while the server runs is used without restart ($r)" $rc

echo "== 3. CLI backend（opencode run --standalone --agent，cwd 是工作區）"
cli() { docker exec -w /p/ws "$box" sh -c "timeout 180 opencode run --standalone --print-logs --log-level error --agent $1 '$q' 2> /dev/null"; }
out="$(cli wkprimary)"; [[ "$out" == PRIMARY-OK* ]]; rc=$?; check "cli: --agent project primary ($out)" $rc
out="$(cli wkgprimary)"; [[ "$out" == GLOBAL-OK* ]]; rc=$?; check "cli: --agent global primary ($out)" $rc
cli nosuchagent > /dev/null; rc=$?
[[ $rc != 0 ]]; check "cli: --agent unknown exits non-zero (rc=$rc)" $?

echo "== 4. 本機按需模式的情形：剛啟動的 server，第一個請求就是 prompt（--network none，不需要模型）"
# 按需模式每個回合起一個新的 serve。定義還沒載入時會回 Agent not found；
# 已載入時會走到 provider，因為沒有網路而進入重試。兩者用失敗訊息區分。
for i in 1 2 3; do
    fresh="probe45-fresh-$$-$i"
    docker run -d --rm --name "$fresh" --network none "${run_args[@]}" -e OPENCODE_SERVER_PASSWORD="$pw" \
        -e "OPENCODE_CONFIG_CONTENT={\"model\":\"$model\"}" \
        --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
    for _ in $(seq 50); do
        docker exec "$fresh" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && break
        sleep 0.2
    done
    docker exec -e PW="$pw" "$fresh" python3 -I /p/v2turn.py "/p/out/fresh$i" "$q" --agent wkprimary --max-secs 25 > /dev/null 2>&1
    docker kill "$fresh" > /dev/null 2>&1
    ! grep -q 'Agent not found' "$work/out/fresh$i.ndjson" && grep -q '"type": "session.step.started"' "$work/out/fresh$i.ndjson"
    rc=$?; check "fresh server $i: the first prompt resolves the project agent (no 'Agent not found', a step started)" $rc
done

echo "fails=$fails"
exit $((fails > 0))
