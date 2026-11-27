#!/usr/bin/env bash
# #45 Task 6.2（SCN-001）：issue-0030 bench-turn.sh 的延伸版（原檔是 issue-0030 的證據，不改它），每回合另記
#   - llm_steps：這個回合新增的 assistant 訊息數（每則是一次 LLM 呼叫），以及各 agent 的筆數；
#   - opencode_serves：這個回合啟動了幾次 `opencode serve`（本機按需模式）。
# b65b487 的 planner 跑在暫時的 session，回合成功後就被刪掉，回合結束後才數會漏掉它。所以在 opencode.db
# 加一張 bench_steps 表與 AFTER INSERT trigger，assistant 訊息一寫入就記一筆，session 刪掉也還在。
# 表要等 V2 建好 schema 才能加：第一個回合前先短暫啟動一次 opencode serve 讓它遷移，再加 trigger。
# 啟動次數由 PATH 最前面的包裝程式記錄（wukong 以 PATH 找 opencode），只算 serve，不算 --version 檢查。
# 用法：bench-turn-steps.sh IMAGE RUN_DIR TASK N（RUN_DIR 與 TASK 同 bench-turn.sh）
# 每回合輸出一行 JSON。prompt 是固定的合成內容，不帶任何對話資料。
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
  -e OPENCODE_CONFIG=/home/wukong/.config/opencode/user.json \
  -e BENCH_IMAGE="$image" -e BENCH_TASK="$task" -e BENCH_N="$n" -e BENCH_PROMPT="$prompt" \
  -v "$run/data":/data -v "$run/opencode":/home/wukong/.local/share/opencode \
  -v "$run/config":/home/wukong/.config/opencode -v "$run/workspace":/workspace -v "$run/logs":/logs \
  --entrypoint bash "$image" -c '
    set -e
    /usr/local/bin/docker-entrypoint.sh wukong --help > /dev/null 2>&1
    # opencode 包裝程式：記下每次呼叫的參數，再交給真的 opencode
    mkdir -p /bench-bin
    real="$(command -v opencode)"
    printf "#!/bin/sh\necho \"\$*\" >> /logs/opencode-calls.log\nexec %s \"\$@\"\n" "$real" > /bench-bin/opencode
    chmod 755 /bench-bin/opencode; : > /logs/opencode-calls.log; chmod 666 /logs/opencode-calls.log
    export PATH=/bench-bin:$PATH
    db=/home/wukong/.local/share/opencode/opencode.db
    # 讓 V2 建好 schema，再加量測用的表與 trigger
    gosu wukong "$real" serve --hostname 127.0.0.1 --port 4099 > /dev/null 2>&1 &
    serve=$!
    for _ in $(seq 120); do
      gosu wukong python3 -I -c "import sqlite3,sys; c=sqlite3.connect(\"$db\"); sys.exit(0 if c.execute(\"select 1 from sqlite_master where name=\x27session_message\x27\").fetchone() else 1)" 2>/dev/null && break
      sleep 0.5
    done
    kill $serve; wait $serve 2>/dev/null || true
    gosu wukong python3 -I -c "
import sqlite3
c = sqlite3.connect(\"$db\")
c.executescript(\"\"\"
create table if not exists bench_steps (id integer primary key, session_id text, agent text, created integer);
create trigger if not exists bench_steps_log after insert on session_message when new.type = \x27assistant\x27
begin insert into bench_steps (session_id, agent, created) values (new.session_id, json_extract(new.data, \x27\$.agent\x27), new.time_created); end;
\"\"\")
c.commit()"
    steps_now() { gosu wukong python3 -I -c "import sqlite3; print(sqlite3.connect(\"$db\").execute(\"select coalesce(max(id),0) from bench_steps\").fetchone()[0])"; }
    agents_since() { gosu wukong python3 -I -c "
import json, sqlite3
rows = sqlite3.connect(\"$db\").execute(\"select agent, count(*) from bench_steps where id > $1 group by agent\").fetchall()
print(json.dumps(dict(rows)))"; }
    stat_of() { sed -n "s/^$1 //p" /sys/fs/cgroup/cpu.stat; }
    for i in $(seq 1 "$BENCH_N"); do
      rm -f /workspace/bench.txt
      s0=$(steps_now); c0=$(wc -l < /logs/opencode-calls.log)
      cpu0=$(stat_of usage_usec); t0=$(date +%s%3N)
      set +e
      gosu wukong env PATH="$PATH" wukong --scope "project:bench-$BENCH_TASK" --no-stream "$BENCH_PROMPT" > /logs/out-$i.txt 2> /logs/err-$i.txt
      rc=$?
      set -e
      t1=$(date +%s%3N)
      bench_file=n/a
      if [ "$BENCH_TASK" = tool ]; then
        [ "$(cat /workspace/bench.txt 2>/dev/null)" = hello ] && bench_file=ok || bench_file=missing
      fi
      s1=$(steps_now)
      serves=$(tail -n +$((c0 + 1)) /logs/opencode-calls.log | grep -c "^serve" || true)
      printf "{\"image\":\"%s\",\"task\":\"%s\",\"run\":%d,\"turn_ms\":%d,\"rc\":%d,\"bench_file\":\"%s\",\"out_bytes\":%d,\"cpu_usec\":%d,\"llm_steps\":%d,\"steps_by_agent\":%s,\"opencode_serves\":%d}\n" \
        "$BENCH_IMAGE" "$BENCH_TASK" $i $((t1 - t0)) $rc $bench_file $(wc -c < /logs/out-$i.txt) \
        $(( $(stat_of usage_usec) - cpu0 )) $((s1 - s0)) "$(agents_since $s0)" $serves
    done
  '
