#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
INSTALLER="$ROOT/scripts/install.sh"
CASE="${1:-all}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

HOME="$TMP/home"
DEPLOYMENT="$TMP/deployment"
RELEASES="$TMP/releases"
BIN="$TMP/bin"
LOG="$TMP/commands.log"
# Docker named volumes the fake docker serves: one directory per volume name.
VOLUMES="$TMP/volumes"
export HOME PATH="$BIN:$PATH" WUKONG_TEST_LOG="$LOG" WUKONG_TEST_VOLUMES="$VOLUMES"
mkdir -p "$HOME" "$DEPLOYMENT" "$RELEASES" "$BIN"

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
assert_file() { [[ -f "$1" ]] || fail "missing file: $1"; }
assert_contains() { grep -Fq -- "$2" "$1" || fail "missing '$2' in $1"; }
assert_not_contains() { ! grep -Fq -- "$2" "$1" || fail "unexpected '$2' in $1"; }
assert_same() { cmp -s "$1" "$2" || fail "files differ: $1 $2"; }
sha() { sha256sum "$1" | awk '{print $1}'; }

make_binary() {
    local name="$1" target="$2" out="$RELEASES/$name-$target.tar.gz" stage="$TMP/$name"
    mkdir -p "$stage"
    printf '#!/bin/sh\necho %s %s\n' "$name" "${FIXTURE_TAG:-v9.9.9}" > "$stage/$name"
    chmod +x "$stage/$name"
    tar -C "$stage" -czf "$out" "$name"
    rm -rf "$stage"
}

make_release() {
    local target="x86_64-unknown-linux-musl" tag="${FIXTURE_TAG:-v9.9.9}" docker_stage="$TMP/wukong-docker"
    rm -f "$RELEASES"/*
    for name in wukong wukong-telegram wukong-web wukong-schedulerd; do make_binary "$name" "$target"; done
    mkdir -p "$docker_stage/scripts"
    printf 'services:\n  wukong:\n    image: ghcr.io/raybird/wukong:%s\n  opencode-server:\n    profiles: [server]\n    image: ghcr.io/raybird/wukong:%s\n' "$tag" "$tag" > "$docker_stage/docker-compose.yml"
    printf 'EXAMPLE=1\n' > "$docker_stage/.env.example"
    printf 'MIT\n' > "$docker_stage/LICENSE"
    # FIXTURE_STATE_BACKUP=1 makes this release ask for opencode-state to be backed up
    # before activation, the way the OpenCode V2 release does (issue-0044).
    python3 - "$docker_stage/data-compatibility.json" <<'PY'
import json, os, sys
backup = os.environ.get("FIXTURE_STATE_BACKUP") == "1"
state = json.loads(os.environ["FIXTURE_AFFECTED_STATE"]) if "FIXTURE_AFFECTED_STATE" in os.environ else (["opencode-state"] if backup else [])
json.dump({"affectedState": state, "backupRequired": backup, "instructionsUrl": None, "irreversibleMigration": False, "rollbackSafeTo": os.environ.get("FIXTURE_SAFE_TO", "v0.17.1"), "schemaVersion": 1}, open(sys.argv[1], "w"), sort_keys=True)
PY
    # The optional Memoria overlay. The fixture must mirror the real bundle's
    # shape: a fixture that is missing files the shipped bundle carries does not
    # merely under-test the installer, it goes green while the shipped installer
    # rejects the real thing. That is how v0.21.0 escaped — its archive check
    # demanded an exact match against six entries while the bundle carried eleven.
    mkdir -p "$docker_stage/docker/memoria-runtime"
    printf 'services: {}\n' > "$docker_stage/docker-compose.memoria.yml"
    printf 'FROM scratch\n' > "$docker_stage/docker/memoria-runtime/Dockerfile"
    for stub in publish.sh memoria-wrapper.sh memoria-vector-sync.sh; do
        printf '#!/usr/bin/env bash\n' > "$docker_stage/docker/memoria-runtime/$stub"
        chmod +x "$docker_stage/docker/memoria-runtime/$stub"
    done
    cp "$INSTALLER" "$docker_stage/scripts/install.sh"
    python3 - "$RELEASES/release-manifest.json" "$tag" <<'PY'
import json, os, sys
backup = os.environ.get("FIXTURE_STATE_BACKUP") == "1"
state = json.loads(os.environ["FIXTURE_AFFECTED_STATE"]) if "FIXTURE_AFFECTED_STATE" in os.environ else (["opencode-state"] if backup else [])
json.dump({"schemaVersion": 1, "productTag": sys.argv[2], "image": {"reference": "ghcr.io/raybird/wukong:" + sys.argv[2], "digest": "sha256:" + "a" * 64}, "dataCompatibility": {"schemaVersion": 1, "affectedState": state, "backupRequired": backup, "instructionsUrl": None, "irreversibleMigration": False, "rollbackSafeTo": os.environ.get("FIXTURE_SAFE_TO", "v0.17.1")}}, open(sys.argv[1], "w"), sort_keys=True)
PY
    cp "$RELEASES/release-manifest.json" "$docker_stage/release-manifest.json"
    tar -C "$TMP" -czf "$RELEASES/wukong-docker-$tag.tar.gz" wukong-docker
    rm -rf "$docker_stage"
    (cd "$RELEASES" && sha256sum ./*.tar.gz ./release-manifest.json | sed 's|  \./|  |' > SHA256SUMS)
}

make_fakes() {
    cat > "$BIN/curl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
out=""
while (($#)); do
  case "$1" in -o) out="$2"; shift 2;; -fsSL|-f|-s|-S|-L) shift;; *) url="$1"; shift;; esac
done
if [[ "$url" == */releases/latest ]]; then printf '{"tag_name":"%s"}\n' "${FIXTURE_TAG:-v9.9.9}"; exit 0; fi
if [[ "$url" == */workspace/SOUL.md ]]; then printf 'soul\n'; exit 0; fi
if [[ "$url" == */workspace/AGENTS.md ]]; then printf 'agents\n'; exit 0; fi
source="${WUKONG_TEST_RELEASES}/$(basename "$url")"
[[ -f "$source" ]] || { printf 'missing fixture for %s\n' "$url" >&2; exit 22; }
if [[ -n "${FIXTURE_BAD_CHECKSUM:-}" && "$(basename "$source")" == SHA256SUMS ]]; then sed 's/^./0/' "$source" > "$out"; else cp "$source" "$out"; fi
SH
    cat > "$BIN/docker" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'docker %s\n' "$*" >> "$WUKONG_TEST_LOG"
if [[ "$1" == compose && "$2" == version ]]; then exit 0; fi
# FIXTURE_COMPOSE_NO_WAIT=1: a Compose whose `up` has no --wait-timeout.
if [[ "$1" == compose && "$2" == up && "$3" == --help ]]; then
    printf '      --wait                         Wait for services\n'
    [[ "${FIXTURE_COMPOSE_NO_WAIT:-0}" == 1 ]] || printf '      --wait-timeout int             Maximum duration\n'
    exit 0
fi
if [[ "${WUKONG_REQUIRE_STAGED_PULL:-}" == 1 && "$1" == compose && "${*: -1}" == pull && " $* " != *" -f "* ]]; then
    printf "pull used deployment compose\n" >> "$WUKONG_TEST_LOG"
    exit 42
fi
if [[ "$1" == compose && "${*: -2}" == 'config --services' ]]; then
    printf 'wukong\n'
    if grep -Fq '  opencode-server:' docker-compose.yml && [[ " $* " == *' --profile server '* || "${FIXTURE_SERVER_ACTIVE:-0}" == 1 ]]; then printf 'opencode-server\n'; fi
    exit 0
fi
if [[ "$1" == inspect ]]; then
    name="${@: -1}"
    case "$name" in
        wukong-opencode-server) project="${FIXTURE_OPENCODE_PROJECT:-${FIXTURE_DOCKER_PROJECT:-}}" ;;
        wukong-web) project="${FIXTURE_WEB_PROJECT:-${FIXTURE_DOCKER_PROJECT:-}}" ;;
        wukong-cli|wukong-telegram|wukong-schedulerd) project="${FIXTURE_DOCKER_PROJECT:-}" ;;
        *) exit 1 ;;
    esac
    [[ "$project" != __unlabeled__ ]] || exit 0
    [[ -n "$project" ]] || exit 1
    printf '%s\n' "$project"
    exit 0
fi
# Named volumes live under $WUKONG_TEST_VOLUMES/<name>. `volume ls` answers the
# compose-label lookup; `run` maps each volume mount onto that directory and runs
# the container command on the host, so tar really archives and restores files.
if [[ "$1" == volume && "$2" == ls ]]; then
    project="" volume=""
    for arg in "$@"; do
        case "$arg" in
            label=com.docker.compose.project=*) project="${arg#label=com.docker.compose.project=}" ;;
            label=com.docker.compose.volume=*) volume="${arg#label=com.docker.compose.volume=}" ;;
        esac
    done
    [[ ! -d "$WUKONG_TEST_VOLUMES/${project}_$volume" ]] || printf '%s_%s\n' "$project" "$volume"
    exit 0
fi
if [[ "$1" == run ]]; then
    shift; mounts=(); entrypoint=""
    while [[ "$1" == -* ]]; do
        case "$1" in
            -v) mounts+=("$2"); shift 2 ;;
            --entrypoint) entrypoint="$2"; shift 2 ;;
            --network|--user|-u|-w) shift 2 ;;
            *) shift ;;
        esac
    done
    shift # image
    args=("$entrypoint" "$@")
    for mount in "${mounts[@]}"; do
        IFS=: read -r src dst _ <<< "$mount"
        [[ -d "$WUKONG_TEST_VOLUMES/$src" ]] || { echo "fake docker: no volume $src" >&2; exit 1; }
        args=("${args[@]//$dst/$WUKONG_TEST_VOLUMES/$src}")
    done
    [[ -z "${FIXTURE_DOCKER_RUN_EXIT:-}" ]] || exit "$FIXTURE_DOCKER_RUN_EXIT"
    [[ -z "${FIXTURE_RESTORE_EXIT:-}" || " ${args[*]} " != *" -xpf "* ]] || exit "$FIXTURE_RESTORE_EXIT"
    exec "${args[@]}"
fi
# FIXTURE_MIGRATING_TAG=<tag>: starting that release rewrites opencode.db and adds a
# file, the way OpenCode V2 migrates the database on its first start.
if [[ "$1" == compose && " $* " == *' up '* && "${*: -2}" == '-d --force-recreate' && -n "${FIXTURE_MIGRATING_TAG:-}" ]] &&
    grep -Fq "wukong:$FIXTURE_MIGRATING_TAG" docker-compose.yml && [[ -d "$WUKONG_TEST_VOLUMES/wukong_opencode-state" ]]; then
    printf 'migrated by %s\n' "$FIXTURE_MIGRATING_TAG" > "$WUKONG_TEST_VOLUMES/wukong_opencode-state/opencode.db"
    printf 'new\n' > "$WUKONG_TEST_VOLUMES/wukong_opencode-state/v2-only"
fi
if [[ "$1" == image && "$2" == inspect ]]; then printf 'ghcr.io/raybird/wukong@sha256:%s\n' "${FIXTURE_IMAGE_DIGEST:-$(printf 'a%.0s' {1..64})}"; exit 0; fi
# FIXTURE_UNHEALTHY_TAG=<tag>: that release's containers are created (and migrate the
# database), but never become healthy; only `up --wait` notices (PR #46 review P2).
if [[ "$1" == compose && " $* " == *' up --wait '* && "${*: -2}" == '-d --force-recreate' && -n "${FIXTURE_UNHEALTHY_TAG:-}" ]] &&
    grep -Fq "wukong:$FIXTURE_UNHEALTHY_TAG" docker-compose.yml; then
    exit 1
fi
exit "${FIXTURE_DOCKER_EXIT:-0}"
SH
    cat > "$BIN/systemctl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf 'systemctl %s\n' "$*" >> "$WUKONG_TEST_LOG"
state="${HOME}/.wukong/test-systemd-enabled"
if [[ "$1" == --user && "$2" == is-enabled ]]; then grep -Fxq "$3" "$state" 2>/dev/null; exit $?; fi
if [[ "$1" == --user && "$2" == enable ]]; then mkdir -p "$(dirname "$state")"; printf '%s\n' "${@: -1}" >> "$state"; fi
exit 0
SH
    cat > "$BIN/loginctl" <<'SH'
#!/usr/bin/env bash
printf 'Linger=yes\n'
SH
    chmod +x "$BIN"/*
}

run_installer() {
    local input="${1:-}"; shift || true
    (cd "$DEPLOYMENT" && printf '%b' "$input" | WUKONG_TEST_RELEASES="$RELEASES" FIXTURE_TAG="${FIXTURE_TAG:-v9.9.9}" bash "$INSTALLER" "$@")
}

run_piped_installer() {
    (
        cd "$DEPLOYMENT"
        cat "$INSTALLER" |
            WUKONG_TEST_RELEASES="$RELEASES" FIXTURE_TAG="${FIXTURE_TAG:-v9.9.9}" bash -s -- "$@"
    )
}

prepare() { rm -rf "$HOME" "$DEPLOYMENT" "$VOLUMES"; mkdir -p "$HOME" "$DEPLOYMENT" "$VOLUMES"; : > "$LOG"; make_release; make_fakes; }

test_parsing() {
    prepare
    run_installer '' --upgrade --version v9.9.9 --dry-run >/dev/null
    run_installer '' --mode binary --upgrade --version v9.9.9 --dry-run >/dev/null
    ! run_installer '' --mode docker --with-schedulerd --version v9.9.9 --dry-run >/dev/null 2>&1 || fail "Docker accepted scheduler flag"
    ! run_installer '' --upgrade --rollback --version v9.9.9 --dry-run >/dev/null 2>&1 || fail "upgrade and rollback were accepted together"
    ! run_installer '' --mode --dry-run >/dev/null 2>&1 || fail "missing mode value reached downloads"
}

test_verification() {
    prepare
    mkdir -p "$HOME/.local/bin"; printf 'old\n' > "$HOME/.local/bin/wukong"; cp "$HOME/.local/bin/wukong" "$TMP/before"
    if FIXTURE_BAD_CHECKSUM=1 run_installer '' --mode binary --upgrade --version v9.9.9 >/dev/null 2>&1; then fail "bad checksums accepted"; fi
    assert_same "$TMP/before" "$HOME/.local/bin/wukong"
    grep -v 'wukong-x86_64-unknown-linux-musl.tar.gz' "$RELEASES/SHA256SUMS" > "$TMP/sums"; mv "$TMP/sums" "$RELEASES/SHA256SUMS"
    ! run_installer '' --mode binary --upgrade --version v9.9.9 >/dev/null 2>&1 || fail "missing checksum entry accepted"
    assert_same "$TMP/before" "$HOME/.local/bin/wukong"
    make_release
    python3 - "$RELEASES/release-manifest.json" <<'PY'
import json, sys
path=sys.argv[1]; d=json.load(open(path)); d["productTag"]="v0.0.1"; json.dump(d, open(path,"w"))
PY
    (cd "$RELEASES" && sha256sum ./*.tar.gz ./release-manifest.json | sed 's|  \./|  |' > SHA256SUMS)
    ! run_installer '' --mode binary --upgrade --version v9.9.9 >/dev/null 2>&1 || fail "manifest tag mismatch accepted"
    assert_same "$TMP/before" "$HOME/.local/bin/wukong"
    make_release
    printf '{bad json\n' > "$RELEASES/release-manifest.json"
    (cd "$RELEASES" && sha256sum ./*.tar.gz ./release-manifest.json | sed 's|  \./|  |' > SHA256SUMS)
    ! run_installer '' --mode binary --upgrade --version v9.9.9 >/dev/null 2>&1 || fail "malformed manifest accepted"
    assert_same "$TMP/before" "$HOME/.local/bin/wukong"
    for kind in absolute parent symlink hardlink duplicate unexpected; do
        make_release
        python3 - "$RELEASES/wukong-x86_64-unknown-linux-musl.tar.gz" "$kind" <<'PY'
import io, sys, tarfile
path, kind = sys.argv[1:]
with tarfile.open(path, "w:gz") as tar:
    names = {"absolute": ["/wukong"], "parent": ["../wukong"], "symlink": ["wukong"], "hardlink": ["wukong"], "duplicate": ["wukong", "wukong"], "unexpected": ["not-wukong"]}[kind]
    for name in names:
        entry = tarfile.TarInfo(name)
        if kind == "symlink": entry.type = tarfile.SYMTYPE; entry.linkname = "target"
        elif kind == "hardlink": entry.type = tarfile.LNKTYPE; entry.linkname = "target"
        else: entry.size = 1
        tar.addfile(entry, None if entry.islnk() or entry.issym() else io.BytesIO(b"x"))
PY
        (cd "$RELEASES" && sha256sum ./*.tar.gz ./release-manifest.json | sed 's|  \./|  |' > SHA256SUMS)
        ! run_installer '' --mode binary --upgrade --version v9.9.9 >/dev/null 2>&1 || fail "$kind archive entry accepted"
        assert_same "$TMP/before" "$HOME/.local/bin/wukong"
    done
}

test_docker() {
    prepare
    # WUKONG_OPENCODE_CPUS stands in for the whole resource-limit escape hatch: the
    # compose file is bundle-owned and replaced on upgrade, so .env is the only place
    # a tuned limit can survive. If .env ever became release-owned, this catches it.
    printf 'USER_SECRET=preserve\nWUKONG_OPENCODE_CPUS=1.0\n' > "$DEPLOYMENT/.env"
    printf 'keep\n' > "$DEPLOYMENT/compose.override.yml"
    mkdir -p "$DEPLOYMENT/workspace"; printf 'state\n' > "$DEPLOYMENT/workspace/custom"
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    assert_file "$DEPLOYMENT/.wukong-release"
    assert_contains "$LOG" ' pull'
    assert_contains "$LOG" 'docker compose -p wukong up --wait --wait-timeout 180 -d --force-recreate'
    assert_contains "$LOG" 'docker compose -p wukong --profile server stop opencode-server'
    assert_not_contains "$LOG" 'build'
    assert_not_contains "$LOG" ' down'
    assert_contains "$DEPLOYMENT/.env" 'USER_SECRET=preserve'
    assert_contains "$DEPLOYMENT/.env" 'WUKONG_OPENCODE_CPUS=1.0'
    assert_contains "$DEPLOYMENT/compose.override.yml" 'keep'
    assert_contains "$DEPLOYMENT/workspace/custom" 'state'
    : > "$LOG"
    FIXTURE_SERVER_ACTIVE=1 run_installer '' --mode docker --version v9.9.9 >/dev/null
    assert_not_contains "$LOG" ' stop opencode-server'

    # A fresh deployment gets its .env from .env.example, readable by the owner only:
    # it is where the server password lives (review-c4d47d8 N-C).
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    assert_file "$DEPLOYMENT/.env"
    [[ "$(stat -c %a "$DEPLOYMENT/.env")" == 600 ]] || fail "a new .env is not 0600"
}

test_docker_env_migration() {
    prepare
    # v0.22.0 moved the default off the shared server; .env is user-owned and kept
    # across upgrades, so only the old template's literal default may be rewritten.
    printf '%s\n' 'USER_SECRET=preserve' \
        'WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions' \
        '# WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions' \
        'WUKONG_AGENT_SERVER_URL=http://opencode-server:4096' \
        'WUKONG_AGENT_SERVER_PASSWORD=kept' > "$DEPLOYMENT/.env"
    cp "$DEPLOYMENT/.env" "$TMP/env-original"
    run_installer '' --mode docker --version v9.9.9 > "$TMP/env-out"
    printf '%s\n' 'USER_SECRET=preserve' \
        'WUKONG_AGENT_CMD=opencode run' \
        '# WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions' \
        'WUKONG_AGENT_SERVER_URL=http://opencode-server:4096' \
        'WUKONG_AGENT_SERVER_PASSWORD=kept' \
        'COMPOSE_PROFILES=server' > "$TMP/env-expected"
    assert_same "$TMP/env-expected" "$DEPLOYMENT/.env"
    assert_contains "$TMP/env-out" 'WUKONG_AGENT_CMD'
    assert_contains "$TMP/env-out" 'COMPOSE_PROFILES=server'
    [[ "$(find "$DEPLOYMENT/.wukong-backups" -name .env | wc -l)" == 1 ]] || fail "expected one .env backup"
    assert_same "$TMP/env-original" "$(find "$DEPLOYMENT/.wukong-backups" -name .env)"
    FIXTURE_TAG=v9.9.8 make_release
    run_installer '' --mode docker --upgrade --version v9.9.8 >/dev/null
    assert_same "$TMP/env-expected" "$DEPLOYMENT/.env"
    [[ "$(find "$DEPLOYMENT/.wukong-backups" -name .env | wc -l)" == 1 ]] || fail "unchanged .env was backed up again"

    prepare
    printf '%s\n' 'WUKONG_AGENT_CMD="opencode run --dangerously-skip-permissions"' \
        'WUKONG_AGENT_SERVER_URL=http://10.0.0.5:4096' > "$DEPLOYMENT/.env"
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    printf '%s\n' 'WUKONG_AGENT_CMD=opencode run' 'WUKONG_AGENT_SERVER_URL=http://10.0.0.5:4096' > "$TMP/env-expected"
    assert_same "$TMP/env-expected" "$DEPLOYMENT/.env"

    prepare
    printf '%s\n' 'WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions --model x/y' \
        'COMPOSE_PROFILES=cli' \
        'WUKONG_AGENT_SERVER_URL=http://opencode-server:4096' > "$DEPLOYMENT/.env"
    cp "$DEPLOYMENT/.env" "$TMP/env-original"
    run_installer '' --mode docker --version v9.9.9 > "$TMP/env-out"
    assert_same "$TMP/env-original" "$DEPLOYMENT/.env"
    assert_contains "$TMP/env-out" 'COMPOSE_PROFILES does not include server'

    prepare
    printf '%s\n' 'WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions' > "$DEPLOYMENT/.env"
    cp "$DEPLOYMENT/.env" "$TMP/env-original"
    ! FIXTURE_UNHEALTHY_TAG=v9.9.9 run_installer '' --mode docker --version v9.9.9 >/dev/null 2>&1 || fail "unhealthy Docker install succeeded"
    assert_same "$TMP/env-original" "$DEPLOYMENT/.env"
}

# issue-0044 Task 3.2（SCN-017）：OpenCode V2 server 一定要密碼。內建 opencode-server 會跑起來、
# .env 卻沒有密碼時，先備份再產生一組；已設時不動；不是內建 server 時不加（產生的密碼對不上
# 別人的 server），只提醒。
test_docker_env_server_password() {
    local line password
    prepare
    printf '%s\n' 'USER_SECRET=preserve' 'COMPOSE_PROFILES=server' \
        'WUKONG_AGENT_SERVER_URL=http://opencode-server:4096' > "$DEPLOYMENT/.env"
    cp "$DEPLOYMENT/.env" "$TMP/env-original"
    run_installer '' --mode docker --version v9.9.9 > "$TMP/env-out"
    line="$(grep '^WUKONG_AGENT_SERVER_PASSWORD=' "$DEPLOYMENT/.env")" || fail "no password was generated"
    password="${line#*=}"
    [[ "$password" =~ ^[0-9a-f]{48}$ ]] || fail "generated password is not 48 hex characters: $password"
    assert_contains "$TMP/env-out" 'generated WUKONG_AGENT_SERVER_PASSWORD'
    assert_not_contains "$TMP/env-out" "$password"
    assert_same "$TMP/env-original" "$(find "$DEPLOYMENT/.wukong-backups" -name .env)"
    [[ "$(grep -c . "$DEPLOYMENT/.env")" == 4 ]] || fail "migration changed more than the password"
    # review-0405603 S2: the password and its backup are readable by the owner only.
    [[ "$(stat -c %a "$DEPLOYMENT/.env")" == 600 ]] || fail ".env with the generated password is not 0600"
    [[ "$(stat -c %a "$DEPLOYMENT/.wukong-backups")" == 700 ]] || fail ".wukong-backups is not 0700"
    cp "$DEPLOYMENT/.env" "$TMP/env-generated"
    FIXTURE_TAG=v9.9.8 make_release
    run_installer '' --mode docker --upgrade --version v9.9.8 >/dev/null
    assert_same "$TMP/env-generated" "$DEPLOYMENT/.env"

    # An empty assignment counts as unset and is filled in place.
    prepare
    printf '%s\n' 'WUKONG_AGENT_SERVER_URL=http://opencode-server:4096' 'WUKONG_AGENT_SERVER_PASSWORD=' \
        'COMPOSE_PROFILES=server' > "$DEPLOYMENT/.env"
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    [[ "$(grep -c '^WUKONG_AGENT_SERVER_PASSWORD=' "$DEPLOYMENT/.env")" == 1 ]] || fail "expected one password line"
    grep -Eq '^WUKONG_AGENT_SERVER_PASSWORD=[0-9a-f]{48}$' "$DEPLOYMENT/.env" || fail "empty password was not filled"

    # Already set: untouched, and nothing to back up.
    prepare
    printf '%s\n' 'WUKONG_AGENT_SERVER_URL=http://opencode-server:4096' 'WUKONG_AGENT_SERVER_PASSWORD=mine' \
        'COMPOSE_PROFILES=server' > "$DEPLOYMENT/.env"
    cp "$DEPLOYMENT/.env" "$TMP/env-original"
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    assert_same "$TMP/env-original" "$DEPLOYMENT/.env"
    [[ ! -d "$DEPLOYMENT/.wukong-backups" ]] || [[ -z "$(find "$DEPLOYMENT/.wukong-backups" -name .env)" ]] ||
        fail "unchanged .env was backed up"

    # Local mode: no server, no password.
    prepare
    printf '%s\n' 'WUKONG_AGENT_CMD=opencode run' > "$DEPLOYMENT/.env"
    cp "$DEPLOYMENT/.env" "$TMP/env-original"
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    assert_same "$TMP/env-original" "$DEPLOYMENT/.env"

    # Someone else's server: a generated password could never match it.
    prepare
    printf '%s\n' 'WUKONG_AGENT_SERVER_URL=http://10.0.0.5:4096' > "$DEPLOYMENT/.env"
    cp "$DEPLOYMENT/.env" "$TMP/env-original"
    run_installer '' --mode docker --version v9.9.9 > "$TMP/env-out"
    assert_same "$TMP/env-original" "$DEPLOYMENT/.env"
    assert_contains "$TMP/env-out" 'WUKONG_AGENT_SERVER_PASSWORD'
}

test_metadata() {
    prepare
    run_installer '1\nn\n\n\n\n\n' --mode binary --version v9.9.9 >/dev/null
    assert_file "$HOME/.wukong/install.json"
    python3 - "$HOME/.wukong/install.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
assert d["schemaVersion"] == 1 and d["mode"] == "binary"
assert d["components"] == ["wukong"] and d["services"] == []
PY
    [[ "$(stat -c %a "$HOME/.wukong/install.json")" == 600 ]] || fail "metadata permissions are not 0600"
}

test_binary_clean() {
    prepare
    mkdir -p "$HOME/.wukong/workspace"
    printf 'WUKONG_WEB_HOST="0.0.0.0"\n' > "$HOME/.wukong/config.env"
    printf 'custom\n' > "$HOME/.wukong/workspace/SOUL.md"
    run_installer '' --mode binary --version v9.9.9 >/dev/null
    assert_contains "$HOME/.wukong/config.env" '0.0.0.0'
    assert_contains "$HOME/.wukong/workspace/SOUL.md" custom
}

test_binary_upgrade() {
    prepare
    mkdir -p "$HOME/.local/bin" "$HOME/.wukong"
    for name in wukong wukong-web; do printf 'old-%s\n' "$name" > "$HOME/.local/bin/$name"; chmod +x "$HOME/.local/bin/$name"; done
    python3 - "$HOME/.wukong/install.json" <<'PY'
import json, sys
json.dump({"schemaVersion": 1, "mode": "binary", "target": "x86_64-unknown-linux-musl", "productTag": "v1", "previousVersion": None, "components": ["wukong", "wukong-web"], "services": [], "installedAt": "old", "updatedAt": "old", "previousBackupPath": None}, open(sys.argv[1], "w"))
PY
    run_installer '' --mode binary --upgrade --version v9.9.9 >/dev/null
    assert_contains "$HOME/.local/bin/wukong" 'v9.9.9'
    assert_contains "$HOME/.local/bin/wukong-web" 'v9.9.9'
    [[ ! -e "$HOME/.local/bin/wukong-telegram" ]] || fail "unselected component downloaded"
}

test_upgrade_noop() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    cp "$DEPLOYMENT/.wukong-release" "$TMP/docker-metadata-before"
    : > "$LOG"
    output="$(run_installer '' --mode docker --upgrade --version v9.9.9)"
    [[ "$output" == *'Wukong is already up to date: v9.9.9'* ]] || fail "Docker no-op upgrade did not report current version"
    assert_same "$TMP/docker-metadata-before" "$DEPLOYMENT/.wukong-release"
    [[ ! -s "$LOG" ]] || fail "Docker no-op upgrade invoked Docker"

    prepare
    run_installer '1\nn\n\n\n\n\n' --mode binary --version v9.9.9 >/dev/null
    cp "$HOME/.wukong/install.json" "$TMP/binary-metadata-before"
    cp "$HOME/.local/bin/wukong" "$TMP/binary-before"
    output="$(run_installer '' --mode binary --upgrade --version v9.9.9)"
    [[ "$output" == *'Wukong is already up to date: v9.9.9'* ]] || fail "Binary no-op upgrade did not report current version"
    assert_same "$TMP/binary-metadata-before" "$HOME/.wukong/install.json"
    assert_same "$TMP/binary-before" "$HOME/.local/bin/wukong"
}

test_docker_compose_repair() {
    prepare
    run_installer "" --mode docker --version v9.9.9 >/dev/null
    printf "services:\n  wukong-telegram:\n    build: .\n    image: wukong:latest\n" > "$DEPLOYMENT/docker-compose.yml"
    : > "$LOG"
    WUKONG_REQUIRE_STAGED_PULL=1 run_installer "" --mode docker --upgrade --version v9.9.9 >/dev/null
    assert_contains "$LOG" " pull"
    assert_contains "$DEPLOYMENT/docker-compose.yml" "ghcr.io/raybird/wukong:v9.9.9"
}

test_docker_project_resolution() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    assert_contains "$LOG" 'docker compose -p wukong '
    python3 - "$DEPLOYMENT/.wukong-release" <<'PY'
import json, sys
assert json.load(open(sys.argv[1]))["composeProject"] == "wukong"
PY

    prepare
    COMPOSE_PROJECT_NAME=custom run_installer '' --mode docker --version v9.9.9 >/dev/null
    assert_contains "$LOG" 'docker compose -p custom '

    prepare
    printf '%s\n' '{"schemaVersion":1,"productTag":"v9.9.8","imageDigest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}' > "$DEPLOYMENT/.wukong-release"
    FIXTURE_DOCKER_PROJECT=runwukong run_installer '' --mode docker --upgrade --version v9.9.9 >/dev/null
    assert_contains "$LOG" 'docker compose -p runwukong '
    python3 - "$DEPLOYMENT/.wukong-release" <<'PY'
import json, sys
assert json.load(open(sys.argv[1]))["composeProject"] == "runwukong"
PY
}

test_docker_project_conflicts() {
    prepare
    printf '%s\n' '{"schemaVersion":1,"productTag":"v9.9.8","imageDigest":"sha256:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","composeProject":"runwukong"}' > "$DEPLOYMENT/.wukong-release"
    ! FIXTURE_DOCKER_PROJECT=other run_installer '' --mode docker --upgrade --version v9.9.9 >/dev/null 2>&1 || fail "metadata/label project conflict accepted"
    assert_not_contains "$LOG" ' pull'

    prepare
    ! FIXTURE_OPENCODE_PROJECT=runwukong FIXTURE_WEB_PROJECT=other run_installer '' --mode docker --upgrade --version v9.9.9 >/dev/null 2>&1 || fail "multiple labeled projects accepted"
    assert_not_contains "$LOG" ' pull'

    prepare
    ! COMPOSE_PROJECT_NAME=other FIXTURE_DOCKER_PROJECT=runwukong run_installer '' --mode docker --upgrade --version v9.9.9 >/dev/null 2>&1 || fail "explicit project replaced existing ownership"
    assert_not_contains "$LOG" ' pull'

    prepare
    ! COMPOSE_PROJECT_NAME=Bad.Name run_installer '' --mode docker --version v9.9.9 >/dev/null 2>&1 || fail "invalid explicit project accepted"
    assert_not_contains "$LOG" ' pull'

    prepare
    ! FIXTURE_DOCKER_PROJECT=__unlabeled__ run_installer '' --mode docker --upgrade --version v9.9.9 >/dev/null 2>&1 || fail "unlabeled existing container accepted"
    assert_not_contains "$LOG" ' pull'
}

test_docker_project_persistence() {
    prepare
    COMPOSE_PROJECT_NAME=custom run_installer '' --mode docker --version v9.9.9 >/dev/null
    FIXTURE_TAG=v9.9.8 FIXTURE_SAFE_TO=v9.9.9 make_release
    : > "$LOG"
    run_installer '' --mode docker --upgrade --version v9.9.8 >/dev/null
    assert_contains "$LOG" 'docker compose -p custom '
    : > "$LOG"
    run_installer '' --mode docker --rollback >/dev/null
    assert_contains "$LOG" 'docker compose -p custom '
}

test_forced_upgrade() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    : > "$LOG"
    run_installer '' --mode docker --upgrade --force --version v9.9.9 >/dev/null
    assert_contains "$LOG" ' pull'
    assert_contains "$LOG" 'docker compose -p wukong up --wait --wait-timeout 180 -d --force-recreate'
}

test_systemd() {
    prepare
    run_installer '1\nn\n\n\n\n\n' --mode binary --with-schedulerd --version v9.9.9 >/dev/null
    local unit="$HOME/.config/systemd/user/wukong-schedulerd.service"
    assert_file "$unit"
    assert_contains "$unit" 'Managed by Wukong install.sh'
    assert_contains "$unit" 'ExecStart=%h/.local/bin/wukong-schedulerd'
    assert_contains "$LOG" 'systemctl --user enable --now wukong-schedulerd'
}

test_rollback_metadata() {
    prepare
    mkdir -p "$HOME/.local/bin" "$HOME/.wukong"
    printf 'old\n' > "$HOME/.local/bin/wukong"; chmod +x "$HOME/.local/bin/wukong"
    python3 - "$HOME/.wukong/install.json" <<'PY'
import json, sys
json.dump({"schemaVersion": 1, "mode": "binary", "target": "x86_64-unknown-linux-musl", "productTag": "v1.0.0", "previousVersion": "v0.9.0", "components": ["wukong"], "services": [], "installedAt": "old", "updatedAt": "old", "previousBackupPath": None}, open(sys.argv[1], "w"))
PY
    run_installer '' --mode binary --upgrade --version v9.9.9 >/dev/null
    python3 - "$HOME/.wukong/install.json" <<'PY'
import json, sys
d=json.load(open(sys.argv[1])); assert d["productTag"] == "v9.9.9"; assert d["previousVersion"] == "v1.0.0"; assert d["previousBackupPath"]
PY
}

test_legacy_rollback() {
    prepare
    mkdir -p "$HOME/.local/bin" "$HOME/.config/systemd/user"
    printf 'legacy-cli\n' > "$HOME/.local/bin/wukong"; chmod 755 "$HOME/.local/bin/wukong"
    printf 'legacy-web\n' > "$HOME/.local/bin/wukong-web"; chmod 755 "$HOME/.local/bin/wukong-web"
    printf '# Managed by Wukong install.sh\nlegacy unit\n' > "$HOME/.config/systemd/user/wukong-web.service"
    run_installer '' --mode binary --upgrade --version v9.9.9 >/dev/null
    legacy_backup="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["previousBackupPath"])' "$HOME/.wukong/install.json")"
    [[ "$legacy_backup" == *"/legacy-"* ]] || fail "legacy backup was not recorded"
    assert_file "$legacy_backup/manifest.json"
    run_installer '' --mode binary --rollback >/dev/null
    assert_contains "$HOME/.local/bin/wukong" legacy-cli
    assert_contains "$HOME/.local/bin/wukong-web" legacy-web
    [[ ! -f "$HOME/.wukong/install.json" ]] || fail "legacy rollback retained modern metadata"
}

test_rollback_guard() {
    prepare
    mkdir -p "$HOME/.local/bin" "$HOME/.wukong/backups/previous"
    printf 'current\n' > "$HOME/.local/bin/wukong"; printf 'previous\n' > "$HOME/.wukong/backups/previous/wukong"
    python3 - "$HOME/.wukong/install.json" <<'PY'
import json,sys
json.dump({"schemaVersion":1,"mode":"binary","target":"x86_64-unknown-linux-musl","productTag":"v9.9.9","previousVersion":"v1.0.0","previousBackupPath":sys.argv[1].rsplit('/',1)[0]+"/backups/previous","components":["wukong"],"services":[],"installedAt":"old","updatedAt":"old"},open(sys.argv[1],"w"))
PY
    cp "$HOME/.local/bin/wukong" "$TMP/current-before"
    ! run_installer '' --mode binary --rollback >/dev/null 2>&1 || fail "rollback accepted missing compatibility declaration"
    assert_same "$TMP/current-before" "$HOME/.local/bin/wukong"
}

# issue-0044 SCN-013: a release whose dataCompatibility asks for opencode-state to be
# backed up stops the services first, archives the volume, and records where.
test_docker_state_backup() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    mkdir -p "$VOLUMES/wukong_opencode-state/snapshot"
    head -c 65536 /dev/urandom > "$VOLUMES/wukong_opencode-state/opencode.db"
    printf 'wal\n' > "$VOLUMES/wukong_opencode-state/opencode.db-wal"
    cp "$VOLUMES/wukong_opencode-state/opencode.db" "$TMP/opencode.db.before"

    # Control: a release that does not ask for a backup leaves the volume alone.
    FIXTURE_TAG=v9.9.8 make_release
    : > "$LOG"
    run_installer '' --mode docker --upgrade --version v9.9.8 >/dev/null
    assert_not_contains "$LOG" 'docker run'
    ! grep -Fxq 'docker compose -p wukong --profile server stop' "$LOG" || fail "services stopped without a required backup"
    python3 - "$DEPLOYMENT/.wukong-release" <<'PY'
import json, sys
assert not json.load(open(sys.argv[1])).get("stateBackups"), "backup recorded without being required"
PY
    ! compgen -G "$DEPLOYMENT/.wukong-backups/*/volumes" >/dev/null || fail "volume archived without being required"

    FIXTURE_TAG=v9.9.7 FIXTURE_STATE_BACKUP=1 make_release
    # A backup root an older installer left world-readable is tightened too (review-0405603 S2).
    mkdir -p "$DEPLOYMENT/.wukong-backups"; chmod 755 "$DEPLOYMENT/.wukong-backups"
    # A backup that cannot be taken aborts before any release file changes, and starts
    # the stopped services again.
    cp "$DEPLOYMENT/.wukong-release" "$TMP/metadata-before"; cp "$DEPLOYMENT/docker-compose.yml" "$TMP/compose-before"
    : > "$LOG"
    ! FIXTURE_DOCKER_RUN_EXIT=1 FIXTURE_STATE_BACKUP=1 run_installer '' --mode docker --upgrade --version v9.9.7 >/dev/null 2>&1 ||
        fail "upgrade went ahead without its required backup"
    assert_same "$TMP/metadata-before" "$DEPLOYMENT/.wukong-release"
    assert_same "$TMP/compose-before" "$DEPLOYMENT/docker-compose.yml"
    grep -Fxq 'docker compose -p wukong start' "$LOG" || fail "stopped services were not started again"
    assert_not_contains "$LOG" 'force-recreate'

    : > "$LOG"
    FIXTURE_STATE_BACKUP=1 run_installer '' --mode docker --upgrade --version v9.9.7 >/dev/null
    # Order: every service is stopped, then the volume is read, then the new release starts.
    python3 - "$LOG" <<'PY'
import sys
lines = open(sys.argv[1]).read().splitlines()
def first(pred, what):
    for i, line in enumerate(lines):
        if pred(line): return i
    raise SystemExit(f"never ran: {what}\n" + "\n".join(lines))
stop = first(lambda l: l == "docker compose -p wukong --profile server stop", "stop every service")
archive = first(lambda l: l.startswith("docker run ") and "wukong_opencode-state:" in l, "archive opencode-state")
start = first(lambda l: l.endswith(" -d --force-recreate"), "activate the new release")
assert stop < archive < start, (stop, archive, start)
PY
    path="$(python3 - "$DEPLOYMENT/.wukong-release" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
assert d["productTag"] == "v9.9.7"
print(d["stateBackups"]["opencode-state"])
PY
)" || fail "metadata does not record the opencode-state backup"
    [[ "$path" == .wukong-backups/v9.9.8-*/volumes/opencode-state.tar ]] || fail "unexpected backup location: $path"
    # The archive holds the provider credentials (auth.json) and every conversation.
    [[ "$(stat -c %a "$DEPLOYMENT/$path")" == 600 ]] || fail "the opencode-state archive is not 0600"
    [[ "$(stat -c %a "$DEPLOYMENT/.wukong-backups")" == 700 ]] || fail ".wukong-backups is not 0700"
    mkdir "$TMP/restored"
    tar -C "$TMP/restored" -xf "$DEPLOYMENT/$path"
    assert_same "$TMP/opencode.db.before" "$TMP/restored/opencode.db"
    assert_contains "$TMP/restored/opencode.db-wal" 'wal'
    [[ -d "$TMP/restored/snapshot" ]] || fail "backup lost a directory"
}

# issue-0044 SCN-014: when the new release fails to start, opencode.db goes back to
# the pre-upgrade backup together with the release files, before the old release starts.
state_backup_fixture() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    mkdir -p "$VOLUMES/wukong_opencode-state/snapshot"
    head -c 65536 /dev/urandom > "$VOLUMES/wukong_opencode-state/opencode.db"
    printf 's\n' > "$VOLUMES/wukong_opencode-state/snapshot/f"
    rm -rf "$TMP/state-before"; cp -a "$VOLUMES/wukong_opencode-state" "$TMP/state-before"
    cp "$DEPLOYMENT/.wukong-release" "$TMP/metadata-before"; cp "$DEPLOYMENT/docker-compose.yml" "$TMP/compose-before"
    FIXTURE_TAG=v9.9.7 FIXTURE_STATE_BACKUP=1 make_release
    : > "$LOG"
}

test_docker_state_restore() {
    state_backup_fixture
    ! FIXTURE_MIGRATING_TAG=v9.9.7 FIXTURE_UNHEALTHY_TAG=v9.9.7 FIXTURE_STATE_BACKUP=1 \
        run_installer '' --mode docker --upgrade --version v9.9.7 >"$TMP/out" 2>&1 || fail "unhealthy upgrade succeeded"
    assert_contains "$TMP/out" "see 'docker compose -p wukong ps'"
    diff -r "$TMP/state-before" "$VOLUMES/wukong_opencode-state" || fail "opencode-state was not restored to the backup"
    assert_same "$TMP/metadata-before" "$DEPLOYMENT/.wukong-release"
    assert_same "$TMP/compose-before" "$DEPLOYMENT/docker-compose.yml"
    # Order: new release starts → everything stops → volume restored → old release starts.
    python3 - "$LOG" <<'PY'
import sys
lines = open(sys.argv[1]).read().splitlines()
ups = [i for i, l in enumerate(lines) if l.endswith(" -d --force-recreate")]
stops = [i for i, l in enumerate(lines) if l == "docker compose -p wukong --profile server stop"]
restores = [i for i, l in enumerate(lines) if l.startswith("docker run ") and " -i " in f" {l} " and "wukong_opencode-state:/state " in f"{l} "]
assert len(ups) == 2 and restores, "\n".join(lines)
assert any(ups[0] < s < restores[0] for s in stops) and restores[0] < ups[1], "\n".join(lines)
PY

    # The restore itself fails: release files still go back, but nothing is started on
    # a database the old release may not be able to read; the message names the backup.
    state_backup_fixture
    ! FIXTURE_MIGRATING_TAG=v9.9.7 FIXTURE_UNHEALTHY_TAG=v9.9.7 FIXTURE_RESTORE_EXIT=1 FIXTURE_STATE_BACKUP=1 \
        run_installer '' --mode docker --upgrade --version v9.9.7 >"$TMP/out" 2>&1 || fail "unhealthy upgrade succeeded"
    assert_same "$TMP/compose-before" "$DEPLOYMENT/docker-compose.yml"
    assert_same "$TMP/metadata-before" "$DEPLOYMENT/.wukong-release"
    [[ "$(grep -c 'force-recreate' "$LOG")" == 1 ]] || fail "a release was started on an unrestored opencode-state"
    grep -Eq 'volumes/opencode-state\.tar' "$TMP/out" || fail "failure message does not point at the backup: $(cat "$TMP/out")"
}

# PR #46 review P2-1: activation waits for the services to be healthy, so Compose must
# support `up --wait-timeout`; without it the upgrade stops before anything changes.
test_docker_compose_wait_required() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    assert_contains "$LOG" 'up --wait --wait-timeout 180 -d --force-recreate'
    cp "$DEPLOYMENT/.wukong-release" "$TMP/metadata-before"; cp "$DEPLOYMENT/docker-compose.yml" "$TMP/compose-before"
    FIXTURE_TAG=v9.9.7 FIXTURE_STATE_BACKUP=1 make_release
    : > "$LOG"
    ! FIXTURE_COMPOSE_NO_WAIT=1 FIXTURE_STATE_BACKUP=1 run_installer '' --mode docker --upgrade --version v9.9.7 >"$TMP/out" 2>&1 ||
        fail "upgrade went ahead without a way to wait for health"
    assert_contains "$TMP/out" 'wait-timeout'
    assert_same "$TMP/metadata-before" "$DEPLOYMENT/.wukong-release"
    assert_same "$TMP/compose-before" "$DEPLOYMENT/docker-compose.yml"
    assert_not_contains "$LOG" ' stop'
    assert_not_contains "$LOG" ' pull'
    assert_not_contains "$LOG" 'force-recreate'
}

# PR #46 review P2-2: a backup list that cannot be read stops the upgrade before any
# service is stopped; it is never taken as "nothing to back up".
test_docker_state_backup_list() {
    local state
    for state in '["../escape", "opencode-state"]' '[1]' '["opencode-state", "../escape"]'; do
        state_backup_fixture
        FIXTURE_TAG=v9.9.7 FIXTURE_STATE_BACKUP=1 FIXTURE_AFFECTED_STATE="$state" make_release
        : > "$LOG"
        ! FIXTURE_STATE_BACKUP=1 run_installer '' --mode docker --upgrade --version v9.9.7 >"$TMP/out" 2>&1 ||
            fail "upgrade went ahead with an unreadable backup list: $state"
        assert_same "$TMP/metadata-before" "$DEPLOYMENT/.wukong-release"
        assert_same "$TMP/compose-before" "$DEPLOYMENT/docker-compose.yml"
        assert_not_contains "$LOG" ' stop'
        assert_not_contains "$LOG" 'force-recreate'
    done
}

# issue-0044 SCN-015: --rollback after an upgrade that backed up opencode-state brings
# the image, the release files and opencode.db back to the pre-upgrade backup.
upgraded_with_state_backup() {
    state_backup_fixture
    FIXTURE_SAFE_TO=v9.9.9 FIXTURE_TAG=v9.9.7 FIXTURE_STATE_BACKUP=1 make_release
    FIXTURE_MIGRATING_TAG=v9.9.7 FIXTURE_SAFE_TO=v9.9.9 FIXTURE_STATE_BACKUP=1 \
        run_installer '' --mode docker --upgrade --version v9.9.7 >/dev/null
    rm -rf "$TMP/state-v2"; cp -a "$VOLUMES/wukong_opencode-state" "$TMP/state-v2"
    cmp -s "$TMP/state-before/opencode.db" "$TMP/state-v2/opencode.db" && fail "fixture: the new release did not migrate opencode.db"
    cp "$DEPLOYMENT/.wukong-release" "$TMP/metadata-v2"; cp "$DEPLOYMENT/docker-compose.yml" "$TMP/compose-v2"
    : > "$LOG"
}

test_docker_state_rollback() {
    upgraded_with_state_backup
    run_installer '' --mode docker --rollback >"$TMP/out" 2>&1 || fail "rollback failed: $(cat "$TMP/out")"
    diff -r "$TMP/state-before" "$VOLUMES/wukong_opencode-state" || fail "opencode-state is not the pre-upgrade backup"
    assert_same "$TMP/compose-before" "$DEPLOYMENT/docker-compose.yml"
    python3 - "$DEPLOYMENT/.wukong-release" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
assert d["productTag"] == "v9.9.9" and d["previousVersion"] == "v9.9.7", d
PY
    assert_contains "$LOG" 'docker image inspect ghcr.io/raybird/wukong:v9.9.9'
    # Conversations held since the backup are dropped, and the installer says so.
    grep -Eq 'v9\.9\.9-[0-9]+/volumes/opencode-state\.tar; conversations after it was taken are not kept' "$TMP/out" ||
        fail "rollback does not say later conversations are dropped: $(cat "$TMP/out")"
    # Order: the old image is pulled while the current release still runs, then
    # everything stops, the volume is restored, and the old release starts.
    python3 - "$LOG" <<'PY'
import sys
lines = open(sys.argv[1]).read().splitlines()
def first(pred, what, after=-1):
    for i, l in enumerate(lines):
        if i > after and pred(l): return i
    raise SystemExit(f"never ran: {what}\n" + "\n".join(lines))
pull = first(lambda l: l.endswith(" pull"), "pull the previous image")
stop = first(lambda l: l == "docker compose -p wukong --profile server stop", "stop every service", pull)
restore = first(lambda l: l.startswith("docker run ") and " -i " in l and "wukong_opencode-state:/state " in l + " ", "restore opencode-state", stop)
first(lambda l: l.endswith(" -d --force-recreate"), "start the previous release", restore)
PY
    # What the rolled-back release held is set aside, not destroyed.
    saved="$(ls "$DEPLOYMENT"/.wukong-backups/v9.9.9-rollback-*/volumes/opencode-state.tar)" || fail "the V2 state was not set aside"
    [[ "$(stat -c %a "$saved")" == 600 ]] || fail "the set-aside opencode-state archive is not 0600"
    mkdir "$TMP/set-aside"; tar -C "$TMP/set-aside" -xf "$saved"
    diff -r "$TMP/state-v2" "$TMP/set-aside" || fail "the set-aside copy is not the state before rollback"

    # A missing backup refuses the rollback, says why, and changes nothing.
    upgraded_with_state_backup
    rm "$DEPLOYMENT"/.wukong-backups/v9.9.9-[0-9]*/volumes/opencode-state.tar
    ! run_installer '' --mode docker --rollback >"$TMP/out" 2>&1 || fail "rollback went ahead without the opencode-state backup"
    assert_contains "$TMP/out" 'opencode-state'
    assert_same "$TMP/metadata-v2" "$DEPLOYMENT/.wukong-release"
    assert_same "$TMP/compose-v2" "$DEPLOYMENT/docker-compose.yml"
    diff -r "$TMP/state-v2" "$VOLUMES/wukong_opencode-state" || fail "a refused rollback touched opencode-state"
    ! grep -Eq ' stop$| up -d' "$LOG" || fail "a refused rollback stopped or started services"

    # The pulled previous image is not the one recorded before the upgrade: refused too.
    upgraded_with_state_backup
    ! FIXTURE_IMAGE_DIGEST="$(printf 'b%.0s' {1..64})" run_installer '' --mode docker --rollback >"$TMP/out" 2>&1 ||
        fail "rollback went ahead with a different image"
    assert_contains "$TMP/out" 'digest'
    assert_same "$TMP/compose-v2" "$DEPLOYMENT/docker-compose.yml"
    diff -r "$TMP/state-v2" "$VOLUMES/wukong_opencode-state" || fail "a refused rollback touched opencode-state"
    ! grep -Eq ' stop$| up -d' "$LOG" || fail "a refused rollback stopped or started services"
}

test_docker_rollback() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    FIXTURE_TAG=v9.9.8 FIXTURE_SAFE_TO=v9.9.9 make_release
    FIXTURE_SAFE_TO=v9.9.9 run_installer '' --mode docker --upgrade --version v9.9.8 >/dev/null
    # An explicit --version that is not the previous release is refused.
    cp "$DEPLOYMENT/.wukong-release" "$TMP/metadata-before"
    ! run_installer '' --mode docker --rollback --version v9.9.7 >/dev/null 2>&1 || fail "rollback to a version that is not the previous one ran"
    assert_same "$TMP/metadata-before" "$DEPLOYMENT/.wukong-release"
    # Without --version the target is the previous release, whatever the latest release
    # is; here the latest is the release that was upgraded to, as after a normal upgrade
    # (review-2dd4369 M-1).
    FIXTURE_TAG=v9.9.8 run_installer '' --mode docker --rollback >"$TMP/out" 2>&1 || fail "rollback without --version was refused: $(cat "$TMP/out")"
    # The message names the release it went back to, not the latest one (review-e48c402 M-2).
    assert_contains "$TMP/out" 'Wukong rollback completed: v9.9.9'
    python3 - "$DEPLOYMENT/.wukong-release" <<'PY'
import json,sys
assert json.load(open(sys.argv[1]))["productTag"] == "v9.9.9"
PY

    # The previous release never gets healthy (PR #46 review P2-1, follow-up S-1). Its
    # files and state are already back, so the metadata says so too: the rollback fails,
    # tells how to start it once fixed, and a second --rollback does not run again.
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    FIXTURE_TAG=v9.9.8 FIXTURE_SAFE_TO=v9.9.9 make_release
    FIXTURE_SAFE_TO=v9.9.9 run_installer '' --mode docker --upgrade --version v9.9.8 >/dev/null
    ! FIXTURE_UNHEALTHY_TAG=v9.9.9 run_installer '' --mode docker --rollback >"$TMP/out" 2>&1 || fail "rollback to an unhealthy release succeeded"
    python3 - "$DEPLOYMENT/.wukong-release" <<'PY'
import json,sys
d = json.load(open(sys.argv[1]))
assert d["productTag"] == "v9.9.9" and d["previousVersion"] == "v9.9.8", d
PY
    assert_contains "$TMP/out" "see 'docker compose -p wukong ps'"
    assert_contains "$TMP/out" "docker compose -p wukong up -d"
    cp "$DEPLOYMENT/.wukong-release" "$TMP/metadata-before"; cp "$DEPLOYMENT/docker-compose.yml" "$TMP/compose-before"
    : > "$LOG"
    ! run_installer '' --mode docker --rollback >/dev/null 2>&1 || fail "a second rollback ran after the first one already went back"
    assert_same "$TMP/metadata-before" "$DEPLOYMENT/.wukong-release"
    assert_same "$TMP/compose-before" "$DEPLOYMENT/docker-compose.yml"
    assert_not_contains "$LOG" 'force-recreate'
}

test_docker_recovery() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    cp "$DEPLOYMENT/.wukong-release" "$TMP/docker-metadata-before"
    cp "$DEPLOYMENT/docker-compose.yml" "$TMP/compose-before"
    FIXTURE_TAG=v9.9.8 make_release
    ! FIXTURE_UNHEALTHY_TAG=v9.9.8 run_installer '' --mode docker --upgrade --version v9.9.8 >/dev/null 2>&1 || fail "unhealthy Docker upgrade succeeded"
    assert_same "$TMP/docker-metadata-before" "$DEPLOYMENT/.wukong-release"
    assert_same "$TMP/compose-before" "$DEPLOYMENT/docker-compose.yml"
}

test_binary_recovery() {
    prepare
    mkdir -p "$HOME/.local/bin" "$HOME/.wukong"
    printf 'old-cli\n' > "$HOME/.local/bin/wukong"; chmod +x "$HOME/.local/bin/wukong"
    python3 - "$HOME/.wukong/install.json" <<'PY'
import json,sys
json.dump({"schemaVersion":1,"mode":"binary","target":"x86_64-unknown-linux-musl","productTag":"v1.0.0","previousVersion":None,"previousBackupPath":None,"components":["wukong"],"services":[],"installedAt":"old","updatedAt":"old"},open(sys.argv[1],"w"))
PY
    cp "$HOME/.local/bin/wukong" "$TMP/binary-before"; cp "$HOME/.wukong/install.json" "$TMP/metadata-before"
    ! WUKONG_FAIL_BINARY_ACTIVATION=1 run_installer '' --mode binary --upgrade --version v9.9.9 >/dev/null 2>&1 || fail "injected binary activation failure succeeded"
    assert_same "$TMP/binary-before" "$HOME/.local/bin/wukong"
    assert_same "$TMP/metadata-before" "$HOME/.wukong/install.json"
}

# A bundle from a FUTURE release will carry files this installer has never heard
# of. Rejecting them deadlocks every existing deployment: the installer that
# understands the new bundle can only be delivered inside that same bundle.
# v0.21.0 hit exactly this. The required entries stay required; extras must not
# abort the install.
test_docker_forward_compatible_bundle() {
    prepare
    local stage="$TMP/fwd"
    rm -rf "$stage"; mkdir -p "$stage"
    tar -xzf "$RELEASES/wukong-docker-v9.9.9.tar.gz" -C "$stage"
    mkdir -p "$stage/wukong-docker/docker/some-future-thing"
    printf 'from a later release\n' > "$stage/wukong-docker/docker/some-future-thing/config.yml"
    printf 'also new\n' > "$stage/wukong-docker/FUTURE.md"
    tar -C "$stage" -czf "$RELEASES/wukong-docker-v9.9.9.tar.gz" wukong-docker
    (cd "$RELEASES" && sha256sum ./*.tar.gz ./release-manifest.json | sed 's|  \./|  |' > SHA256SUMS)

    run_installer '' --mode docker --version v9.9.9 >/dev/null ||
        fail "a bundle carrying unknown extra files must still install"
    assert_file "$DEPLOYMENT/.wukong-release"
    # Unknown files are not owned, so they are validated but never written out.
    [[ ! -e "$DEPLOYMENT/FUTURE.md" ]] || fail "unowned bundle file must not be installed"
}

# The flip side: relaxing "exact" to "at least" must not let a bundle missing a
# file the installer will copy slip through and fail mid-install, after the pull.
test_docker_bundle_missing_required_entry() {
    prepare
    local stage="$TMP/missing"
    rm -rf "$stage"; mkdir -p "$stage"
    tar -xzf "$RELEASES/wukong-docker-v9.9.9.tar.gz" -C "$stage"
    rm -f "$stage/wukong-docker/docker-compose.memoria.yml"
    tar -C "$stage" -czf "$RELEASES/wukong-docker-v9.9.9.tar.gz" wukong-docker
    (cd "$RELEASES" && sha256sum ./*.tar.gz ./release-manifest.json | sed 's|  \./|  |' > SHA256SUMS)

    ! run_installer '' --mode docker --version v9.9.9 >/dev/null 2>&1 ||
        fail "a bundle missing an owned file must abort"
    [[ ! -f "$DEPLOYMENT/.wukong-release" ]] || fail "aborted install must not write release metadata"
}

# The installer is release-owned, so an upgrade overwrites scripts/install.sh
# while bash is still reading it. Nothing goes wrong while the two versions are
# byte-identical — which they were from v0.20.0 to v0.21.0, hiding this for three
# releases — but the moment the shipped installer differs in length, bash resumes
# at a stale byte offset inside the new file and runs whatever text is there.
# The fixture must therefore ship an installer of a DIFFERENT size than the one
# under test, or it reproduces nothing.
test_docker_installer_replaces_itself_mid_run() {
    prepare
    local stage="$TMP/selfrepl" out
    rm -rf "$stage"; mkdir -p "$stage"
    tar -xzf "$RELEASES/wukong-docker-v9.9.9.tar.gz" -C "$stage"
    # Inserted near the TOP, not appended: only a change BEFORE the reader's
    # current offset shifts the rest of the file out from under it. Appending
    # leaves every earlier byte where it was and reproduces nothing.
    python3 - "$stage/wukong-docker/scripts/install.sh" <<'PAD'
import sys
path = sys.argv[1]
lines = open(path).read().split("\n")
pad = ["# padding that shifts every following byte in the shipped installer"] * 400
open(path, "w").write("\n".join(lines[:2] + pad + lines[2:]))
PAD
    tar -C "$stage" -czf "$RELEASES/wukong-docker-v9.9.9.tar.gz" wukong-docker
    (cd "$RELEASES" && sha256sum ./*.tar.gz ./release-manifest.json | sed 's|  \./|  |' > SHA256SUMS)

    # Run the DEPLOYMENT's own copy, which is what a real upgrade does and the
    # only way the running file is also the file being overwritten. run_installer
    # executes the repo copy, so the two paths never collide and it reproduces
    # nothing.
    mkdir -p "$DEPLOYMENT/scripts"
    cp "$INSTALLER" "$DEPLOYMENT/scripts/install.sh"
    out="$(cd "$DEPLOYMENT" && WUKONG_TEST_RELEASES="$RELEASES" FIXTURE_TAG=v9.9.9 \
        bash scripts/install.sh --mode docker --version v9.9.9 2>&1)" ||
        fail "upgrade failed while replacing its own script: $out"
    grep -Fq 'completed: v9.9.9' <<<"$out" ||
        fail "installer did not reach its completion message: $out"
    ! grep -Eq 'command not found|syntax error|unexpected token' <<<"$out" ||
        fail "installer executed fragments of its replacement: $out"
    assert_file "$DEPLOYMENT/.wukong-release"
}

test_stdin_installer() {
    prepare
    local out
    out="$(run_piped_installer --mode docker --version v9.9.9 2>&1)" ||
        fail "installer failed when read from stdin: $out"
    grep -Fq 'completed: v9.9.9' <<<"$out" ||
        fail "stdin installer did not reach its completion message: $out"
    assert_file "$DEPLOYMENT/.wukong-release"
}

case "$CASE" in
    parsing) test_parsing ;;
    verification) test_verification ;;
    docker) test_docker ;;
    docker-env-migration) test_docker_env_migration ;;
    docker-env-server-password) test_docker_env_server_password ;;
    docker-forward-compat) test_docker_forward_compatible_bundle ;;
    docker-missing-entry) test_docker_bundle_missing_required_entry ;;
    docker-self-replace) test_docker_installer_replaces_itself_mid_run ;;
    stdin) test_stdin_installer ;;
    metadata) test_metadata ;;
    binary-clean) test_binary_clean ;;
    binary-upgrade) test_binary_upgrade ;;
    upgrade-noop) test_upgrade_noop ;;
    forced-upgrade) test_forced_upgrade ;;
    systemd) test_systemd ;;
    rollback-metadata) test_rollback_metadata ;;
    legacy-rollback) test_legacy_rollback ;;
    compatibility) test_rollback_guard ;;
    docker-state-backup) test_docker_state_backup ;;
    docker-state-restore) test_docker_state_restore ;;
    docker-state-rollback) test_docker_state_rollback ;;
    docker-state-backup-list) test_docker_state_backup_list ;;
    docker-compose-wait) test_docker_compose_wait_required ;;
    docker-rollback) test_docker_rollback ;;
    docker-recovery) test_docker_recovery ;;
    binary-recovery) test_binary_recovery ;;
    docker-project) test_docker_project_resolution ;;
    docker-project-conflicts) test_docker_project_conflicts ;;
    docker-project-persistence) test_docker_project_persistence ;;
    all) test_parsing; test_verification; test_docker; test_docker_env_migration; test_docker_env_server_password; test_docker_forward_compatible_bundle; test_docker_bundle_missing_required_entry; test_docker_installer_replaces_itself_mid_run; test_stdin_installer; test_metadata; test_binary_clean; test_binary_upgrade; test_upgrade_noop; test_docker_compose_repair; test_docker_project_resolution; test_docker_project_conflicts; test_docker_project_persistence; test_forced_upgrade; test_systemd; test_rollback_metadata; test_legacy_rollback; test_rollback_guard; test_docker_state_backup; test_docker_state_restore; test_docker_state_rollback; test_docker_state_backup_list; test_docker_compose_wait_required; test_docker_rollback; test_docker_recovery; test_binary_recovery ;;
    *) fail "unknown test case: $CASE" ;;
esac

printf 'installer upgrade checks passed (%s)\n' "$CASE"
