#!/usr/bin/env bash
# Task 1.2：V2 `opencode run` 的 CLI 契約（SCN-010 支援、TBD-1）。
# 用法：probe-cli.sh V2_PREFIX WORK_DIR
#   V2_PREFIX：`npm install -g --prefix V2_PREFIX @opencode/cli@2.3.2` 的目錄
#   WORK_DIR：空目錄，當作容器的 HOME 與 workspace（全新資料，不含任何正式對話）
# 第 1～4 段 --network none；第 5、6 段要連模型，只送固定的合成提示詞給預設免費模型。
set -uo pipefail
v2=$1 work=$2
image=ghcr.io/raybird/wukong:v0.25.1
tag=cli-$$
mkdir -p "$work/home" "$work/workspace" "$work/out"
cleanup() { docker ps -aq --filter "name=^$tag-" | xargs -r docker rm -f > /dev/null; }
trap cleanup EXIT

box() { # box NAME none|bridge：起一個常駐的空容器，之後用 docker exec 執行
  docker run -d --name "$tag-$1" --user 1000:1000 --network "$2" -e HOME=/p/home \
    -v "$work":/p -v "$v2":/v2prefix:ro -w /p/workspace --entrypoint sleep "$image" infinity > /dev/null
}
oc() { local name=$1; shift; docker exec "$tag-$name" timeout 120 /v2prefix/bin/opencode "$@"; }
# 映像沒有 ps：從 /proc 找 cmdline 含 /v2prefix/bin/opencode 的程序
plist() { docker exec "$tag-$1" sh -c 'for d in /proc/[0-9]*; do [ "${d#/proc/}" = "$$" ] && continue; c=$(tr "\0" " " < $d/cmdline 2>/dev/null); case "$c" in */v2prefix/bin/opencode*|*/opencode2*|*.opencode*) echo "${d#/proc/} $c";; esac; done'; }
procs() { plist "$1" | grep -vc "^$" ; }
summarize() { # 印 NDJSON 每行的 type 與帶 session 的欄位名稱，不印內容
  python3 -I -c '
import json, sys
types, keys = [], set()
for line in open(sys.argv[1]):
    line = line.strip()
    if not line:
        continue
    try:
        ev = json.loads(line)
    except ValueError:
        types.append("<non-json>"); continue
    types.append(ev.get("type", "<no type>"))
    def walk(o, path=""):
        if isinstance(o, dict):
            for k, v in o.items():
                p = f"{path}.{k}" if path else k
                if "session" in k.lower():
                    keys.add(p)
                walk(v, p)
    walk(ev)
print("  event types:", types)
print("  session fields:", sorted(keys))' "$1"
}
run_case() { # run_case NAME BOX ARGS...：記 rc、stdout 事件、stderr 行數與前幾行、結束後殘留程序
  local name=$1 b=$2; shift 2
  oc "$b" "$@" > "$work/out/$name.out" 2> "$work/out/$name.err"; local rc=$?
  echo "-- $name: rc=$rc stdout_lines=$(grep -c . "$work/out/$name.out") stderr_lines=$(grep -c . "$work/out/$name.err")"
  summarize "$work/out/$name.out"
  sed -n '1,4p' "$work/out/$name.err" | cut -c1-220 | sed 's/^/  stderr: /'
  sleep 2; echo "  opencode processes after exit: $(procs "$b")"
}

echo "== 1. 旗標（--help）；附記：V2 的 --log-level 只收小寫，V1 用的 ERROR 會被拒"
box none none
oc none run --help 2>&1 | grep -E -- '--standalone|--auto|--format|--print-logs|--session|--dangerously' | sed 's/^/  /'

echo "== 2. --network none，無效模型，--standalone"
run_case none-standalone none run --standalone --print-logs --log-level error --format json -m missing/nope "probe"

echo "== 3. --network none，無效模型，不加 --standalone（看會不會留下背景 service）"
run_case none-service none run --print-logs --log-level error --format json -m missing/nope "probe"
plist none | cut -c1-160 | sed 's/^/  proc: /'
plist none | cut -d' ' -f1 | xargs -r docker exec "$tag-none" kill 2>/dev/null; true

echo "== 4. --network none，帶 V1 的 --dangerously-skip-permissions（TBD-1）"
run_case none-dsp none run --standalone --dangerously-skip-permissions --print-logs --log-level error --format json -m missing/nope "probe"

echo "== 5. 有網路，預設免費模型，合成提示詞，--standalone --thinking"
box net bridge
run_case net-first net run --standalone --print-logs --log-level error --format json --thinking "Reply with exactly the word OK."
sid=$(python3 -I -c '
import json, re, sys
for line in open(sys.argv[1]):
    m = re.search(r"\"(ses_[A-Za-z0-9]+)\"", line)
    if m:
        print(m.group(1)); break' "$work/out/net-first.out")
echo "  first session: ${sid:0:12}…"

echo "== 6. 有網路，-s 接續同一個 session"
if [ -n "$sid" ]; then
  run_case net-continue net run --standalone --print-logs --log-level error --format json -s "$sid" "Reply with exactly the word AGAIN."
  grep -c "$sid" "$work/out/net-continue.out" | sed 's/^/  lines mentioning first session: /'
  grep -oE 'ses_[A-Za-z0-9]+' "$work/out/net-continue.out" | sort -u | grep -vc "^$sid$" | sed 's/^/  other session ids: /'
fi

echo "== 7. --network none，預設模型（上游連不到，看會不會卡住；TBD-3 的 CLI 路徑）"
start=$(date +%s)
run_case none-default none run --standalone --print-logs --log-level error --format json "probe"
echo "  elapsed seconds: $(( $(date +%s) - start ))（timeout 120 秒）"

echo "== 8. 有網路，讀 workspace 外的檔案（external_directory 預設 ask），比較三種權限旗標；stdin 為 null，同 Wukong"
for mode in none dsp auto; do
  case $mode in none) flag=();; dsp) flag=(--dangerously-skip-permissions);; auto) flag=(--auto);; esac
  docker exec "$tag-net" timeout 90 /v2prefix/bin/opencode run --standalone --format json "${flag[@]}" \
    "Use the read tool to read the file /etc/hostname, then reply with only the word DONE." \
    < /dev/null > "$work/out/perm-$mode.out" 2> "$work/out/perm-$mode.err"; rc=$?
  python3 -I -c '
import json, sys
tools, perm, text = [], [], []
for line in open(sys.argv[1]):
    try: ev = json.loads(line)
    except ValueError: continue
    t = ev.get("type")
    if t == "tool_use":
        st = ev.get("part", {}).get("state", {})
        tools.append((ev["part"].get("tool"), st.get("status"), (st.get("error") or "")[:80]))
    elif "permission" in str(t):
        perm.append(t)
    elif t == "text":
        text.append(ev["part"].get("text", "")[:20])
print("  tools:", tools, "permission events:", perm, "text:", text)' "$work/out/perm-$mode.out"
  echo "  -- $mode: rc=$rc"
done
