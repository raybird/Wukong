#!/usr/bin/env bash
# #45 Task 6.1（SCN-003）派工門檻：固定的六題各跑 RUNS 次（預設 3），記下每次派給誰、用的模型，再以
# dispatch_judge.py 判定。真實的 wukong 入口（常駐 server 模式）接容器裡的 V2 2.3.2。
# 用法：dispatch-eval.sh IMAGE WORK_DIR AUTH_JSON MODEL [RUNS] [TASKS]
#   TASKS：只跑其中幾題（空白分隔，例如 "implement"），用來重跑被上游限流打斷的題目；這時不做判定，
#   由呼叫端把結果併回完整的一輪再判定。
# 在 repo 根目錄執行。只送固定的合成提示詞。HOME、WUKONG_WORKSPACE、記憶庫、設定檔都在 WORK_DIR。
# 每次回合前把工作區重置成同一份種子專案（rsync --delete，保留 .opencode 與 .wukong），並用新的 scope 與
# 新的記憶庫，前一題的檔案與記憶都不會影響下一題。派工取自 opencode.db 的 session_v2（parent_id 是本回合的
# primary session），模型與 primary 呼叫的工具取自 session_message 的 assistant 訊息。
set -uo pipefail
image="${1:?usage: dispatch-eval.sh IMAGE WORK_DIR AUTH_JSON MODEL [RUNS]}"
work="$(realpath "${2:?}")"
auth="${3:?}" model="${4:?}" runs="${5:-3}" tasks="${6:-implement explore lookup architecture design direct}"
here="$(cd "$(dirname "$0")" && pwd)"
repo="$(pwd)"
box="probe45-eval-$$"
pw="probe-$$"
cleanup() { docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

ws="$work/ws" seed="$work/seed"
mkdir -p "$work/home/.local/share/opencode" "$ws" "$work/out" "$work/mem" "$seed/sample/tests" "$seed/sample/web"
install -m 600 "$auth" "$work/home/.local/share/opencode/auth.json"
printf '{"agent":{"default_model":"%s"}}\n' "$model" > "$work/home/settings.json"
cat > "$seed/sample/models.py" << 'PY'
from dataclasses import dataclass


@dataclass
class Todo:
    title: str
    done: bool = False
PY
cat > "$seed/sample/store.py" << 'PY'
import json
from pathlib import Path

from models import Todo

PATH = Path("todos.json")


def load():
    if not PATH.exists():
        return []
    return [Todo(**item) for item in json.loads(PATH.read_text())]


def save(todos):
    PATH.write_text(json.dumps([t.__dict__ for t in todos]))
PY
cat > "$seed/sample/render.py" << 'PY'
from models import Todo


def render(todos: list[Todo]) -> str:
    return "\n".join(f"[{'x' if t.done else ' '}] {t.title}" for t in todos)
PY
cat > "$seed/sample/app.py" << 'PY'
import sys

import render
import store
from models import Todo


def main():
    todos = store.load()
    if len(sys.argv) > 1:
        todos.append(Todo(" ".join(sys.argv[1:])))
        store.save(todos)
    print(render.render(todos))


if __name__ == "__main__":
    main()
PY
cat > "$seed/sample/tests/test_render.py" << 'PY'
from models import Todo
from render import render


def test_render_marks_done():
    assert render([Todo("a", True), Todo("b")]) == "[x] a\n[ ] b"
PY
cat > "$seed/sample/web/index.html" << 'HTML'
<!doctype html>
<html><body>
<h1>Todos</h1>
<ul id="list"></ul>
<input id="title"><button id="add">Add</button>
</body></html>
HTML

cargo build -q -p wukong-cli || { echo "FAIL cargo build"; exit 1; }
docker run -d --rm --name "$box" --user "$(id -u):$(id -g)" -e HOME="$work/home" -v "$work":"$work" -w "$ws" \
    -p 127.0.0.1::4096 -e OPENCODE_SERVER_PASSWORD="$pw" \
    --entrypoint opencode "$image" serve --hostname 0.0.0.0 --port 4096 > /dev/null
port="$(docker port "$box" 4096/tcp | head -1 | sed 's/.*://')"
url="http://127.0.0.1:$port"
for _ in $(seq 30); do curl -sf -u "opencode:$pw" "$url/api/info" > /dev/null && break; sleep 1; done

declare -A prompt=(
    [implement]="請在 sample/store.py 加一個函式 count_done(todos)，回傳已完成的項目數，並在 sample/tests/test_store.py 補上單元測試。"
    [explore]="請掃描工作區 sample/ 這個專案的結構，列出每個 Python 模組 import 了哪些其他模組，整理出模組之間的依賴關係。"
    [lookup]="請查閱工作區 sample/ 每個模組的程式碼，整理各模組的用途與對外的函式，寫成 sample/README.md 說明文件。"
    [architecture]="sample/store.py 目前直接讀寫 JSON 檔，之後要讓多個使用者同時新增待辦。請比較改用 SQLite 與維持 JSON 加檔案鎖兩個方案的取捨，給我建議。先不要改檔案。"
    [design]="請幫 sample/web/index.html 這個待辦清單頁面設計介面：版面結構、元件、互動與視覺風格的建議。先不要改檔案。"
    [direct]="台灣的首都是哪一個城市？只回答城市名稱。"
)
results="$work/out/results.jsonl"
: > "$results"
ocdb="$work/home/.local/share/opencode/opencode.db"
for task in $tasks; do
    for run in $(seq "$runs"); do
        rsync -a --delete --exclude .opencode --exclude .wukong "$seed/" "$ws/"
        name="$task-$run" db="$work/mem/$task-$run.db"
        start=$(date +%s)
        env -i PATH="$PATH" HOME="$work/home" WUKONG_WORKSPACE="$ws" WUKONG_SETTINGS_FILE="$work/home/settings.json" \
            WUKONG_MEMORY_DB="sqlite://$db" WUKONG_AGENT_SERVER_URL="$url" WUKONG_AGENT_SERVER_PASSWORD="$pw" \
            WUKONG_AGENT_TIMEOUT_SECS=500 \
            timeout 600 "$repo/target/debug/wukong" --scope "project:eval-$name" --no-stream "${prompt[$task]}" \
            > "$work/out/$name.out" 2> "$work/out/$name.err" < /dev/null
        rc=$?
        root="$(sqlite3 "file:$db?mode=ro" "select session_id from agent_session_state where scope='project:eval-$name'")"
        /usr/bin/python3 -I - "$ocdb" "$root" "$task" "$run" "$rc" "$(( $(date +%s) - start ))" >> "$results" << 'EOF'
import json, sqlite3, sys
db, root, task, run, rc, secs = sys.argv[1:]
c = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
kids = c.execute("select id, agent from session_v2 where parent_id = ?", (root,)).fetchall()
models, primary_tools = set(), []
for sid in [root, *(k[0] for k in kids)]:
    for (d,) in c.execute("select data from session_message where session_id = ? and type = 'assistant' order by seq", (sid,)):
        msg = json.loads(d)
        m = msg.get("model") or {}
        models.add(f"{m.get('providerID')}/{m.get('id')}")
        if sid == root:  # primary 自己呼叫的工具與結果（看得出改檔是否被權限擋下、有沒有用 bash 繞過）
            for item in msg.get("content") or []:
                if item.get("type") == "tool":
                    primary_tools.append(f"{item.get('name')}:{(item.get('state') or {}).get('status')}")
print(json.dumps({"task": task, "run": int(run), "ok": rc == "0" and bool(root), "secs": int(secs),
                  "subagents": sorted({k[1] for k in kids}), "models": sorted(models),
                  "primary_tools": primary_tools, "session": root},
                 ensure_ascii=False))
EOF
        echo "  $(tail -1 "$results")"
    done
done
[ -z "${6:-}" ] || exit 0
python3 -I "$here/dispatch_judge.py" "$results" "$model"
