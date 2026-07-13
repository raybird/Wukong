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
export HOME PATH="$BIN:$PATH" WUKONG_TEST_LOG="$LOG"
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
    printf 'services:\n  wukong:\n    image: ghcr.io/raybird/wukong:%s\n' "$tag" > "$docker_stage/docker-compose.yml"
    printf 'EXAMPLE=1\n' > "$docker_stage/.env.example"
    printf 'MIT\n' > "$docker_stage/LICENSE"
    printf '%s\n' '{"affectedState":[],"backupRequired":false,"instructionsUrl":null,"irreversibleMigration":false,"rollbackSafeTo":"v0.17.1","schemaVersion":1}' > "$docker_stage/data-compatibility.json"
    cp "$INSTALLER" "$docker_stage/scripts/install.sh"
    python3 - "$RELEASES/release-manifest.json" "$tag" <<'PY'
import json, os, sys
json.dump({"schemaVersion": 1, "productTag": sys.argv[2], "image": {"reference": "ghcr.io/raybird/wukong:" + sys.argv[2], "digest": "sha256:" + "a" * 64}, "dataCompatibility": {"schemaVersion": 1, "affectedState": [], "backupRequired": False, "instructionsUrl": None, "irreversibleMigration": False, "rollbackSafeTo": os.environ.get("FIXTURE_SAFE_TO", "v0.17.1")}}, open(sys.argv[1], "w"), sort_keys=True)
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
if [[ "${WUKONG_REQUIRE_STAGED_PULL:-}" == 1 && "$1" == compose && "${*: -1}" == pull && " $* " != *" -f "* ]]; then
    printf "pull used deployment compose\n" >> "$WUKONG_TEST_LOG"
    exit 42
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
if [[ "$1" == image && "$2" == inspect ]]; then printf 'ghcr.io/raybird/wukong@sha256:%s\n' "${FIXTURE_IMAGE_DIGEST:-$(printf 'a%.0s' {1..64})}"; exit 0; fi
if [[ "$1" == compose && "${*: -1}" == ps && -n "${FIXTURE_DOCKER_PS_EXIT:-}" ]]; then exit "$FIXTURE_DOCKER_PS_EXIT"; fi
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

prepare() { rm -rf "$HOME" "$DEPLOYMENT"; mkdir -p "$HOME" "$DEPLOYMENT"; : > "$LOG"; make_release; make_fakes; }

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
    printf 'USER_SECRET=preserve\n' > "$DEPLOYMENT/.env"
    printf 'keep\n' > "$DEPLOYMENT/compose.override.yml"
    mkdir -p "$DEPLOYMENT/workspace"; printf 'state\n' > "$DEPLOYMENT/workspace/custom"
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    assert_file "$DEPLOYMENT/.wukong-release"
    assert_contains "$LOG" ' pull'
    assert_contains "$LOG" 'docker compose -p wukong up -d --force-recreate'
    assert_not_contains "$LOG" 'build'
    assert_not_contains "$LOG" ' down'
    assert_contains "$DEPLOYMENT/.env" 'USER_SECRET=preserve'
    assert_contains "$DEPLOYMENT/compose.override.yml" 'keep'
    assert_contains "$DEPLOYMENT/workspace/custom" 'state'
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
    assert_contains "$LOG" 'docker compose -p wukong up -d --force-recreate'
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

test_docker_rollback() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    FIXTURE_TAG=v9.9.8 FIXTURE_SAFE_TO=v9.9.9 make_release
    FIXTURE_SAFE_TO=v9.9.9 run_installer '' --mode docker --upgrade --version v9.9.8 >/dev/null
    run_installer '' --mode docker --rollback >/dev/null
    python3 - "$DEPLOYMENT/.wukong-release" <<'PY'
import json,sys
assert json.load(open(sys.argv[1]))["productTag"] == "v9.9.9"
PY
}

test_docker_recovery() {
    prepare
    run_installer '' --mode docker --version v9.9.9 >/dev/null
    cp "$DEPLOYMENT/.wukong-release" "$TMP/docker-metadata-before"
    cp "$DEPLOYMENT/docker-compose.yml" "$TMP/compose-before"
    FIXTURE_TAG=v9.9.8 make_release
    ! FIXTURE_DOCKER_PS_EXIT=1 run_installer '' --mode docker --upgrade --version v9.9.8 >/dev/null 2>&1 || fail "unhealthy Docker upgrade succeeded"
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

case "$CASE" in
    parsing) test_parsing ;;
    verification) test_verification ;;
    docker) test_docker ;;
    metadata) test_metadata ;;
    binary-clean) test_binary_clean ;;
    binary-upgrade) test_binary_upgrade ;;
    upgrade-noop) test_upgrade_noop ;;
    forced-upgrade) test_forced_upgrade ;;
    systemd) test_systemd ;;
    rollback-metadata) test_rollback_metadata ;;
    legacy-rollback) test_legacy_rollback ;;
    compatibility) test_rollback_guard ;;
    docker-rollback) test_docker_rollback ;;
    docker-recovery) test_docker_recovery ;;
    binary-recovery) test_binary_recovery ;;
    docker-project) test_docker_project_resolution ;;
    docker-project-conflicts) test_docker_project_conflicts ;;
    docker-project-persistence) test_docker_project_persistence ;;
    all) test_parsing; test_verification; test_docker; test_metadata; test_binary_clean; test_binary_upgrade; test_upgrade_noop; test_docker_compose_repair; test_docker_project_resolution; test_docker_project_conflicts; test_docker_project_persistence; test_forced_upgrade; test_systemd; test_rollback_metadata; test_legacy_rollback; test_rollback_guard; test_docker_rollback; test_docker_recovery; test_binary_recovery ;;
    *) fail "unknown test case: $CASE" ;;
esac

printf 'installer upgrade checks passed (%s)\n' "$CASE"
