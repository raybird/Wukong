#!/usr/bin/env bash
# 步驟 5：在副本上起一個與正式部署同限制的容器，entrypoint 只跑一次，之後在同一個容器裡連跑 N 個
# Wukong CLI 回合（第 1 個算冷，之後算暖），逐回合記錄耗時、CPU throttling 增量、記憶體峰值、DB 增量。
# 用法：bench-turn.sh IMAGE RUN_DIR TASK N
#   RUN_DIR 要有 data/memory.db、opencode/opencode.db、config/（opencode-config 的複本）、workspace/
#   TASK：short（只回 OK）或 tool（建立並讀回一個檔案）
# 每回合輸出一行 JSON；stderr 存在 RUN_DIR/logs/。prompt 是固定的合成內容，不帶任何對話資料。
set -euo pipefail
image=$1 run=$2 task=$3 n=$4
case $task in
  short) prompt='只回覆 OK 兩個字母，不要其他內容。' ;;
  tool) prompt='在目前的工作目錄建立 bench.txt，內容是 hello，然後讀出它的內容回覆給我。' ;;
  *) echo "unknown task $task" >&2; exit 2 ;;
esac
mkdir -p "$run/logs"
docker run --rm --cpus 1.5 --memory 3g --pids-limit 256 \
  -e WUKONG_AGENT_SERVER_URL= -e WUKONG_OPENCODE_SESSION_RETENTION_DAYS=30 \
  -e BENCH_IMAGE="$image" -e BENCH_TASK="$task" -e BENCH_N="$n" -e BENCH_PROMPT="$prompt" \
  -v "$run/data":/data -v "$run/opencode":/home/wukong/.local/share/opencode \
  -v "$run/config":/home/wukong/.config/opencode -v "$run/workspace":/workspace -v "$run/logs":/logs \
  --entrypoint bash "$image" -c '
    # entrypoint 的初始化（寫 opencode 設定基線等）只做一次；--help 不會跑回合
    /usr/local/bin/docker-entrypoint.sh wukong --help > /dev/null 2>&1
    stat_of() { sed -n "s/^$1 //p" /sys/fs/cgroup/cpu.stat; }
    size() { echo $(( $(stat -c %s "$1" 2>/dev/null || echo 0) + $(stat -c %s "$1-wal" 2>/dev/null || echo 0) )); }
    for i in $(seq 1 "$BENCH_N"); do
      echo 0 > /sys/fs/cgroup/memory.peak 2>/dev/null || true   # 有支援時重設峰值
      thr0=$(stat_of nr_throttled); tus0=$(stat_of throttled_usec); cpu0=$(stat_of usage_usec)
      db0=$(size /home/wukong/.local/share/opencode/opencode.db); mem0=$(size /data/memory.db)
      t0=$(date +%s%3N)
      gosu wukong wukong --scope "project:bench-$BENCH_TASK" --no-stream "$BENCH_PROMPT" > /logs/out-$i.txt 2> /logs/err-$i.txt
      rc=$?
      t1=$(date +%s%3N)
      printf "{\"image\":\"%s\",\"task\":\"%s\",\"run\":%d,\"turn_ms\":%d,\"rc\":%d,\"out_bytes\":%d,\"nr_throttled\":%d,\"throttled_usec\":%d,\"cpu_usec\":%d,\"memory_peak\":%d,\"opencode_db_delta\":%d,\"memory_db_delta\":%d}\n" \
        "$BENCH_IMAGE" "$BENCH_TASK" $i $((t1 - t0)) $rc $(wc -c < /logs/out-$i.txt) \
        $(( $(stat_of nr_throttled) - thr0 )) $(( $(stat_of throttled_usec) - tus0 )) $(( $(stat_of usage_usec) - cpu0 )) \
        $(cat /sys/fs/cgroup/memory.peak) \
        $(( $(size /home/wukong/.local/share/opencode/opencode.db) - db0 )) $(( $(size /data/memory.db) - mem0 ))
    done
  '
