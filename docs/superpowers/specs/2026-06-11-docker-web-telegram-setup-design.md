# Docker Web + Telegram First-Run Setup Design

## Goals

- `docker compose up -d` starts the local Web console and the Telegram service by default.
- The CLI service remains passive and is used through `docker compose run --rm wukong ...`.
- A clean Docker install can be configured from the local Web UI without editing `.env` first.
- Telegram does not restart-loop when missing bot settings; it waits until settings exist.
- Docker installs the current OpenCode package instead of the legacy installer result.

## Docker Compose Shape

- Keep three services: `wukong-web`, `wukong-telegram`, and `wukong`.
- `wukong-web` and `wukong-telegram` are default services, so plain `docker compose up -d` starts both.
- `wukong` gets the Compose profile `cli` so it does not start during default `up`; it remains available through `docker compose run --rm wukong ...`.
- All services share the existing `wukong-data` volume at `/data` and the `opencode-config` volume at `/home/wukong/.config/opencode`.

## OpenCode Installation

- Install OpenCode with npm: `npm install -g opencode-ai@latest`.
- Do not use the old GitHub raw install script because it produced a legacy OpenCode binary in testing while npm exposes the current `opencode-ai` package.
- The Docker build must run `opencode --version` after installation and fail if the command is missing.
- `docker compose run --rm wukong opencode ...` remains a direct passthrough for setup and authentication commands.

## Shared Settings

- Store first-run settings in `/data/settings.json` on the `wukong-data` volume.
- Initial schema:

```json
{
  "telegram": {
    "token": "",
    "allowed": ""
  }
}
```

- `token` is the Telegram bot token.
- `allowed` is the same comma/whitespace-separated chat/user ID format currently accepted by `WUKONG_TG_ALLOWED`.
- Environment variables remain supported and take precedence over the shared settings file.

## Telegram Startup Behavior

- `wukong-telegram` no longer exits when `WUKONG_TG_TOKEN` and shared settings are both missing.
- Without a token it logs a clear waiting message and reloads `/data/settings.json` every 5 seconds.
- Once a token exists, it initializes the Telegram client and starts long polling.
- If settings change while running, the service detects token or allowlist changes and restarts its polling loop internally without requiring container restart.
- If token authentication fails or polling returns errors, it logs and retries with backoff, preserving the existing long-running container behavior.

## Web Settings UI

- Add a settings screen reachable at `/settings` and linked from the existing Web console.
- First version manages:
  - Telegram bot token.
  - Allowed chat/user IDs.
  - Read-only status: configured/missing token and last known save result.
- Saving writes `/data/settings.json` through a Web API.
- The UI explains that Telegram will start automatically after saving valid settings.

## Web API

- Add JSON endpoints under `/api/settings`:
  - `GET` returns current effective settings with secrets redacted where appropriate.
  - `POST` validates and writes Telegram settings.
- In local trust mode, settings are open by default.
- If `WUKONG_WEB_TOKEN` is configured, settings endpoints require the same token protection as `/chat`.

## Security Model

- Default is local trust mode for simple local Docker onboarding.
- Users who expose the Web console should set `WUKONG_WEB_TOKEN`.
- The API should not log the Telegram token.
- The Web UI should avoid re-displaying the full saved token unless needed; it can show masked token status after save.

## Tests

- Dockerfile verification: build image and assert `opencode --version` works.
- Entrypoint verification: `docker compose run --rm wukong opencode --version` executes OpenCode, not the Wukong CLI.
- Web tests:
  - Settings GET returns default missing-token state.
  - Settings POST writes valid settings.
  - Settings endpoints require token when `WUKONG_WEB_TOKEN` is configured.
- Telegram tests:
  - Missing token enters waiting mode instead of exiting.
  - Shared settings are parsed into token and allowlist.
  - Environment variables override shared settings.

## Out of Scope

- Web controlling Docker or mounting `/var/run/docker.sock`.
- Telegram webhook mode.
- Full OpenCode provider configuration UI.
- Multi-user admin accounts or password management.
