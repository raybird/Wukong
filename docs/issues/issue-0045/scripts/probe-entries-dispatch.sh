#!/usr/bin/env bash
# #45 Task 5.1（SCN-002）外迴圈：Web 與 Telegram 入口接真實的常駐 V2 2.3.2 與真實模型，派工給 fixer 時
# 入口看得到「悟空·fixer」與產出卡片。Telegram 只替換 transport（MockTgClient）。
# 用法：probe-entries-dispatch.sh IMAGE WORK_DIR AUTH_JSON MODEL
# 在 repo 根目錄執行。測試本身是 crates/wukong-{web,telegram}/tests/managed_actual.rs 裡 #[ignore] 的
# web_streams_the_dispatched_subagent、telegram_names_the_dispatched_subagent；這支腳本起 server、
# 以隔離的 HOME 執行編好的測試 binary。只送固定的合成提示詞。
set -uo pipefail
image="${1:?usage: probe-entries-dispatch.sh IMAGE WORK_DIR AUTH_JSON MODEL}"
work="$(realpath "${2:?}")"
auth="${3:?}" model="${4:?}"
box="probe45-entries-$$"
pw="probe-$$"
fails=0
cleanup() { docker kill "$box" > /dev/null 2>&1; }
trap cleanup EXIT

ws="$work/ws"
mkdir -p "$work/home/.local/share/opencode" "$ws" "$work/out"
install -m 600 "$auth" "$work/home/.local/share/opencode/auth.json"
# Web 與 Telegram 每回合從設定檔取模型（GatewayConfig.default_model 會被它蓋過），所以寫在這裡
printf '{"agent":{"default_model":"%s"}}\n' "$model" > "$work/home/settings.json"
docker run -d --rm --name "$box" --user "$(id -u):$(id -g)" -e HOME="$work/home" -v "$work":"$work" -w "$ws" \
    -p 127.0.0.1::4096 -e OPENCODE_SERVER_PASSWORD="$pw" \
    --entrypoint opencode "$image" serve --hostname 0.0.0.0 --port 4096 > /dev/null
port="$(docker port "$box" 4096/tcp | head -1 | sed 's/.*://')"
url="http://127.0.0.1:$port"
for _ in $(seq 30); do curl -sf -u "opencode:$pw" "$url/api/info" > /dev/null && break; sleep 1; done

run() { # run CRATE TEST
    local bin
    bin="$(cargo test -p "$1" --test managed_actual --no-run 2>&1 | sed -n 's/.*Executable .*(\(.*\))$/\1/p' | head -1)"
    touch "$work/out/$2.log.start"
    env -i PATH="$PATH" HOME="$work/home" WUKONG_WORKSPACE="$ws" WUKONG_SETTINGS_FILE="$work/home/settings.json" \
        WUKONG_AGENT_SERVER_URL="$url" WUKONG_AGENT_SERVER_PASSWORD="$pw" WUKONG_AGENT_TIMEOUT_SECS=280 \
        WUKONG_ISSUE45_MODEL="$model" \
        timeout 400 "$bin" --ignored --exact "$2" --nocapture > "$work/out/$2.log" 2>&1 < /dev/null
    local rc=$?
    if [[ $rc == 0 ]]; then echo "PASS $2"; else echo "FAIL $2 (rc=$rc, see out/$2.log)"; fails=$((fails + 1)); fi
    # Telegram 的成品寫進產出目錄（.wukong/artifacts/...），Web 寫在工作區；列出這個回合寫的
    find "$ws" -name hello.txt -newer "$work/out/$2.log.start" -exec grep -lx 'hello from wukong' {} + | sed "s|^$ws/|  hello.txt: |"
}
run wukong-web web_streams_the_dispatched_subagent
run wukong-telegram telegram_names_the_dispatched_subagent
echo "fails=$fails"
exit $(( fails > 0 ))
