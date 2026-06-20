# Agent Reach Docker Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Wukong's Docker opencode runtime ready for Agent Reach internet retrieval and GitHub CLI workflows while keeping user-specific setup interactive and persistent.

**Architecture:** The Docker image preinstalls baseline tools and the `agent-reach` CLI, but it does not run `agent-reach install`. Docker Compose persists Agent Reach and `gh` state across all four runtimes. `workspace/AGENTS.md` and README explain how opencode should discover and guide use of the capability.

**Tech Stack:** Docker, Docker Compose, Debian packages, `pipx`, `agent-reach`, GitHub CLI `gh`, opencode instruction files, Markdown documentation.

---

## File Structure

- Modify `Dockerfile`: install Python tooling, `pipx`, `gh`, and preinstall `agent-reach` for the `wukong` runtime user after `useradd`.
- Modify `docker-compose.yml`: mount persistent `agent-reach-state` and `gh-config` volumes into all four services.
- Modify `workspace/AGENTS.md`: add concise runtime guidance so opencode knows Agent Reach and `gh` may be available and how to initialize safely.
- Modify `.env.example`: add comments explaining that Agent Reach and `gh` secrets are handled through interactive setup and Docker volumes, not `.env` variables.
- Modify `README.md`: document first-time setup and multi-runtime sharing behavior.
- No Rust source files should change for this implementation. If an implementation worker decides Rust prompt injection is needed, stop and run GitNexus impact analysis for the exact Rust symbol before editing it.

## Task 1: Install Runtime Tools In Docker Image

**Files:**
- Modify: `Dockerfile`

- [ ] **Step 1: Inspect the current Dockerfile runtime dependency block**

Read `Dockerfile` and confirm the runtime package installation block currently contains:

```dockerfile
# Install runtime deps + gosu + current opencode npm package
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git gosu nodejs npm ripgrep fzf && \
    npm install -g opencode-ai@latest && \
    opencode --version && \
    rm -rf /var/lib/apt/lists/* /root/.npm
```

Expected: the block exists before `useradd -m -s /bin/bash wukong`.

- [ ] **Step 2: Add baseline packages to the runtime dependency block**

Change the block to include `python3`, `python3-pip`, `pipx`, and `gh`:

```dockerfile
# Install runtime deps + gosu + current opencode npm package
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git gh gosu nodejs npm python3 python3-pip pipx ripgrep fzf && \
    npm install -g opencode-ai@latest && \
    opencode --version && \
    rm -rf /var/lib/apt/lists/* /root/.npm
```

Expected: package names are in one apt install list, and existing packages remain present.

- [ ] **Step 3: Move `HOME`, `PATH`, and pipx env before Agent Reach install**

After the `RUN useradd -m -s /bin/bash wukong` line, add these environment variables before the Agent Reach install step:

```dockerfile
ENV HOME=/home/wukong
ENV PIPX_HOME=/home/wukong/.local/pipx
ENV PIPX_BIN_DIR=/home/wukong/.local/bin
ENV PATH="/home/wukong/.local/bin:/usr/local/bin:${PATH}"
```

Then remove the later duplicate `ENV HOME=/home/wukong` and duplicate `ENV PATH="/home/wukong/.local/bin:/usr/local/bin:${PATH}"` from the "Default environment" section, leaving only:

```dockerfile
# Default environment
ENV WUKONG_WORKSPACE=/workspace
```

Expected: `HOME`, `PIPX_HOME`, `PIPX_BIN_DIR`, and `PATH` are defined once, before preinstalling Agent Reach.

- [ ] **Step 4: Preinstall the Agent Reach CLI without running setup**

Add this block immediately after the env variables from Step 3:

```dockerfile
# Preinstall Agent Reach CLI only; user-specific channel setup runs interactively.
RUN mkdir -p "$PIPX_HOME" "$PIPX_BIN_DIR" && \
    chown -R wukong:wukong /home/wukong/.local && \
    gosu wukong pipx install agent-reach && \
    gosu wukong agent-reach --help >/dev/null
```

Expected: Docker build installs the `agent-reach` command for the non-root `wukong` user, but does not run `agent-reach install`.

- [ ] **Step 5: Build the image**

Run:

```bash
docker compose build wukong
```

Expected: build succeeds. If Debian cannot find `gh`, replace the apt package approach with GitHub CLI's official Debian repository setup, then rerun this same build command.

- [ ] **Step 6: Verify runtime commands exist**

Run:

```bash
docker compose run --rm wukong gh --version
docker compose run --rm wukong agent-reach --help
docker compose run --rm wukong python3 --version
```

Expected: `gh --version` prints a GitHub CLI version, `agent-reach --help` prints help text, and `python3 --version` prints a Python version from inside the container.

- [ ] **Step 7: Commit Dockerfile changes**

Run:

```bash
git status --short
git diff -- Dockerfile
git add Dockerfile
git commit -m "feat(docker): add agent reach runtime tools"
```

Expected: only `Dockerfile` is staged and committed for this task.

## Task 2: Persist Agent Reach And GitHub CLI State Across Runtimes

**Files:**
- Modify: `docker-compose.yml`

- [ ] **Step 1: Add Agent Reach and gh volumes to the `wukong` service**

In the `wukong` service `volumes:` block, after `opencode-state`, add:

```yaml
      # Persistent Agent Reach channel config/cookies
      - agent-reach-state:/home/wukong/.agent-reach
      # Persistent GitHub CLI auth/config
      - gh-config:/home/wukong/.config/gh
```

Expected: CLI setup commands persist Agent Reach and `gh` state.

- [ ] **Step 2: Add the same volumes to `wukong-telegram`**

In the `wukong-telegram` service `volumes:` block, add:

```yaml
      - agent-reach-state:/home/wukong/.agent-reach
      - gh-config:/home/wukong/.config/gh
```

Expected: Telegram runtime can reuse state initialized by the CLI runtime.

- [ ] **Step 3: Add the same volumes to `wukong-web`**

In the `wukong-web` service `volumes:` block, add:

```yaml
      - agent-reach-state:/home/wukong/.agent-reach
      - gh-config:/home/wukong/.config/gh
```

Expected: Web runtime can reuse state initialized by the CLI runtime.

- [ ] **Step 4: Add the same volumes to `wukong-schedulerd`**

In the `wukong-schedulerd` service `volumes:` block, add:

```yaml
      - agent-reach-state:/home/wukong/.agent-reach
      - gh-config:/home/wukong/.config/gh
```

Expected: scheduled turns can reuse state initialized by the CLI runtime.

- [ ] **Step 5: Declare the two named volumes**

At the bottom `volumes:` section, add:

```yaml
  # Persistent Agent Reach channel config/cookies
  agent-reach-state:
    driver: local
  # Persistent GitHub CLI auth/config
  gh-config:
    driver: local
```

Expected: `docker compose config` recognizes both volumes.

- [ ] **Step 6: Validate compose config**

Run:

```bash
docker compose config
```

Expected: command succeeds and rendered config includes `agent-reach-state` and `gh-config` on all four services.

- [ ] **Step 7: Commit compose changes**

Run:

```bash
git status --short
git diff -- docker-compose.yml
git add docker-compose.yml
git commit -m "feat(docker): persist retrieval auth state"
```

Expected: only `docker-compose.yml` is staged and committed for this task.

## Task 3: Add opencode Runtime Guidance

**Files:**
- Modify: `workspace/AGENTS.md`

- [ ] **Step 1: Add a network retrieval capability section**

Append this section after the existing `## 🧠 記憶與歷史整合` section:

```markdown

## 🌐 網路資訊檢索能力

Docker runtime 可能已預裝 `agent-reach` 與 `gh`，用來擴充即時網路資訊檢索能力。當使用者要求最新資訊、閱讀網頁、查 GitHub repository/issue、整理 YouTube/RSS/社群平台內容、或進行全網調研時，不要只依賴模型記憶，應先檢查可用工具。

- 使用 `agent-reach doctor` 檢查目前 Agent Reach channel 狀態。
- 若尚未初始化，先向使用者說明需要一次性設定，並建議在互動式 CLI runtime 執行：`docker compose run --rm wukong agent-reach install --env=auto`。
- 需要登入、Cookie、Token 或平台帳號的 channel，必須先取得使用者明確同意，並提醒憑證會保存在 Docker volume 中。
- GitHub 查詢與操作優先使用 `gh`；若尚未登入，建議使用 `docker compose run --rm wukong gh auth login` 完成互動式認證。
- 若 Agent Reach 安裝或 opencode MCP 設定有變更，提醒使用者重啟 Web/Telegram/Scheduler 等常駐服務。
```

Expected: the guidance is concise, Chinese, and does not claim Agent Reach is always initialized.

- [ ] **Step 2: Verify instruction file readability**

Run:

```bash
git diff -- workspace/AGENTS.md
```

Expected: only the new `網路資訊檢索能力` section is added.

- [ ] **Step 3: Commit instruction changes**

Run:

```bash
git status --short
git add workspace/AGENTS.md
git commit -m "docs(runtime): advertise retrieval capability"
```

Expected: only `workspace/AGENTS.md` is staged and committed for this task.

## Task 4: Document User Setup Flow

**Files:**
- Modify: `README.md`
- Modify: `.env.example`

- [ ] **Step 1: Add README subsection after Docker quick-start commands**

In `README.md`, after the Docker quick-start command block that includes `docker compose run --rm wukong opencode` and `docker compose run --rm wukong wukong`, add:

````markdown

**啟用網路資訊檢索（Agent Reach + GitHub CLI）：**

Docker image 會預裝 `agent-reach` CLI 與 `gh`，但不會在 build 或 daemon 啟動時自動執行登入、Cookie 或 MCP 設定。若要讓 opencode/Wukong 具備更強的網路資訊檢索能力，請先用互動式 CLI runtime 完成一次性初始化：

```bash
docker compose run --rm wukong agent-reach install --env=auto
docker compose run --rm wukong agent-reach doctor
docker compose run --rm wukong gh auth login
docker compose up -d --force-recreate
```

請從 `wukong` CLI service 執行初始化，不要從 `wukong-web`、`wukong-telegram` 或 `wukong-schedulerd` 這類常駐服務執行互動式設定。初始化後，Agent Reach 狀態會保存在 `agent-reach-state` volume，GitHub CLI 認證會保存在 `gh-config` volume，Web、Telegram 與 Scheduler 會共用這些狀態。

部分 Agent Reach channel 需要 Cookie、Token 或平台登入態。請只在你信任的部署環境中提供這些憑證；不要把 Cookie 或 Token 寫進 `.env`，除非你明確接受該風險。若 Agent Reach 安裝流程改動了 opencode MCP 設定，請重啟相關 Docker 服務，因為 opencode 啟動後不會熱載入設定。
````

Expected: the subsection appears before the existing paragraph that starts with `第一次啟動時`.

- [ ] **Step 2: Add `.env.example` comments**

Append this section to `.env.example` after the existing memory/semantic settings comments:

```dotenv

# ── 網路資訊檢索（Agent Reach / gh）──
# Agent Reach 與 GitHub CLI 認證請透過互動式 Docker 指令設定，狀態會保存在 Docker volumes。
# 建議指令：
#   docker compose run --rm wukong agent-reach install --env=auto
#   docker compose run --rm wukong agent-reach doctor
#   docker compose run --rm wukong gh auth login
# 不建議把 Cookie、Token 或平台憑證直接寫入 .env。
```

Expected: no new secret variables are introduced.

- [ ] **Step 3: Check Markdown and env diffs**

Run:

```bash
git diff -- README.md .env.example
```

Expected: README includes first-time setup instructions; `.env.example` only adds comments.

- [ ] **Step 4: Commit documentation changes**

Run:

```bash
git status --short
git add README.md .env.example
git commit -m "docs(docker): explain retrieval setup"
```

Expected: only `README.md` and `.env.example` are staged and committed for this task.

## Task 5: End-To-End Verification

**Files:**
- Read only unless fixes are required.

- [ ] **Step 1: Run Docker build verification**

Run:

```bash
docker compose build wukong
```

Expected: build succeeds with `gh` and `agent-reach` installed.

- [ ] **Step 2: Verify tool availability inside the CLI runtime**

Run:

```bash
docker compose run --rm wukong gh --version
docker compose run --rm wukong agent-reach --help
docker compose run --rm wukong agent-reach doctor
```

Expected: `gh --version` and `agent-reach --help` succeed. `agent-reach doctor` may report unconfigured channels, but the command itself should run and produce diagnostics.

- [ ] **Step 3: Verify compose volumes are wired**

Run:

```bash
docker compose config
```

Expected: rendered config includes these mounts for `wukong`, `wukong-web`, `wukong-telegram`, and `wukong-schedulerd`:

```yaml
- agent-reach-state:/home/wukong/.agent-reach
- gh-config:/home/wukong/.config/gh
```

- [ ] **Step 4: Verify entrypoint still seeds opencode config**

Run:

```bash
docker compose run --rm wukong test -f /home/wukong/.config/opencode/opencode.json
```

Expected: command exits successfully.

- [ ] **Step 5: Run repository tests relevant to changed files**

Run:

```bash
cargo test
```

Expected: all Rust tests pass. Even though this change is mostly Docker/docs, running the full suite catches accidental repo-wide regressions.

- [ ] **Step 6: Analyze uncommitted changes with GitNexus before final commit**

Run the GitNexus change detector with scope `all`.

Expected: affected changes are limited to Docker/runtime docs and no unexpected Rust execution flow appears.

- [ ] **Step 7: Final status check**

Run:

```bash
git status --short
git log --oneline -5
```

Expected: working tree is clean after the task commits, and recent commits correspond to this implementation.

## Self-Review

- Spec coverage: Docker runtime tools are covered by Task 1; persistent multi-runtime state by Task 2; opencode guidance by Task 3; user setup docs and `.env.example` by Task 4; verification by Task 5.
- Red-flag scan: no placeholder or vague implementation steps remain.
- Type and name consistency: volume names are consistently `agent-reach-state` and `gh-config`; commands consistently use `agent-reach install --env=auto`, `agent-reach doctor`, and `gh auth login`.
