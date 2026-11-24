"""Task 2.5 對照：V1 1.19.6 在提問被取消、權限被拒絕後，回合怎麼結束。

在跑著 V1 `opencode serve --port 4096` 的容器裡執行：
`docker exec -e PW=... 容器 python3 -I /scripts/v1-reject.py question|permission`
權限情境要在啟動 serve 時以 OPENCODE_CONFIG_CONTENT 把 bash 設成 ask。
只送固定的合成提示詞；印出事件類型的順序、錯誤事件與最後的 assistant 文字。
"""
import base64
import json
import os
import sys
import threading
import time
import urllib.error
import urllib.request

BASE = "http://127.0.0.1:4096"
AUTH = "Basic " + base64.b64encode(f"opencode:{os.environ['PW']}".encode()).decode()
events = []
lock = threading.Lock()


def call(method, path, body=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(BASE + path, data=data, method=method,
                                 headers={"Authorization": AUTH, "content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            raw = r.read()
            return r.status, json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()[:300]


def listen():
    req = urllib.request.Request(BASE + "/event", headers={"Authorization": AUTH})
    with urllib.request.urlopen(req) as r:
        for line in r:
            line = line.decode().strip()
            if line.startswith("data:"):
                try:
                    ev = json.loads(line[5:].strip())
                except ValueError:
                    continue
                ev = ev.get("payload", ev)
                with lock:
                    events.append(ev)


def wait_for(pred, seconds):
    end = time.time() + seconds
    while time.time() < end:
        with lock:
            hit = next((e for e in events if pred(e)), None)
        if hit:
            return hit
        time.sleep(0.5)
    return None


def mine(sid, *types):
    return lambda e: e.get("type") in types and e.get("properties", {}).get("sessionID") == sid


threading.Thread(target=listen, daemon=True).start()
time.sleep(1)
name = sys.argv[1]
status, body = call("POST", "/session", {"title": "issue-0044-v1-reject"})
sid = body["id"]
if name == "question":
    text = ("Use the question tool to ask me to choose between the options A and B. "
            "After the question is answered or dismissed, reply with exactly the word AFTER.")
    asked_type = "question.asked"
else:
    text = "Use the bash tool to run exactly: echo probe. If it is not allowed, reply with exactly the word DENIED."
    asked_type = "permission.asked"
status, _ = call("POST", f"/session/{sid}/prompt_async", {"parts": [{"type": "text", "text": text}]})
asked = wait_for(mine(sid, asked_type), 120)
if not asked:
    print("FAIL no", asked_type)
    sys.exit(1)
rid = asked["properties"]["id"]
if name == "question":
    status, body = call("POST", f"/question/{rid}/reject")
else:
    status, body = call("POST", f"/permission/{rid}/reply", {"reply": "reject"})
print("reject call:", status, str(body)[:120])
idle = wait_for(mine(sid, "session.idle"), 120)
time.sleep(1)
with lock:
    seen = [e["type"] for e in events if e.get("properties", {}).get("sessionID") == sid
            and e["type"] in ("question.asked", "question.rejected", "permission.asked", "permission.replied",
                              "session.error", "session.idle")]
    errors = [e["properties"].get("error") for e in events
              if e.get("type") == "session.error" and e.get("properties", {}).get("sessionID") == sid]
status, messages = call("GET", f"/session/{sid}/message")
last = [m for m in messages if m["info"]["role"] == "assistant"][-1]
texts = [p.get("text", "") for p in last["parts"] if p.get("type") == "text"]
print("idle:", bool(idle))
print("events:", seen)
print("errors:", json.dumps(errors)[:300])
print("last assistant error:", json.dumps(last["info"].get("error"))[:300])
print("last assistant text:", texts)
