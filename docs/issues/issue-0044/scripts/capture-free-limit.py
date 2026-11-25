"""review-0405603 S3：從真實 V2 server 取一次 free-limit 429 的完整事件，看 V2 在回報前做了什麼。

在 V2 容器內執行（server 已在 127.0.0.1:4096 聽，密碼在環境變數 PW）：
    python3 -I capture-free-limit.py OUT.ndjson [MAX_SECS=240]
只送固定的合成提示詞給預設模型。先開事件串流再建 session，樣本才含 `session.created`（測試的
fixture_events 靠它換掉 session ID）；server 是全新的，只有這一個 session，所以不篩選。
每行輸出一個事件，外加 `_t`（收到時距送出 prompt 的秒數；存成 fixture 時拿掉）。
"""
import base64
import json
import os
import sys
import threading
import time
import urllib.request

out_path = sys.argv[1]
max_secs = float(sys.argv[2]) if len(sys.argv) > 2 else 240.0
base = "http://127.0.0.1:4096"
auth = "Basic " + base64.b64encode(("opencode:" + os.environ["PW"]).encode()).decode()


def call(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(base + path, data=data, method=method)
    req.add_header("Authorization", auth)
    req.add_header("Content-Type", "application/json")
    with urllib.request.urlopen(req, timeout=30) as resp:
        text = resp.read().decode()
        return json.loads(text) if text else None


events = urllib.request.Request(base + "/api/event")
events.add_header("Authorization", auth)
stream = urllib.request.urlopen(events, timeout=max_secs + 30)
started = [None]
session = [None]


def prompt():
    time.sleep(1)
    session[0] = call("POST", "/api/session", {})["data"]["id"]
    time.sleep(1)
    started[0] = time.monotonic()
    call("POST", f"/api/session/{session[0]}/prompt", {"text": "Reply with exactly the word OK."})


threading.Thread(target=prompt, daemon=True).start()
deadline = time.monotonic() + max_secs + 2
end_types = {"session.execution.succeeded", "session.execution.failed", "session.execution.interrupted"}
with open(out_path, "w") as out:
    for raw in stream:
        line = raw.decode().strip()
        if not line.startswith("data:"):
            if time.monotonic() > deadline:
                break
            continue
        event = json.loads(line[5:])
        t = None if started[0] is None else round(time.monotonic() - started[0], 2)
        event["_t"] = t
        out.write(json.dumps(event) + "\n")
        out.flush()
        etype = event.get("type", "")
        if etype.startswith("session.retry") or "error" in json.dumps(event.get("data", {})):
            print(f"{t}s {etype} {json.dumps(event.get('data'))[:300]}", file=sys.stderr)
        else:
            print(f"{t}s {etype}", file=sys.stderr)
        if etype in end_types:
            break
        if time.monotonic() > deadline:
            print("deadline reached; interrupting", file=sys.stderr)
            call("POST", f"/api/session/{session[0]}/interrupt")
            break
