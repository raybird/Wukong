# Release Docker Installer Design

## Goal

Wukong installation should support two clear paths from the same one-liner installer:

- **Docker mode**: deploy Wukong into the current directory from a GitHub Release Docker bundle, without cloning the repository and without compiling Rust locally.
- **Binary mode**: keep the current host binary installation flow that downloads release binaries into `~/.local/bin` and optionally configures Telegram/Web services.

Docker mode is the preferred path for users who want an isolated runtime with `opencode`, persistent memory, and host workspace mounting. Binary mode remains useful for direct host installs and systemd user services.

## Current Problem

The latest Docker packaging path regressed toward local source builds: the Dockerfile compiles the Rust workspace in a `rust` builder stage. That makes Docker installs slow and requires the full source tree and Rust dependencies. This conflicts with the release-based installer model, where users should be able to install from release artifacts only.

The installer also currently presents only the binary installation flow. Users cannot choose Docker deployment from the same entry point.

## User Experience

The one-liner remains the primary entry point:

```bash
curl -fsSL https://raw.githubusercontent.com/raybird/Wukong/main/scripts/install.sh | bash
```

The installer resolves the requested version, then asks for the install mode:

```text
你要使用哪種安裝模式？
  [1] Docker mode（推薦，部署到目前目錄）
  [2] Binary mode（安裝到 ~/.local/bin）
選擇 [1-2] (預設 1):
```

Docker mode behavior:

1. Check for `docker` and `docker compose`.
2. Download the release asset `wukong-docker-${VERSION}.tar.gz`.
3. Extract it into the current directory.
4. Copy `.env.example` to `.env` only if `.env` does not already exist.
5. Explain the generated files and next commands.
6. Ask whether to start immediately with `docker compose up -d`.

Binary mode keeps the existing component selection, binary download, config file, workspace template, and optional systemd user service flow.

## Release Docker Bundle

Each release should include a Docker bundle asset named:

```text
wukong-docker-${VERSION}.tar.gz
```

The tarball should contain:

```text
docker-compose.yml
.env.example
Dockerfile
scripts/docker-entrypoint.sh
workspace/SOUL.md
workspace/AGENTS.md
```

The bundle must be sufficient for deployment in an empty directory. Users should not need the source repository.

## Dockerfile Design

The release Dockerfile must not compile Wukong from source. It should download release binaries matching the same Wukong version used by the bundle.

Inputs:

- `VERSION`: GitHub release tag. Default should match the release the bundle was produced for.
- `TARGET`: Linux binary target. Default `x86_64-unknown-linux-musl`.
- `REPO`: GitHub repository. Default `raybird/Wukong`.

Build flow:

1. Use a small Debian downloader stage.
2. Download and extract:
   - `wukong-${TARGET}.tar.gz`
   - `wukong-telegram-${TARGET}.tar.gz`
   - `wukong-web-${TARGET}.tar.gz`
3. Copy the three binaries into the runtime image.
4. Install runtime dependencies and `opencode`.
5. Copy `workspace/SOUL.md`, `workspace/AGENTS.md`, and `scripts/docker-entrypoint.sh`.

This keeps Docker builds fast and release-driven while preserving the existing runtime behavior.

## Docker Compose Design

The bundle's `docker-compose.yml` should remain service-oriented:

- `wukong-web` and `wukong-telegram` are long-running services.
- `wukong` is a CLI/REPL profile used with `docker compose run --rm wukong`.
- All services share the same locally built `wukong:latest` image.
- Volumes persist `opencode` configuration and Wukong data.
- The host workspace is mounted at `/workspace`.

The default command after Docker mode installation is:

```bash
docker compose up -d
```

CLI usage remains:

```bash
docker compose run --rm wukong
```

## Safety and Overwrite Rules

Docker mode must avoid destructive writes by default.

- If a target file already exists, do not overwrite it unless `--force` is supplied.
- If `.env` already exists, never overwrite it automatically.
- If `.env.example` exists and `--force` is not supplied, keep the existing file and warn.
- If the release Docker bundle is unavailable, fail with a clear message that the selected version does not include Docker deployment assets.

Installer flags:

- `--mode docker|binary`: skip the interactive mode prompt.
- `--version <tag>`: install a specific release.
- `--flavor gnu|musl`: binary mode Linux target selection. Docker mode uses `musl` by default unless a future flag is added.
- `--force`: allow overwriting generated Docker bundle files except `.env`.
- `--dry-run`: print planned actions without writing files or starting services.

## Error Handling

Docker mode should fail early when required tools are missing:

- Missing `docker`: explain Docker must be installed.
- Missing `docker compose`: explain Docker Compose v2 is required.
- Release download failure: show the exact bundle asset name and version.
- Existing files without `--force`: list conflicts and show the retry command.

Binary mode should keep the current checksum verification behavior for downloaded binaries.

## Documentation Updates

README should describe:

- The two install modes in the quick install section.
- Docker mode as the recommended deployment path.
- The generated Docker files and expected commands.
- The fact that Docker builds use release binaries and do not compile Rust.

`.env.example` should remain the copy-and-edit entry point for Docker users.

## Verification

Implementation should be verified with:

```bash
bash -n scripts/install.sh
scripts/install.sh --mode docker --version <tag> --dry-run
docker compose config
docker build --build-arg VERSION=<tag> -t wukong:test .
docker run --rm wukong:test wukong --help
```

Release packaging should be verified by inspecting the release assets and confirming `wukong-docker-${VERSION}.tar.gz` contains all required files.

## Out of Scope

- Publishing a GHCR image.
- Changing Wukong runtime behavior.
- Replacing the existing binary installer with Docker-only installation.
- Changing Telegram/Web configuration semantics beyond generated `.env` defaults.
