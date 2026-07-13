# Docker Compose Project Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Preserve the Compose project identity of fresh and legacy Docker deployments through install, upgrade, recovery, and rollback.

**Architecture:** Add one shell resolver that combines persisted metadata, existing container labels, an explicit fresh-install override, and the `wukong` default into `DOCKER_PROJECT_NAME`. Run every installer-owned Compose command with `docker compose -p "$DOCKER_PROJECT_NAME"`, and persist the resolved project in schema-1 Docker metadata without changing Binary behavior.

**Tech Stack:** Bash, embedded Python 3 JSON parsing, Docker Compose v2, shell fixture tests.

## Global Constraints

- Run `gitnexus_impact` before editing every existing shell function.
- Run `gitnexus_detect_changes` before every commit.
- Preserve `.wukong-release` schema version 1; legacy files may omit `composeProject`.
- Valid project names match `[a-z0-9][a-z0-9_-]*`.
- Never rename, copy, or delete Docker volumes.
- Never invoke `docker compose down` or `down -v`.
- Resolve ownership before downloads, backups, file replacement, image pulls, or container recreation.
- An explicit `COMPOSE_PROJECT_NAME` may select a fresh install project but may not override existing metadata or container labels.

---

## File Structure

- Modify `scripts/install.sh`: resolve project ownership, route Compose operations through the resolved name, and persist metadata.
- Modify `scripts/test-installer-upgrade.sh`: emulate container labels and verify fresh, legacy, conflict, recovery, and rollback behavior.
- Modify `docs/docker.md`: document automatic legacy project preservation and the fresh-install-only override.
- Modify `docs/installation.md`: document Docker project identity in persistent metadata and upgrade safety.

### Task 1: Lock Project Resolution Behavior with Tests

**Files:**
- Modify: `scripts/test-installer-upgrade.sh:69-84`
- Modify: `scripts/test-installer-upgrade.sh:150-260`
- Test: `scripts/test-installer-upgrade.sh`

**Interfaces:**
- Consumes: `run_installer`, fake `docker`, `.wukong-release` fixture.
- Produces: fixture variables `FIXTURE_DOCKER_PROJECT`, `FIXTURE_OPENCODE_PROJECT`, and `FIXTURE_WEB_PROJECT`; assertions for `docker compose -p <project>` and persisted `composeProject`.

- [ ] **Step 1: Run impact analysis before editing the test helpers**

Run `gitnexus_impact({target: "make_fakes", direction: "upstream", includeTests: true, repo: "Wukong"})` and report the result. Shell helpers may be absent from the graph; record `UNKNOWN` and manually limit the blast radius to `scripts/test-installer-upgrade.sh` if so.

- [ ] **Step 2: Extend fake Docker with deterministic project labels**

Add this branch before the existing `docker image inspect` branch:

```bash
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
```

The fake continues logging every Docker command before branching.

- [ ] **Step 3: Add fresh and legacy project tests**

Add:

```bash
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
```

- [ ] **Step 4: Add ambiguity and override rejection tests**

Add:

```bash
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
```

Add a persistence test:

```bash
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
```

Register `docker-project`, `docker-project-conflicts`, and `docker-project-persistence` cases and include all three in `all`.

- [ ] **Step 5: Run tests to verify RED**

Run:

```bash
scripts/test-installer-upgrade.sh docker-project
scripts/test-installer-upgrade.sh docker-project-conflicts
scripts/test-installer-upgrade.sh docker-project-persistence
```

Expected: the first fails because Compose commands still use hard-coded `wukong` and metadata lacks `composeProject`; the second fails because no resolver rejects conflicting evidence; the third fails because project metadata is not preserved through upgrade and rollback.

- [ ] **Step 6: Commit the failing tests**

Run `gitnexus_detect_changes({scope: "unstaged", repo: "Wukong"})`, verify only the installer test file is affected, then:

```bash
git add -- scripts/test-installer-upgrade.sh
git commit -m "test(installer): cover Compose project ownership"
```

### Task 2: Resolve and Persist Compose Project Ownership

**Files:**
- Modify: `scripts/install.sh:20-22`
- Modify: `scripts/install.sh:149-180`
- Modify: `scripts/install.sh:457-543`
- Test: `scripts/test-installer-upgrade.sh`

**Interfaces:**
- Consumes: `.wukong-release`, `COMPOSE_PROJECT_NAME`, Docker container labels.
- Produces: global `DOCKER_PROJECT_NAME`, function `resolve_docker_project`, required metadata field `composeProject` on newly-written Docker transactions.

- [ ] **Step 1: Run impact analysis on modified functions**

Run and report:

```text
gitnexus_impact({target: "install_docker", direction: "upstream", includeTests: true, repo: "Wukong"})
gitnexus_impact({target: "rollback_docker", direction: "upstream", includeTests: true, repo: "Wukong"})
gitnexus_impact({target: "skip_current_upgrade", direction: "upstream", includeTests: true, repo: "Wukong"})
```

If GitNexus does not index the shell symbols, report `UNKNOWN`; manual callers are the script entrypoint and `install_docker` rollback branch.

- [ ] **Step 2: Add project globals and resolver**

Add near the existing globals:

```bash
DOCKER_PROJECT_NAME=""
DOCKER_CONTAINER_NAMES=(wukong-cli wukong-opencode-server wukong-telegram wukong-web wukong-schedulerd)
```

Add after archive helpers:

```bash
valid_compose_project() { [[ "$1" =~ ^[a-z0-9][a-z0-9_-]*$ ]]; }

resolve_docker_project() {
    local metadata_project="" labeled_project="" explicit="${COMPOSE_PROJECT_NAME:-}" name discovered=""
    if [[ -f .wukong-release ]]; then
        metadata_project="$(python3 - .wukong-release <<'PY'
import json, sys
try:
    value=json.load(open(sys.argv[1], encoding="utf-8")).get("composeProject", "")
    if not isinstance(value, str): raise ValueError
    print(value)
except (OSError, ValueError, TypeError, json.JSONDecodeError):
    raise SystemExit(1)
PY
)" || abort "invalid Docker release metadata"
    fi
    [[ -z "$metadata_project" ]] || valid_compose_project "$metadata_project" || abort "invalid Compose project in Docker release metadata"

    for name in "${DOCKER_CONTAINER_NAMES[@]}"; do
        if discovered="$(docker inspect --format '{{ with index .Config.Labels "com.docker.compose.project" }}{{ . }}{{ end }}' "$name" 2>/dev/null)"; then
            [[ -n "$discovered" ]] || abort "existing container $name has no Compose project label"
            valid_compose_project "$discovered" || abort "existing container $name has an invalid Compose project label"
            [[ -z "$labeled_project" || "$labeled_project" == "$discovered" ]] || abort "Wukong containers belong to multiple Compose projects"
            labeled_project="$discovered"
        fi
    done

    [[ -z "$metadata_project" || -z "$labeled_project" || "$metadata_project" == "$labeled_project" ]] || abort "Docker metadata and containers disagree on Compose project"
    [[ -z "$explicit" ]] || valid_compose_project "$explicit" || abort "invalid COMPOSE_PROJECT_NAME"
    if [[ -n "$explicit" && -n "${metadata_project:-$labeled_project}" && "$explicit" != "${metadata_project:-$labeled_project}" ]]; then
        abort "COMPOSE_PROJECT_NAME cannot replace an existing Compose project"
    fi
    DOCKER_PROJECT_NAME="${metadata_project:-${labeled_project:-${explicit:-wukong}}}"
}
```

- [ ] **Step 3: Resolve before Docker no-op or mutation**

At the start of `install_docker`, after Docker/Compose availability checks:

```bash
resolve_docker_project
skip_current_upgrade
```

Add `skip_current_upgrade` as the first operation in `install_binary`, before `mkdir -p`. Remove the global `skip_current_upgrade` call from the script entrypoint. This ensures Docker ownership conflicts are checked before a same-version no-op while retaining Binary no-op behavior.

- [ ] **Step 4: Route install, recovery, and health checks through the project**

Replace hard-coded or implicit Compose calls in `install_docker` with:

```bash
docker compose -p "$DOCKER_PROJECT_NAME" --project-directory "$PWD" -f "$stage/wukong-docker/docker-compose.yml" pull
docker compose -p "$DOCKER_PROJECT_NAME" up -d --force-recreate
docker compose -p "$DOCKER_PROJECT_NAME" ps >/dev/null
```

The recovery `up` command must also use `-p "$DOCKER_PROJECT_NAME"`.

- [ ] **Step 5: Persist the project in install metadata**

Pass `"$DOCKER_PROJECT_NAME"` to the embedded Python writer and include:

```python
"composeProject": sys.argv[6],
```

Adjust following argument indexes so the release manifest is still read from the final argument. Keep `schemaVersion: 1` and all existing rollback/data-compatibility fields.

- [ ] **Step 6: Preserve the project through rollback**

Run `resolve_docker_project` before the rollback branch. Replace rollback pull, activation, and status commands with `docker compose -p "$DOCKER_PROJECT_NAME" ...`. Pass the resolved name into the rollback metadata writer and include:

```python
"composeProject": sys.argv[6]
```

- [ ] **Step 7: Run focused and full tests to verify GREEN**

Run:

```bash
bash -n scripts/install.sh scripts/test-installer-upgrade.sh
scripts/test-installer-upgrade.sh docker-project
scripts/test-installer-upgrade.sh docker-project-conflicts
scripts/test-installer-upgrade.sh docker-project-persistence
scripts/test-installer-upgrade.sh docker-rollback
scripts/test-installer-upgrade.sh docker-recovery
scripts/test-installer-upgrade.sh all
```

Expected: every command exits 0; each case prints `installer upgrade checks passed (<case>)`.

- [ ] **Step 8: Commit resolver and metadata behavior**

Run `gitnexus_detect_changes({scope: "unstaged", repo: "Wukong"})`, confirm low risk and only expected installer/test scope, then:

```bash
git add -- scripts/install.sh scripts/test-installer-upgrade.sh
git commit -m "fix(installer): preserve legacy Compose projects"
```

### Task 3: Document Compatibility and Complete Verification

**Files:**
- Modify: `docs/docker.md:35-49`
- Modify: `docs/installation.md:35-86`
- Test: `scripts/test-installer-upgrade.sh`

**Interfaces:**
- Consumes: `composeProject` metadata contract and resolver behavior from Task 2.
- Produces: user-facing upgrade and override guidance.

- [ ] **Step 1: Update Docker deployment documentation**

Add after the Docker upgrade example:

```markdown
Installer 會保留既有部署的 Docker Compose project。舊部署若由目錄名稱產生 `runwukong` 等 project，升級與 rollback 會沿用原 project 與 named volumes；新安裝預設使用 `wukong`。請勿為了解決容器名稱衝突而刪除 volumes。

全新安裝可用 `COMPOSE_PROJECT_NAME=<name>` 選擇 project。既有 metadata 或容器 labels 已確立 project 時，installer 會拒絕不同的手動值，避免連到空白 volumes。
```

- [ ] **Step 2: Update installation metadata documentation**

Document that Docker `.wukong-release` stores `composeProject`, that legacy schema-1 metadata is detected from container labels, and that ambiguous ownership aborts before mutation.

- [ ] **Step 3: Run final verification**

Run:

```bash
bash -n scripts/install.sh scripts/test-installer-upgrade.sh
scripts/test-installer-upgrade.sh all
git diff --check
git status --short
```

Expected: shell syntax succeeds, all installer checks pass, `git diff --check` has no output, and status shows only the two documentation files.

- [ ] **Step 4: Run final GitNexus scope check and commit**

Run `gitnexus_detect_changes({scope: "unstaged", repo: "Wukong"})`, review the expected low-risk documentation-only scope, then:

```bash
git add -- docs/docker.md docs/installation.md
git commit -m "docs(installer): explain Compose project preservation"
```

- [ ] **Step 5: Verify branch state before publication**

Run:

```bash
git status --short
git log --oneline -4
```

Expected: the worktree is clean and the design, tests, implementation, and documentation commits are present in order. Publication to `origin/main` requires the user's explicit release request or the already-established release scope for this task.
