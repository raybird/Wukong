#!/usr/bin/env bash
# 以真實 opencode 驗證 entrypoint 寫的設定 baseline（issue-0044 Task 3.1，SCN-009）。
#
# 用法：scripts/test-opencode-baseline.sh IMAGE [V2_PREFIX]
#   IMAGE：Wukong 映像。用它的 opencode，或 V2_PREFIX 的 opencode。
#   V2_PREFIX：`npm install -g --prefix V2_PREFIX @opencode/cli@2.3.2` 的目錄，映像還沒換
#              V2 時用；放在 PATH 最前面。
#
# 容器跑的是本 repo 的 scripts/docker-entrypoint.sh，不是映像裡的那份，所以驗的是
# 這次要出貨的 baseline。全程 --network none：工具呼叫來自 driver 內建的本機假模型，
# 不連任何外部模型，也不碰正式部署的資料。
set -euo pipefail

image="${1:?usage: test-opencode-baseline.sh IMAGE [V2_PREFIX]}"
v2="${2:-}"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workspace="$(mktemp -d)"
trap 'rm -rf "$workspace"' EXIT

args=(--rm --network none
    -e USER_ID="$(id -u)" -e GROUP_ID="$(id -g)" -e WUKONG_WORKSPACE=/workspace
    -v "$workspace":/workspace
    -v "$repo/scripts/docker-entrypoint.sh":/probe/docker-entrypoint.sh:ro
    -v "$repo/scripts/test-support/opencode-baseline-driver.py":/probe/driver.py:ro)
if [[ -n "$v2" ]]; then
    path="$(docker image inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "$image" |
        sed -n 's/^PATH=//p')"
    args+=(-v "$(cd "$v2" && pwd)":/v2prefix:ro -e PATH="/v2prefix/bin:$path")
fi

# 第一個參數是 python3 時 entrypoint 照常寫好設定，再以 wukong 使用者執行 driver。
docker run "${args[@]}" --entrypoint bash "$image" /probe/docker-entrypoint.sh python3 -I /probe/driver.py
