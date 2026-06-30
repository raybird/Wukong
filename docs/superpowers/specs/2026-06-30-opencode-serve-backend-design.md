# Opencode Serve Backend Design

Date: 2026-06-30

## Goal

Reduce perceived latency for Docker-based Wukong services by reusing a long-lived `opencode serve` process instead of spawning `opencode run` for every agent call.

The first version is Docker-first. Binary installs keep the current `opencode run` behavior and do not auto-start or manage a local opencode server.

## Current Behavior

Wukong currently drives opencode through `wukong-gateway::AgentCliBackend`.

- Each backend call spawns a new subprocess from `WUKONG_AGENT_CMD`, usually `opencode run`.
- Streaming uses `opencode run --format json` and parses newline-delimited JSON events.
- The final step of a turn passes `-s <session_id>` when Wukong has an opencode session id for the current scope.
- Helper steps remain stateless and do not update the persisted opencode session.
- Wukong persists `scope -> session_id` in the `agent_sessions` table.

This preserves conversation continuity, but every call still pays opencode process startup and context loading cost.

## Proposed Behavior

Add a second backend that talks to a long-lived `opencode serve` HTTP server.

- Keep `AgentCliBackend` as the default and fallback backend.
- Add an `OpencodeServerBackend` selected when `WUKONG_AGENT_SERVER_URL` is set.
- Docker Compose adds an `opencode-server` service running `opencode serve --hostname 0.0.0.0 --port 4096`.
- Docker Wukong services set `WUKONG_AGENT_SERVER_URL=http://opencode-server:4096`.
- Binary installs do not start or manage `opencode serve` in v1.
- Advanced binary users may manually run `opencode serve` and set `WUKONG_AGENT_SERVER_URL`, but this is not the documented primary path for v1.

## Architecture

The existing `AiBackend` trait remains the abstraction boundary.

```text
run_turn
  -> AiBackend
       -> AgentCliBackend          existing: spawn opencode run
       -> OpencodeServerBackend    new: HTTP calls to opencode serve
```

Backend selection should happen at the entry points that already construct `AgentCliBackend`, such as CLI, Web, Telegram, Scheduler, and orchestrator demo binaries.

Selection rule:

```text
if WUKONG_AGENT_SERVER_URL is set and non-empty:
  use OpencodeServerBackend
else:
  use AgentCliBackend
```

`WUKONG_AGENT_CMD` remains supported for CLI mode, tests, and fallback behavior.

## Docker Layout

Docker adds a dedicated service for opencode server.

```yaml
opencode-server:
  command: opencode serve --hostname 0.0.0.0 --port 4096
  working_dir: /workspace
  volumes:
    - opencode-config:/home/wukong/.config/opencode
    - opencode-state:/home/wukong/.local/share/opencode
    - ${WUKONG_HOST_WORKSPACE:-./workspace}:/workspace

wukong-web:
  environment:
    WUKONG_AGENT_SERVER_URL: http://opencode-server:4096
```

The exact Compose stanza should follow the existing Docker image, user, volume, and workspace conventions. The important boundary is that `opencode-server` owns the long-lived opencode process while Wukong services remain clients.

## Session Flow

Wukong continues to own scope-level session mapping.

For the final user-facing step:

1. Load `agent_sessions[scope]` from memory.
2. If a session id exists, send the message to that opencode session.
3. If no session id exists, create a new opencode session through the server API, then send the message.
4. Persist the resulting session id back to `agent_sessions[scope]`.

For helper steps:

- Preserve current stateless behavior.
- The simplest v1 behavior is to create a temporary opencode session per helper call and not persist it.
- A later optimization may reuse a helper session pool, but that is out of scope for v1 because it risks polluting role-specific context.

## API Use

The backend should use the documented opencode server API.

- `GET /global/health` for readiness checks.
- `POST /session` to create a session when Wukong has no persisted id.
- `POST /session/:id/message` to send a message and wait for the response.
- Optional later phase: `POST /session/:id/prompt_async` plus `GET /event` for streaming.

The first implementation may return whole-message output through `run_streaming` by emitting one `StreamEvent::Text` after the HTTP response arrives. True token or event streaming is a second phase.

## Configuration

New environment variables:

- `WUKONG_AGENT_SERVER_URL`: base URL for an existing opencode server.
- `WUKONG_AGENT_SERVER_USERNAME`: optional HTTP basic auth username. Defaults to `opencode` when password is set.
- `WUKONG_AGENT_SERVER_PASSWORD`: optional HTTP basic auth password.

Existing variables remain valid:

- `WUKONG_AGENT_CMD`: CLI fallback command, still defaulting to `opencode run`.
- `WUKONG_AGENT_TIMEOUT_SECS`: applies to HTTP calls as well as subprocess calls.

## Error Handling

If `WUKONG_AGENT_SERVER_URL` is set but the server is unreachable, Wukong should return a clear configuration/runtime error instead of silently falling back to CLI. Silent fallback can hide Docker deployment mistakes and create confusing session divergence.

Useful error cases:

- server URL cannot be parsed
- health check fails
- session id stored by Wukong no longer exists in opencode
- HTTP request times out
- opencode returns a permission request or non-final response shape that v1 cannot handle

For missing stored sessions, the backend may create a new opencode session and let Wukong overwrite the stored session id, provided the server clearly reports that the old session is gone.

## Binary Behavior

Binary installs keep the existing behavior in v1.

- No auto-started daemon.
- No automatic port allocation.
- No background process lifecycle management.
- No change to the default `opencode run` path.

Reasoning:

- Binary Wukong is commonly used from arbitrary project directories.
- Automatic server management would need per-project lifecycle, port, cleanup, and scope isolation policy.
- Docker already provides the right long-lived service model and isolated volumes.

Manual binary use with `WUKONG_AGENT_SERVER_URL` is allowed by architecture but not promoted as the primary supported workflow until Docker mode proves stable.

## Testing

Unit tests should cover backend selection and request construction without requiring a real opencode server.

Integration tests can use a small mock HTTP server that implements:

- `GET /global/health`
- `POST /session`
- `POST /session/:id/message`

Behavior tests should verify:

- CLI backend remains default without `WUKONG_AGENT_SERVER_URL`.
- Server backend is selected when URL is set.
- A new session is created and persisted when no scope session exists.
- Existing scope sessions are reused.
- Helper steps do not persist their temporary sessions.
- Server errors produce clear `GatewayError` messages.

Docker verification should check:

- `opencode-server` starts and shares existing opencode config/state volumes.
- `wukong-web`, `wukong-telegram`, and `wukong-schedulerd` can reach `http://opencode-server:4096`.
- Removing `WUKONG_AGENT_SERVER_URL` restores the current CLI path.

## Rollout

1. Add `OpencodeServerBackend` behind `WUKONG_AGENT_SERVER_URL`.
2. Add Docker Compose service and environment wiring.
3. Document Docker low-latency mode and CLI fallback behavior.
4. Keep binary behavior unchanged.
5. Add true event streaming only after whole-response mode is stable.

## Non-Goals

- Auto-starting `opencode serve` for binary installs.
- Replacing `AgentCliBackend`.
- Changing Wukong memory recall or role planning behavior.
- Reusing helper-step sessions across turns.
- Implementing full SSE streaming in the first server backend version.
