#!/usr/bin/env bash
# #45 Task 6.4（TBD-5）：在容器裡的 V2 2.3.2 確認 session inbox 的 queue 行為（probe_inbox.py）。
# 用法：probe-inbox.sh IMAGE WORK_DIR AUTH_JSON MODEL
# 在 repo 根目錄執行。HOME 與工作區都在 WORK_DIR；只送固定的合成提示詞。輸出在 WORK_DIR/out/inbox.{json,ndjson}。
set -euo pipefail
image="${1:?usage: probe-inbox.sh IMAGE WORK_DIR AUTH_JSON MODEL}"
work="$(realpath "${2:?}")" auth="${3:?}" model="${4:?}"
here="$(cd "$(dirname "$0")" && pwd)"
box="probe45-inbox-$$" pw="probe-$$"
trap 'docker kill "$box" > /dev/null 2>&1' EXIT
mkdir -p "$work/home/.local/share/opencode" "$work/ws" "$work/out"
install -m 600 "$auth" "$work/home/.local/share/opencode/auth.json"
cp "$here/v2turn.py" "$here/probe_inbox.py" "$work/"
docker run -d --rm --name "$box" --user "$(id -u):$(id -g)" -e HOME="$work/home" -e OPENCODE_SERVER_PASSWORD="$pw" \
    -v "$work":"$work" -v "$work":/p -w "$work/ws" --entrypoint opencode "$image" serve --hostname 127.0.0.1 --port 4096 > /dev/null
for _ in $(seq 30); do
    docker exec -e PW="$pw" "$box" python3 -I -c 'import sys; sys.path.insert(0,"/p"); from v2turn import call; sys.exit(0 if call("GET","/api/info")[0]==200 else 1)' 2> /dev/null && break
    sleep 1
done
docker exec -e PW="$pw" "$box" python3 -I /p/probe_inbox.py /p/out/inbox "$model"
