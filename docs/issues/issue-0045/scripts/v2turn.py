"""#45 Phase 1 探測客戶端：對跑著 V2 `opencode serve` 的容器送一個回合，記下事件與結果。

在容器內執行（server 在 127.0.0.1:4096，密碼在環境變數 PW）：
    python3 -I v2turn.py OUT_PREFIX PROMPT [--agent NAME] [--session ID] [--model provider/model]
                         [--skill ID]... [--max-secs N]
不帶 --session 時先建一個新的 session。先開事件串流再送 prompt，事件寫到 OUT_PREFIX.ndjson
（每行一個事件，外加 `_t`：距送出 prompt 的秒數），結束後把以下內容寫到 OUT_PREFIX.json：
session 的狀態（含 agent）、訊息列表、事件裡出現過的 agent／model 欄位，以及最後的回覆文字。
只送呼叫端給的合成提示詞。
"""
import argparse
import base64
import json
import os
import threading
import time
import urllib.error
import urllib.request

BASE = "http://127.0.0.1:4096"
AUTH = "Basic " + base64.b64encode(("opencode:" + os.environ["PW"]).encode()).decode()
END = {"session.execution.succeeded", "session.execution.failed", "session.execution.interrupted"}


def call(method, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=data, method=method,
                                 headers={"Authorization": AUTH, "content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            raw = resp.read().decode()
            return resp.status, (json.loads(raw) if raw else None)
    except urllib.error.HTTPError as err:
        raw = err.read().decode()
        try:
            return err.code, json.loads(raw)
        except ValueError:
            return err.code, raw


def walk(value, keys, found):
    """收集巢狀結構裡指定鍵的所有值（agent、model 這類欄位出現的位置在 V2 文件裡沒有寫明）。"""
    if isinstance(value, dict):
        for k, v in value.items():
            if k in keys and isinstance(v, (str, dict)):
                found.setdefault(k, [])
                text = json.dumps(v, sort_keys=True) if isinstance(v, dict) else v
                if text not in found[k]:
                    found[k].append(text)
            walk(v, keys, found)
    elif isinstance(value, list):
        for v in value:
            walk(v, keys, found)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("prompt")
    ap.add_argument("--agent")
    ap.add_argument("--session")
    ap.add_argument("--model")
    ap.add_argument("--skill", action="append", default=[], help="附在 prompt 的 skills（可重複）")
    ap.add_argument("--max-secs", type=float, default=240.0)
    args = ap.parse_args()

    result = {"steps": []}
    sid = args.session
    if not sid:
        status, body = call("POST", "/api/session", {"title": "probe45"})
        result["steps"].append(["create_session", status])
        sid = body["data"]["id"]
    result["session"] = sid
    if args.agent:
        status, body = call("POST", f"/api/session/{sid}/agent", {"agent": args.agent})
        result["steps"].append(["select_agent", status, body])
    if args.model:
        provider, _, model = args.model.partition("/")
        status, body = call("POST", f"/api/session/{sid}/model",
                            {"model": {"providerID": provider, "id": model}})
        result["steps"].append(["select_model", status, body])

    events_req = urllib.request.Request(BASE + "/api/event", headers={"Authorization": AUTH})
    stream = urllib.request.urlopen(events_req, timeout=args.max_secs + 30)
    started = [None]

    def send():
        time.sleep(1)
        started[0] = time.monotonic()
        prompt = {"text": args.prompt}
        if args.skill:
            prompt["skills"] = [{"id": s} for s in args.skill]
        status, body = call("POST", f"/api/session/{sid}/prompt", prompt)
        result["steps"].append(["prompt", status, body])

    threading.Thread(target=send, daemon=True).start()
    deadline = time.monotonic() + args.max_secs + 2
    end_type = None
    found = {}
    with open(args.out + ".ndjson", "w") as out:
        for raw in stream:
            line = raw.decode().strip()
            if time.monotonic() > deadline:
                call("POST", f"/api/session/{sid}/interrupt")
                end_type = "deadline"
                break
            if not line.startswith("data:"):
                continue
            event = json.loads(line[5:])
            event["_t"] = None if started[0] is None else round(time.monotonic() - started[0], 2)
            out.write(json.dumps(event) + "\n")
            walk(event, {"agent", "agentID", "subagent", "model", "modelID"}, found)
            etype = event.get("type", "")
            data = event.get("data") or {}
            if etype in END and data.get("sessionID", sid) == sid:
                end_type = etype
                break

    result["end"] = end_type
    result["event_fields"] = found
    result["session_state"] = call("GET", f"/api/session/{sid}")[1]
    status, messages = call("GET", f"/api/session/{sid}/message")
    result["messages_status"] = status
    result["messages"] = messages
    texts = []
    walk(messages, {"text"}, texts_holder := {})
    result["texts"] = texts_holder.get("text", [])
    with open(args.out + ".json", "w") as out:
        json.dump(result, out, ensure_ascii=False, indent=1)
    print(json.dumps({"session": sid, "end": end_type, "steps": [s[:2] for s in result["steps"]],
                      "agent_fields": found.get("agent", []) + found.get("agentID", []),
                      "last_texts": result["texts"][-2:]}, ensure_ascii=False))


if __name__ == "__main__":
    main()
