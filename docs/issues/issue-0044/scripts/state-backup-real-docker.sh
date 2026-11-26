#!/usr/bin/env bash
# 以真實 docker 驗證 install.sh 的 backup_docker_state／restore_docker_state（Task 5.1、5.2，
# 2026-11-25 review-0405603 S2 補上權限檢查）。只動自己建立的測試 volume，結束時刪除。
# 用法：state-backup-real-docker.sh IMAGE WORK_DIR
#   IMAGE：有 tar、sh、python3 的映像（例如本機建的 wukong-issue44:v2-local）
#   WORK_DIR：空目錄，當成部署目錄
set -euo pipefail
image=$1 work=$2
repo=$(cd "$(dirname "$0")/../../../.." && pwd)
project=wk44probe volume=wk44probe_opencode-state
mkdir -p "$work" && cd "$work"

# 只取出要測的函式，不執行 installer 本體。
python3 - "$repo/scripts/install.sh" > functions.sh <<'PY'
import re, sys
text = open(sys.argv[1]).read()
for name in ["info", "state_volume", "make_backup_dir", "backup_docker_state", "state_backup_entries", "restore_docker_state"]:
    m = re.search(r"^" + name + r"\(\) (\{|\().*?^(\}|\))$", text, re.S | re.M)
    if not m:
        raise SystemExit("function not found: " + name)
    print(m.group(0))
PY
# shellcheck disable=SC1091
source ./functions.sh
DOCKER_PROJECT_NAME=$project

cleanup() { docker volume rm "$volume" >/dev/null 2>&1 || true; }
trap cleanup EXIT
docker volume create --label com.docker.compose.project=$project --label com.docker.compose.volume=opencode-state "$volume" >/dev/null
# 正式部署由 entrypoint 把 volume 交給 wukong（uid 1000）
docker run --rm --network none -v "$volume:/state" --entrypoint chown "$image" 1000:1000 /state
docker run --rm --network none --user 1000:1000 -v "$volume:/state" --entrypoint python3 "$image" -c '
import os, sqlite3
db = sqlite3.connect("/state/opencode.db"); db.execute("pragma journal_mode=wal")
db.execute("create table t(x)"); db.executemany("insert into t values (?)", [(i,) for i in range(5000)]); db.commit()
os.makedirs("/state/snapshot", exist_ok=True); open("/state/auth.json", "w").write("{}")
'
before=$(docker run --rm --network none -v "$volume:/state:ro" --entrypoint sh "$image" -c 'cd /state && find . -printf "%p %u:%g %m %s\n" | sort && sha256sum opencode.db')

mkdir -p .wukong-backups && chmod 755 .wukong-backups   # 舊版留下的目錄
saved=$(backup_docker_state .wukong-backups/v0-1 "$image" opencode-state missing-state)
archive=$(state_backup_entries "$saved" | cut -f2)
echo "saved=$saved"
echo "archive mode=$(stat -c %a "$archive") volumes dir=$(stat -c %a .wukong-backups/v0-1/volumes) root=$(stat -c %a .wukong-backups)"
[[ "$(stat -c %a "$archive")" == 600 && "$(stat -c %a .wukong-backups)" == 700 ]] || { echo "FAIL: backup is readable by others"; exit 1; }
echo "umask after backup in this shell: $(umask)"

# 模擬 V2 改寫 volume，再還原
docker run --rm --network none --user 1000:1000 -v "$volume:/state" --entrypoint python3 "$image" -c '
import sqlite3
db = sqlite3.connect("/state/opencode.db"); db.execute("create table v2(x)"); db.execute("delete from t"); db.commit()
open("/state/v2-only", "w").write("x")
'
restore_docker_state "$saved" "$image"
after=$(docker run --rm --network none -v "$volume:/state:ro" --entrypoint sh "$image" -c 'cd /state && find . -printf "%p %u:%g %m %s\n" | sort && sha256sum opencode.db')
[[ "$before" == "$after" ]] || { echo "FAIL: restored state differs"; diff <(echo "$before") <(echo "$after"); exit 1; }
# WAL 模式要寫 -shm，所以這一步不用唯讀 mount（比對已在上面做完）
docker run --rm --network none --user 1000:1000 -v "$volume:/state" --entrypoint python3 "$image" -c '
import sqlite3
db = sqlite3.connect("/state/opencode.db")
print("integrity", db.execute("pragma integrity_check").fetchone()[0], "rows", db.execute("select count(*) from t").fetchone()[0],
      "v2 table", db.execute("select count(*) from sqlite_master where name=\"v2\"").fetchone()[0])
'
echo "ok: backup is private, restore matches the state before the backup"
