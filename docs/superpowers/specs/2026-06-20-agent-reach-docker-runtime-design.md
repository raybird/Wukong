# Agent Reach Docker Runtime Design

## Goal

Strengthen Wukong's Docker-based opencode runtime with optional internet retrieval capabilities through [Agent Reach](https://github.com/Panniantong/agent-reach), while keeping sensitive setup and platform-specific login state out of image build and daemon startup.

The Docker image should become ready to run Agent Reach and GitHub CLI workflows, but users should explicitly initialize Agent Reach from an interactive CLI container when they want this capability.

## Context

Wukong's Docker mode runs multiple runtimes from the same image:

- `wukong` for interactive CLI and REPL use.
- `wukong-web` for the Web Console.
- `wukong-telegram` for Telegram bot execution.
- `wukong-schedulerd` for scheduled turns.

All services share these relevant volumes:

- `/workspace`, mounted from the host working directory.
- `/home/wukong/.config/opencode`, persisted by `opencode-config`.
- `/home/wukong/.local/share/opencode`, persisted by `opencode-state`.
- `/data`, persisted by `wukong-data`.

The entrypoint currently seeds `SOUL.md`, `AGENTS.md`, and a default `~/.config/opencode/opencode.json` when missing. The default opencode config focuses on non-interactive permission handling and destructive command guardrails.

Agent Reach is not a single stable MCP server to wire directly into Docker. It is an installer and capability layer that installs and selects current backends such as Jina Reader, `yt-dlp`, `gh`, RSS parsing, platform CLIs, and optional MCP integrations. Some channels require user login, cookies, or local platform-specific setup. For that reason, image build and long-running service startup are the wrong places to run full `agent-reach install`.

## Requirements

- The Docker runtime must include `gh` CLI.
- The Docker runtime must include enough Python tooling to install or run Agent Reach from the `wukong` runtime user.
- The image must not run full Agent Reach initialization during `docker build`.
- The entrypoint must not run interactive Agent Reach setup automatically for Web, Telegram, or Scheduler services.
- opencode must receive clear instructions that internet retrieval is available after initialization and how to initialize it if missing.
- Documentation must explain the recommended setup flow across all Docker runtimes.
- Sensitive tokens, cookies, and login state must remain user-managed and must not be added to `.env.example` as plain values.

## Recommended Approach

Use a "runtime-ready, user-initialized" integration.

### Docker Image

Install these baseline tools in the runtime stage:

- `python3`
- `python3-pip`
- `pipx`
- `gh`

Keep the existing `/home/wukong/.local/bin` path in `PATH`, because `pipx` installs executable shims there for the `wukong` user.

Preinstall the `agent-reach` CLI in the image, but do not run `agent-reach install`. The Dockerfile should install the CLI for the `wukong` runtime user after that user exists, so the executable lands under `/home/wukong/.local/bin` and is available through the existing `PATH`.

This gives users a simple first command, `agent-reach install --env=auto`, while keeping login and channel configuration outside the build.

### Persistent Runtime State

Agent Reach and `gh` may write state under the runtime user's home directory. The current compose file persists opencode config and opencode state, but it does not persist all of `/home/wukong`.

The design should avoid storing secrets in the image or `.env`. For persistence, either:

- Add a dedicated volume for Agent Reach state, mounted at `/home/wukong/.agent-reach`, and rely on `opencode-config` for opencode MCP config changes.
- Or document that users who need long-lived Agent Reach login state should mount a dedicated host or Docker volume for `/home/wukong/.agent-reach`.

The recommended implementation is to add an `agent-reach-state` named volume mounted at `/home/wukong/.agent-reach` for all Docker services. This keeps platform cookies and channel config persistent without broadening persistence to the entire home directory.

For `gh`, the default auth state is usually under `/home/wukong/.config/gh`. If GitHub authentication is expected to persist across container recreation, add a dedicated `gh-config` volume mounted at `/home/wukong/.config/gh` for all Docker services. This is preferred over telling users to put tokens in `.env`.

### opencode Prompting

Update `workspace/AGENTS.md` with a concise capability section rather than injecting a large prompt into every Wukong final turn.

The prompt should tell opencode:

- The Docker runtime may have Agent Reach available for internet retrieval.
- For web pages, current facts, GitHub repositories/issues, YouTube, RSS, and social/search tasks, check available tools instead of relying only on model memory.
- If Agent Reach is not initialized, ask the user for approval and run setup from an interactive `wukong` container.
- Use `agent-reach doctor` to inspect current channel status.
- For platforms requiring login, cookies, or tokens, ask for explicit user consent and explain the security implications.
- Use `gh` for GitHub operations when available; authenticate with `gh auth login` when needed.

This keeps the capability visible to opencode through its normal instruction file without increasing Wukong runtime prompt size or causing stateless helper steps to attempt setup unexpectedly.

### Documentation

Update the Docker section of `README.md` with a short "Enable Internet Retrieval" subsection.

Recommended first-time flow:

```bash
docker compose run --rm wukong agent-reach install --env=auto
docker compose run --rm wukong agent-reach doctor
docker compose run --rm wukong gh auth login
docker compose up -d --force-recreate
```

The docs should explain:

- Run setup from the interactive `wukong` CLI service, not from daemon services.
- Web, Telegram, and Scheduler reuse the same image and mounted state after setup.
- Some Agent Reach channels require cookies, browser login state, or platform-specific credentials.
- Do not put sensitive cookies or tokens in `.env` unless the user intentionally accepts that risk.
- If setup changes opencode MCP configuration, restart opencode-backed Wukong services because opencode config is loaded at process startup.

### `.env.example`

Add comments only, not secrets:

- Mention that Agent Reach and `gh` authentication state are managed through interactive setup commands and persisted through Docker volumes.
- Avoid adding token variables for cookies or GitHub unless a specific non-interactive use case is later designed.

## Alternatives Considered

### Run Agent Reach Install During Image Build

This would make the image appear ready immediately, but it is brittle. Agent Reach intentionally performs environment detection, installs changing channel backends, and may configure MCP or skills. Build-time setup cannot safely handle user login, cookies, or network variability.

Rejected.

### Run Agent Reach Install From Entrypoint

This would help users discover the feature, but it is inappropriate for daemon services. Web, Telegram, and Scheduler should not block on interactive setup or repeatedly mutate config during startup.

Rejected.

### Inject a Runtime Capability Block Into Every Wukong Final Prompt

This follows the existing scheduling capability pattern, but internet retrieval is broader and depends on optional user initialization. Repeating it every turn adds prompt noise and risks tool attempts before setup is complete.

Rejected for the initial integration. It can be reconsidered later if users miss the `AGENTS.md` guidance.

## Error Handling

- If `agent-reach doctor` reports missing channels, opencode should report the missing backend and suggest the relevant Agent Reach setup or update command.
- If a platform requires login or cookies, opencode should ask before collecting or writing credentials.
- If `gh auth status` fails, opencode should suggest `gh auth login` rather than trying to infer credentials.
- If opencode does not see newly installed MCP tools, the user should restart the affected Docker services.

## Testing

Implementation should verify:

- Dockerfile installs `gh`, Python tooling, and the chosen Agent Reach CLI availability path.
- `docker compose run --rm wukong gh --version` succeeds.
- `docker compose run --rm wukong agent-reach --help` succeeds if the CLI is preinstalled.
- The entrypoint still seeds default opencode config when missing.
- The new volumes are present for all four services if persistent Agent Reach and `gh` state are implemented.
- README instructions match the actual commands.

## Scope Boundaries

This design does not add a Wukong-native web search abstraction. It also does not add automatic cookie collection, browser automation, or platform-specific login flows. Those remain Agent Reach responsibilities and must be initiated by the user.
