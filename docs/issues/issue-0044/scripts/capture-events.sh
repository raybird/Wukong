#!/usr/bin/env bash
# Task 1.3：從真實 V2 server 的 /api/event 取事件樣本，存成 wukong-gateway 測試 fixture。
# 用法：capture-events.sh V2_PREFIX WORK_DIR OUT_DIR
#   V2_PREFIX：`npm install -g --prefix V2_PREFIX @opencode/cli@2.3.2` 的目錄
#   WORK_DIR：空目錄，當容器的 HOME 與 workspace（全新資料，不含任何正式對話）
#   OUT_DIR：輸出目錄，每個情境一個 .ndjson（每行一個事件的 JSON）
# 只送固定的合成提示詞給預設免費模型；retry 情境在 --network none 的另一個容器取。
# 環境變數 SCENARIOS 有值時，只在有網路的容器跑列出的情境（Task 2.2 的附件、agent、壓縮探測）。
set -uo pipefail
v2=$1 work=$2 out=$3
image=ghcr.io/raybird/wukong:v0.25.1
pw=probe-secret
tag=cap-$$
mkdir -p "$work/net/home" "$work/net/workspace" "$work/off/home" "$work/off/workspace" "$out"
cleanup() { docker ps -aq --filter "name=^$tag-" | xargs -r docker rm -f > /dev/null; }
trap cleanup EXIT

start() { # start NAME none|bridge
  docker run -d --name "$tag-$1" --user 1000:1000 --network "$2" -e HOME=/p/home -e OPENCODE_SERVER_PASSWORD=$pw \
    -v "$work/$1":/p -v "$v2":/v2prefix:ro -v "$(cd "$(dirname "$0")" && pwd)":/scripts:ro -w /p/workspace \
    --entrypoint /v2prefix/bin/opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
  for _ in $(seq 1 60); do docker logs "$tag-$1" 2>&1 | grep -q 'listening on' && return 0; sleep 1; done
  echo "$1 did not report listening" >&2; return 1
}
scenario() { # scenario BOX NAME：在容器內跑 capture-scenario.py 的同名情境
  docker exec -e PW=$pw "$tag-$1" python3 -I /scripts/capture-scenario.py "$2" > "$out/server-$2.ndjson" 2> "$work/$2.err"
  echo "-- $2: rc=$? events=$(grep -c . "$out/server-$2.ndjson") $(tail -1 "$work/$2.err")"
  grep -v '^PASS\|^FAIL' "$work/$2.err" | head -5
}

start net bridge
if [ -n "${SCENARIOS:-}" ]; then
  for s in $SCENARIOS; do scenario net "$s"; done
else
  for s in text-tool permission form interrupt no-route; do scenario net "$s"; done
  start off none
  scenario off retry
fi
echo "== event types"
for f in "$out"/server-*.ndjson; do
  printf '%s: ' "$(basename "$f")"
  python3 -I -c 'import json,sys; print(sorted({json.loads(l)["type"] for l in open(sys.argv[1]) if l.strip()}))' "$f"
done
