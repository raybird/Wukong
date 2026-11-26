#!/usr/bin/env bash
# #45 Task 2.2 外迴圈：Wukong 寫出的技能（crates/wukong-runtime/src/skill_assets.rs）出現在 V2 2.3.2 的
# /api/skill，而且模型在符合技能的任務裡會用到它。
# 用法：probe-managed-skills.sh IMAGE WORK_DIR MODEL
#   WORK_DIR：空目錄。腳本先以 ignored 測試 write_definitions_for_probe 把定義與技能寫到 WORK_DIR/ws。
#   MODEL：以 session 選取的模型（例如 opencode/big-pickle）。沒有憑證，只能用免費模型。
# 只送固定的合成提示詞。
set -uo pipefail
image="${1:?usage: probe-managed-skills.sh IMAGE WORK_DIR MODEL}"
work="$(realpath "${2:?}")"
model="${3:?}"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
box="probe45-mskills-$$"
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
check "the ignored writer test wrote the definitions and skills" $?
echo "  written: $(ls "$work/ws/.opencode/skills" | tr '\n' ' ')"
run_args=(--user "$(id -u):$(id -g)" -e HOME=/p/home -v "$work":/p -w /p/ws)

docker run -d --rm --name "$box" "${run_args[@]}" -e OPENCODE_SERVER_PASSWORD="$pw" \
    --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
for _ in $(seq 30); do
    docker exec "$box" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && break
    sleep 1
done

echo "== 1. /api/skill"
for _ in $(seq 20); do
    docker exec "$box" sh -c "curl -s -u opencode:$pw 'http://127.0.0.1:4096/api/skill?location%5Bdirectory%5D=/p/ws'" \
        > "$work/out/skills.json"
    grep -q '"test-driven-development"' "$work/out/skills.json" && break
    sleep 1
done
( cd "$repo" && ls crates/wukong-skills/assets/superpowers ) | grep -v '\.md$' > "$work/out/expected.txt"
python3 - "$work/out/skills.json" "$work/out/expected.txt" <<'EOF_SKILLS'
import json, sys
skills = json.load(open(sys.argv[1]))["data"]
listed = {s["id"]: s for s in skills}
bad = 0
for name in open(sys.argv[2]).read().split():
    s = listed.get(name)
    ok = bool(s) and s["path"].endswith(f"/.opencode/skills/{name}/SKILL.md") and bool(s.get("description"))
    print(("PASS" if ok else "FAIL") + f" /api/skill lists {name} from .opencode/skills")
    bad += not ok
custom = sorted(i for i, s in listed.items() if not s["path"].startswith("skill://"))
ok = custom == sorted(open(sys.argv[2]).read().split())
print(("PASS" if ok else "FAIL") + f" only the embedded skills appear from the workspace: {custom}")
bad += not ok
sys.exit(bad)
EOF_SKILLS
rc=$?; fails=$((fails + rc))

echo "== 2. 符合技能的任務：模型用 skill 工具載入技能（$model）"
docker exec -e PW="$pw" "$box" python3 -I /p/v2turn.py /p/out/tdd \
    "請用測試先行的方式，在工作區寫一個 Python 函式 add(a, b) 回傳兩數之和，以及它的 pytest 測試。" \
    --agent wukong --model "$model" --wait-jobs --max-secs 500 > /dev/null 2>&1
python3 - "$work/out/tdd.json" "$work/out/tdd.ndjson" "$model" <<'EOF_TDD'
import json, sys
res = json.load(open(sys.argv[1])); sid = res["session"]
names, used = {}, []
for line in open(sys.argv[2]):
    e = json.loads(line); d = e.get("data") or {}
    if e.get("type") == "session.tool.input.started":
        names[d["id"]] = d.get("name")
    if e.get("type") == "session.tool.called" and names.get(d.get("id")) == "skill":
        who = "primary" if d.get("sessionID") == sid else "subagent"
        used.append(f"{who}:{(d.get('input') or {}).get('id')}")
def models(msgs):
    return {f"{(m.get('model') or {}).get('providerID')}/{(m.get('model') or {}).get('id')}"
            for m in (msgs or {}).get("data", []) if m.get("type") == "assistant"}
seen = models(res.get("messages"))
for c in res.get("children", {}).values():
    seen |= models(c.get("messages"))
kids = sorted({m.get("agent") for c in res.get("children", {}).values()
               for m in (c.get("messages") or {}).get("data", []) if m.get("type") == "assistant"})
print(f"  end={res['end']} executions={len(res.get('executions') or [])} subagents={kids} models={sorted(seen)}")
print(f"  skill tool calls: {used}")
bad = 0
ok = seen == {sys.argv[3]}
print(("PASS" if ok else "FAIL") + f" every assistant message ran on {sys.argv[3]}")
bad += not ok
ok = any(u.endswith(":test-driven-development") for u in used)
print(("PASS" if ok else "FAIL") + " the test-driven-development skill was loaded through the skill tool")
bad += not ok
# prompt 不含技能規範指引由 run_turn 的單元測試 run_turn_leaves_skills_to_v2 確認；這裡的 prompt 是腳本自己送的
sys.exit(bad)
EOF_TDD
rc=$?; fails=$((fails + rc))
# 隱藏目錄（.opencode、模型可能建立的 .venv）不列
echo "  workspace files: $(cd "$work/ws" && find . -mindepth 1 -name '.*' -prune -o -name '*.py' -print | sort | tr '\n' ' ')"

echo "fails=$fails"
exit $((fails > 0))
