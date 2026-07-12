#!/usr/bin/env bash
set -euo pipefail

REPO="raybird/Wukong"
GITHUB="https://github.com"
API="https://api.github.com/repos"
INSTALL_DIR="${HOME}/.local/bin"
CONFIG_DIR="${HOME}/.wukong"
CONFIG_FILE="${CONFIG_DIR}/config.env"
METADATA_FILE="${CONFIG_DIR}/install.json"
ACTION=install
MODE=""
EXPLICIT_MODE=false
WITH_SCHEDULERD=false
FORCE=false
DRY_RUN=false
VERSION=""
VERSION_EXPLICIT=false
FLAVOR=musl
TEMP_DIRS=()
DOCKER_RELEASE_OWNED=(docker-compose.yml .env.example LICENSE scripts/install.sh)

abort() { printf 'installer: %s\n' "$*" >&2; exit 1; }
info() { printf '  %s\n' "$*"; }
cleanup() { local dir; for dir in "${TEMP_DIRS[@]:-}"; do rm -rf "$dir"; done; }
trap cleanup EXIT

make_temp_dir() { local dir; dir="$(mktemp -d "${TMPDIR:-/tmp}/wukong-install.XXXXXX")"; TEMP_DIRS+=("$dir"); printf '%s\n' "$dir"; }
sha256_file() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print $1}'; else shasum -a 256 "$1" | awk '{print $1}'; fi; }
lowercase() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]'; }

usage() {
    cat <<'USAGE'
Usage: install.sh [--mode docker|binary] [--version TAG] [--flavor gnu|musl]
                   [--upgrade|--rollback] [--with-schedulerd] [--force] [--dry-run]

--upgrade defaults to Docker for compatibility; use --mode binary --upgrade for host binaries.
--with-schedulerd selects the Binary scheduler explicitly (Linux systemd only).
USAGE
}

parse_args() {
    while (($#)); do
        case "$1" in
            --mode) (($# >= 2)) || abort "--mode needs a value"; MODE="$2"; EXPLICIT_MODE=true; shift 2 ;;
            --version) (($# >= 2)) || abort "--version needs a value"; VERSION="$2"; VERSION_EXPLICIT=true; shift 2 ;;
            --flavor) (($# >= 2)) || abort "--flavor needs a value"; FLAVOR="$2"; shift 2 ;;
            --upgrade) [[ "$ACTION" == install ]] || abort "only one action is allowed"; ACTION=upgrade; shift ;;
            --rollback) [[ "$ACTION" == install ]] || abort "only one action is allowed"; ACTION=rollback; shift ;;
            --with-schedulerd) WITH_SCHEDULERD=true; shift ;;
            --force) FORCE=true; shift ;;
            --dry-run) DRY_RUN=true; shift ;;
            --help) usage; exit 0 ;;
            *) abort "unknown option: $1" ;;
        esac
    done
}

resolve_mode_and_action() {
    if [[ "$ACTION" != install && "$EXPLICIT_MODE" == false ]]; then MODE=docker; fi
    [[ -z "$MODE" || "$MODE" == docker || "$MODE" == binary ]] || abort "--mode must be docker or binary"
}

validate_args() {
    if [[ "$MODE" == docker && "$WITH_SCHEDULERD" == true ]]; then abort "--with-schedulerd is only available in Binary mode"; fi
    [[ "$FLAVOR" == gnu || "$FLAVOR" == musl ]] || abort "--flavor must be gnu or musl"
    for cmd in curl tar python3; do command -v "$cmd" >/dev/null 2>&1 || abort "requires $cmd"; done
}

detect_platform() {
    OS="$(uname -s)"; ARCH="$(uname -m)"
    case "$OS" in
        Linux) TARGET="x86_64-unknown-linux-${FLAVOR}"; HAS_SYSTEMD=true ;;
        Darwin)
            [[ "$ARCH" == arm64 ]] || abort "Intel Mac binaries are unavailable; use Docker or build from source"
            TARGET=aarch64-apple-darwin; HAS_SYSTEMD=false ;;
        *) abort "unsupported platform: $OS" ;;
    esac
}

download_release_file() {
    local name="$1" destination="$2"
    curl -fsSL "${BASE_URL}/${name}" -o "$destination" || abort "could not download $name"
}

verify_sha256sums_entry() {
    local sums="$1" file="$2" name="$3" expected actual
    expected="$(awk -v name="$name" '$2 == name { print $1 }' "$sums")"
    [[ "$expected" =~ ^[0-9a-fA-F]{64}$ ]] || abort "SHA256SUMS has no valid entry for $name"
    actual="$(sha256_file "$file")"
    [[ "$(lowercase "$expected")" == "$(lowercase "$actual")" ]] || abort "checksum mismatch for $name"
}

read_manifest_field() {
    python3 - "$1" "$2" <<'PY'
import json, sys
document = json.load(open(sys.argv[1], encoding="utf-8"))
value = document
for key in sys.argv[2].split("."):
    if not isinstance(value, dict) or key not in value: raise SystemExit(1)
    value = value[key]
if not isinstance(value, str): raise SystemExit(1)
print(value)
PY
}

validate_manifest_version() {
    local manifest="$1"
    python3 - "$manifest" "$VERSION" <<'PY'
import json, re, sys
try:
    d = json.load(open(sys.argv[1], encoding="utf-8"))
    assert d["schemaVersion"] == 1
    assert d["productTag"] == sys.argv[2]
    image = d["image"]
    assert image["reference"] == "ghcr.io/raybird/wukong:" + sys.argv[2]
    assert re.fullmatch(r"sha256:[0-9a-f]{64}", image["digest"])
    c = d["dataCompatibility"]
    assert c["schemaVersion"] == 1 and isinstance(c["affectedState"], list)
    assert isinstance(c["backupRequired"], bool) and isinstance(c["irreversibleMigration"], bool)
    assert c["instructionsUrl"] is None or isinstance(c["instructionsUrl"], str)
    assert c["rollbackSafeTo"] is None or re.fullmatch(r"v\d+\.\d+\.\d+(?:-rc\.\d+)?", c["rollbackSafeTo"])
except (AssertionError, KeyError, TypeError, json.JSONDecodeError):
    raise SystemExit("invalid release-manifest.json")
PY
}

safe_list_archive() {
    local archive="$1"; shift
    python3 - "$archive" "$@" <<'PY'
import sys, tarfile
archive, allowed = sys.argv[1], set(sys.argv[2:])
try:
    with tarfile.open(archive, "r:gz") as tar:
        names = []
        for member in tar.getmembers():
            name = member.name.rstrip("/")
            if name.startswith("/") or ".." in name.split("/") or member.issym() or member.islnk():
                raise ValueError("unsafe archive entry: " + member.name)
            if member.isdir(): continue
            if not member.isfile() or name not in allowed: raise ValueError("unexpected archive entry: " + member.name)
            names.append(name)
        if len(names) != len(set(names)) or set(names) != allowed: raise ValueError("archive entries do not match allowlist")
except (tarfile.TarError, ValueError) as error:
    raise SystemExit(str(error))
PY
}

validate_archive_entries() { safe_list_archive "$@" || abort "unsafe or unexpected archive contents"; }
extract_archive_to() { tar -xzf "$1" -C "$2"; }

prepare_release_metadata() {
    RELEASE_DIR="$(make_temp_dir)"
    download_release_file SHA256SUMS "$RELEASE_DIR/SHA256SUMS"
    download_release_file release-manifest.json "$RELEASE_DIR/release-manifest.json"
    verify_sha256sums_entry "$RELEASE_DIR/SHA256SUMS" "$RELEASE_DIR/release-manifest.json" release-manifest.json
    validate_manifest_version "$RELEASE_DIR/release-manifest.json"
}

write_json_atomically() {
    local destination="$1" payload="$2" dir temp
    dir="$(dirname "$destination")"; mkdir -p "$dir"; temp="$(mktemp "$dir/.install.json.XXXXXX")"
    chmod 600 "$temp"
    printf '%s\n' "$payload" > "$temp"
    mv -f "$temp" "$destination"
}

write_binary_metadata() {
    local components="$1" services="$2" previous_backup="$3" previous_version="$4" installed_at="${5:-}"
    local payload
    payload="$(python3 - "$VERSION" "$TARGET" "$components" "$services" "$previous_backup" "$previous_version" "$installed_at" "$RELEASE_DIR/release-manifest.json" <<'PY'
import datetime, json, sys
version, target, components, services, previous_backup, previous_version, installed_at, manifest_path = sys.argv[1:]
now = datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")
print(json.dumps({"schemaVersion": 1, "mode": "binary", "target": target, "productTag": version,
                  "components": sorted(set(json.loads(components))), "services": sorted(set(json.loads(services))),
                  "installedAt": installed_at or now, "updatedAt": now,
                   "previousVersion": previous_version or None, "previousBackupPath": previous_backup or None,
                   "dataCompatibility": json.load(open(manifest_path))["dataCompatibility"]}, sort_keys=True))
PY
)"
    write_json_atomically "$METADATA_FILE" "$payload"
}

read_metadata_components() {
    python3 - "$METADATA_FILE" <<'PY'
import json, sys
try:
    d=json.load(open(sys.argv[1])); assert d["schemaVersion"] == 1 and d["mode"] == "binary"
    assert isinstance(d.get("previousVersion"), (str, type(None))) and isinstance(d.get("previousBackupPath"), (str, type(None)))
    assert not (d["previousVersion"] is None and d["previousBackupPath"] is not None)
    assert all(x in {"wukong", "wukong-telegram", "wukong-web", "wukong-schedulerd"} for x in d["components"])
    print(" ".join(d["components"]))
except Exception: raise SystemExit("unknown or invalid install metadata schema")
PY
}

metadata_installed_at() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["installedAt"])' "$METADATA_FILE"; }
metadata_product_tag() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["productTag"])' "$METADATA_FILE"; }

create_legacy_backup() {
    local backup="$1" name unit
    mkdir -p "$backup/units"
    for name in wukong wukong-telegram wukong-web wukong-schedulerd; do [[ ! -f "$INSTALL_DIR/$name" ]] || cp -p "$INSTALL_DIR/$name" "$backup/$name"; done
    for unit in "$HOME"/.config/systemd/user/wukong{-telegram,-web,-schedulerd}.service; do [[ ! -f "$unit" ]] || cp -p "$unit" "$backup/units/$(basename "$unit")"; done
    python3 - "$backup" <<'PY'
import hashlib, json, os, stat, sys
root=sys.argv[1]; entries=[]
for base, _, names in os.walk(root):
    for name in names:
        path=os.path.join(base,name); rel=os.path.relpath(path,root)
        if rel == "manifest.json" or "/" in rel and not rel.startswith("units/"): raise SystemExit("unsafe legacy backup path")
        data=open(path,"rb").read()
        entries.append({"path":rel,"mode":stat.S_IMODE(os.stat(path).st_mode),"sha256":hashlib.sha256(data).hexdigest()})
with open(os.path.join(root,"manifest.json"),"w",encoding="utf-8") as out: json.dump({"schemaVersion":1,"entries":sorted(entries,key=lambda x:x["path"])},out,sort_keys=True)
PY
}

restore_legacy_backup() {
    local backup="$1"
    python3 - "$backup" "$INSTALL_DIR" "$HOME/.config/systemd/user" <<'PY'
import hashlib, json, os, shutil, stat, sys
root, bins, units=sys.argv[1:]
d=json.load(open(os.path.join(root,"manifest.json")))
assert d["schemaVersion"] == 1
allowed={"wukong","wukong-telegram","wukong-web","wukong-schedulerd"}|{"units/wukong-telegram.service","units/wukong-web.service","units/wukong-schedulerd.service"}
for item in d["entries"]:
    rel=item["path"]; assert rel in allowed and not os.path.islink(os.path.join(root,rel))
    source=os.path.join(root,rel); assert hashlib.sha256(open(source,"rb").read()).hexdigest()==item["sha256"]
    destination=os.path.join(units,os.path.basename(rel)) if rel.startswith("units/") else os.path.join(bins,rel)
    os.makedirs(os.path.dirname(destination),exist_ok=True); shutil.copyfile(source,destination); os.chmod(destination,item["mode"])
PY
}

initialize_workspace_templates_if_missing() {
    local workspace="$1"; mkdir -p "$workspace"
    [[ -f "$workspace/SOUL.md" ]] || curl -fsSL "${GITHUB}/${REPO}/raw/${VERSION}/workspace/SOUL.md" -o "$workspace/SOUL.md" || abort "could not initialize SOUL.md"
    [[ -f "$workspace/AGENTS.md" ]] || curl -fsSL "${GITHUB}/${REPO}/raw/${VERSION}/workspace/AGENTS.md" -o "$workspace/AGENTS.md" || abort "could not initialize AGENTS.md"
}

initialize_config_if_missing() {
    [[ -f "$CONFIG_FILE" ]] && return 0
    mkdir -p "$CONFIG_DIR"
    local mem_db workspace embed md_dir web_host web_port web_token tg_token tg_allowed
    if [[ "${1:-interactive}" == noninteractive ]]; then
        printf 'WUKONG_MEMORY_DB="%s"\nWUKONG_WORKSPACE="%s"\nWUKONG_EMBED=0\nWUKONG_THINKING=1\n' \
            "${HOME}/.wukong/memory.db" "${HOME}/.wukong/workspace" > "$CONFIG_FILE"
        initialize_workspace_templates_if_missing "${HOME}/.wukong/workspace"
        return 0
    fi
    read -r -p "Memory database [${HOME}/.wukong/memory.db]: " mem_db
    read -r -p "Workspace [${HOME}/.wukong/workspace]: " workspace
    read -r -p "Enable embeddings? (y/N): " embed
    read -r -p "Markdown mirror (optional): " md_dir
    if [[ " ${COMPONENTS[*]} " == *' wukong-telegram '* ]]; then
        read -r -p "Telegram Bot token: " tg_token
        read -r -p "Telegram allowed user IDs: " tg_allowed
    fi
    if [[ " ${COMPONENTS[*]} " == *' wukong-web '* ]]; then
        read -r -p "Web host [127.0.0.1]: " web_host
        read -r -p "Web port [8787]: " web_port
        read -r -p "Web token (optional): " web_token
    fi
    workspace="${workspace:-${HOME}/.wukong/workspace}"
    {
        printf 'WUKONG_MEMORY_DB="%s"\n' "${mem_db:-${HOME}/.wukong/memory.db}"
        printf 'WUKONG_WORKSPACE="%s"\n' "$workspace"
        printf 'WUKONG_EMBED=%s\n' "$( [[ "$(lowercase "$embed")" =~ ^(y|yes|1)$ ]] && echo 1 || echo 0 )"
        printf 'WUKONG_THINKING=1\n'
        [[ -z "$md_dir" ]] || printf 'WUKONG_MD_DIR="%s"\n' "$md_dir"
        [[ " ${COMPONENTS[*]} " != *' wukong-telegram '* ]] || { printf 'WUKONG_TG_TOKEN="%s"\n' "$tg_token"; printf 'WUKONG_TG_ALLOWED="%s"\n' "$tg_allowed"; }
        if [[ " ${COMPONENTS[*]} " == *' wukong-web '* ]]; then
            printf 'WUKONG_WEB_HOST="%s"\n' "${web_host:-127.0.0.1}"
            printf 'WUKONG_WEB_PORT="%s"\n' "${web_port:-8787}"
            printf 'WUKONG_WEB_TOKEN="%s"\n' "$web_token"
        fi
    } > "$CONFIG_FILE"
    initialize_workspace_templates_if_missing "$workspace"
}

select_components_interactively() {
    local choice scheduler
    printf 'Components: 1) CLI 2) CLI+Telegram 3) CLI+Web 4) all\n'
    read -r -p "Select [1-4] (default 1): " choice
    case "${choice:-1}" in
        1) COMPONENTS=(wukong) ;;
        2) COMPONENTS=(wukong wukong-telegram) ;;
        3) COMPONENTS=(wukong wukong-web) ;;
        4) COMPONENTS=(wukong wukong-telegram wukong-web) ;;
        *) abort "invalid component selection" ;;
    esac
    if [[ "$WITH_SCHEDULERD" == false ]]; then
        read -r -p "Enable Scheduler? (y/N): " scheduler
        if [[ "$(lowercase "$scheduler")" =~ ^(y|yes)$ ]]; then COMPONENTS+=(wukong-schedulerd); fi
    else
        COMPONENTS+=(wukong-schedulerd)
    fi
}

unit_name_for() { printf '%s.service\n' "$1"; }
render_unit() {
    local component="$1" unit="$2"
    cat > "$unit" <<UNIT
# Managed by Wukong install.sh
[Unit]
Description=Wukong ${component}
After=network-online.target
[Service]
Type=simple
EnvironmentFile=%h/.wukong/config.env
ExecStart=%h/.local/bin/${component}
Restart=always
RestartSec=10
[Install]
WantedBy=default.target
UNIT
}

manage_services() {
    [[ "$HAS_SYSTEMD" == true ]] || return 0
    local component unit enabled service_list=() enabled_components=()
    mkdir -p "$HOME/.config/systemd/user"
    for component in "${COMPONENTS[@]}"; do
        [[ "$component" == wukong ]] && continue
        unit="$(unit_name_for "$component")"
        enabled=false
        systemctl --user is-enabled "$unit" >/dev/null 2>&1 && enabled=true
        if [[ -f "$HOME/.config/systemd/user/$unit" ]] && ! grep -Fq 'Managed by Wukong install.sh' "$HOME/.config/systemd/user/$unit"; then abort "refusing to overwrite unmanaged unit: $unit"; fi
        render_unit "$component" "$HOME/.config/systemd/user/$unit"
        [[ "$enabled" == false ]] || enabled_components+=("$component")
    done
    systemctl --user daemon-reload
    for component in "${COMPONENTS[@]}"; do
        [[ "$component" == wukong ]] && continue
        unit="$(unit_name_for "$component")"
        if [[ "$ACTION" == install || "$component" == wukong-schedulerd && "$WITH_SCHEDULERD" == true ]]; then
            systemctl --user enable --now "$unit"
            service_list+=("$unit")
        elif [[ " ${enabled_components[*]} " == *" $component "* ]]; then
            systemctl --user restart "$unit"
            service_list+=("$unit")
        fi
    done
    SERVICES_JSON="$(printf '%s\n' "${service_list[@]}" | python3 -c 'import json,sys; print(json.dumps(sorted(x.strip() for x in sys.stdin if x.strip())))')"
}

install_binary() {
    mkdir -p "$INSTALL_DIR" "$CONFIG_DIR"
    if [[ "$ACTION" == rollback ]]; then
        rollback_binary
        return
    elif [[ "$ACTION" == upgrade ]]; then
        if [[ -f "$METADATA_FILE" ]]; then read -r -a COMPONENTS <<< "$(read_metadata_components)"; INSTALLED_AT="$(metadata_installed_at)"; else
            COMPONENTS=(wukong); for c in wukong-telegram wukong-web wukong-schedulerd; do [[ -x "$INSTALL_DIR/$c" ]] && COMPONENTS+=("$c"); done; INSTALLED_AT=""
        fi
        [[ "$WITH_SCHEDULERD" == false || " ${COMPONENTS[*]} " == *' wukong-schedulerd '* ]] || COMPONENTS+=(wukong-schedulerd)
    else
        if [[ -f "$CONFIG_FILE" ]]; then
            # Existing configuration is user-owned: never re-prompt or rewrite it.
            COMPONENTS=(wukong)
            [[ "$WITH_SCHEDULERD" == false ]] || COMPONENTS+=(wukong-schedulerd)
        else
            select_components_interactively
        fi
        INSTALLED_AT=""
    fi
    prepare_release_metadata
    local stage archive name backup_dir previous previous_version components_json
    stage="$(make_temp_dir)"; backup_dir="${CONFIG_DIR}/backups/${VERSION}-$(date +%s)"; previous=""; previous_version=""
    [[ ! -f "$METADATA_FILE" ]] || previous_version="$(metadata_product_tag)"
    if [[ "$ACTION" == upgrade && ! -f "$METADATA_FILE" && -d "$INSTALL_DIR" && -n "$(ls -A "$INSTALL_DIR" 2>/dev/null)" ]]; then
        previous_version="legacy-$(date -u +%Y%m%dT%H%M%SZ)"
        backup_dir="${CONFIG_DIR}/backups/${previous_version}"
        create_legacy_backup "$backup_dir"
    fi
    for name in "${COMPONENTS[@]}"; do
        archive="$RELEASE_DIR/${name}-${TARGET}.tar.gz"
        download_release_file "${name}-${TARGET}.tar.gz" "$archive"
        verify_sha256sums_entry "$RELEASE_DIR/SHA256SUMS" "$archive" "${name}-${TARGET}.tar.gz"
        validate_archive_entries "$archive" "$name"
        extract_archive_to "$archive" "$stage"
        [[ -f "$stage/$name" ]] || abort "archive did not contain $name"
        chmod +x "$stage/$name"
    done
    mkdir -p "$backup_dir"; previous="$backup_dir"
    for name in "${COMPONENTS[@]}"; do [[ ! -e "$INSTALL_DIR/$name" ]] || cp -p "$INSTALL_DIR/$name" "$backup_dir/$name"; done
    local activation_failed=false
    for name in "${COMPONENTS[@]}"; do
        if ! mv -f "$stage/$name" "$INSTALL_DIR/$name"; then activation_failed=true; break; fi
        [[ "${WUKONG_FAIL_BINARY_ACTIVATION:-0}" != 1 ]] || { activation_failed=true; break; }
    done
    if [[ "$activation_failed" == true ]]; then
        for name in "${COMPONENTS[@]}"; do
            if [[ -f "$backup_dir/$name" ]]; then mv -f "$backup_dir/$name" "$INSTALL_DIR/$name"; else rm -f "$INSTALL_DIR/$name"; fi
        done
        abort "binary activation failed; restored backups"
    fi
    if [[ "$ACTION" == upgrade ]]; then initialize_config_if_missing noninteractive; else initialize_config_if_missing; fi
    SERVICES_JSON='[]'; manage_services
    components_json="$(printf '%s\n' "${COMPONENTS[@]}" | python3 -c 'import json,sys; print(json.dumps(sorted(x.strip() for x in sys.stdin if x.strip())))')"
    write_binary_metadata "$components_json" "$SERVICES_JSON" "$previous" "$previous_version" "$INSTALLED_AT"
}

rollback_binary() {
    [[ -f "$METADATA_FILE" ]] || abort "rollback requires verified install metadata"
    local backup version components_json services_json installed_at name stage
    read -r backup version components_json services_json installed_at < <(python3 - "$METADATA_FILE" "$VERSION" "$VERSION_EXPLICIT" <<'PY'
import json, sys
d=json.load(open(sys.argv[1]))
assert d["schemaVersion"] == 1 and d["mode"] == "binary"
backup=d.get("previousBackupPath"); version=d.get("previousVersion")
assert backup and version
requested=sys.argv[2] if sys.argv[3] == "true" else ""
assert not requested or requested == version
if not version.startswith("legacy-"):
    c=d.get("dataCompatibility", {})
    assert c.get("schemaVersion") == 1 and not c.get("irreversibleMigration") and c.get("rollbackSafeTo") == version
print(backup, version, json.dumps(d["components"]), json.dumps(d["services"]), d["installedAt"])
PY
    ) || abort "rollback source is absent or metadata is invalid"
    [[ -d "$backup" ]] || abort "rollback backup is unavailable"
    if [[ "$version" == legacy-* ]]; then
        restore_legacy_backup "$backup" || abort "legacy backup validation failed"
        rm -f "$METADATA_FILE"
        return
    fi
    stage="$(make_temp_dir)"
    read -r -a COMPONENTS <<< "$(python3 -c 'import json,sys; print(" ".join(json.loads(sys.argv[1])))' "$components_json")"
    for name in "${COMPONENTS[@]}"; do [[ -f "$backup/$name" ]] || abort "rollback backup is incomplete"; cp -p "$backup/$name" "$stage/$name"; done
    VERSION="$version"
    local current_backup="${CONFIG_DIR}/backups/${version}-rollback-$(date +%s)"
    mkdir -p "$current_backup"
    for name in "${COMPONENTS[@]}"; do cp -p "$INSTALL_DIR/$name" "$current_backup/$name"; mv -f "$stage/$name" "$INSTALL_DIR/$name"; done
    SERVICES_JSON="$services_json"
    write_binary_metadata "$components_json" "$services_json" "$current_backup" "$(metadata_product_tag)" "$installed_at"
}

install_docker() {
    command -v docker >/dev/null 2>&1 || abort "Docker is required"
    docker compose version >/dev/null 2>&1 || abort "Docker Compose v2 is required"
    if [[ "$ACTION" == rollback ]]; then
        rollback_docker
        return
    fi
    prepare_release_metadata
    local archive stage expected actual file previous_version="" previous_digest="" backup=""
    if [[ -f .wukong-release ]]; then
        read -r previous_version previous_digest < <(python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); print(d["productTag"], d["imageDigest"])' .wukong-release) || abort "invalid Docker release metadata"
        backup=".wukong-backups/${previous_version}-$(date +%s)"; mkdir -p "$backup"
        for file in "${DOCKER_RELEASE_OWNED[@]}"; do [[ ! -f "$file" ]] || { mkdir -p "$backup/$(dirname "$file")"; cp -p "$file" "$backup/$file"; }; done
    fi
    archive="$RELEASE_DIR/wukong-docker-${VERSION}.tar.gz"
    download_release_file "wukong-docker-${VERSION}.tar.gz" "$archive"
    verify_sha256sums_entry "$RELEASE_DIR/SHA256SUMS" "$archive" "wukong-docker-${VERSION}.tar.gz"
    validate_archive_entries "$archive" wukong-docker/docker-compose.yml wukong-docker/.env.example wukong-docker/LICENSE wukong-docker/scripts/install.sh wukong-docker/release-manifest.json
    stage="$(mktemp -d "${PWD}/.wukong-stage.XXXXXX")"; TEMP_DIRS+=("$stage")
    extract_archive_to "$archive" "$stage"
    expected="$(read_manifest_field "$RELEASE_DIR/release-manifest.json" image.digest)"
    COMPOSE_PROJECT_NAME=wukong docker compose pull
    actual="$(docker image inspect "ghcr.io/raybird/wukong:${VERSION}" --format '{{index .RepoDigests 0}}' | sed -n 's/.*@\(sha256:[0-9a-f]*\).*/\1/p')"
    [[ "$actual" == "$expected" ]] || abort "pulled image digest does not match release manifest"
    for file in "${DOCKER_RELEASE_OWNED[@]}"; do mkdir -p "$(dirname "$file")"; cp "$stage/wukong-docker/$file" "$file"; done
    [[ -f .env ]] || cp .env.example .env
    if ! COMPOSE_PROJECT_NAME=wukong docker compose up -d --force-recreate || ! docker compose ps >/dev/null; then
        # A failed recreation must not leave release-owned files or metadata advanced.
        if [[ -n "$backup" ]]; then
            for file in "${DOCKER_RELEASE_OWNED[@]}"; do [[ ! -f "$backup/$file" ]] || cp -p "$backup/$file" "$file"; done
            COMPOSE_PROJECT_NAME=wukong docker compose up -d --force-recreate || true
        else
            for file in "${DOCKER_RELEASE_OWNED[@]}"; do rm -f "$file"; done
        fi
        abort "Docker activation or health check failed; restored previous release files"
    fi
    write_json_atomically .wukong-release "$(python3 - "$VERSION" "$expected" "$previous_version" "$previous_digest" "$backup" "$RELEASE_DIR/release-manifest.json" <<'PY'
import json,sys
print(json.dumps({"schemaVersion":1,"productTag":sys.argv[1],"imageDigest":sys.argv[2],"previousVersion":sys.argv[3] or None,"previousImageDigest":sys.argv[4] or None,"previousBackupPath":sys.argv[5] or None,"dataCompatibility":json.load(open(sys.argv[6]))["dataCompatibility"]},sort_keys=True))
PY
)"
}

rollback_docker() {
    [[ -f .wukong-release ]] || abort "rollback requires Docker release metadata"
    local version digest backup current_backup file
    read -r version digest backup current_version current_digest < <(python3 - .wukong-release "$VERSION" <<'PY'
import json,sys
d=json.load(open(sys.argv[1])); requested=sys.argv[2]
assert d["schemaVersion"] == 1 and d.get("previousVersion") and d.get("previousImageDigest") and d.get("previousBackupPath")
assert not requested or requested == d["previousVersion"]
compatibility=d.get("dataCompatibility", {})
assert compatibility.get("schemaVersion") == 1 and not compatibility.get("irreversibleMigration") and compatibility.get("rollbackSafeTo") == d["previousVersion"]
print(d["previousVersion"], d["previousImageDigest"], d["previousBackupPath"], d["productTag"], d["imageDigest"])
PY
    ) || abort "rollback source is absent or metadata is invalid"
    [[ -d "$backup" ]] || abort "Docker rollback backup is unavailable"
    current_backup=".wukong-backups/${version}-rollback-$(date +%s)"; mkdir -p "$current_backup"
    for file in "${DOCKER_RELEASE_OWNED[@]}"; do [[ ! -f "$file" ]] || { mkdir -p "$current_backup/$(dirname "$file")"; cp -p "$file" "$current_backup/$file"; }; [[ ! -f "$backup/$file" ]] || cp -p "$backup/$file" "$file"; done
    COMPOSE_PROJECT_NAME=wukong docker compose pull
    COMPOSE_PROJECT_NAME=wukong docker compose up -d --force-recreate
    docker compose ps >/dev/null
    write_json_atomically .wukong-release "$(python3 - "$version" "$digest" "$current_backup" "$current_version" "$current_digest" <<'PY'
import json,sys
print(json.dumps({"schemaVersion":1,"productTag":sys.argv[1],"imageDigest":sys.argv[2],"previousVersion":sys.argv[4],"previousImageDigest":sys.argv[5],"previousBackupPath":sys.argv[3]},sort_keys=True))
PY
)"
}

parse_args "$@"
resolve_mode_and_action
validate_args
detect_platform
if [[ -z "$VERSION" ]]; then VERSION="$(curl -fsSL "${API}/${REPO}/releases/latest" | python3 -c 'import json,sys; print(json.load(sys.stdin)["tag_name"])')" || abort "could not resolve latest release"; fi
BASE_URL="${GITHUB}/${REPO}/releases/download/${VERSION}"
if [[ -z "$MODE" ]]; then read -r -p "Mode [docker/binary] (default docker): " MODE; MODE="${MODE:-docker}"; fi
if [[ "$DRY_RUN" == true ]]; then info "dry-run: mode=$MODE action=$ACTION version=$VERSION"; exit 0; fi
if [[ "$MODE" == docker ]]; then install_docker; else install_binary; fi
info "Wukong ${ACTION} completed: ${VERSION}"
