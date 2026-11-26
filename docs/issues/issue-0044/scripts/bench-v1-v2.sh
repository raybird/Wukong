#!/usr/bin/env bash
# Task 6.2：用 issue-0030 的 bench-turn.sh 對照 V1 與 V2 映像。
# 用法：bench-v1-v2.sh WORK_DIR AUTH_FILE MODEL [N]
#   WORK_DIR：空目錄，每個映像、每種任務各建一份全新的 RUN_DIR
#   AUTH_FILE：只含要用的 provider 的 auth.json（例如只有 nvidia 一筆），複製進每份 RUN_DIR
#   MODEL：provider/model，寫進每份 RUN_DIR 的 user.json，兩個映像用同一個
#   N：每份 RUN_DIR 連跑的回合數，預設 3（第 1 回合算冷）
# 在 repo 根目錄執行。輸出每回合一行 JSON 到 WORK_DIR/results.ndjson。
#
# 與 issue-0030 的差異：RUN_DIR 是全新的合成狀態，不是正式部署的副本。副本裡的記憶會被召回進
# prompt 送給外部模型，違反「不把正式對話送給外部模型」；兩個映像都從空的 opencode.db 起算，
# V2 也就不含遷移的成本（遷移另見 rehearsal.md）。
set -euo pipefail
work="$(realpath "${1:?usage: bench-v1-v2.sh WORK_DIR AUTH_FILE MODEL [N]}")"
auth="${2:?}" model="${3:?}" n="${4:-3}"
repo="$(pwd)"
: > "$work/results.ndjson"
fresh() { # fresh DIR：全新的 RUN_DIR，憑證與模型就緒
    rm -rf "$1"; mkdir -p "$1/data" "$1/opencode" "$1/config" "$1/workspace"
    install -m 600 "$auth" "$1/opencode/auth.json"
    printf '{"model":"%s"}\n' "$model" > "$1/config/user.json"
}
for task in short tool; do
    for image in ghcr.io/raybird/wukong:v0.25.1 wukong-issue44:v2-local; do
        run="$work/$(basename "${image//:/-}")-$task"
        fresh "$run"
        echo "== $image $task" >&2
        bash "$repo/docs/issues/issue-0030/scripts/bench-turn.sh" "$image" "$run" "$task" "$n" | tee -a "$work/results.ndjson"
    done
done
