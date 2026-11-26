#!/usr/bin/env bash
# #45：常駐 V2 server 執行中改寫 agent 定義檔，server 什麼時候用新內容（SCN-008 的前提）。
# 用法：probe-agent-reload.sh IMAGE WORK_DIR AUTH_JSON MODEL
# 只送固定的合成提示詞；HOME 與工作區都在 WORK_DIR。
#   1. 定義寫「暗號是 ALPHA」後啟動 server，等 /api/agent 列出它，問暗號。
#   2. 就地改寫成 BRAVO：立刻看 /api/agent 的內容並問暗號；之後每秒看一次 /api/agent，最多 60 秒，再問一次。
#   3. 以 wukong 的寫法（先寫暫存檔再 rename，agent_assets::write_if_changed）改寫成 CHARLIE，同樣立刻問、等 /api/agent 更新後再問。
#   4. 再以 rename 改寫成 DELTA，這次改寫後不先查 /api/agent（與 wukong 寫完定義就送 prompt 相同），連問兩次。
set -uo pipefail
image="${1:?usage: probe-agent-reload.sh IMAGE WORK_DIR AUTH_JSON MODEL}"
work="$(realpath "${2:?}")"
auth="${3:?}" model="${4:?}"
here="$(cd "$(dirname "$0")" && pwd)"
box="probe45-reload-$$"
pw="probe-$$"
cleanup() { docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

mkdir -p "$work/home/.local/share/opencode" "$work/ws/.opencode/agents" "$work/out"
install -m 600 "$auth" "$work/home/.local/share/opencode/auth.json"
cp "$here/v2turn.py" "$work/v2turn.py"
define() { printf -- '---\ndescription: reload probe\nmode: primary\n---\n你是測試用 agent。今天的暗號是 %s。被問到暗號時只回覆暗號本身。\n' "$1" \
    > "$work/ws/.opencode/agents/wkreload.md"; }
define ALPHA
define_renamed() { # 與 agent_assets::write_if_changed 相同：同目錄的暫存檔，再 rename 蓋過
    local tmp="$work/ws/.opencode/agents/.wkreload.md.tmp.$$"
    printf -- '---\ndescription: reload probe\nmode: primary\n---\n你是測試用 agent。今天的暗號是 %s。被問到暗號時只回覆暗號本身。\n' "$1" > "$tmp"
    mv "$tmp" "$work/ws/.opencode/agents/wkreload.md"
}

docker run -d --rm --name "$box" --user "$(id -u):$(id -g)" -e HOME=/p/home -v "$work":/p -w /p/ws \
    -e OPENCODE_SERVER_PASSWORD="$pw" -e "OPENCODE_CONFIG_CONTENT={\"model\":\"$model\"}" \
    --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
for _ in $(seq 30); do
    docker exec "$box" sh -c "curl -sf -u opencode:$pw http://127.0.0.1:4096/api/info" > /dev/null 2>&1 && break
    sleep 1
done
# /api/agent 裡 wkreload 的內容含哪個暗號（沒有列出時印 none）
listed() { docker exec "$box" sh -c "curl -s -u opencode:$pw 'http://127.0.0.1:4096/api/agent?location%5Bdirectory%5D=/p/ws'" \
    | python3 -c '
import json, sys
a = next((a for a in json.load(sys.stdin)["data"] if a["id"] == "wkreload"), None)
s = json.dumps(a, ensure_ascii=False) if a else ""
print("none" if not a else next((w for w in ("ALPHA", "BRAVO", "CHARLIE", "DELTA") if w in s), "listed-without-prompt"))'; }
ask() { # ask NAME：問暗號，印出回覆
    docker exec -e PW="$pw" "$box" python3 -I /p/v2turn.py "/p/out/$1" "暗號是什麼？" --agent wkreload --model "$model" --max-secs 120 \
        > /dev/null 2>&1
    python3 -c '
import json, sys
msgs = (json.load(open(sys.argv[1])).get("messages") or {}).get("data") or []
for m in msgs:
    if m.get("type") == "assistant":
        print("".join(c.get("text", "") for c in m.get("content", []) if c.get("type") == "text").strip()[:40]); break' "$work/out/$1.json"
}

for _ in $(seq 20); do [[ "$(listed)" != none ]] && break; sleep 1; done
echo "start:        /api/agent=$(listed)  reply=$(ask a1)"
define BRAVO
echo "rewritten:    /api/agent=$(listed)  reply=$(ask b0)"
start=$(date +%s); seen=""
for _ in $(seq 60); do seen="$(listed)"; [[ "$seen" == BRAVO ]] && break; sleep 1; done
echo "after $(( $(date +%s) - start ))s: /api/agent=$seen  reply=$(ask b1)"
echo "new session after another prompt: reply=$(ask b2)"
define_renamed CHARLIE
echo "renamed:      /api/agent=$(listed)  reply=$(ask c0)"
start=$(date +%s); seen=""
for _ in $(seq 60); do seen="$(listed)"; [[ "$seen" == CHARLIE ]] && break; sleep 1; done
echo "after $(( $(date +%s) - start ))s: /api/agent=$seen  reply=$(ask c1)"
define_renamed DELTA
echo "renamed, no list first: reply=$(ask d0)"
echo "                  again: reply=$(ask d1)  /api/agent=$(listed)"
