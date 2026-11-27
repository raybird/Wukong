#!/usr/bin/env bash
# #45 Task 6.3 演練用，在 dind 裡以 /reh/bin/state 執行（rehearsal-env.sh 建立的環境），見 rehearsal.md。
# 改寫自 issue-0044 的同名腳本，改查 V2 的表（session_v2、session_message）。
# state LABEL：用 wukong-web 容器裡的 python 唯讀查 memory.db 與 opencode.db，印出每個 scope 的 session、
# 訊息類型統計、子 session 的 agent，以及 user message 有沒有固定區塊（SCN-006）。
label=$1
docker exec -i -u wukong wukong-web python3 - "$label" <<'PY'
import json, sqlite3, sys
label = sys.argv[1]
mem = sqlite3.connect("file:/data/memory.db?mode=ro", uri=True)
rows = mem.execute("select scope, session_id, turn_count from agent_session_state order by scope").fetchall()
db = sqlite3.connect("file:/home/wukong/.local/share/opencode/opencode.db?mode=ro", uri=True)
FIXED = ["[排程能力]", "[長期記憶能力]", "[輸出要求]", "[檔案互動規則]", "[派工]", "你是 Explorer", "你是 Fixer"]
print(f"[{label}] scopes: {rows}")
for scope, sid, _ in rows:
    if not sid:
        continue
    types = dict(db.execute("select type, count(*) from session_message where session_id=? group by type", (sid,)).fetchall())
    kids = db.execute("select agent, count(*) from session_v2 where parent_id=? group by agent", (sid,)).fetchall()
    bad = set()
    for (d,) in db.execute("select data from session_message where session_id=? and type='user'", (sid,)):
        text = json.dumps(json.loads(d), ensure_ascii=False)
        bad |= {h for h in FIXED if h in text}
    print(f"[{label}]   {scope}: {sid[:16]}… types={types} subagents={kids} fixed_headers_in_user={sorted(bad)}")
PY
