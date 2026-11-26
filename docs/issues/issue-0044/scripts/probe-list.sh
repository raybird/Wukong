#!/usr/bin/env bash
# Task 2.1（地雷 W-6）：V2 `GET /api/session` 的分頁行為，以及遷移後的列表是否包含全部舊 session。
# 用法：probe-list.sh SNAPSHOT_DIR V2_PREFIX WORK_DIR
#   SNAPSHOT_DIR：#30 snapshot.py 產生的副本目錄（只讀 opencode.db，不會改它）
#   V2_PREFIX：`npm install -g --prefix V2_PREFIX @opencode/cli@2.3.2` 的目錄
#   WORK_DIR：空目錄
# 全程 --network none，不送提示詞；只印筆數、順序判定與 cursor 有無，不印 session 標題或內容。
set -euo pipefail
snap=$1 v2=$2 work=$3
image=ghcr.io/raybird/wukong:v0.25.1
pw=probe-secret
tag=list-$$
mkdir -p "$work/fresh/home" "$work/migrated/home/.local/share/opencode" "$work/workspace"
cp "$snap/opencode.db" "$work/migrated/home/.local/share/opencode/opencode.db"
cleanup() { docker ps -aq --filter "name=^$tag-" | xargs -r docker rm -f > /dev/null; }
trap cleanup EXIT

start() { # start fresh|migrated
  docker run -d --name "$tag-$1" --user 1000:1000 --network none -e HOME=/p/home -e OPENCODE_SERVER_PASSWORD=$pw \
    -v "$work/$1":/p -v "$work/workspace":/workspace -v "$v2":/v2prefix:ro -w /workspace \
    --entrypoint /v2prefix/bin/opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
  for _ in $(seq 1 90); do docker logs "$tag-$1" 2>&1 | grep -q 'listening on' && return 0; sleep 1; done
  echo "$1 did not report listening" >&2; return 1
}
py() { local box=$1; shift; docker exec -i -e PW=$pw "$tag-$box" python3 -I - "$@"; }
LIB='
import base64, json, os, sys, urllib.error, urllib.parse, urllib.request
B = "http://127.0.0.1:4096"
H = {"Authorization": "Basic " + base64.b64encode(("opencode:" + os.environ["PW"]).encode()).decode(),
     "content-type": "application/json"}
def call(method, path, body=None, auth=True):
    req = urllib.request.Request(B + path, method=method, data=json.dumps(body).encode() if body is not None else None,
                                 headers=H if auth else {})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            raw = r.read(); return r.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()[:200]
def walk(limit, order=None):
    q = {"limit": str(limit)}
    if order: q["order"] = order
    pages, seen = [], []
    while True:
        status, body = call("GET", "/api/session?" + urllib.parse.urlencode(q))
        assert status == 200, (status, body)
        pages.append(len(body["data"])); seen += [s["id"] for s in body["data"]]
        nxt = body["cursor"].get("next")
        if not nxt or not body["data"]: return pages, seen, body
        q = {"cursor": nxt, "limit": str(limit)}
'

start fresh
echo "== 1. 全新資料：建立 7 個 session，limit=3 走 cursor"
py fresh <<EOF
$LIB
created = []
for i in range(7):
    s, b = call("POST", "/api/session", {"title": f"list-probe-{i}"}); assert s == 200, (s, b); created.append(b["data"]["id"])
s, b = call("GET", "/api/session"); print("default page size:", len(b["data"]), "cursor.next:", bool(b["cursor"].get("next")))
for order in (None, "asc", "desc"):
    pages, seen, last = walk(3, order)
    rank = [created.index(x) for x in seen if x in created]
    print(f"order={order}: pages={pages} all_listed={sorted(seen)==sorted(created)} dup={len(seen)!=len(set(seen))}",
          f"creation_rank={rank} last_cursor={last['cursor']}")
s, b = call("GET", "/api/session?limit=0"); print("limit=0:", s, str(b)[:120])
s, b = call("GET", "/api/session?limit=abc"); print("limit=abc:", s, str(b)[:120])
s, b = call("GET", "/api/session?cursor=bogus"); print("cursor=bogus:", s, str(b)[:160])
s, b = call("GET", "/api/session?limit=100000"); print("limit=100000:", s, len(b["data"]) if s == 200 else str(b)[:160])
s, b = call("GET", "/api/info", auth=False); print("info no-auth:", s, str(b)[:160])
H["Authorization"] = "Basic " + base64.b64encode(b"opencode:wrong").decode()
s, b = call("GET", "/api/info"); print("info wrong-password:", s, str(b)[:160])
EOF

start migrated
echo "== 2. 遷移後的正式資料副本：列表總數對照 session 表"
v1_total=$(docker run --rm --user 1000:1000 --network none --entrypoint python3 -v "$snap":/s:ro "$image" -I -c '
import sqlite3
db = sqlite3.connect("file:/s/opencode.db?immutable=1", uri=True)
print(db.execute("select count(*) from session").fetchone()[0])')
echo "V1 session rows (snapshot): $v1_total"
py migrated <<EOF
$LIB
pages, seen, _ = walk(100)
print("V2 listed:", len(seen), "pages:", len(pages), "unique:", len(set(seen)))
pages, seen2, _ = walk(100, "asc")
print("V2 listed asc:", len(seen2), "same set:", set(seen) == set(seen2))
s, b = call("GET", "/api/session?archived=true&limit=1000"); print("archived=true:", s, len(b["data"]) if s == 200 else b)
EOF
