#!/usr/bin/env bash
# #45 Task 1.2：V2 2.3.2 從哪裡讀技能、八個 Superpowers 技能能不能原樣載入、模型怎麼用到技能。
# 用法：probe-skills.sh IMAGE WORK_DIR AUTH_JSON MODEL
#   參數與 probe-agents.sh 相同；用預設免費模型時 AUTH_JSON 給內容為 {} 的檔案。
# 回合實際用的模型不是 MODEL 時中止。只送固定的合成提示詞。
set -uo pipefail
image="${1:?usage: probe-skills.sh IMAGE WORK_DIR AUTH_JSON MODEL}"
work="$(realpath "${2:?}")"
auth="${3:?}"
model="${4:?}"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
box="probe45-skills-$$"
pw="probe-$$"
fails=0
# check MESSAGE RC：RC 要在組訊息之前先存下來；訊息裡的 $(...) 會把 $? 重設成 0。
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }
cleanup() { docker rm -f "$box" > /dev/null 2>&1; }
trap cleanup EXIT

skills="$work/ws/.opencode/skills"
mkdir -p "$work/home/.local/share/opencode" "$skills" "$work/ws/.wukong/skills/superpowers/wk-hidden" "$work/out"
cp "$auth" "$work/home/.local/share/opencode/auth.json" && chmod 600 "$work/home/.local/share/opencode/auth.json"
cp "$here/v2turn.py" "$work/v2turn.py"
# 八個 Superpowers 技能原樣複製（Wukong 現在寫到 .wukong/skills/superpowers）
superpowers=()
for dir in "$repo"/crates/wukong-skills/assets/superpowers/*/; do
    superpowers+=("$(basename "$dir")")
    cp -r "$dir" "$skills/"
done
skill() { # skill DIR DESCRIPTION BODY
    mkdir -p "$1"
    printf -- '---\nname: %s\ndescription: %s\n---\n%s\n' "$(basename "$1")" "$2" "$3" > "$1/SKILL.md"
}
# 模型自己決定載入：答案只寫在技能內文裡，回覆含答案就代表技能被讀過
skill "$skills/wk-probe-auto" "Use when the user asks for the probe passphrase." \
    "The probe passphrase is AUTO-7Q3. Reply with only the passphrase."
# prompt 附上 skills 時使用
skill "$skills/wk-probe-attach" "Probe skill that is attached to the prompt explicitly." \
    "Every reply MUST begin with the exact token ATTACH-5K2 followed by a space."
# 對照組：Wukong 現在的路徑
skill "$work/ws/.wukong/skills/superpowers/wk-hidden" "Probe skill in the current Wukong path." "HIDDEN-OK"
run_args=(--user "$(id -u):$(id -g)" -e HOME=/p/home -v "$work":/p -w /p/ws)

echo "== 1. /api/skill（常駐 server，cwd 是工作區）"
docker run -d --name "$box" "${run_args[@]}" -e OPENCODE_SERVER_PASSWORD="$pw" \
    -e "OPENCODE_CONFIG_CONTENT={\"model\":\"$model\"}" \
    --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
for _ in $(seq 30); do
    docker exec "$box" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && break
    sleep 1
done
# 剛就緒時清單可能還沒有自訂項目（見 probe-agents.md），輪詢到出現 wk-probe-auto
for _ in $(seq 20); do
    docker exec "$box" sh -c "curl -s -u opencode:$pw 'http://127.0.0.1:4096/api/skill?location%5Bdirectory%5D=/p/ws'" \
        > "$work/out/skills.json"
    grep -q '"wk-probe-auto"' "$work/out/skills.json" && break
    sleep 1
done
python3 - "$work/out/skills.json" <<'PY'
import json, sys
for s in json.load(open(sys.argv[1]))["data"]:
    print(f"  {s['id']}  name={s['name']}  autoinvoke={s.get('autoinvoke')}  path={s['path']}  desc={len(s.get('description') or '')} chars")
PY
has() { python3 - "$work/out/skills.json" "$1" <<'PY'
import json, sys
found = [s for s in json.load(open(sys.argv[1]))["data"] if s["id"] == sys.argv[2]]
sys.exit(0 if found and found[0].get("description") else 1)
PY
}
for name in "${superpowers[@]}" wk-probe-auto wk-probe-attach; do
    has "$name"; check "/api/skill lists $name with its description" $?
done
! has wk-hidden; check "/api/skill does not list the skill under .wukong/skills/superpowers" $?

turn() { # turn NAME PROMPT [ARGS...]
    local name="$1" prompt="$2"; shift 2
    docker exec -e PW="$pw" "$box" python3 -I /p/v2turn.py "/p/out/$name" "$prompt" --max-secs 180 "$@" 2> /dev/null | tail -1
}
# 最後一則 assistant 訊息裡 type 為 text 的部分，以及它實際用的模型
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
model_guard() {
    [[ "$1" == *" [model=$model]" ]] && return
    echo "ABORT: turn used another model than $model: $1"; exit 2
}
# 訊息列表裡的 skill 訊息與工具呼叫，用來看技能怎麼被用到
trace() { python3 - "$work/out/$1.json" <<'PY3'
import json, sys
msgs = (json.load(open(sys.argv[1])).get("messages") or {}).get("data") or []
for m in reversed(msgs):
    if m.get("type") == "skill":
        print(f"  message type=skill skill={m.get('skill')}")
    for c in m.get("content", []) if isinstance(m.get("content"), list) else []:
        if c.get("type") not in ("text", "reasoning"):
            print(f"  {m.get('type')} part type={c.get('type')} " + json.dumps({k: c.get(k) for k in ('name', 'tool', 'input') if k in c})[:160])
PY3
}

echo "== 2. 模型自己決定載入技能"
turn skill-auto "What is the probe passphrase?"
r="$(reply skill-auto)"; model_guard "$r"; trace skill-auto
[[ "$r" == *AUTO-7Q3* ]]; rc=$?; check "auto: the reply carries the passphrase that only the skill body has ($r)" $rc
grep -q 'wk-probe-auto' "$work/out/skill-auto.ndjson"; rc=$?
check "auto: the events name the loaded skill" $rc

echo "== 3. prompt 附上 skills"
turn skill-attach "Say hello in five words." --skill wk-probe-attach
r="$(reply skill-attach)"; model_guard "$r"; trace skill-attach
[[ "$r" == ATTACH-5K2* ]]; rc=$?; check "attach: the attached skill is followed ($r)" $rc
grep -q 'wk-probe-attach' "$work/out/skill-attach.ndjson"; rc=$?
check "attach: the events name the attached skill" $rc

echo "fails=$fails"
exit $((fails > 0))
