#!/usr/bin/env bash
# #45 Task 1.4：V2 2.3.2 的 subagent 用哪個模型、事件裡怎麼辨認 subagent、回合怎樣才算結束，
# 以及實作題會不會派給 fixer。
# 用法：probe-subagent.sh IMAGE WORK_DIR AUTH_JSON CONFIG_MODEL SELECT_MODEL SAMPLE_MODEL [RUNS]
#   CONFIG_MODEL：寫進 OPENCODE_CONFIG_CONTENT 的 model（例如 opencode/glm-5.3-flash-free）
#   SELECT_MODEL：以 POST /api/session/{id}/model 選取的 model（例如 opencode/deepseek-v4.1-flash-free）
#     兩者要彼此不同，也不同於 V2 的預設模型，才分得出各回合實際沿用的是哪一個。
#   SAMPLE_MODEL：派工抽樣用的 model，以選取的方式指定（例如 opencode/big-pickle）
#   RUNS：派工抽樣的次數（預設 3）
# 只送固定的合成提示詞。
set -uo pipefail
image="${1:?usage: probe-subagent.sh IMAGE WORK_DIR AUTH_JSON CONFIG_MODEL SELECT_MODEL SAMPLE_MODEL [RUNS]}"
work="$(realpath "${2:?}")"
auth="${3:?}"
config_model="${4:?}"
select_model="${5:?}"
sample_model="${6:?}"
runs="${7:-3}"
here="$(cd "$(dirname "$0")" && pwd)"
box="probe45-sub-$$"
pw="probe-$$"
fails=0
# check MESSAGE RC：RC 要在組訊息之前先存下來；訊息裡的 $(...) 會把 $? 重設成 0。
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }
cleanup() { docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

agents="$work/ws/.opencode/agents"
mkdir -p "$work/home/.local/share/opencode" "$agents" "$work/out"
cp "$auth" "$work/home/.local/share/opencode/auth.json" && chmod 600 "$work/home/.local/share/opencode/auth.json"
cp "$here/v2turn.py" "$work/v2turn.py"
# Phase 2 要寫的定義的雛形：primary agent wukong 與五個 subagent，描述沿用 role.rs 的角色卡
cat > "$agents/wukong.md" <<'EOF_WUKONG'
---
description: 悟空，協調者，決定自己回答或派工給 subagent
mode: primary
---
你是悟空，負責協調。依任務性質決定自己回答，或用 subagent 工具派工：
- 建立或修改工作區裡的檔案、寫程式、補測試：派給 fixer
- 掃描專案結構、追蹤依賴：派給 explorer
- 架構取捨、重構方案：派給 oracle
- 撰寫文件、註解、翻譯：派給 librarian
- 介面結構、互動與視覺：派給 designer
簡單的問題直接回答，不要派工。
EOF_WUKONG
sub() { # sub NAME DESCRIPTION
    printf -- '---\ndescription: %s\nmode: subagent\n---\n%s\n完成後回報的第一行寫 ROLE-DONE:%s。\n' "$2" "$2" "$1" > "$agents/$1.md"
}
sub explorer "你是 Explorer，結構洞察專家。負責快速掃描專案結構、追蹤依賴關係，揭開陌生程式碼的面貌。"
sub oracle "你是 Oracle，架構專家。擅長重構、決策分析與技術取捨；當架構混亂或 Bug 難解時提供可執行方案。"
sub librarian "你是 Librarian，文件專家。負責撰寫文件、API 註解與翻譯，條理分明。"
sub fixer "你是 Fixer，實作專家。負責程式碼修正、單元測試補全、語法修正，以最高效率交付可運作的程式。"
sub designer "你是 Designer，設計專家。負責 UI/UX 介面結構、互動與視覺一致性。"
run_args=(--user "$(id -u):$(id -g)" -e HOME=/p/home -v "$work":/p -w /p/ws)

docker run -d --rm --name "$box" "${run_args[@]}" -e OPENCODE_SERVER_PASSWORD="$pw" \
    -e "OPENCODE_CONFIG_CONTENT={\"model\":\"$config_model\"}" \
    --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
for _ in $(seq 30); do
    docker exec "$box" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && break
    sleep 1
done
# 等自訂定義載入（見 probe-agents.md 的啟動競態）
for _ in $(seq 20); do
    docker exec "$box" sh -c "curl -s -u opencode:$pw 'http://127.0.0.1:4096/api/agent?location%5Bdirectory%5D=/p/ws'" \
        | grep -q '"wukong"' && break
    sleep 1
done
default="$(docker exec "$box" sh -c "curl -s -u opencode:$pw 'http://127.0.0.1:4096/api/model/default?location%5Bdirectory%5D=/p/ws'" \
    | python3 -c 'import json,sys; d=json.load(sys.stdin)["data"]; print(d["providerID"] + "/" + d["id"])')"
echo "config model: $config_model ; selected: $select_model ; sample: $sample_model ; GET /api/model/default: $default"

turn() { # turn NAME PROMPT [ARGS...]
    local name="$1" prompt="$2"; shift 2
    docker exec -e PW="$pw" "$box" python3 -I /p/v2turn.py "/p/out/$name" "$prompt" --max-secs 400 "$@" \
        2> /dev/null | tail -1 > /dev/null
}
# info NAME KEY：從回合結果取值
info() { python3 - "$work/out/$1.json" "$2" <<'EOF_INFO'
import json, sys
res = json.load(open(sys.argv[1]))
def model(m):
    return f"{(m.get('model') or {}).get('providerID')}/{(m.get('model') or {}).get('id')}"
def assistants(msgs):
    return [m for m in (msgs or {}).get("data", []) if m.get("type") == "assistant"]
def text(m):
    return "".join(c.get("text", "") for c in m.get("content", []) if c.get("type") == "text").strip()
kids = res.get("children", {})
prim = assistants(res.get("messages"))
vals = {
    "primary_models": " ".join(sorted({model(m) for m in prim})),
    "primary_agents": " ".join(sorted({m.get("agent") or "" for m in prim})),
    "child_agents": " ".join(sorted({m.get("agent") or "" for c in kids.values() for m in assistants(c.get("messages"))})),
    "child_models": " ".join(sorted({model(m) for c in kids.values() for m in assistants(c.get("messages"))})),
    "executions": str(len(res.get("executions") or [])),
    "final": next((text(m) for m in prim if text(m)), "").replace("\n", " ")[:120],
}
print(vals[sys.argv[2]])
EOF_INFO
}
# show NAME：primary 的工具呼叫、每次執行結束時的背景工作、子 session 的 agent 與模型
show() { python3 - "$work/out/$1.json" "$work/out/$1.ndjson" <<'EOF_SHOW'
import json, sys
res = json.load(open(sys.argv[1])); sid = res["session"]
names = {}
for line in open(sys.argv[2]):
    e = json.loads(line); d = e.get("data") or {}
    if e.get("type") == "session.tool.input.started":
        names[d["id"]] = d.get("name")
    if e.get("type") == "session.tool.called" and d.get("sessionID") == sid:
        print(f"  primary tool {names.get(d['id'])} {json.dumps(d.get('input'), ensure_ascii=False)[:160]}")
for x in res.get("executions") or []:
    print(f"  primary {x['end']} at {x['_t']}s, background jobs then: {x['jobs']}")
for cid, c in res.get("children", {}).items():
    ms = [m for m in (c.get("messages") or {}).get("data", []) if m.get("type") == "assistant"]
    tags = sorted({"{}@{}/{}".format(m.get("agent"), (m.get("model") or {}).get("providerID"),
                                     (m.get("model") or {}).get("id")) for m in ms})
    print(f"  child {cid}: " + ", ".join(tags))
EOF_SHOW
}
file_ok() { [[ "$(cat "$work/ws/$1.txt" 2> /dev/null)" == "hello from probe" ]]; }

echo "== 1. 只靠設定檔指定模型時，回合用哪個模型"
turn cfg-build "Say hello in five words." --agent build
r="$(info cfg-build primary_models)"; echo "  build agent ran on: $r"
turn cfg-wukong "Say hello in five words." --agent wukong
r2="$(info cfg-wukong primary_models)"; echo "  wukong agent ran on: $r2"
[[ "$r" == "$config_model" && "$r2" == "$config_model" ]]; rc=$?
check "a session created after the server settled uses the config model for build and wukong ($r / $r2)" $rc

echo "== 2. 選取模型後明確派給 fixer：subagent 的模型、背景執行與回合結束"
turn dispatch "請用 subagent 工具派給 fixer 處理：在工作區建立 dispatch.txt，內容只有一行：hello from probe" \
    --agent wukong --model "$select_model" --wait-jobs
show dispatch
p="$(info dispatch primary_models)"; c="$(info dispatch child_models)"; a="$(info dispatch child_agents)"
[[ "$p" == "$select_model" ]]; rc=$?; check "the primary agent runs on the selected model ($p)" $rc
[[ "$a" == fixer ]]; rc=$?; check "the child session runs as fixer ($a)" $rc
echo "  child model: $c (selected $select_model, config $config_model, default $default)"
file_ok dispatch; check "the file the subagent was asked for exists" $?
n="$(info dispatch executions)"; [[ "$n" -ge 2 ]]; rc=$?
check "the primary runs again after the subagent reports ($n executions)" $rc
echo "  final reply: $(info dispatch final)"

echo "== 3. 派工抽樣：實作題 $runs 次（$sample_model，不指明要派工）"
dispatched=0
for i in $(seq "$runs"); do
    turn "sample-$i" "在工作區建立 sample-$i.txt，內容只有一行：hello from probe" \
        --agent wukong --model "$sample_model" --wait-jobs
    a="$(info "sample-$i" child_agents)"; [[ " $a " == *" fixer "* ]] && dispatched=$((dispatched + 1))
    echo "  sample-$i: subagents=[${a}] primary=$(info "sample-$i" primary_models) child=$(info "sample-$i" child_models)" \
        "executions=$(info "sample-$i" executions) file_ok=$(file_ok "sample-$i" && echo yes || echo no)"
done
echo "  dispatched to fixer: $dispatched/$runs"

echo "== 4. CLI（opencode run --agent wukong）會不會等 subagent 做完"
docker exec -w /p/ws "$box" sh -c "timeout 300 opencode run --standalone --agent wukong --model $select_model \
    '請用 subagent 工具派給 fixer 處理：在工作區建立 cli.txt，內容只有一行：hello from probe' 2> /dev/null" > "$work/out/cli.txt"
rc=$?
echo "  rc=$rc output: $(tr '\n' ' ' < "$work/out/cli.txt" | cut -c1-200)"
file_ok cli; rc=$?; check "after opencode run exits, the file the subagent was asked for exists" $rc

echo "fails=$fails"
exit $((fails > 0))
