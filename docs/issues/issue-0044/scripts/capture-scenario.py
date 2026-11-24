"""Task 1.3：在跑著 V2 `opencode serve` 的容器裡執行一個情境，把這段期間的 /api/event 事件印到 stdout。

由 capture-events.sh 以 `docker exec -e PW=... 容器 python3 -I /scripts/capture-scenario.py 情境` 呼叫。
每行一個事件的 JSON；進度與判定印到 stderr，最後一行是 PASS／FAIL 摘要。
只送固定的合成提示詞，不帶任何專案或使用者資料。
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
        with urllib.request.urlopen(req, timeout=30) as r:
            raw = r.read()
            return r.status, json.loads(raw) if raw else None
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()[:300]


def listen():
    req = urllib.request.Request(BASE + "/api/event", headers={"Authorization": AUTH})
    with urllib.request.urlopen(req) as r:
        for line in r:
            line = line.decode().strip()
            if line.startswith("data:"):
                try:
                    ev = json.loads(line[5:].strip())
                except ValueError:
                    continue
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


def of_type(*names):
    return lambda e: e.get("type") in names


def new_session(**extra):
    status, body = call("POST", "/api/session", {"title": "issue-0044-capture", **extra})
    assert status == 200, (status, body)
    return body["data"]["id"]


def prompt(sid, text):
    status, body = call("POST", f"/api/session/{sid}/prompt", {"text": text})
    assert status == 200, (status, body)


def done(ok, note):
    with lock:
        for ev in events:
            print(json.dumps(ev, ensure_ascii=False))
    print(("PASS " if ok else "FAIL ") + note, file=sys.stderr)
    sys.exit(0 if ok else 1)


threading.Thread(target=listen, daemon=True).start()
time.sleep(1)
name = sys.argv[1]
finished = of_type("session.execution.succeeded", "session.execution.failed", "session.execution.interrupted")

if name == "text-tool":
    sid = new_session()
    prompt(sid, "Use the shell tool to run exactly: echo probe. Then reply with exactly the word DONE.")
    end = wait_for(finished, 120)
    tool = wait_for(of_type("session.tool.success"), 1)
    done(bool(end and tool and end["type"] == "session.execution.succeeded"), f"end={end and end['type']} tool={bool(tool)}")

elif name == "permission":
    sid = new_session(permissions=[{"action": "shell", "resource": "*", "effect": "ask"}])
    prompt(sid, "Use the shell tool to run exactly: echo probe. Then reply with exactly the word DONE.")
    asked = wait_for(of_type("permission.asked"), 120)
    if not asked:
        done(False, "no permission.asked")
    status, pending = call("GET", f"/api/session/{sid}/permission")
    rid = pending["data"][0]["id"]
    status, _ = call("POST", f"/api/session/{sid}/permission/{rid}/reply", {"decision": "once"})
    end = wait_for(finished, 120)
    done(bool(end and end["type"] == "session.execution.succeeded"), f"reply={status} end={end and end['type']}")

elif name == "form":
    sid = new_session()
    prompt(sid, "Use the question tool to ask me to choose between the options A and B. "
                "After I answer, reply with only the letter I chose.")
    created = wait_for(of_type("form.created"), 120)
    if not created:
        done(False, "no form.created")
    status, forms = call("GET", f"/api/session/{sid}/form")
    form = forms["data"][0]
    field = form["fields"][0] if isinstance(form.get("fields"), list) else next(iter(form.get("fields", {}).values()))
    key = field.get("id") or field.get("key") or field.get("name")
    options = field.get("options") or []
    value = options[0].get("value", options[0].get("label")) if options else "A"
    if field.get("type") == "multiselect" or field.get("multiple"):
        value = [value]
    status, body = call("POST", f"/api/session/{sid}/form/{form['id']}/reply", {"answer": {key: value}})
    end = wait_for(finished, 120)
    done(bool(end and end["type"] == "session.execution.succeeded"), f"reply={status} {str(body)[:120]} end={end and end['type']}")

elif name == "interrupt":
    sid = new_session()
    prompt(sid, "Count from 1 to 400, one number per line.")
    wait_for(of_type("session.text.delta"), 120)
    status, body = call("POST", f"/api/session/{sid}/interrupt")
    end = wait_for(finished, 30)
    done(bool(end and end["type"] == "session.execution.interrupted"), f"interrupt={status} end={end and end['type']}")

elif name == "no-route":
    sid = new_session()
    status, _ = call("POST", f"/api/session/{sid}/model", {"model": {"id": "nope", "providerID": "missing"}})
    prompt(sid, "Reply with exactly the word OK.")
    end = wait_for(finished, 60)
    done(bool(end and end["type"] == "session.execution.failed"), f"model={status} end={end and end['type']}")

elif name == "retry":
    sid = new_session()
    prompt(sid, "Reply with exactly the word OK.")
    retry = wait_for(of_type("session.retry.scheduled"), 60)
    second = wait_for(lambda e: e.get("type") == "session.retry.scheduled" and e is not retry, 30)
    status, _ = call("POST", f"/api/session/{sid}/interrupt")
    end = wait_for(finished, 30)
    done(bool(retry and end), f"retry={bool(retry)} second={bool(second)} interrupt={status} end={end and end['type']}")

else:
    done(False, f"unknown scenario {name}")
