# Release Docker Installer Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Wukong's Docker install path release-based: the installer can deploy a Docker bundle into the current directory, and the Dockerfile downloads release binaries instead of compiling Rust.

**Architecture:** `scripts/install.sh` becomes a two-mode installer: Docker mode downloads and extracts `wukong-docker-${VERSION}.tar.gz`; binary mode preserves the existing binary install flow. The release Docker bundle contains deployable Docker files, and its Dockerfile builds an image by downloading the same release's Linux binaries.

**Tech Stack:** Bash, Docker Compose v2, GitHub Actions, GitHub Release assets, Debian slim runtime, Wukong release tarballs.

---

## File Map

- Modify `Dockerfile`: replace Rust source build stage with release-binary downloader stage; keep runtime dependencies, workspace templates, and entrypoint behavior.
- Modify `scripts/install.sh`: add `--mode docker|binary`, `--force`, `--dry-run`; add Docker mode flow; keep binary mode behavior intact.
- Modify `.github/workflows/release.yml`: package Docker bundle as `wukong-docker-${VERSION}.tar.gz` and ensure bundle files are complete.
- Modify `README.md`: document the two installer modes and clarify Docker uses release binaries.
- Optional modify `.env.example`: only if implementation reveals a missing Docker variable; otherwise leave unchanged.

---

### Task 1: Convert Dockerfile to Release Binary Downloads

**Files:**
- Modify: `Dockerfile`

- [ ] **Step 1: Inspect current Dockerfile and identify symbol impact**

Run:

```bash
gitnexus_impact target=Dockerfile direction=upstream repo=Wukong || true
```

Expected: If GitNexus is unavailable for this repository, record that the repo is not indexed and proceed with file-level review. If it returns HIGH or CRITICAL risk, stop and report the risk before editing.

- [ ] **Step 2: Replace the Rust builder stage with a downloader stage**

Edit `Dockerfile` so the top of the file is:

```dockerfile
# syntax=docker/dockerfile:1
# ── Build stage: download release binaries ──
ARG VERSION=v0.13.1
ARG TARGET=x86_64-unknown-linux-musl
ARG REPO=raybird/Wukong
FROM debian:bookworm-slim AS downloader

ARG VERSION
ARG TARGET
ARG REPO

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl tar && \
    rm -rf /var/lib/apt/lists/*

WORKDIR /bins

RUN set -eux; \
    base_url="https://github.com/${REPO}/releases/download/${VERSION}"; \
    for bin in wukong wukong-telegram wukong-web; do \
      tarball="${bin}-${TARGET}.tar.gz"; \
      curl -fsSL "${base_url}/${tarball}" -o "/tmp/${tarball}"; \
      tar -xzf "/tmp/${tarball}" -C /bins "${bin}"; \
      chmod +x "/bins/${bin}"; \
      rm -f "/tmp/${tarball}"; \
    done
```

Keep the existing runtime stage, including `nodejs npm ripgrep fzf`, `npm install -g opencode-ai@latest`, user creation, workspace template copy, and entrypoint copy.

- [ ] **Step 3: Replace binary COPY lines**

Change the three source-build copy lines to:

```dockerfile
# Copy wukong binaries from downloader stage
COPY --from=downloader /bins/wukong /usr/local/bin/wukong
COPY --from=downloader /bins/wukong-telegram /usr/local/bin/wukong-telegram
COPY --from=downloader /bins/wukong-web /usr/local/bin/wukong-web
```

- [ ] **Step 4: Verify Dockerfile syntax path without building**

Run:

```bash
docker build --help >/dev/null
```

Expected: exits 0, confirming Docker CLI is available for later build verification. If Docker is unavailable in the environment, note it and continue to script/static verification.

- [ ] **Step 5: Commit Dockerfile change**

Run:

```bash
git add Dockerfile
git commit -m "fix(docker): build image from release binaries"
```

Expected: commit includes only `Dockerfile`.

---

### Task 2: Add Installer Mode Parsing and Docker Helpers

**Files:**
- Modify: `scripts/install.sh`

- [ ] **Step 1: Inspect installer impact**

Run:

```bash
gitnexus_impact target=install.sh direction=upstream repo=Wukong || true
```

Expected: If GitNexus is unavailable for this repository, record that the repo is not indexed and proceed. If HIGH or CRITICAL risk is reported, stop and report before editing.

- [ ] **Step 2: Add mode variables near existing argument defaults**

In `scripts/install.sh`, change the argument defaults block to:

```bash
VERSION=""
FLAVOR="musl"    # Linux default: static musl
MODE=""
FORCE=false
DRY_RUN=false
```

- [ ] **Step 3: Update usage text**

Replace the `usage()` heredoc content with:

```bash
Usage: install.sh [--mode docker|binary] [--version v0.13.1] [--flavor gnu|musl] [--force] [--dry-run]

Options:
  --mode <name>    docker: deploy Docker bundle into current directory; binary: install host binaries
  --version <tag>  Install a specific version (default: latest)
  --flavor <name>  Binary mode Linux only: gnu (glibc) or musl (static, default)
  --force          Docker mode only: overwrite generated bundle files except .env
  --dry-run        Print planned actions without writing files or starting services
  --help           Show this help
```

- [ ] **Step 4: Extend argument parsing**

Add cases to the existing `while (($#)); do` block:

```bash
    --mode)    MODE="${2:?missing mode}"; shift 2 ;;
    --force)   FORCE=true; shift ;;
    --dry-run) DRY_RUN=true; shift ;;
```

After parsing, add validation:

```bash
case "$MODE" in
  ""|docker|binary) ;;
  *) abort "--mode 必須是 docker 或 binary，收到: $MODE" ;;
esac
```

- [ ] **Step 5: Add Docker helper functions after `step()`**

Insert:

```bash
has_docker_compose() {
  docker compose version >/dev/null 2>&1
}

copy_env_if_needed() {
  if [[ -f ".env" ]]; then
    info ".env 已存在，保留現有設定"
    return 0
  fi
  if $DRY_RUN; then
    info "dry-run: 會由 .env.example 建立 .env"
    return 0
  fi
  cp .env.example .env
  info "已建立 .env，請依需求編輯"
}

ensure_no_conflicts() {
  local conflicts=()
  local path
  for path in docker-compose.yml Dockerfile .env.example scripts/docker-entrypoint.sh workspace/SOUL.md workspace/AGENTS.md; do
    if [[ -e "$path" && "$path" != ".env" ]]; then
      conflicts+=("$path")
    fi
  done
  if (( ${#conflicts[@]} > 0 )) && ! $FORCE; then
    printf '%s\n' "$(red "✗") 目前目錄已有 Docker 部署檔案，避免覆蓋：" >&2
    printf '  - %s\n' "${conflicts[@]}" >&2
    abort "若要覆蓋，請加 --force"
  fi
}
```

- [ ] **Step 6: Add Docker install function before platform detection**

Insert:

```bash
install_docker_bundle() {
  local bundle="wukong-docker-${VERSION}.tar.gz"
  local url="${BASE_URL}/${bundle}"

  step "準備 Docker 模式部署..."
  command -v docker >/dev/null 2>&1 || abort "需要 Docker，請先安裝 Docker"
  has_docker_compose || abort "需要 Docker Compose v2（docker compose）"

  ensure_no_conflicts

  if $DRY_RUN; then
    info "dry-run: 會下載 ${url}"
    info "dry-run: 會解壓到目前目錄 $(pwd)"
    copy_env_if_needed
    return 0
  fi

  step "下載 ${bundle} ..."
  curl -fsSL "$url" -o "/tmp/${bundle}" || abort "無法下載 Docker bundle: ${bundle}"

  step "解壓 Docker 部署檔案..."
  tar -xzf "/tmp/${bundle}" --strip-components=1
  rm -f "/tmp/${bundle}"

  copy_env_if_needed

  info "Docker 部署檔案已建立於 $(pwd)"
  echo ""
  echo "下一步："
  echo "  1. 視需求編輯 .env"
  echo "  2. 執行 docker compose up -d"
  echo "  3. 開啟 http://localhost:8787/"
  echo ""

  read -r -p "是否現在啟動 docker compose up -d？(y/N): " START_DOCKER
  case "$(printf '%s' "${START_DOCKER:-n}" | tr '[:upper:]' '[:lower:]')" in
    y|yes)
      docker compose up -d
      ;;
    *)
      info "略過啟動，可稍後執行 docker compose up -d"
      ;;
  esac
}
```

- [ ] **Step 7: Commit parser/helper changes**

Run:

```bash
bash -n scripts/install.sh
git add scripts/install.sh
git commit -m "feat(installer): add docker mode options"
```

Expected: shell syntax check passes; commit includes only `scripts/install.sh`.

---

### Task 3: Split Installer Flow Between Docker and Binary Modes

**Files:**
- Modify: `scripts/install.sh`

- [ ] **Step 1: Add interactive mode prompt after version resolution**

After:

```bash
info "版本: ${VERSION}"
```

Add:

```bash
if [[ -z "$MODE" ]]; then
  echo ""
  echo "$(bold '安裝模式')"
  echo "  [1] Docker mode（推薦，部署到目前目錄）"
  echo "  [2] Binary mode（安裝到 ~/.local/bin）"
  read -r -p "選擇 [1-2] (預設 1): " MODE_CHOICE
  case "${MODE_CHOICE:-1}" in
    1) MODE="docker" ;;
    2) MODE="binary" ;;
    *) abort "請輸入 1 或 2" ;;
  esac
fi
```

- [ ] **Step 2: Move Docker mode before binary install work**

Immediately after `BASE_URL="${GITHUB}/${REPO}/releases/download/${VERSION}"`, add:

```bash
if [[ "$MODE" == "docker" ]]; then
  install_docker_bundle
  exit 0
fi
```

This ensures Docker mode does not create `~/.local/bin`, edit `~/.bashrc`, ask binary component questions, or configure systemd.

- [ ] **Step 3: Keep binary prerequisites binary-only**

Ensure the existing prerequisite block:

```bash
for cmd in curl tar grep; do
```

and the `mkdir -p "$INSTALL_DIR"` / PATH update block remain after the Docker-mode early exit, so they only run for binary mode.

- [ ] **Step 4: Add dry-run behavior to binary mode downloads**

At the top of `download_and_verify()`, after `local tarball=...`, add:

```bash
  if $DRY_RUN; then
    info "dry-run: 會下載並驗證 ${BASE_URL}/${tarball}"
    return 0
  fi
```

- [ ] **Step 5: Verify non-interactive dry runs**

Run:

```bash
bash -n scripts/install.sh
scripts/install.sh --mode docker --version v0.13.1 --dry-run
scripts/install.sh --mode binary --version v0.13.1 --dry-run < /dev/null || true
```

Expected:

- Syntax check passes.
- Docker dry-run prints the bundle URL and target directory without writing files.
- Binary dry-run reaches the first interactive binary prompt or exits due stdin; it must not attempt network binary downloads.

- [ ] **Step 6: Commit flow split**

Run:

```bash
git add scripts/install.sh
git commit -m "feat(installer): support docker bundle installation"
```

---

### Task 4: Fix Release Bundle Naming and Contents

**Files:**
- Modify: `.github/workflows/release.yml`

- [ ] **Step 1: Inspect workflow impact**

Run:

```bash
gitnexus_impact target=release.yml direction=upstream repo=Wukong || true
```

Expected: If GitNexus is unavailable for this repository, record that the repo is not indexed and proceed.

- [ ] **Step 2: Update Docker bundle packaging step**

In `.github/workflows/release.yml`, replace the Docker bundle packaging command block with:

```yaml
      - name: Package Docker compose bundle
        if: matrix.target == 'x86_64-unknown-linux-musl'
        run: |
          VERSION="${GITHUB_REF_NAME}"
          mkdir -p dist/wukong-docker/scripts dist/wukong-docker/workspace
          cp .env.example dist/wukong-docker/.env.example
          cp Dockerfile dist/wukong-docker/Dockerfile
          cp scripts/docker-entrypoint.sh dist/wukong-docker/scripts/docker-entrypoint.sh
          cp docker-compose.yml dist/wukong-docker/docker-compose.yml
          cp workspace/SOUL.md dist/wukong-docker/workspace/SOUL.md
          cp workspace/AGENTS.md dist/wukong-docker/workspace/AGENTS.md
          cd dist
          tar -czf "wukong-docker-${VERSION}.tar.gz" wukong-docker
          rm -rf wukong-docker
```

This changes the release asset from `wukong-docker.tar.gz` to `wukong-docker-${VERSION}.tar.gz`, matching installer expectations.

- [ ] **Step 3: Add local packaging smoke command to verify paths**

Run locally:

```bash
VERSION="v0.0.0-test"; rm -rf /tmp/wukong-docker-test; mkdir -p /tmp/wukong-docker-test/dist/wukong-docker/scripts /tmp/wukong-docker-test/dist/wukong-docker/workspace; cp .env.example /tmp/wukong-docker-test/dist/wukong-docker/.env.example; cp Dockerfile /tmp/wukong-docker-test/dist/wukong-docker/Dockerfile; cp scripts/docker-entrypoint.sh /tmp/wukong-docker-test/dist/wukong-docker/scripts/docker-entrypoint.sh; cp docker-compose.yml /tmp/wukong-docker-test/dist/wukong-docker/docker-compose.yml; cp workspace/SOUL.md /tmp/wukong-docker-test/dist/wukong-docker/workspace/SOUL.md; cp workspace/AGENTS.md /tmp/wukong-docker-test/dist/wukong-docker/workspace/AGENTS.md; (cd /tmp/wukong-docker-test/dist && tar -czf "wukong-docker-${VERSION}.tar.gz" wukong-docker && tar -tzf "wukong-docker-${VERSION}.tar.gz")
```

Expected tar listing includes exactly the deployment directory and required files:

```text
wukong-docker/
wukong-docker/.env.example
wukong-docker/Dockerfile
wukong-docker/docker-compose.yml
wukong-docker/scripts/docker-entrypoint.sh
wukong-docker/workspace/SOUL.md
wukong-docker/workspace/AGENTS.md
```

- [ ] **Step 4: Commit workflow change**

Run:

```bash
git add .github/workflows/release.yml
git commit -m "fix(release): version docker bundle asset"
```

---

### Task 5: Update README for Two Install Modes

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Inspect README impact**

Run:

```bash
gitnexus_impact target=README.md direction=upstream repo=Wukong || true
```

Expected: If GitNexus is unavailable for this repository, record that docs are file-level only and proceed.

- [ ] **Step 2: Replace quick install description**

Replace README lines around the quick install section with this text:

```markdown
腳本會自動偵測版本，並詢問使用 Docker mode 或 Binary mode：

- **Docker mode（推薦）**：在目前目錄下載 release Docker bundle，產生 `docker-compose.yml`、`.env.example`、`.env`、`Dockerfile`、entrypoint 與 workspace templates，並透過 Docker 建立隔離執行環境。Dockerfile 會下載 release binaries，不會在本機編譯 Rust。
- **Binary mode**：下載最新預編譯 binary 到 `~/.local/bin`，並以互動問答設定 Telegram / Web / 記憶等選項。
```

- [ ] **Step 3: Update manual installer options**

Replace the manual options block with:

```bash
# 指定 Docker 模式部署到目前目錄
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode docker --version v0.13.1

# 指定 Binary 模式安裝到 ~/.local/bin
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode binary --version v0.13.1

# Linux binary mode 可選 linking flavor：
curl -fsSL ... | bash -s -- --mode binary --flavor gnu   # glibc (動態)
curl -fsSL ... | bash -s -- --mode binary --flavor musl  # musl  (靜態，預設，跨 distro)
```

- [ ] **Step 4: Update Docker quick start to start from installer**

In the Docker section, before the current `cp .env.example .env` instructions, add:

```markdown
若你不是從 Git repository 使用，而是在空目錄部署，建議直接使用 installer：

```bash
mkdir wukong-docker && cd wukong-docker
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash -s -- --mode docker
```

installer 會從 GitHub Release 下載 Docker bundle；bundle 內的 Dockerfile 會再下載同版本 Wukong binaries，因此不需要 Rust 或原始碼。
```

- [ ] **Step 5: Commit docs update**

Run:

```bash
git add README.md
git commit -m "docs: document docker installer mode"
```

---

### Task 6: End-to-End Verification and Final Review

**Files:**
- Read/verify all touched files
- No expected code creation unless fixes are found

- [ ] **Step 1: Run syntax and dry-run checks**

Run:

```bash
bash -n scripts/install.sh
scripts/install.sh --mode docker --version v0.13.1 --dry-run
docker compose config >/tmp/wukong-compose-config.yml
```

Expected:

- `bash -n` exits 0.
- Docker dry-run prints `wukong-docker-v0.13.1.tar.gz` URL and does not write files.
- `docker compose config` exits 0.

- [ ] **Step 2: Verify Docker build if Docker is available**

Run:

```bash
docker build --build-arg VERSION=v0.13.1 -t wukong:release-binary-test .
docker run --rm wukong:release-binary-test wukong --help
```

Expected:

- Image build downloads release tarballs instead of running `cargo build`.
- `wukong --help` exits 0.

If Docker daemon is unavailable, record the skipped verification in the final summary.

- [ ] **Step 3: Inspect final diff and affected scope**

Run:

```bash
git status --short
git diff HEAD
```

Expected: only intended files have changes since the last task commit. Existing unrelated changes to `AGENTS.md` or `CLAUDE.md` must remain untouched and unstaged.

- [ ] **Step 4: Run GitNexus change detection before final commit or push**

Run:

```bash
gitnexus_detect_changes scope=all repo=Wukong || true
```

Expected: If GitNexus is unavailable for this repository, record that change detection could not run. If available, confirm affected flows are limited to installer, Docker packaging, and docs.

- [ ] **Step 5: Push only after user asks or if this execution session explicitly includes push permission**

Run only when requested:

```bash
git push
```

Expected: branch pushes cleanly. Do not push without explicit permission.

---

## Self-Review Notes

- Spec coverage: installer modes, Docker release bundle, Dockerfile binary downloads, release workflow naming, overwrite rules, docs, and verification are each covered by a task.
- Placeholder scan: no `TBD`, `TODO`, or vague implementation-only steps remain.
- Type/name consistency: installer flags use `MODE`, `FORCE`, `DRY_RUN`; release asset name consistently uses `wukong-docker-${VERSION}.tar.gz`; Docker defaults consistently use `x86_64-unknown-linux-musl`.
