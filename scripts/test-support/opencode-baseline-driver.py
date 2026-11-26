"""在容器裡驗證 entrypoint 寫的 opencode 設定 baseline（issue-0044 Task 3.1，SCN-009）。

由 scripts/test-opencode-baseline.sh 經映像的 entrypoint 以 wukong 使用者啟動，所以
opencode.json、user.json 與 OPENCODE_CONFIG 都是 entrypoint 實際寫出的結果。

工具呼叫由本檔內建的假模型（OpenAI 相容，只綁 127.0.0.1）發出：第一次請求要求呼叫指定的
工具，拿到工具結果後回 DONE。opencode 照真實流程解析設定、比對權限、執行工具，不連外部模型。
"""
import json
import os
import shutil
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

CONFIG_DIR = "/home/wukong/.config/opencode"
BASELINE = os.path.join(CONFIG_DIR, "opencode.json")
USER = os.environ["OPENCODE_CONFIG"]
WORKSPACE = os.environ.get("WUKONG_WORKSPACE", "/workspace")
PORT = 18080
# 只加一個 provider，不帶任何 permissions，不影響要驗證的規則。
PROVIDER = json.dumps({"provider": {"fake": {
    "npm": "@ai-sdk/openai-compatible", "name": "Fake",
    "options": {"baseURL": f"http://127.0.0.1:{PORT}/v1", "apiKey": "unused"},
    "models": {"m": {"name": "m", "tool_call": True}}}}})

call = {}


class FakeModel(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("content-length", 0))) or b"{}")
        names = [(t.get("function") or t).get("name") for t in body.get("tools") or []]
        answered = any(m.get("role") == "tool" for m in body.get("messages", []))
        if names and not answered:
            delta = {"role": "assistant", "tool_calls": [{
                "index": 0, "id": "call_1", "type": "function",
                "function": {"name": call["name"], "arguments": json.dumps(call["arguments"])}}]}
            finish = "tool_calls"
        else:
            delta, finish = {"role": "assistant", "content": "DONE"}, "stop"
        chunks = [{"choices": [{"index": 0, "delta": delta, "finish_reason": None}]},
                  {"choices": [{"index": 0, "delta": {}, "finish_reason": finish}],
                   "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}}]
        self.send_response(200)
        self.send_header("content-type", "text/event-stream")
        self.end_headers()
        for chunk in chunks:
            chunk.update(id="c1", object="chat.completion.chunk", created=0, model="m")
            self.wfile.write(f"data: {json.dumps(chunk)}\n\n".encode())
        self.wfile.write(b"data: [DONE]\n\n")


def run_tool(name, arguments, *flags):
    """讓 opencode 執行一次工具呼叫，回傳 (status, 輸出或錯誤)。"""
    call.update(name=name, arguments=arguments)
    out = subprocess.run(
        ["opencode", "run", "--standalone", *flags, "-m", "fake/m", "--format", "json", "go"],
        env={**os.environ, "OPENCODE_CONFIG_CONTENT": PROVIDER}, cwd=WORKSPACE,
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=120)
    for line in out.stdout.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        part = event.get("part") or {}
        if event.get("type") == "tool_use" and part.get("id") == "call_1":
            state = part["state"]
            return state["status"], state.get("output") or state.get("error") or ""
    return "no-tool-call", f"rc={out.returncode} stderr={out.stderr[-600:]}"


def shell(command, *flags):
    return run_tool("shell", {"command": command, "description": "baseline probe"}, *flags)


def write_json(path, value):
    with open(path, "w") as fh:
        json.dump(value, fh, indent=2)


original_baseline = open(BASELINE).read()
original_user = open(USER).read()


def with_baseline(edit):
    """暫時改寫 baseline 的 permissions（反向自檢用）；呼叫端負責 restore()。"""
    config = json.loads(original_baseline)
    config["permissions"] = edit(config.get("permissions", []))
    write_json(BASELINE, config)


def restore():
    open(BASELINE, "w").write(original_baseline)
    open(USER, "w").write(original_user)


results = []


def check(name, ok, detail):
    results.append(ok)
    print(f"{'PASS' if ok else 'FAIL'} {name}: {detail}", flush=True)


def victim(path):
    os.makedirs(path, exist_ok=True)
    open(os.path.join(path, "file"), "w").write("x")
    return path


def main():
    threading.Thread(
        target=ThreadingHTTPServer(("127.0.0.1", PORT), FakeModel).serve_forever,
        daemon=True).start()

    # 1. V2 讀進的 baseline 與檔案內容一致：沒有被當成舊格式轉換，也沒有欄位被默默丟掉。
    out = subprocess.run(["opencode", "debug", "config"], cwd=WORKSPACE,
                         capture_output=True, text=True, timeout=60)
    sources = json.loads(out.stdout) if out.returncode == 0 else []
    parsed = next((s.get("info") for s in sources if s.get("path") == BASELINE), None)
    written = json.loads(original_baseline)
    lost = sorted(k for k in written if parsed is None or parsed.get(k) != written[k])
    check("baseline read intact", out.returncode == 0 and not lost,
          f"rc={out.returncode} keys V2 changed or dropped: {lost or 'none'}"
          + (f" (V2 read {json.dumps({k: parsed.get(k) for k in lost})})" if parsed and lost else ""))
    check("user.json is a config layer", any(s.get("path") == USER for s in sources), USER)

    # 2. 對絕對路徑的遞迴刪除被拒；CLI backend 的旗標也一樣。
    for flags in ([], ["--dangerously-skip-permissions"]):
        target = victim("/tmp/victim")
        status, detail = shell(f"rm -rf {target}", *flags)
        check(f"absolute rm -rf denied {flags or ''}",
              status == "error" and os.path.exists(target), f"{status}: {detail[:120]}")

    # 反向自檢：拿掉 deny 規則，同一命令會通過。
    with_baseline(lambda rules: [r for r in rules if r.get("effect") != "deny"])
    target = victim("/tmp/victim")
    status, detail = shell(f"rm -rf {target}")
    check("reverse: same rm passes without the deny rules",
          status == "completed" and not os.path.exists(target), f"{status}: {detail[:120]}")
    restore()

    # 3. /workspace 內的刪除允許。
    target = victim(os.path.join(WORKSPACE, "victim"))
    status, detail = shell(f"rm -rf {target}")
    check("rm -rf inside /workspace allowed",
          status == "completed" and not os.path.exists(target), f"{status}: {detail[:120]}")

    # 4. 存取 /tmp 不詢問（非互動的 run 遇到詢問會直接拒絕）。
    open("/tmp/probe.txt", "w").write("TMP_OK\n")
    status, detail = run_tool("read", {"path": "/tmp/probe.txt"})
    check("read /tmp without asking", status == "completed" and "TMP_OK" in detail,
          f"{status}: {detail[:120]}")
    # 反向自檢：/tmp 的規則改成 ask 時，同一讀取會被擋，證明這個檢查抓得到詢問。
    with_baseline(lambda rules: [
        {**r, "effect": "ask"} if r.get("action") == "external_directory" else r for r in rules])
    status, detail = run_tool("read", {"path": "/tmp/probe.txt"})
    check("reverse: the same read is blocked when /tmp asks", status == "error",
          f"{status}: {detail[:120]}")
    restore()

    # 5. user.json 的鍵覆蓋 baseline：V2 寫法、沿用至今的 V1 寫法、非權限的鍵。
    for label, user in [
        ("V2 permissions", {"permissions": [
            {"action": "shell", "resource": "rm -rf /tmp/victim", "effect": "allow"}]}),
        ("V1 permission", {"permission": {"bash": {"rm -rf /tmp/victim": "allow"}}}),
    ]:
        write_json(USER, user)
        target = victim("/tmp/victim")
        status, detail = shell(f"rm -rf {target}")
        check(f"user.json {label} overrides the deny",
              status == "completed" and not os.path.exists(target), f"{status}: {detail[:120]}")
        restore()

    def numbers(text):
        return sum(line.strip().isdigit() for line in text.splitlines())

    status, detail = shell("seq 1 50")
    full = status == "completed" and numbers(detail) == 50
    write_json(USER, {"tool_output": {"max_lines": 3}})
    status, cut = shell("seq 1 50")
    check("user.json tool_output overrides the baseline",
          full and status == "completed" and numbers(cut) < 50,
          f"baseline shows {numbers(detail)} of 50 lines, with user.json max_lines=3 "
          f"{status} shows {numbers(cut)}: {cut[-160:]!r}")
    restore()

    failed = results.count(False)
    print(f"{len(results) - failed} passed, {failed} failed", flush=True)
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    try:
        main()
    finally:
        restore()
        shutil.rmtree("/tmp/victim", ignore_errors=True)
