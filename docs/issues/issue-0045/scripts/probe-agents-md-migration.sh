#!/usr/bin/env bash
# #45 Task 2.3 外迴圈（SCN-009）：三種工作區（全新部署、AGENTS.md 與舊範本 ac60ebe 相同、改過的）
# 由真實的 wukong 入口啟動一次，檢查 AGENTS.md 的遷移與 log；再在 V2 2.3.2 用 wukong agent 問「你是誰」。
# 用法：probe-agents-md-migration.sh IMAGE WORK_DIR MODEL
#   WORK_DIR：空目錄。MODEL：以 session 選取的模型（例如 opencode/big-pickle）。
# wukong 入口在本機執行，HOME、WUKONG_WORKSPACE、設定檔都指到 WORK_DIR；agent server 指到關閉的
# port，回合必定失敗，只看啟動時的遷移。只送固定的合成提示詞。
set -uo pipefail
image="${1:?usage: probe-agents-md-migration.sh IMAGE WORK_DIR MODEL}"
work="$(realpath "${2:?}")"
model="${3:?}"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(cd "$here/../../../.." && pwd)"
retired="$repo/crates/wukong-runtime/assets/retired-agents-md/ac60ebe.md"
box="probe45-agentsmd-$$"
pw="probe-$$"
fails=0
# check MESSAGE RC：RC 要在組訊息之前先存下來；訊息裡的 $(...) 會把 $? 重設成 0。
check() { if [[ "$2" == 0 ]]; then echo "PASS $1"; else echo "FAIL $1"; fails=$((fails + 1)); fi; }
cleanup() { docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

(cd "$repo" && cargo build -q -p wukong-cli) || { echo "FAIL cargo build"; exit 1; }
wukong="$repo/target/debug/wukong"
custom_rule=$'\n## 我自己的規則\n- 回答一律附上出處\n'

for case in fresh retired custom; do
    echo "== $case"
    d="$work/$case"
    mkdir -p "$d/home/.local/share/opencode" "$d/ws" "$d/out"
    echo '{}' > "$d/home/.local/share/opencode/auth.json"
    cp "$here/v2turn.py" "$d/v2turn.py"
    # entrypoint 與 installer 在工作區缺檔時複製範本；這裡照做
    cp "$repo/workspace/SOUL.md" "$d/ws/SOUL.md"
    case "$case" in
        fresh) cp "$repo/workspace/AGENTS.md" "$d/ws/AGENTS.md" ;;
        retired) cp "$retired" "$d/ws/AGENTS.md" ;;
        custom) { cat "$retired"; printf '%s' "$custom_rule"; } > "$d/ws/AGENTS.md" ;;
    esac
    cp "$d/ws/AGENTS.md" "$d/out/agents-before.md"

    env -i PATH="$PATH" HOME="$d/home" WUKONG_WORKSPACE="$d/ws" \
        WUKONG_SETTINGS_FILE="$d/home/settings.json" \
        WUKONG_AGENT_SERVER_URL=http://127.0.0.1:9 WUKONG_AGENT_SERVER_PASSWORD=x \
        timeout 60 "$wukong" "你是誰" > "$d/out/entry.stdout" 2> "$d/out/entry.log" < /dev/null
    grep '\[wukong\]' "$d/out/entry.log" | sed 's/^/  log: /'
    [[ -f "$d/ws/.opencode/agents/wukong.md" ]]; check "$case: the entry point wrote the wukong agent definition" $?
    case "$case" in
        fresh)
            cmp -s "$d/ws/AGENTS.md" "$repo/workspace/AGENTS.md" && ! grep -q 'AGENTS.md' "$d/out/entry.log" \
                && ! ls "$d/ws" | grep -q '\.bak-'
            check "fresh: AGENTS.md stays the new template, no backup and no log line" $? ;;
        retired)
            cmp -s "$d/ws/AGENTS.md" "$repo/workspace/AGENTS.md"
            check "retired: AGENTS.md was replaced by the new template" $?
            cmp -s "$d/ws/AGENTS.md.bak-ac60ebe" "$retired"
            check "retired: the old file was backed up to AGENTS.md.bak-ac60ebe" $?
            grep -q '與舊範本 ac60ebe 相同，已備份到' "$d/out/entry.log"
            check "retired: the log names the backup" $? ;;
        custom)
            cmp -s "$d/ws/AGENTS.md" "$d/out/agents-before.md" && ! ls "$d/ws" | grep -q '\.bak-'
            check "custom: AGENTS.md is left alone, no backup" $?
            grep -q '有過時的段落，請自行更新：「技能動態加載機制」' "$d/out/entry.log"
            check "custom: the log points out the outdated skill-loading section" $? ;;
    esac

    docker run -d --rm --name "$box" --user "$(id -u):$(id -g)" -e HOME=/p/home -v "$d":/p -w /p/ws \
        -e OPENCODE_SERVER_PASSWORD="$pw" --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 \
        > /dev/null
    for _ in $(seq 30); do
        docker exec "$box" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && break
        sleep 1
    done
    docker exec -e PW="$pw" "$box" python3 -I /p/v2turn.py /p/out/who "你是誰？請用一句話回答。" \
        --agent wukong --model "$model" --wait-jobs --max-secs 180 > /dev/null 2>&1
    docker kill "$box" > /dev/null 2>&1
    for _ in $(seq 20); do docker inspect "$box" > /dev/null 2>&1 || break; sleep 0.5; done
    python3 - "$d/out/who.json" "$model" "$case" <<'EOF_WHO'
import json, sys
res = json.load(open(sys.argv[1]))
msgs = (res.get("messages") or {}).get("data") or []
text, seen = "", set()
for m in msgs:  # V2 由新到舊排列
    if m.get("type") == "assistant":
        mm = m.get("model") or {}
        seen.add(f"{mm.get('providerID')}/{mm.get('id')}")
        if not text:
            text = "".join(c.get("text", "") for c in m.get("content", []) if c.get("type") == "text").strip()
print(f"  reply: {text.replace(chr(10), ' ')[:120]} [models={sorted(seen)}]")
# 孫悟空的自稱：「俺老孫乃齊天大聖」這類回答沒有「悟空」兩字（2026-11-26 第一次執行）
ok = seen == {sys.argv[2]} and any(w in text for w in ("悟空", "老孫", "齊天大聖"))
print(("PASS" if ok else "FAIL") + f" {sys.argv[3]}: asked who it is, wukong answers as 孫悟空 on {sys.argv[2]}")
sys.exit(0 if ok else 1)
EOF_WHO
    rc=$?; fails=$((fails + rc))
done

echo "fails=$fails"
exit $((fails > 0))
