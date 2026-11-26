#!/usr/bin/env bash
# Task 1.1（SCN-001）：V1 的 session 在 V2 遷移後能否接續。全程 --network none，對話內容不離開本機。
# 用法：probe-session.sh SNAPSHOT_DIR V2_PREFIX WORK_DIR
#   SNAPSHOT_DIR：#30 snapshot.py 產生的副本目錄（只讀 memory.db、opencode.db，不會改它們）
#   V2_PREFIX：`npm install -g --prefix V2_PREFIX @opencode/cli@2.3.2` 的目錄
#   WORK_DIR：空目錄，放 V1、V2 各自的 opencode.db 再複製與日誌
# 只印 session 的筆數與編號前綴，不印對話內容。
set -euo pipefail
snap=$1 v2=$2 work=$3
image=ghcr.io/raybird/wukong:v0.25.1
pw=probe-secret
tag=sess-$$
for side in v1 v2; do
  mkdir -p "$work/$side/home/.local/share/opencode"
  cp "$snap/opencode.db" "$work/$side/home/.local/share/opencode/opencode.db"
done
mkdir -p "$work/workspace" "$work/logs"
cleanup() { docker ps -aq --filter "name=^$tag-" | xargs -r docker rm -f > /dev/null; }
trap cleanup EXIT

start() { # start v1|v2
  local side=$1 bin=opencode extra=()
  if [ "$side" = v2 ]; then bin=/v2prefix/bin/opencode; extra=(-v "$v2":/v2prefix:ro -e OPENCODE_SERVER_PASSWORD=$pw); fi
  docker run -d --name "$tag-$side" --user 1000:1000 --network none -e HOME=/p/home \
    -v "$work/$side":/p -v "$work/workspace":/workspace -w /workspace "${extra[@]}" \
    --entrypoint "$bin" "$image" serve --hostname 127.0.0.1 --port 4096 --print-logs > /dev/null
  for _ in $(seq 1 60); do docker logs "$tag-$side" 2>&1 | grep -q 'listening on' && return 0; sleep 1; done
  echo "$side did not report listening" >&2; docker logs "$tag-$side" 2>&1 | tail -20 >&2; return 1
}
py() { local side=$1; shift; docker exec -i -e PW=$pw "$tag-$side" python3 -I - "$@"; }

echo "== 1. Wukong 記錄的 scope → session（memory.db 副本，唯讀）"
ids=$(docker run --rm --user 1000:1000 --network none --entrypoint python3 -v "$snap":/s:ro "$image" -I -c '
import sqlite3
db = sqlite3.connect("file:/s/memory.db?immutable=1", uri=True)
for (sid,) in db.execute("select session_id from agent_session_state where session_id is not null order by scope"):
    print(sid)')
echo "scopes with a session: $(echo "$ids" | grep -c .)"

echo "== 2. V1 基準（另一份再複製，V1 1.x 讀）"
start v1
py v1 $ids > "$work/v1-baseline.json" <<'PY'
import json, sys, urllib.request
out = {}
for sid in sys.argv[1:]:
    msgs = json.load(urllib.request.urlopen(f"http://127.0.0.1:4096/session/{sid}/message?limit=1000000"))
    last = max((i for i, m in enumerate(msgs) if any(p.get("type") == "compaction" for p in m.get("parts", []))), default=None)
    out[sid] = {"messages": len(msgs), "last_compaction_index": last,
                "after_last_compaction": len(msgs) - last if last is not None else len(msgs)}
print(json.dumps(out))
PY
docker exec "$tag-v1" opencode --version | sed 's/^/v1 version: /'
docker logs "$tag-v1" > "$work/logs/v1.log" 2>&1; docker rm -f "$tag-v1" > /dev/null
python3 -I -c '
import json,sys
for sid, v in json.load(open(sys.argv[1])).items():
    print("  %s… messages=%s after_last_compaction=%s" % (sid[:12], v["messages"], v["after_last_compaction"]))' "$work/v1-baseline.json"

echo "== 3. V2 第一次啟動（就地遷移 v2 這份再複製）"
start v2
echo "migrations applied: $(docker logs "$tag-v2" 2>&1 | grep -c 'database migration completed')"
docker exec -e PW=$pw "$tag-v2" sh -c 'curl -s -u opencode:$PW http://127.0.0.1:4096/api/info' | python3 -I -c 'import json,sys; print("v2 version:", json.load(sys.stdin).get("version"))'

echo "== 4. 每個 session 在 V2 的查詢與 context"
py v2 $ids > "$work/v2-sessions.json" <<'PY'
import base64, json, os, sys, urllib.error, urllib.request
auth = "Basic " + base64.b64encode(f"opencode:{os.environ['PW']}".encode()).decode()
def get(path):
    req = urllib.request.Request("http://127.0.0.1:4096" + path, headers={"Authorization": auth})
    try:
        with urllib.request.urlopen(req) as r:
            return r.status, json.load(r)
    except urllib.error.HTTPError as e:
        return e.code, None
def count_messages(sid):
    total, cursor = 0, None
    while True:
        status, body = get(f"/api/session/{sid}/message?limit=200" + (f"&cursor={cursor}" if cursor else ""))
        if status != 200:
            return None
        total += len(body["data"])
        cursor = (body.get("cursor") or {}).get("next")
        if not cursor or not body["data"]:
            return total
out = {}
for sid in sys.argv[1:] + ["ses_doesnotexist000000000000"]:
    s_status, _ = get(f"/api/session/{sid}")
    c_status, ctx = get(f"/api/session/{sid}/context")
    out[sid] = {"session_status": s_status, "context_status": c_status,
                "context_messages": len(ctx["data"]) if ctx else None,
                "messages": count_messages(sid) if s_status == 200 else None}
print(json.dumps(out))
PY
python3 -I -c '
import json,sys
for sid, v in json.load(open(sys.argv[1])).items():
    print("  %s… session=%s context=%s/%s messages=%s" % (sid[:12], v["session_status"], v["context_status"], v["context_messages"], v["messages"]))' "$work/v2-sessions.json"

echo "== 5. 對第一個 session 送 prompt（沒有網路，模型呼叫會失敗；只看輸入有沒有接到同一個 session）"
first=$(echo "$ids" | head -1)
py v2 "$first" > "$work/v2-prompt.json" <<'PY'
import base64, json, os, sys, time, urllib.error, urllib.request
auth = "Basic " + base64.b64encode(f"opencode:{os.environ['PW']}".encode()).decode()
sid = sys.argv[1]
def call(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request("http://127.0.0.1:4096" + path, data=data, method=method,
                                 headers={"Authorization": auth, "content-type": "application/json"})
    try:
        with urllib.request.urlopen(req) as r:
            return r.status, json.load(r)
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()[:300]
def context_ids():
    status, body = call("GET", f"/api/session/{sid}/context")
    return [m.get("id") for m in body["data"]] if status == 200 else None
before = context_ids()
status, resp = call("POST", f"/api/session/{sid}/prompt", {"text": "wukong issue-0044 probe: continuation check"})
admitted_session = (resp.get("data") or {}).get("sessionID") if isinstance(resp, dict) else None
for _ in range(60):
    _, active = call("GET", "/api/session/active")
    busy = sid in json.dumps(active)
    if not busy:
        break
    time.sleep(1)
after = context_ids()
print(json.dumps({"prompt_status": status, "admitted_session_matches": admitted_session == sid if admitted_session else None,
                  "response_keys": sorted(resp["data"].keys()) if isinstance(resp, dict) and isinstance(resp.get("data"), dict) else str(resp)[:200],
                  "context_before": len(before) if before is not None else None,
                  "context_after": len(after) if after is not None else None,
                  "old_context_kept_as_prefix": before is not None and after is not None and after[:len(before)] == before,
                  "still_busy_after_wait": busy}))
PY
cat "$work/v2-prompt.json"; echo
docker logs "$tag-v2" > "$work/logs/v2.log" 2>&1
echo "upstream/network errors in v2 log: $(grep -ciE 'ENOTFOUND|EAI_AGAIN|network|fetch failed|ECONNREFUSED' "$work/logs/v2.log" || true)"
