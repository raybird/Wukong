#!/usr/bin/env bash
# 步驟 4：V2 遷移與回復探測，外加啟動訊息、認證、V1 路徑與 openapi 的檢查。全程 --network none，不呼叫模型。
# 用法：v2-migration.sh SNAPSHOT_DIR V2_PREFIX WORK_DIR
#   SNAPSHOT_DIR：步驟 1 的副本目錄（只讀其中的 opencode.db，不會改它）
#   V2_PREFIX：`npm install -g --prefix V2_PREFIX @opencode/cli@2.3.2` 的目錄
#   WORK_DIR：空目錄，會放 opencode.db 的再複製與各步驟的日誌
set -euo pipefail
snap=$1 v2=$2 work=$3
image=ghcr.io/raybird/wukong:v0.25.1
data=$work/home/.local/share/opencode
mkdir -p "$data" "$work/workspace" "$work/logs"
cp "$snap/opencode.db" "$data/opencode.db"
pw=probe-secret
tag=mig-$$
cleanup() { docker ps -aq --filter "name=^$tag-" | xargs -r docker rm -f > /dev/null; }
trap cleanup EXIT

start() { # start NAME v1|v2 [額外的 docker run 參數]
  # V1 照 Wukong 本機模式不設密碼；V2 不設會自己產生隨機密碼，所以給一個固定值
  local name=$tag-$1 ver=$2 bin=opencode extra=("${@:3}")
  if [ "$ver" = v2 ]; then bin=/v2prefix/bin/opencode; extra+=(-v "$v2":/v2prefix:ro -e OPENCODE_SERVER_PASSWORD=$pw); fi
  docker run -d --name "$name" --user 1000:1000 --network none -e HOME=/p/home \
    -w /p/workspace -v "$work":/p "${extra[@]}" \
    --entrypoint "$bin" "$image" serve --hostname 127.0.0.1 --port 4096 --print-logs > /dev/null
  for _ in $(seq 1 30); do docker logs "$name" 2>&1 | grep -q 'listening on' && return 0; sleep 1; done
  echo "$name did not report listening" >&2; return 1
}
stop() { docker logs "$tag-$1" > "$work/logs/$1.log" 2>&1; docker rm -f "$tag-$1" > /dev/null; }
x() { docker exec "$tag-$1" sh -c "$2"; }
logs() { docker logs "$tag-$1" 2>&1; }
count_v1='curl -s "http://127.0.0.1:4096/session?limit=10000" | python3 -c "import json,sys; print(len(json.load(sys.stdin)))"'
count_v2="curl -s -u opencode:$pw 'http://127.0.0.1:4096/api/session?limit=100' | python3 -c \"import json,sys; print(len(json.load(sys.stdin)['data']))\""

echo "== 1. V2 第一次啟動"
start m-v2a v2
echo "listening line: $(logs m-v2a | grep -m1 'listening on' | sed 's/:[0-9]*$/:PORT/')"
echo "migrations applied: $(logs m-v2a | grep -c 'database migration completed')"
x m-v2a "curl -s -o /dev/null -w 'no-auth GET /global/health: %{http_code} %{content_type}\n' http://127.0.0.1:4096/global/health"
x m-v2a "curl -s -o /dev/null -w 'no-auth GET /api/info: %{http_code}\n' http://127.0.0.1:4096/api/info"
x m-v2a "curl -s -u opencode:$pw http://127.0.0.1:4096/api/info | python3 -c \"import json,sys; d=json.load(sys.stdin); print('auth GET /api/info version:', d['version'])\""
x m-v2a "curl -s -u opencode:$pw http://127.0.0.1:4096/openapi.json | python3 -c \"import json,sys; print('openapi paths:', len(json.load(sys.stdin)['paths']))\""
x m-v2a "curl -s -u opencode:$pw http://127.0.0.1:4096/api/experimental/migration/v1; echo"
echo "v2 sessions: $(x m-v2a "$count_v2")"
stop m-v2a

echo "== 2. V1 開遷移後的檔案"
start m-v1a v1
x m-v1a "curl -s http://127.0.0.1:4096/global/health; echo"
echo "v1 sessions: $(x m-v1a "$count_v1")"
# 最大的 session：逐一讀各 session 的訊息，取 part 最多的那個
x m-v1a "curl -s 'http://127.0.0.1:4096/session?limit=10000' | python3 -c \"
import json, sys, urllib.request
best = (0, 0)
for s in json.load(sys.stdin):
    msgs = json.load(urllib.request.urlopen('http://127.0.0.1:4096/session/%s/message?limit=100000' % s['id']))
    best = max(best, (sum(len(m.get('parts', [])) for m in msgs), len(msgs)))
print('v1 largest session: messages', best[1], 'parts', best[0])
\""
x m-v1a "curl -s -X POST -H 'content-type: application/json' -d '{}' http://127.0.0.1:4096/session > /dev/null && echo 'v1 created a session'"
stop m-v1a

echo "== 3. 再開 V2"
start m-v2b v2
echo "migrations applied: $(logs m-v2b | grep -c 'database migration completed')"
echo "v2 sessions: $(x m-v2b "$count_v2")"
x m-v2b "curl -s -u opencode:$pw -X POST -H 'content-type: application/json' -d '{}' http://127.0.0.1:4096/api/session > /dev/null && echo 'v2 created a session'"
stop m-v2b

echo "== 4. 再開 V1"
start m-v1b v1
echo "v1 sessions: $(x m-v1b "$count_v1")"
stop m-v1b

echo "== 5. OPENCODE_SERVER_USERNAME 是否還有效（另一份資料目錄，不影響上面的結果）"
mkdir -p "$work/user-check"
docker run -d --name "$tag-user" --user 1000:1000 --network none -e HOME=/tmp/home \
  -e OPENCODE_SERVER_PASSWORD=$pw -e OPENCODE_SERVER_USERNAME=custom -v "$v2":/v2prefix:ro \
  --entrypoint /v2prefix/bin/opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
for _ in $(seq 1 30); do logs user | grep -q 'listening on' && break; sleep 1; done
x user "curl -s -o /dev/null -w 'custom:pw %{http_code}\n' -u custom:$pw http://127.0.0.1:4096/api/info"
x user "curl -s -o /dev/null -w 'opencode:pw %{http_code}\n' -u opencode:$pw http://127.0.0.1:4096/api/info"
stop user

echo "== 表的差異（副本 → 探測後）"
docker run --rm --user 1000:1000 --network none --entrypoint python3 \
  -v "$snap":/a:ro -v "$data":/b:ro "$image" -I -c '
import sqlite3
def tables(uri):
    return {r[0] for r in sqlite3.connect(uri, uri=True).execute("select name from sqlite_master where type=\"table\"")}
a = tables("file:/a/opencode.db?immutable=1"); b = tables("file:/b/opencode.db?mode=ro")
src = sqlite3.connect("file:/a/opencode.db?immutable=1", uri=True)
print("removed:", {t: src.execute(f"select count(*) from \"{t}\"").fetchone()[0] for t in sorted(a - b)})
print("added:", sorted(b - a))'
