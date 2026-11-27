"""#45 Task 6.4（TBD-5）：V2 session inbox 的 queue 行為。在容器內執行（server 在 127.0.0.1:4096，密碼在 PW）：
    python3 -I probe_inbox.py OUT_PREFIX MODEL
同一個 session 先送 prompt A（不帶 delivery），等 A 開始執行後立刻送 prompt B（delivery=queue），
B 送出後馬上列一次 inbox。事件寫到 OUT_PREFIX.ndjson，結果寫到 OUT_PREFIX.json 並印出摘要：
兩則 prompt 的回應（含 V2 記下的 delivery）、B 送出時的 inbox、事件順序、訊息順序。只送固定的合成提示詞。
"""
import json
import sys
import threading
import time
import urllib.request

sys.path.insert(0, "/p")
from v2turn import AUTH, BASE, END, call  # noqa: E402

PROMPT_A = "請用繁體中文寫一段大約 300 字的短文，主題是海邊的早晨。"
PROMPT_B = "只回覆 QUEUED 這個字，不要其他內容。"
WATCH = {"session.inbox.enqueued", "session.inbox.delivered", "session.execution.started",
         *END, "session.inbox.delivery.changed"}


def main():
    out, model = sys.argv[1], sys.argv[2]
    result = {}
    sid = call("POST", "/api/session", {"title": "probe45-inbox"})[1]["data"]["id"]
    provider, _, mid = model.partition("/")
    call("POST", f"/api/session/{sid}/model", {"model": {"providerID": provider, "id": mid}})
    stream = urllib.request.urlopen(urllib.request.Request(BASE + "/api/event", headers={"Authorization": AUTH}),
                                    timeout=300)
    t0 = [None]

    def send_a():
        time.sleep(1)
        t0[0] = time.monotonic()
        result["prompt_a"] = call("POST", f"/api/session/{sid}/prompt", {"text": PROMPT_A})

    threading.Thread(target=send_a, daemon=True).start()
    order, ends, sent_b = [], 0, False
    deadline = time.monotonic() + 280
    with open(out + ".ndjson", "w") as f:
        for raw in stream:
            line = raw.decode().strip()
            if time.monotonic() > deadline:
                result["deadline"] = True
                break
            if not line.startswith("data:"):
                continue
            event = json.loads(line[5:])
            data = event.get("data") or {}
            if data.get("sessionID") != sid:
                continue
            t = None if t0[0] is None else round(time.monotonic() - t0[0], 2)
            event["_t"] = t
            f.write(json.dumps(event, ensure_ascii=False) + "\n")
            etype = event.get("type", "")
            if etype in WATCH:
                item = data.get("item") or {}
                text = ((item.get("payload") or {}).get("text") or "")[:12]
                order.append([t, etype, item.get("delivery"), text])
            if etype == "session.execution.started" and not sent_b:
                sent_b = True
                result["prompt_b"] = call("POST", f"/api/session/{sid}/prompt",
                                          {"text": PROMPT_B, "delivery": "queue"})
                result["inbox_after_b"] = call("GET", f"/api/session/{sid}/inbox")
                order.append([round(time.monotonic() - t0[0], 2), "(prompt B 送出、列 inbox)", None, None])
            if etype in END:
                ends += 1
                if ends >= 2:  # queue：A 與 B 各一次執行；B 若併進 A 的執行，就只有一次，等到 deadline
                    break
    result["order"] = order
    result["inbox_at_end"] = call("GET", f"/api/session/{sid}/inbox")
    msgs = call("GET", f"/api/session/{sid}/message")[1]
    result["messages"] = msgs
    summary = []
    for m in (msgs or {}).get("data", []):
        texts = []
        for key in ("text",):
            def walk(v):
                if isinstance(v, dict):
                    for k, x in v.items():
                        if k == key and isinstance(x, str):
                            texts.append(x)
                        else:
                            walk(x)
                elif isinstance(v, list):
                    for x in v:
                        walk(x)
            walk(m)
        summary.append([m.get("type"), (" ".join(texts))[:40]])
    result["message_order"] = summary
    with open(out + ".json", "w") as f:
        json.dump(result, f, ensure_ascii=False, indent=1)
    show = {
        "prompt_a": [result["prompt_a"][0], (result["prompt_a"][1] or {}).get("data", {}).get("delivery")],
        "prompt_b": [result["prompt_b"][0], (result["prompt_b"][1] or {}).get("data", {}).get("delivery")]
        if "prompt_b" in result else None,
        "inbox_after_b": [[i.get("type"), i.get("delivery")] for i in (result.get("inbox_after_b", [0, {}])[1] or {}).get("data", [])],
        "inbox_at_end": len((result["inbox_at_end"][1] or {}).get("data", [])),
        "order": order,
        "message_order": summary,
    }
    print(json.dumps(show, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
