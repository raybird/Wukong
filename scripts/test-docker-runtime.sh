#!/usr/bin/env bash
set -euo pipefail

release_compose="docker-compose.release.yml"
if [[ ! -f "$release_compose" ]]; then
  echo "missing release compose: $release_compose" >&2
  exit 1
fi

if grep -Eq '^[[:space:]]*build:' "$release_compose"; then
  echo "release compose must not contain build directives" >&2
  exit 1
fi

image_count=$(grep -Fc 'ghcr.io/raybird/wukong:__WUKONG_VERSION__' "$release_compose" || true)
if [[ "$image_count" != 5 ]]; then
  echo "release compose must pin all five services to the product placeholder" >&2
  exit 1
fi

if grep -Fq ':latest' "$release_compose"; then
  echo "release compose must not use latest" >&2
  exit 1
fi

compose_file="docker-compose.yml"
entrypoint="scripts/docker-entrypoint.sh"
dockerfile="Dockerfile"
release_workflow=".github/workflows/release.yml"

require_in_file() {
    local pattern="$1"
    local file="$2"
    local message="$3"

    if ! grep -Fq -- "$pattern" "$file"; then
        echo "FAIL: $message" >&2
        echo "missing pattern: $pattern" >&2
        exit 1
    fi
}

require_count_in_file() {
    local pattern="$1"
    local expected="$2"
    local file="$3"
    local message="$4"
    local actual

    actual=$(grep -Fc -- "$pattern" "$file" || true)
    if [[ "$actual" != "$expected" ]]; then
        echo "FAIL: $message" >&2
        echo "expected $expected occurrences of '$pattern', found $actual" >&2
        exit 1
    fi
}

require_in_file "opencode-state:/home/wukong/.local/share/opencode" "$compose_file" \
    "docker compose must persist opencode session state"
require_in_file "opencode-state:" "$compose_file" \
    "docker compose must declare the opencode state volume"
require_in_file "OPENCODE_STATE=\"/home/wukong/.local/share/opencode\"" "$entrypoint" \
    "entrypoint must name opencode state directory"
require_in_file "OPENCODE_RUNTIME=\"/home/wukong/.local/state\"" "$entrypoint" \
    "entrypoint must name opencode runtime state directory"
require_in_file 'mkdir -p "$OPENCODE_STATE" "$OPENCODE_RUNTIME"' "$entrypoint" \
    "entrypoint must create opencode runtime directories before gosu"
require_in_file "chown -R wukong:wukong /home/wukong/.local" "$entrypoint" \
    "entrypoint must chown Docker-created .local directories"
require_in_file "agent-reach-state:/home/wukong/.agent-reach" "$compose_file" \
    "docker compose must persist Agent Reach state"
require_in_file "AGENT_REACH_STATE=\"/home/wukong/.agent-reach\"" "$entrypoint" \
    "entrypoint must name Agent Reach state directory"
require_in_file 'mkdir -p "$AGENT_REACH_STATE"' "$entrypoint" \
    "entrypoint must create Agent Reach state directory before gosu"
require_in_file 'chown -R wukong:wukong "$AGENT_REACH_STATE"' "$entrypoint" \
    "entrypoint must chown Docker-created Agent Reach state volume"
require_in_file "COPY crates/wukong-skills/assets/superpowers /usr/local/share/wukong/skills/superpowers" "$dockerfile" \
    "Docker image must package Superpowers skill assets"
require_in_file "/usr/local/share/wukong/skills/superpowers" "$dockerfile" \
    "Dockerfile must use the canonical image skill asset path"
require_in_file "# Development-only Compose" "$compose_file" \
    "development Compose must identify its local-build role"
require_in_file "# Release deployment template" "$release_compose" \
    "release Compose must identify its pull-only role"
for service in wukong opencode-server wukong-telegram wukong-web wukong-schedulerd; do
    require_in_file "  $service:" "$release_compose" "release Compose must include $service"
done
require_in_file 'WUKONG_WEB_BIND:-127.0.0.1' "$release_compose" \
    "release Compose must retain the loopback web default"
require_in_file 'curl -fsS http://localhost:4096/global/health || exit 1' "$release_compose" \
    "release Compose must retain the OpenCode healthcheck"
require_count_in_file "condition: service_healthy" 3 "$release_compose" \
    "release services must retain server dependencies"
require_in_file 'IMAGE_SKILLS="/usr/local/share/wukong/skills/superpowers"' "$entrypoint" \
    "entrypoint must define image skill asset source"
require_in_file 'WORKSPACE_SKILLS="$WUKONG_WORKSPACE/.wukong/skills/superpowers"' "$entrypoint" \
    "entrypoint must define workspace skill asset destination"
require_in_file 'sync_wukong_skills()' "$entrypoint" \
    "entrypoint must provide a skill asset sync function"
require_in_file 'cmp -s "$IMAGE_SKILLS/SOURCE.md" "$WORKSPACE_SKILLS/SOURCE.md"' "$entrypoint" \
    "entrypoint must skip skill sync when SOURCE.md matches"
require_in_file 'cp -a "$IMAGE_SKILLS/." "$tmp_dir/"' "$entrypoint" \
    "entrypoint must copy image skill assets into a temporary directory"
require_in_file 'mv "$tmp_dir" "$WORKSPACE_SKILLS"' "$entrypoint" \
    "entrypoint must atomically install workspace skill assets"
require_in_file 'OPENCODE_CONFIG_DIR="/home/wukong/.config/opencode"' "$entrypoint" \
    "entrypoint must keep the config directory out of the OPENCODE_CONFIG variable name"
require_in_file 'export OPENCODE_CONFIG="$OPENCODE_USER_CONFIG_FILE"' "$entrypoint" \
    "entrypoint must point opencode at the user config layer"
require_in_file 'cat > "$OPENCODE_CONFIG_FILE" <<' "$entrypoint" \
    "entrypoint must rewrite the baseline on every start so upgrades ship new defaults"
require_in_file 'cp -a "$OPENCODE_CONFIG_FILE" "$OPENCODE_CONFIG_FILE.pre-baseline.bak"' "$entrypoint" \
    "entrypoint must back up a pre-baseline config before taking over opencode.json"
require_in_file 'if [[ ! -f "$OPENCODE_USER_CONFIG_FILE" ]]; then' "$entrypoint" \
    "entrypoint must create the user config layer only when missing"
require_in_file '"external_directory": {' "$entrypoint" \
    "baseline must grant external_directory access explicitly"
require_in_file '"*rm -rf /*": "deny"' "$entrypoint" \
    "baseline must retain the destructive-rm denylist"
require_in_file "curl -fsS http://localhost:4096/global/health || exit 1" "$compose_file" \
    "opencode server must expose a Compose healthcheck"
require_count_in_file "condition: service_healthy" 3 "$compose_file" \
    "web, telegram, and scheduler must wait for a healthy opencode server"
# The point is which files the installer replaces, not how the array is typed.
# Pinning the single-line literal made a formatting change look like a contract
# break, and — worse — it silently stopped asserting anything the moment the real
# list grew: v0.21.0 added five bundle files and this check neither noticed nor
# needed to change. Assert membership instead, and require the entries the
# deployment cannot run without.
for owned in docker-compose.yml .env.example LICENSE scripts/install.sh \
             docker-compose.memoria.yml docker/memoria-runtime/Dockerfile \
             docker/memoria-runtime/publish.sh docker/memoria-runtime/memoria-wrapper.sh \
             docker/memoria-runtime/memoria-vector-sync.sh; do
    awk -v want="$owned" '
        /^DOCKER_RELEASE_OWNED=\(/ { inside = 1 }
        inside && $0 ~ "(^|[( \t])" want "([ \t)]|$)" { found = 1 }
        inside && /^\)/ { inside = 0 }
        END { exit !found }
    ' scripts/install.sh || {
        echo "FAIL: installer must replace Docker release-owned file: $owned" >&2
        exit 1
    }
done
if grep -Eq 'docker compose (build|down)' scripts/install.sh; then
    echo "FAIL: release installer must pull and recreate without local builds or volume removal" >&2
    exit 1
fi

if awk '
    /^  wukong-schedulerd:/ { in_scheduler = 1; next }
    /^  [a-zA-Z0-9_-]+:/ { in_scheduler = 0 }
    in_scheduler && /profiles:/ { found = 1 }
    END { exit found ? 0 : 1 }
' "$compose_file"; then
    echo "FAIL: wukong-schedulerd must start by default, not only through a compose profile" >&2
    exit 1
fi

# ── 資源上限 ──
# 五個 service 都必須有 cgroup 硬邊界。少任何一個，該容器就能用盡整台主機的 CPU、
# 記憶體與 PID——這正是 2026-08-07 整機凍結時的狀態。
for compose in "$compose_file" "$release_compose"; do
    for key in "cpus:" "mem_limit:" "pids_limit:"; do
        require_count_in_file "$key" 5 "$compose" \
            "$compose must set $key on all five services"
    done
done
require_in_file "mem_reservation:" "$compose_file" \
    "opencode-server needs a memory reservation so it is not the first to be reclaimed"
require_in_file "mem_reservation:" "$release_compose" \
    "release opencode-server needs a memory reservation too"

# ── Healthcheck 頻率 ──
# 2 秒間隔每天在容器 cgroup 內 fork 約 43,200 次 shell+curl，那些 CPU 會計入
# docker stats 而被誤讀為 opencode 自身的閒置負載。改用 start_interval 只在啟動期
# 快探，讓三個 depends_on 的 service 仍能立刻解除等待。
for compose in "$compose_file" "$release_compose"; do
    if grep -Eq '^[[:space:]]*interval: 2s' "$compose"; then
        echo "FAIL: $compose still probes every 2s outside start_period" >&2
        echo "use start_interval for startup probing and relax interval to 30s" >&2
        exit 1
    fi
done
require_in_file "start_interval: 2s" "$compose_file" \
    "opencode-server must probe fast during start_period or dependents are delayed"
require_in_file "start_interval: 2s" "$release_compose" \
    "release opencode-server must probe fast during start_period too"

# 上限必須經由 env 變數指定。寫死數值的話，使用者唯一的調整方式就是手改 compose，
# 而那個檔案是 bundle 擁有的、`install.sh --upgrade` 會覆寫它——調整會無聲消失。
# .env 才是升級時保留的那一層，所以逃生口必須留在 env 變數上。
for compose in "$compose_file" "$release_compose"; do
    for var in WUKONG_OPENCODE_CPUS WUKONG_OPENCODE_MEM WUKONG_OPENCODE_PIDS \
               WUKONG_SVC_CPUS WUKONG_SVC_MEM WUKONG_SVC_PIDS; do
        require_in_file "\${$var:-" "$compose" \
            "$compose must expose the limit through $var so .env can override it"
    done
done
require_in_file "WUKONG_OPENCODE_CPUS" .env.example \
    "resource knobs must be documented where operators actually look"

# ── opencode 週期性重啟（W2）──
idle_restart="scripts/opencode-idle-restart.sh"
release_dockerfile="Dockerfile.release"

# v0.20.0 shipped the entrypoint's call to this script but not the script itself:
# the released image is built from Dockerfile.release with a hand-curated context in
# release.yml, and only the dev Dockerfile had been updated. Nothing failed the
# build — the file just was not there, and the feature silently did nothing. So every
# runtime file now has to be asserted in all THREE places that decide whether it
# reaches the image.
for df in "$dockerfile" "$release_dockerfile"; do
    require_in_file "COPY scripts/opencode-idle-restart.sh" "$df" \
        "$df must copy the idle-restart supervisor into the image"
    require_in_file "tzdata" "$df" \
        "$df needs tzdata; without it TZ silently falls back to UTC and the restart window moves"
done
require_in_file "cp scripts/docker-entrypoint.sh scripts/opencode-idle-restart.sh release-context/scripts/" \
    "$release_workflow" \
    "the release build context is curated file-by-file; an omitted script vanishes with no build error"
require_in_file "!scripts/opencode-idle-restart.sh" .dockerignore \
    "scripts/*.sh is ignored, so runtime scripts must be re-included explicitly"
require_in_file "opencode-idle-restart.sh" "$entrypoint" \
    "the entrypoint must start the supervisor alongside opencode serve"
require_in_file '"${1:-}" == "opencode" && "${2:-}" == "serve"' "$entrypoint" \
    "the supervisor must start only for opencode serve, not for run/gh/agent-reach"
require_in_file "the periodic idle restart is DISABLED" "$entrypoint" \
    "a missing supervisor must be reported loudly, not fail silently in the background"

# opencode 只攔 SIGINT，沒攔 SIGTERM，而核心會丟棄送給 PID 1 的預設動作訊號——
# 少了 stop_signal，每次 docker stop 都會空等 10 秒再被 SIGKILL，WAL 不會乾淨收尾。
for compose in "$compose_file" "$release_compose"; do
    require_in_file "stop_signal: SIGINT" "$compose" \
        "$compose must stop opencode with SIGINT; SIGTERM is ignored by pid 1"
done

# 空字串必須真的能停用。compose 與腳本兩層都得用 ${VAR-default}；只要有一層寫成
# ${VAR:-default}，空值就會被當成未設定而套回預設，停用開關就是壞的。
for compose in "$compose_file" "$release_compose"; do
    require_in_file 'WUKONG_OPENCODE_RESTART_WINDOW-03:00-05:00' "$compose" \
        "$compose must use \${VAR-default} so an empty window disables the restart"
done
require_in_file 'WINDOW="${WUKONG_OPENCODE_RESTART_WINDOW-03:00-05:00}"' "$idle_restart" \
    "the supervisor must use \${VAR-default} so an empty window disables the restart"

require_in_file 'kill -INT' "$idle_restart" \
    "the supervisor must signal with SIGINT; SIGTERM never reaches pid 1"

# 預設時區必須留在 compose。放到 .env 的話既有部署永遠拿不到它——.env 是使用者擁有、
# 升級時保留的層，只有 compose 的預設會隨版本送達（同 v0.19.0 的 opencode baseline）。
# 沒有 TZ 就退回 UTC，窗口會安靜地落在錯誤的時間，而唯一症狀是「重啟沒發生」。
for compose in "$compose_file" "$release_compose"; do
    require_in_file 'TZ=${TZ:-Asia/Taipei}' "$compose" \
        "$compose must ship a real timezone default; the restart window is local time"
done
require_in_file '$4 == "01"' "$idle_restart" \
    "only ESTABLISHED sockets count as activity; TIME_WAIT would never drain"

# 這一項刻意**解析 YAML** 而不是 grep 字串：整份檔案裡有沒有 WUKONG_TG_TOKEN，跟它
# 有沒有掛在正確的 service 底下是兩回事——本來 telegram 有、schedulerd 沒有，grep
# 一樣全綠。schedulerd 少了它，build_notifier() 會靜靜停用通知，job 照跑、結果永遠
# 送不出去，而唯一的訊號是一行啟動日誌。
python3 - "$compose_file" "$release_compose" <<'PY'
import sys
import yaml

# 這些 service 會透過 wukong-tg-client 對外送訊息，都需要 token。
NEED_TOKEN = ("wukong-telegram", "wukong-schedulerd")
failed = False

for path in sys.argv[1:]:
    with open(path) as fh:
        services = yaml.safe_load(fh).get("services", {})
    for name in NEED_TOKEN:
        svc = services.get(name)
        if svc is None:
            print(f"FAIL: {path} is missing service {name}", file=sys.stderr)
            failed = True
            continue
        env = svc.get("environment", [])
        # compose 允許 list（`- VAR` / `- VAR=x`）與 mapping 兩種寫法。
        names = (
            set(env)
            if isinstance(env, dict)
            else {str(item).split("=", 1)[0] for item in env}
        )
        if "WUKONG_TG_TOKEN" not in names:
            print(
                f"FAIL: {path} service {name} must receive WUKONG_TG_TOKEN "
                "or its Telegram delivery silently disables itself",
                file=sys.stderr,
            )
            failed = True

sys.exit(1 if failed else 0)
PY

echo "docker runtime persistence checks passed"
