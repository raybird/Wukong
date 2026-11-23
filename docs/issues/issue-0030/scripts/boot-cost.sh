#!/usr/bin/env bash
# 步驟 5 補充：不呼叫模型，量 V1 opencode serve 從啟動到印出 listening 的耗時與 CPU。
# 用法：boot-cost.sh IMAGE RUN_DIR N（RUN_DIR 結構同 bench-turn.sh）
set -euo pipefail
image=$1 run=$2 n=$3
docker run --rm --network none --cpus 1.5 --memory 3g --pids-limit 256 -e BENCH_N="$n" \
  -v "$run/opencode":/home/wukong/.local/share/opencode -v "$run/config":/home/wukong/.config/opencode \
  -v "$run/workspace":/workspace --entrypoint bash "$image" -c '
    /usr/local/bin/docker-entrypoint.sh wukong --help > /dev/null 2>&1
    cpu() { sed -n "s/^usage_usec //p" /sys/fs/cgroup/cpu.stat; }
    cd /workspace
    for i in $(seq 1 "$BENCH_N"); do
      c0=$(cpu); t0=$(date +%s%3N)
      gosu wukong opencode serve --hostname 127.0.0.1 --port 0 --print-logs > /tmp/boot.log 2>&1 &
      pid=$!
      waited=0
      until grep -q "listening on" /tmp/boot.log; do
        sleep 0.05; waited=$((waited + 1))
        [ $waited -lt 600 ] || { echo "opencode did not report listening within 30s" >&2; kill $pid; exit 1; }
      done
      t1=$(date +%s%3N); c1=$(cpu)
      kill $pid; wait $pid 2>/dev/null || true
      printf "{\"run\":%d,\"boot_ms\":%d,\"boot_cpu_usec\":%d}\n" $i $((t1 - t0)) $((c1 - c0))
    done
  '
