#!/usr/bin/env bash
# #45 Task 6.2（SCN-001）：以 bench-turn-steps.sh 在相同條件下對照 b65b487 與本分支的映像。
# 用法：bench-v2-slim.sh WORK_DIR AUTH_FILE MODEL BASE_IMAGE NEW_IMAGE [N]
#   AUTH_FILE：只含要用的 provider 的 auth.json；MODEL：provider/model，寫進每份 RUN_DIR 的 user.json
#   N：每份 RUN_DIR 連跑的回合數，預設 7（第 1 回合算冷）
# 在 repo 根目錄執行。每個映像、每種任務各用一份全新的合成狀態（mktemp -d），輸出每回合一行 JSON 到
# WORK_DIR/results.ndjson。兩個映像交錯執行（同一種任務先舊後新），減少時段造成的差異。
set -euo pipefail
work="$(realpath "${1:?usage: bench-v2-slim.sh WORK_DIR AUTH_FILE MODEL BASE_IMAGE NEW_IMAGE [N]}")"
auth="${2:?}" model="${3:?}" base="${4:?}" new="${5:?}" n="${6:-7}"
repo="$(pwd)"
: > "$work/results.ndjson"
for task in short tool; do
    for image in "$base" "$new"; do
        run="$(mktemp -d "$work/${image//[:\/]/-}-$task.XXXX")"
        mkdir -p "$run/data" "$run/opencode" "$run/config" "$run/workspace"
        install -m 600 "$auth" "$run/opencode/auth.json"
        printf '{"model":"%s"}\n' "$model" > "$run/config/user.json"
        echo "== $image $task ($run)" >&2
        bash "$repo/docs/issues/issue-0045/scripts/bench-turn-steps.sh" "$image" "$run" "$task" "$n" | tee -a "$work/results.ndjson"
    done
done
