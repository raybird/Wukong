"""2026-10-06: SCN-008 探針——按需 OpenCode 下，真正的 wukong-schedulerd 能否完成記憶合併。

用法（repo 根目錄）：
    cargo build -p wukong-schedulerd
    python3 docs/issues/issue-0009/probe-consolidate.py

模型是本機假的 OpenAI 相容 server，回覆「PROBE_SUMMARY sources=<它在 prompt 裡數到的來源筆數>」，
不呼叫外部 LLM。期望值都是字面值：40 筆來源、每批 20 筆、所以 2 筆摘要、每筆 sources=20。

對照組：`PROBE_BLANK=1` 讓模型回傳空白摘要（SCN-007），期望 40 筆來源原封不動、日誌記錄失敗。
"""
import json
import os
import pathlib
import shutil
import sqlite3
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

REPO = pathlib.Path(__file__).resolve().parents[3]
SCHEDULERD = REPO / "target/debug/wukong-schedulerd"
SCOPE = "user:tg-probe"
SOURCES = 40
BLANK = os.environ.get("PROBE_BLANK") == "1"
root = pathlib.Path(tempfile.mkdtemp(prefix="issue9-consolidate-"))
summary_prompts = []


class Model(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        user = json.dumps([m for m in request["messages"] if m["role"] == "user"][-1], ensure_ascii=False)
        if "請把以下記憶濃縮" in user:
            summary_prompts.append(user)
            content = " " if BLANK else f"PROBE_SUMMARY sources={user.count('probe-event-')}"
        else:
            content = "PROBE_OTHER"
        payload = ("data: " + json.dumps({"id": "probe", "object": "chat.completion.chunk", "created": 1,
                   "model": "probe", "choices": [{"index": 0, "delta": {"content": content}, "finish_reason": "stop"}]})
                   + "\n\ndata: [DONE]\n\n").encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def alive(pid):
    try:
        os.kill(int(pid), 0)
        return True
    except (ProcessLookupError, ValueError):
        return False


def main():
    assert SCHEDULERD.exists(), f"build first: {SCHEDULERD}"
    model = ThreadingHTTPServer(("127.0.0.1", 0), Model)
    threading.Thread(target=model.serve_forever, daemon=True).start()

    env = {k: v for k, v in os.environ.items() if not k.startswith(("WUKONG_", "OPENCODE_"))}
    for kind in ["DATA", "CONFIG", "CACHE", "STATE"]:
        env["XDG_" + kind + "_HOME"] = str(root / kind.lower())
    env["HOME"] = str(root / "home")
    (root / "home").mkdir()
    (root / "ws").mkdir()
    env["OPENCODE_CONFIG_CONTENT"] = json.dumps({"plugin": [], "snapshot": False, "model": "probe/probe",
        "compaction": {"auto": False}, "provider": {"probe": {
            "npm": "@ai-sdk/openai-compatible", "name": "Probe", "options": {
                "baseURL": f"http://127.0.0.1:{model.server_port}/v1", "apiKey": "probe"},
            "models": {"probe": {"name": "Probe", "limit": {"context": 32768, "output": 1024}}}}}})
    # 記下每個被啟動的 opencode PID，結束後檢查有沒有殘留。
    binary = shutil.which("opencode")
    (root / "bin").mkdir()
    wrapper = root / "bin/opencode"
    wrapper.write_text('#!/bin/sh\nprintf "%s\\n" "$$" >> "' + str(root / "pids") + '"\nexec "' + binary + '" "$@"\n')
    wrapper.chmod(0o755)
    env["PATH"] = str(root / "bin") + os.pathsep + env["PATH"]
    db = root / "memory.db"
    env.update({
        "WUKONG_AGENT_CMD": "opencode run",
        "WUKONG_MEMORY_DB": f"sqlite://{db}?mode=rwc",
        "WUKONG_WORKSPACE": str(root / "ws"),
        "WUKONG_SETTINGS_FILE": str(root / "settings.json"),
        "WUKONG_THINKING": "0",
    })

    # 第一次 --once 建立 schema（尚無記憶，不會合併）。
    first = subprocess.run([str(SCHEDULERD), "--once"], env=env, cwd=root, capture_output=True, text=True, timeout=120)
    assert first.returncode == 0, first.stderr
    con = sqlite3.connect(db)
    now = int(time.time())
    for i in range(SOURCES):
        text = f"User: probe-event-{i:02d} 討論第 {i} 件事"
        con.execute("INSERT INTO memories (session_id, scope, kind, text, search_text, created_at, importance)"
                    " VALUES (?, ?, 'event', ?, ?, ?, 1.0)", ("ses_probe", SCOPE, text, text, now - SOURCES + i))
    con.commit()

    started = time.time()
    run = subprocess.run([str(SCHEDULERD), "--once"], env=env, cwd=root, capture_output=True, text=True, timeout=600)
    elapsed = time.time() - started
    rows = con.execute("SELECT kind, text FROM memories WHERE scope = ? ORDER BY id", (SCOPE,)).fetchall()
    pids = (root / "pids").read_text().split() if (root / "pids").exists() else []
    time.sleep(1)
    leftover = [p for p in pids if alive(p)]
    result = {
        "exit": run.returncode,
        "elapsed_secs": round(elapsed, 1),
        "stderr_tail": run.stderr.strip().splitlines()[-5:],
        "summary_requests": len(summary_prompts),
        "rows": rows if len(rows) <= 4 else f"{len(rows)} rows, kinds={sorted({k for k, _ in rows})}",
        "opencode_spawns": len(pids),
        "leftover_pids": leftover,
    }
    print(json.dumps(result, ensure_ascii=False, indent=1))
    assert run.returncode == 0, "schedulerd failed"
    assert len(pids) >= 1, "no OpenCode process was spawned"
    assert not leftover, "OpenCode processes left running"
    if BLANK:
        assert len(summary_prompts) == 1, "blank first batch must stop this scope"
        assert [k for k, _ in rows] == ["event"] * SOURCES, "sources must survive a blank summary"
        assert "memory_consolidate_failed scope=user:tg-probe" in run.stderr, "failure not logged"
        print("SCN-007 control passed:", root)
        return
    assert len(summary_prompts) == 2, "expected two summary calls (40 sources / batch 20)"
    assert rows == [("summary", "PROBE_SUMMARY sources=20"), ("summary", "PROBE_SUMMARY sources=20")], "unexpected rows"
    print("SCN-008 probe passed:", root)


if __name__ == "__main__":
    main()
