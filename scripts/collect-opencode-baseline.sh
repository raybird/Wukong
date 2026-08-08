#!/usr/bin/env bash
# 蒐集 opencode-server 的常駐基線樣本（W1）。
#
# 為什麼不用 `ps -eo %cpu`：那個欄位是「程序存活期的平均 CPU」，不是取樣區間的
# 使用率。對一個已經跑了數天的常駐程序，它會把近期的高負載稀釋掉，永遠看不出
# 累積趨勢。本腳本改讀 /proc/<pid>/stat 的 utime+stime 差分。
#
# 為什麼要分兩階段：查 SQLite、算目錄大小都得靠 `docker exec`，而那些指令的 CPU
# 會計進容器自己的 cgroup。2026-08-08 的調查就是這樣把 158-209% 的峰值誤讀成
# opencode 的閒置負載。所以先在完全不碰容器的靜置期量 CPU，量完才做侵入式查詢。
#
# CPU / 記憶體 / thread 全部從 host 端讀 /proc 與 cgroup 檔案，不需要 exec 進容器。
#
# 用法：
#   scripts/collect-opencode-baseline.sh                      # 預設容器，輸出到 stdout
#   scripts/collect-opencode-baseline.sh -o baseline-t0.txt   # 存檔以便日後 diff
#   scripts/collect-opencode-baseline.sh --quiet-secs 60      # 靜置久一點更保險
#   scripts/collect-opencode-baseline.sh --pid 12345          # 直接量某個 PID（開發測試用）
#
# 取樣時機（見 specs/2026-08-08-opencode-server-residency-remediation-design.md）：
#   重啟前先取一次（這是即將被 W2 抹除的證據），重啟後 +0h / +6h / +24h / +72h 各一次。
set -uo pipefail

CONTAINER="wukong-opencode-server"
QUIET_SECS=30
SAMPLE_SECS=10
SAMPLES=3
OUTPUT=""
TARGET_PID=""
SKIP_DOCKER=0

usage() {
    sed -n '2,26p' "$0" | sed 's/^# \{0,1\}//'
    exit "${1:-0}"
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -c|--container) CONTAINER="$2"; shift 2 ;;
        -o|--output)    OUTPUT="$2"; shift 2 ;;
        --quiet-secs)   QUIET_SECS="$2"; shift 2 ;;
        --sample-secs)  SAMPLE_SECS="$2"; shift 2 ;;
        --samples)      SAMPLES="$2"; shift 2 ;;
        --pid)          TARGET_PID="$2"; SKIP_DOCKER=1; shift 2 ;;
        -h|--help)      usage 0 ;;
        *) echo "unknown argument: $1" >&2; usage 1 ;;
    esac
done

CLK_TCK=$(getconf CLK_TCK)
emit() { if [[ -n "$OUTPUT" ]]; then printf '%s\n' "$*" >> "$OUTPUT"; else printf '%s\n' "$*"; fi; }
note() { printf '%s\n' "$*" >&2; }

[[ -n "$OUTPUT" ]] && : > "$OUTPUT"

# ── 容器 metadata（純 host 端查詢，不進容器）─────────────────────────────────
if [[ "$SKIP_DOCKER" -eq 0 ]]; then
    if ! command -v docker >/dev/null 2>&1; then
        note "error: docker not found; use --pid to measure a bare process instead"; exit 1
    fi
    if ! docker inspect "$CONTAINER" >/dev/null 2>&1; then
        note "error: container '$CONTAINER' not found"; exit 1
    fi
    TARGET_PID=$(docker inspect -f '{{.State.Pid}}' "$CONTAINER")
    STARTED_AT=$(docker inspect -f '{{.State.StartedAt}}' "$CONTAINER")
    RESTART_COUNT=$(docker inspect -f '{{.RestartCount}}' "$CONTAINER")
    IMAGE=$(docker inspect -f '{{.Config.Image}}' "$CONTAINER")
    NANO_CPUS=$(docker inspect -f '{{.HostConfig.NanoCpus}}' "$CONTAINER")
    MEM_LIMIT=$(docker inspect -f '{{.HostConfig.Memory}}' "$CONTAINER")
    PIDS_LIMIT=$(docker inspect -f '{{.HostConfig.PidsLimit}}' "$CONTAINER")
fi

if [[ -z "$TARGET_PID" || "$TARGET_PID" == "0" ]]; then
    note "error: could not resolve a target pid (container not running?)"; exit 1
fi
if [[ ! -r "/proc/$TARGET_PID/stat" ]]; then
    note "error: /proc/$TARGET_PID/stat not readable (need root for a container pid?)"; exit 1
fi

# cgroup 路徑由 /proc/<pid>/cgroup 推導，同時相容 systemd 與 cgroupfs driver。
CGROUP_DIR=""
if [[ -r "/proc/$TARGET_PID/cgroup" ]]; then
    rel=$(awk -F: '$1=="0"{print $3; exit}' "/proc/$TARGET_PID/cgroup" 2>/dev/null)
    [[ -n "$rel" && -d "/sys/fs/cgroup$rel" ]] && CGROUP_DIR="/sys/fs/cgroup$rel"
fi

emit "# opencode-server residency baseline"
emit "schema_version: 1"
emit "collected_at: $(date -Is)"
emit "collected_by: $(id -un)@$(hostname)"
emit "host_kernel: $(uname -r)"
emit "host_logical_cpus: $(nproc)"
emit "target_pid: $TARGET_PID"
if [[ "$SKIP_DOCKER" -eq 0 ]]; then
    emit "container: $CONTAINER"
    emit "container_image: $IMAGE"
    emit "container_started_at: $STARTED_AT"
    emit "container_restart_count: $RESTART_COUNT"
    started_epoch=$(date -d "$STARTED_AT" +%s 2>/dev/null || echo 0)
    if [[ "$started_epoch" != "0" ]]; then
        emit "container_uptime_secs: $(( $(date +%s) - started_epoch ))"
    fi
    # 0 或 -1 代表未設限；W3 完成後這三項都應該是非零值。
    emit "limit_nano_cpus: $NANO_CPUS"
    emit "limit_memory_bytes: $MEM_LIMIT"
    emit "limit_pids: $PIDS_LIMIT"
fi
emit "cgroup_dir: ${CGROUP_DIR:-unavailable}"
if [[ "$SKIP_DOCKER" -eq 1 ]]; then
    # --pid 模式下這個 cgroup 通常是呼叫者的 login session，裡面還有幾百個無關程序，
    # 底下的 cgroup_* 數值不代表目標程序。只有 docker 模式的 cgroup 才等同該容器。
    emit "cgroup_scope: SHARED — dev-test mode, cgroup_* values cover unrelated processes"
fi
emit ""

# ── 階段一：靜置量測。這段期間絕不碰容器 ─────────────────────────────────────
note "[1/2] 靜置 ${QUIET_SECS}s 後量測 idle CPU（期間不執行任何 docker exec）..."
sleep "$QUIET_SECS"

read_proc_cpu() { awk '{print $14+$15}' "/proc/$1/stat" 2>/dev/null; }

emit "## interval cpu (utime+stime delta / wall clock, % of ONE logical cpu)"
emit "# 主機共 $(nproc) 個 logical CPU，所以 100% = 一顆核心跑滿，不是整機滿載。"
total=0; got=0
for i in $(seq 1 "$SAMPLES"); do
    c0=$(read_proc_cpu "$TARGET_PID"); t0=$(date +%s.%N)
    sleep "$SAMPLE_SECS"
    c1=$(read_proc_cpu "$TARGET_PID"); t1=$(date +%s.%N)
    if [[ -z "$c1" ]]; then emit "cpu_sample_${i}_pct: process_gone"; break; fi
    pct=$(awk -v a="$c0" -v b="$c1" -v x="$t0" -v y="$t1" -v k="$CLK_TCK" \
          'BEGIN{printf "%.2f", ((b-a)/k)/(y-x)*100}')
    emit "cpu_sample_${i}_pct: $pct"
    total=$(awk -v t="$total" -v p="$pct" 'BEGIN{print t+p}')
    got=$((got+1))
done
[[ "$got" -gt 0 ]] && emit "cpu_mean_pct: $(awk -v t="$total" -v n="$got" 'BEGIN{printf "%.2f", t/n}')"
emit ""

emit "## process footprint"
emit "rss_mib: $(awk '/^VmRSS:/{printf "%.0f", $2/1024}' "/proc/$TARGET_PID/status" 2>/dev/null)"
emit "threads: $(awk '/^Threads:/{print $2}' "/proc/$TARGET_PID/status" 2>/dev/null)"
if [[ -n "$CGROUP_DIR" ]]; then
    emit "cgroup_pids_current: $(cat "$CGROUP_DIR/pids.current" 2>/dev/null || echo n/a)"
    emit "cgroup_memory_current_bytes: $(cat "$CGROUP_DIR/memory.current" 2>/dev/null || echo n/a)"
    for key in anon file kernel; do
        v=$(awk -v k="$key" '$1==k{print $2}' "$CGROUP_DIR/memory.stat" 2>/dev/null)
        emit "cgroup_memory_${key}_bytes: ${v:-n/a}"
    done
    # OOM 必須是 0。W3 加上 mem_limit 之後這兩項就是新失敗模式的偵測點。
    for key in oom oom_kill; do
        v=$(awk -v k="$key" '$1==k{print $2}' "$CGROUP_DIR/memory.events" 2>/dev/null)
        emit "cgroup_memory_events_${key}: ${v:-n/a}"
    done
    for key in nr_throttled throttled_usec; do
        v=$(awk -v k="$key" '$1==k{print $2}' "$CGROUP_DIR/cpu.stat" 2>/dev/null)
        emit "cgroup_cpu_${key}: ${v:-n/a}"
    done
fi
emit ""

# ── 階段二：侵入式查詢。到這裡 CPU 已經量完，可以安全地打擾容器 ──────────────
if [[ "$SKIP_DOCKER" -eq 0 ]]; then
    note "[2/2] 蒐集 opencode 狀態（此後會 docker exec，CPU 數據已於階段一取得）..."
    emit "## opencode state (collected AFTER cpu sampling; these queries perturb the container)"

    OC_DIR="/home/wukong/.local/share/opencode"
    for f in opencode.db opencode.db-wal; do
        sz=$(docker exec "$CONTAINER" stat -c %s "$OC_DIR/$f" 2>/dev/null)
        emit "${f//[.-]/_}_bytes: ${sz:-n/a}"
    done

    # 事件/訊息量是常駐程序被餵大的輸入；每 session 事件數是最有辨識力的指標。
    q="select (select count(*) from session), (select count(*) from event), (select count(*) from part);"
    if row=$(docker exec "$CONTAINER" sqlite3 "$OC_DIR/opencode.db" "$q" 2>/dev/null); then
        IFS='|' read -r s e p <<< "$row"
        emit "opencode_sessions: ${s:-n/a}"
        emit "opencode_events: ${e:-n/a}"
        emit "opencode_parts: ${p:-n/a}"
        [[ -n "${s:-}" && "$s" -gt 0 ]] 2>/dev/null && \
            emit "opencode_events_per_session: $(awk -v e="$e" -v s="$s" 'BEGIN{printf "%.0f", e/s}')"
    else
        emit "opencode_sessions: unavailable (sqlite3 missing in image?)"
    fi
    emit ""
fi

# ── 主機環境。溫度與 PSI 是把 CPU 數據對齊到凍結事件時間線的依據 ─────────────
emit "## host environment"
emit "load_avg: $(awk '{print $1", "$2", "$3}' /proc/loadavg 2>/dev/null)"
for res in cpu memory io; do
    v=$(awk '/^some/{for(i=1;i<=NF;i++) if($i ~ /^avg10=/){sub("avg10=","",$i); print $i; exit}}' \
        "/proc/pressure/$res" 2>/dev/null)
    emit "psi_${res}_some_avg10: ${v:-n/a}"
done
emit "rootfs_use_pct: $(df --output=pcent / 2>/dev/null | tail -1 | tr -d ' %')"
if command -v sensors >/dev/null 2>&1; then
    # 必須帶上晶片名稱：每顆 hwmon 裝置都有自己的 temp1_input，只用 sensor 名當 key
    # 會讓 CPU、NVMe 與主機板互相覆蓋，diff 出來的溫度就對不上來源了。
    sensors -u 2>/dev/null | awk '
        /^[^ \t].*-/ && !/^Adapter/ { chip=$1; next }
        /_input:/ { gsub(/:/,"",$1); if (chip != "") printf "temp_%s_%s: %.1f\n", chip, $1, $2 }
    ' | while read -r l; do emit "$l"; done
else
    emit "temps: sensors not installed"
fi

emit ""
emit "# 判讀：把數份樣本並排 diff。若 cpu_mean_pct 與 rss_mib 隨 container_uptime_secs"
emit "# 單調上升，即為常駐累積，W2 的週期性重置是正確且充分的處置；若無關聯，"
emit "# 則依 spec 的 W7 提前進行 profiler 權限調查。"

[[ -n "$OUTPUT" ]] && note "written to $OUTPUT"
exit 0
