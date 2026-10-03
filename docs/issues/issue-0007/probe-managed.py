"""2026-10-03: 真實 OpenCode 每回合問答、續接與退出探針。"""
import json
import os
import pathlib
import signal
import shutil
import socket
import sys
import subprocess
import tempfile
import threading
import time
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

root = pathlib.Path(tempfile.mkdtemp(prefix="issue7-managed-"))


class Model(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        (root / "model-request.json").write_text(json.dumps(request, ensure_ascii=False))
        messages = request["messages"]
        last_user = max(i for i, m in enumerate(messages) if m["role"] == "user")
        tools = [m for m in messages[last_user + 1:] if m["role"] == "tool"]
        user = json.dumps(messages[last_user], ensure_ascii=False)
        current_input = user.split("[使用者輸入]")[-1]
        if "<role>|<skill-or-none>" in user:
            delta = {"content": "fixer|none"}
            finish = "stop"
        elif "[修復回覆]" in user:
            delta = {"content": "QUESTION_CANCELLED"}
            finish = "stop"
        elif "SERVER_PROBE" in current_input:
            delta = {"content": "SHARED_SERVER_OK"}
            finish = "stop"
        elif not request.get("tools"):
            delta = {"content": "MANAGED_COMPACT_SUMMARY"}
            finish = "stop"
        elif tools:
            delta = {"content": json.dumps(tools, ensure_ascii=False) + (" HISTORY_PRESENT" if sum(m["role"] == "user" for m in messages) > 1 else " NO_HISTORY")}
            finish = "stop"
        else:
            delta = {"tool_calls": [{"index": 0, "id": "call_question", "type": "function", "function": {
                "name": "question", "arguments": json.dumps({"questions": [{
                    "question": "選哪個？", "header": "探針", "options": [
                        {"label": "A", "description": "第一個"},
                        {"label": "B", "description": "第二個"}], "multiple": False}]})}}]}
            finish = "tool_calls"
            if "MULTI_PROBE" in current_input:
                delta["tool_calls"][0]["function"]["arguments"] = json.dumps({"questions": [
                    {"question": "選哪個？", "header": "多選", "options": [{"label": "A", "description": "第一個"},
                     {"label": "B", "description": "第二個"}], "multiple": True},
                    {"question": "補充？", "header": "補充", "options": [{"label": "A", "description": "預設"}], "custom": True}]})
            if "PERMISSION_PROBE" in current_input:
                delta["tool_calls"][0]["function"] = {"name": "bash", "arguments": json.dumps({
                    "command": "printf MANAGED_PERMISSION_OK > " + str(root / "permission-marker"),
                    "workdir": str(root), "description": "permission probe"})}
        payload = ("data: " + json.dumps({"id": "probe", "object": "chat.completion.chunk", "created": 1,
                   "model": "probe", "choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})
                   + "\n\ndata: [DONE]\n\n").encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


model = ThreadingHTTPServer(("127.0.0.1", 0), Model)
threading.Thread(target=model.serve_forever, daemon=True).start()
env = os.environ.copy()
for kind in ["DATA", "CONFIG", "CACHE", "STATE"]:
    env["XDG_" + kind + "_HOME"] = str(root / kind.lower())
env["PWD"] = str(root)
env.pop("OPENCODE_CONFIG", None)
env["OPENCODE_SERVER_PASSWORD"] = "fixture-standard-password-must-not-leak-to-local"
env["OPENCODE_CONFIG_CONTENT"] = json.dumps({"plugin": [], "snapshot": False, "model": "probe/probe",
    "compaction": {"auto": False}, "permission": {"question": "allow", "bash": "ask",
        "external_directory": {str(root): "allow", str(root) + "/*": "allow"}}, "provider": {"probe": {
        "npm": "@ai-sdk/openai-compatible", "name": "Probe", "options": {
            "baseURL": f"http://127.0.0.1:{model.server_port}/v1", "apiKey": "probe"},
        "models": {"probe": {"name": "Probe", "limit": {"context": 8192, "output": 1024}}}}}})
binary = shutil.which("opencode")
(root / "bin").mkdir()
wrapper = root / "bin/opencode"
wrapper.write_text('#!/bin/sh\nprintf "%s\\n" "$$" >> "' + str(root / "pids") + '"\nexec "' + binary + '" "$@"\n')
wrapper.chmod(0o755)
env["PATH"] = str(root / "bin") + os.pathsep + env["PATH"]


ports = set()
tracked_pids = set()
watch_stop = threading.Event()
def watch_ports():
    while not watch_stop.wait(.02):
        try:
            pids = (root / "pids").read_text().splitlines()
        except FileNotFoundError:
            continue
        descendants = list(pids)
        for pid in descendants:
            try:
                descendants.extend(pathlib.Path(f"/proc/{pid}/task/{pid}/children").read_text().split())
            except (FileNotFoundError, ProcessLookupError):
                pass
        tracked_pids.update(descendants)
        for pid in descendants:
            try:
                sockets = {os.readlink(fd) for fd in pathlib.Path("/proc/" + pid + "/fd").iterdir()}
                for protocol in ["tcp", "tcp6"]:
                    for row in pathlib.Path("/proc/" + pid + "/net/" + protocol).read_text().splitlines()[1:]:
                        fields = row.split()
                        if fields[3] == "0A" and "socket:[" + fields[9] + "]" in sockets:
                            ports.add(int(fields[1].split(":")[1], 16))
            except (FileNotFoundError, ProcessLookupError, PermissionError):
                continue
watcher = threading.Thread(target=watch_ports, daemon=True)
watcher.start()

def assert_closed_ports():
    watch_stop.set()
    watcher.join(timeout=1)
    assert ports, "no listening OpenCode port was observed"
    remaining = [pid for pid in tracked_pids if pathlib.Path("/proc/" + pid).exists()]
    assert not remaining, "descendant processes still alive: " + str(remaining)
    for port in ports:
        with socket.socket() as connection:
            assert connection.connect_ex(("127.0.0.1", port)) != 0, "port still open: " + str(port)
    return len(ports)


def api(port, path, body=None):
    request = urllib.request.Request(f"http://127.0.0.1:{port}" + path,
        data=None if body is None else json.dumps(body).encode(),
        headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=2) as response:
        content = response.read()
        return json.loads(content) if content else None


session = None
try:
    if "--binary" in sys.argv:
        import sqlite3
        cli = sys.argv[sys.argv.index("--binary") + 1]
        env["WUKONG_MEMORY_DB"] = "sqlite://" + str(root / "binary-memory.db")
        env["WUKONG_WORKSPACE"] = str(root)
        env["WUKONG_AGENT_CMD"] = "opencode run"
        env["WUKONG_AGENT_TIMEOUT_SECS"] = "8"
        env.pop("WUKONG_AGENT_SERVER_URL", None)
        versions = subprocess.check_output([binary, "--version"], env=env, text=True).strip()
        session = None
        for input_text, flags, expected, prompt in [("1\n", [], "A", "QUESTION_PROBE"), ("自己的答案\n", ["--no-stream"], "自己的答案", "QUESTION_PROBE"),
                                            ("/cancel\n", [], "QUESTION_CANCELLED", "QUESTION_PROBE"), ("", [], "QUESTION_CANCELLED", "QUESTION_PROBE"),
                                            ("\n1,2\n補充文字\n", [], "補充文字", "MULTI_PROBE")]:
            result = subprocess.run([cli, "--scope", "project:issue7-docker", "--no-thinking"] + flags + [prompt],
                                    input=input_text, text=True, capture_output=True, env=env, cwd=root, timeout=20)
            assert result.returncode == 0 and expected in result.stdout, (result.stdout, result.stderr)
            assert "選哪個？" in result.stderr, result.stderr
            if prompt == "MULTI_PROBE":
                assert "A, B" in result.stdout and "請輸入有效選項" in result.stderr, (result.stdout, result.stderr)
            with sqlite3.connect(root / "binary-memory.db") as db:
                current = db.execute("SELECT session_id FROM agent_sessions WHERE scope='project:issue7-docker'").fetchone()[0]
            assert session is None or current == session, (session, current)
            session = current
        result = subprocess.run([cli, "--scope", "project:issue7-repl", "--no-thinking"],
                                input="QUESTION_PROBE\n1\nQUESTION_PROBE\n2\n/exit\n", text=True,
                                capture_output=True, env=env, cwd=root, timeout=25)
        assert result.returncode == 0 and result.stdout.count("User has answered your questions") == 2, (result.stdout, result.stderr)
        env["WUKONG_AGENT_TIMEOUT_SECS"] = "4"
        child = subprocess.Popen([cli, "--scope", "project:issue7-timeout", "--no-thinking", "QUESTION_PROBE"],
                                 env=env, cwd=root, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        # Keep stdin open without answering: Tokio must still honor the turn deadline.
        child.wait(timeout=12)
        child.stdin.close()
        assert child.returncode != 0 and "逾時" in child.stderr.read().decode()
        env.pop("OPENCODE_SERVER_PASSWORD", None)
        shared = subprocess.Popen(["opencode", "serve", "--hostname", "127.0.0.1", "--port", "0"],
                                  env=env, cwd=root, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL, text=True, start_new_session=True)
        try:
            while True:
                line = shared.stdout.readline()
                assert line, "shared server did not start"
                if line.startswith("opencode server listening on "):
                    env["WUKONG_AGENT_SERVER_URL"] = line.strip().split(" on ")[1]
                    break
            env["WUKONG_AGENT_TIMEOUT_SECS"] = "8"
            env["WUKONG_AGENT_CMD"] = "command-that-must-not-run"
            for _ in range(2):
                result = subprocess.run([cli, "--scope", "project:issue7-shared", "--no-thinking", "SERVER_PROBE"],
                                        input="", text=True, capture_output=True, env=env, cwd=root, timeout=20)
                assert result.returncode == 0 and "SHARED_SERVER_OK" in result.stdout, (result.stdout, result.stderr)
                assert shared.poll() is None
        finally:
            shared.send_signal(signal.SIGINT)
            try:
                shared.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(shared.pid, signal.SIGKILL)
                shared.wait(timeout=3)
        time.sleep(.2)
        pids = (root / "pids").read_text().splitlines()
        assert pids and all(not pathlib.Path("/proc/" + pid).exists() for pid in pids), pids
        print(json.dumps({"root": str(root), "opencode": versions, "session": session, "cli_rounds": 5,
                          "repl_rounds": 2, "timeout": True, "shared_server_rounds": 2, "launched": len(pids), "remaining": 0, "closed_ports": assert_closed_ports(), "tracked_processes": len(tracked_pids), "ports": sorted(ports)}), flush=True)
        sys.exit(0)
    if "--gateway-tests" in sys.argv:
        env["WUKONG_ISSUE7_MANAGED_ROOT"] = str(root)
        env.pop("WUKONG_AGENT_SERVER_URL", None)
        packages = ["wukong-cli"] if "--cli" in sys.argv else ["wukong-gateway"]
        if "--all-entrances" in sys.argv:
            packages = ["wukong-gateway", "wukong-cli", "wukong-web", "wukong-telegram", "wukong-scheduler"]
        result = subprocess.run(["cargo", "test"] + sum((["-p", p] for p in packages), []) + ["--test", "managed_actual", "--", "--ignored", "--nocapture", "--test-threads=1"],
            cwd=pathlib.Path(__file__).resolve().parents[3], env=env, timeout=120)
        assert result.returncode == 0
        pids = (root / "pids").read_text().splitlines()
        assert pids, "no real OpenCode processes were launched"
        for pid in pids:
            assert not pathlib.Path("/proc/" + pid).exists(), "process still alive: " + pid
        print(json.dumps({"root": str(root), "launched": len(pids), "remaining": 0, "closed_ports": assert_closed_ports(), "tracked_processes": len(tracked_pids), "ports": sorted(ports)}), flush=True)
        sys.exit(0)
    env.pop("OPENCODE_SERVER_PASSWORD", None)
    for answer in ["A", None, "自己的答案"]:
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            port = listener.getsockname()[1]
        stderr = open(root / f"stderr-{port}", "w")
        child = subprocess.Popen(["opencode", "serve", "--hostname", "127.0.0.1", "--port", str(port)],
            cwd=root, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
            stderr=stderr, start_new_session=True)
        try:
            deadline = time.monotonic() + 15
            while True:
                try:
                    api(port, "/global/health")
                    break
                except (OSError, ValueError):
                    assert time.monotonic() < deadline, "startup timed out"
                    time.sleep(.05)
            if session is None:
                session = api(port, "/session", {})["id"]
            else:
                assert api(port, "/session/" + session)["id"] == session
            api(port, "/session/" + session + "/prompt_async", {"parts": [{"type": "text", "text": "問我問題"}]})
            while True:
                questions = [q for q in api(port, "/question") if q["sessionID"] == session]
                if questions:
                    break
                assert time.monotonic() < deadline, "question timed out"
                time.sleep(.05)
            question = questions[0]
            assert question["questions"][0]["question"] == "選哪個？"
            api(port, "/question/" + question["id"] + ("/reject" if answer is None else "/reply"),
                {} if answer is None else {"answers": [[answer]]})
            while True:
                messages = api(port, "/session/" + session + "/message")
                texts = [p["text"] for p in messages[-1]["parts"] if p["type"] == "text"]
                errors = [p["state"]["error"] for p in messages[-1]["parts"]
                          if p["type"] == "tool" and p["state"]["status"] == "error"]
                if answer is None and errors:
                    texts = errors
                    break
                if texts:
                    break
                assert time.monotonic() < deadline, "answer timed out"
                time.sleep(.05)
            text = "".join(texts)
            assert (answer in text) if answer is not None else ("dismissed" in text.lower() or "rejected" in text.lower()), text
            rss = next(line.strip() for line in pathlib.Path(f"/proc/{child.pid}/status").read_text().splitlines() if line.startswith("VmRSS:"))
            print(json.dumps({"pid": child.pid, "session": session, "answer": answer, "text": text, "rss": rss}, ensure_ascii=False), flush=True)
        finally:
            child.send_signal(signal.SIGINT)
            try:
                child.wait(timeout=3)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL)
                child.wait(timeout=3)
            stderr.close()
            assert not pathlib.Path(f"/proc/{child.pid}").exists()
            with socket.socket() as connection:
                assert connection.connect_ex(("127.0.0.1", port)) != 0
            print(json.dumps({"pid": child.pid, "stopped": True, "port_closed": port}), flush=True)
finally:
    watch_stop.set()
    model.shutdown()
    model.server_close()
