#!/usr/bin/env bash
# Task 6.1 演練用，在 dind 裡以 /reh/bin/webturn 執行（rehearsal-env.sh 建立的環境），見 rehearsal.md
# webturn NAME SCOPE PROMPT：經 Web 入口跑一個回合，SSE 存到 /reh/out/NAME.sse，印出事件統計與答案
name=$1 scope=$2 prompt=$3
q=$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$prompt")
sc=$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$scope")
s=$(date +%s)
curl -sN --max-time "${WEBTURN_MAX:-300}" "http://127.0.0.1:8787/chat?scope=$sc&q=$q" > "/reh/out/$name.sse"; rc=$?
echo "[$name] rc=$rc secs=$(( $(date +%s)-s )) events: $(grep -E '^event:' "/reh/out/$name.sse" | sort | uniq -c | awk '{printf "%s=%s ", $3, $1}')"
python3 - "/reh/out/$name.sse" <<'PY'
import sys, re, html
ev = None
for line in open(sys.argv[1], encoding="utf-8"):
    line = line.rstrip("\n")
    if line.startswith("event: "): ev = line[7:]
    elif line.startswith("data: ") and ev in ("answer", "error", "question"):
        print(f"  {ev}: " + html.unescape(re.sub(r"<[^>]+>", "", line[6:]))[:300])
PY
