#!/usr/bin/env bash
# Task 6.1 演練用，在 dind 裡以 /reh/bin/webquestion 執行（rehearsal-env.sh 建立的環境），見 rehearsal.md
# webquestion NAME SCOPE reply|reject ANSWER：Web 回合等到 question 事件後回覆或取消，再等回合結束
name=$1 scope=$2 action=$3 answer=${4:-}
prompt="${WQ_PROMPT:-Use the question tool to ask me one question: whether I prefer RED or BLUE (two options: RED, BLUE). After I answer, reply with exactly the color I chose, or exactly NO-ANSWER if the question was dismissed.}"
q=$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$prompt")
sc=$(python3 -c 'import sys,urllib.parse;print(urllib.parse.quote(sys.argv[1]))' "$scope")
out=/reh/out/$name.sse; : > "$out"
s=$(date +%s)
curl -sN --max-time 400 "http://127.0.0.1:8787/chat?scope=$sc&q=$q" > "$out" & pid=$!
for _ in $(seq 1 120); do grep -q '^event: question' "$out" && break; kill -0 $pid 2>/dev/null || break; sleep 1; done
data=$(grep -A1 '^event: question' "$out" | sed -n 's/^data: //p' | head -1)
if [[ -z "$data" ]]; then echo "[$name] no question event within the wait"; wait $pid; else
  rid=$(python3 -c 'import json,sys;print(json.loads(sys.argv[1])["request_id"])' "$data")
  sid=$(python3 -c 'import json,sys;print(json.loads(sys.argv[1])["session_id"])' "$data")
  echo "[$name] question after $(( $(date +%s)-s ))s: $(python3 -c 'import json,sys;d=json.loads(sys.argv[1]);print([(x["question"][:60],[o["label"] for o in x["options"]]) for x in d["questions"]])' "$data")"
  if [[ $action == reply ]]; then body=$(python3 -c 'import json,sys;print(json.dumps({"session_id":sys.argv[1],"answers":[[sys.argv[2]]]}))' "$sid" "$answer")
  else body=$(python3 -c 'import json,sys;print(json.dumps({"session_id":sys.argv[1]}))' "$sid"); fi
  code=$(curl -s -o /reh/out/$name.post -w '%{http_code}' -X POST -H 'content-type: application/json' -d "$body" "http://127.0.0.1:8787/api/questions/$rid/$action")
  echo "[$name] POST $action -> HTTP $code $(head -c 200 /reh/out/$name.post)"
  wait $pid
fi
echo "[$name] secs=$(( $(date +%s)-s )) events: $(grep -E '^event:' "$out" | sort | uniq -c | awk '{printf "%s=%s ", $3, $1}')"
python3 - "$out" <<'PY'
import sys, re, html
ev = None
for line in open(sys.argv[1], encoding="utf-8"):
    line = line.rstrip("\n")
    if line.startswith("event: "): ev = line[7:]
    elif line.startswith("data: ") and ev in ("answer", "error"):
        print(f"  {ev}: " + html.unescape(re.sub(r"<[^>]+>", "", line[6:]))[:200]); break
PY
