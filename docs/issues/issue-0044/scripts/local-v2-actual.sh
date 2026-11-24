#!/usr/bin/env bash
# Task 2.4（SCN-002）：本機按需模式接真實的 OpenCode V2 2.3.2。
# 用法：local-v2-actual.sh V2_PREFIX WORK_DIR [MODEL]
#   V2_PREFIX：`npm install -g --prefix V2_PREFIX @opencode/cli@2.3.2` 的目錄
#   WORK_DIR：空目錄，放隔離的 HOME 與 workspace
#   MODEL：指定模型的那一回合用的模型，預設 opencode/deepseek-v4.1-flash-free（V2 2.3.2 預設模型為 big-pickle）
# 測試程式先在一般環境建好，再以只含隔離 HOME 的環境執行，不碰使用者的 opencode 資料。
# 只送固定的合成提示詞給免費模型。
set -euo pipefail
v2=$(realpath "$1") work=$(realpath "$2") model=${3:-opencode/deepseek-v4.1-flash-free}
repo=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
mkdir -p "$work/home" "$work/workspace"

bin=$(cd "$repo" && cargo test -q -p wukong-gateway --test local_v2_actual --no-run --message-format=json 2>/dev/null |
  python3 -c 'import json,sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("reason") == "compiler-artifact" and m.get("executable") and m["target"]["name"] == "local_v2_actual":
        print(m["executable"])')

env -i HOME="$work/home" PATH=/usr/bin:/bin \
  WUKONG_V2_OPENCODE="$v2/bin/opencode" WUKONG_V2_ROOT="$work/workspace" WUKONG_V2_MODEL="$model" \
  "$bin" --ignored --nocapture --test-threads 1
