#!/usr/bin/env bash
# Task 6.1 演練用，在 dind 裡以 /reh/bin/state 執行（rehearsal-env.sh 建立的環境），見 rehearsal.md
# state LABEL：用 wukong-web 容器裡的 python 唯讀查 memory.db 與 opencode.db（主機與 dind 的 python 都可能沒有 sqlite）
label=$1
docker exec -i -u wukong wukong-web python3 - "$label" <<'PY'
import sqlite3, sys, os
label = sys.argv[1]
mem = sqlite3.connect("file:/data/memory.db?mode=ro", uri=True)
rows = mem.execute("select scope, session_id, turn_count from agent_session_state order by scope").fetchall()
oc = "/home/wukong/.local/share/opencode/opencode.db"
db = sqlite3.connect(f"file:{oc}?mode=ro", uri=True)
tables = sorted(r[0] for r in db.execute("select name from sqlite_master where type='table'"))
print(f"[{label}] scopes: {rows}")
print(f"[{label}] opencode.db tables={len(tables)} v1-only={[t for t in ('data_migration','session_context_epoch','session_input') if t in tables]} session_v2={'session_v2' in tables}")
for scope, sid, _ in rows:
    n = db.execute("select count(*) from session where id=?", (sid,)).fetchone()[0]
    m = db.execute("select count(*) from message where session_id=?", (sid,)).fetchone()[0] if "message" in tables else None
    print(f"[{label}]   {scope}: session in opencode.db={n} messages={m}")
PY
