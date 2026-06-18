# Opencode Command Controls Design

## Goal

Let users control opencode from Wukong surfaces without leaving the chat interface.
The first command set covers:

- `/compact`: send an allowlisted passthrough command to the current opencode session.
- `/providers`: list available opencode providers.
- `/models`: list available opencode models.
- `/set_models <model>`: persist a new system-wide default model for future Wukong turns.

The feature must work consistently from CLI/REPL, Web, and Telegram. Web and Telegram command exchanges should remain visible in the shared chat history introduced in v0.16.5.

## Non-Goals

- Do not passthrough arbitrary slash commands.
- Do not edit `.env` from inside the running service.
- Do not require Docker container recreation for model changes to take effect.
- Do not add a Web settings UI for model management in this iteration.
- Do not change opencode's own config file format.

## Command Semantics

### `/compact`

`/compact` is an allowlisted opencode session passthrough command.

Wukong looks up the stored opencode session for the current scope. If there is no stored session, it replies with a clear no-op message such as `🐵 尚無對話可壓縮`. If a session exists, Wukong sends the raw `/compact` prompt to opencode using that session ID, with no Wukong planner, persona prompt, memory recall, or memory write.

This keeps compaction tied to the exact opencode session that future turns for that scope will continue.

### `/providers`

`/providers` is a Wukong command backed by an opencode CLI query. Wukong runs the equivalent of `opencode providers list` and returns the text output to the user.

This command does not use an opencode conversation session and does not invoke `run_turn`. It is a direct utility command.

### `/models`

`/models` is a Wukong command backed by an opencode CLI query. Wukong runs the equivalent of `opencode models` and returns the text output to the user.

This command does not use an opencode conversation session and does not invoke `run_turn`.

### `/set_models <model>`

`/set_models` persists a system-wide default model override in Wukong settings. Example:

```text
/set_models opencode/deepseek-v4-flash-free
```

After the setting is saved, future Web, Telegram, Scheduler, and CLI turns use the configured model when invoking opencode. The setting survives process restarts because it is stored in the same persistent settings database/file used by Wukong.

If the command has no model argument, Wukong returns usage text:

```text
用法：/set_models opencode/deepseek-v4-flash-free
```

## Architecture

### Parser and Command Model

Wukong keeps a small allowlisted session/control command model instead of generic slash passthrough.

The command model should represent:

- `New`: existing Wukong session reset command.
- `Compact`: opencode session passthrough of raw `/compact`.
- `Providers`: direct opencode CLI provider listing.
- `Models`: direct opencode CLI model listing.
- `SetModels(String)`: persistent system-wide model override.

Unknown slash commands remain unsupported and return the existing `指令 /<name> 尚未支援` style response.

### Opencode Utility Runner

Provider and model listing should use a small runner abstraction so tests can inject command output without spawning opencode.

The runner should execute commands based on the configured opencode binary from the base agent command. If `WUKONG_AGENT_CMD` is `opencode run --dangerously-skip-permissions`, the utility runner should use `opencode` as the binary and call:

- `opencode providers list`
- `opencode models`

Arguments intended only for `opencode run` must not be reused for utility subcommands.

### Model Setting Storage

`wukong-settings` should grow an agent setting, for example:

```rust
default_model: Option<String>
```

The exact struct name can follow current `wukong-settings` conventions.

`/set_models` writes this setting through the settings store. It does not mutate environment variables or opencode config files.

### Applying the Model

When Wukong builds opencode requests for future turns, it should load the persisted model override and pass it to opencode as the model flag used by opencode run.

The base command remains configurable through `WUKONG_AGENT_CMD`. The persisted model setting overlays that base command by adding or replacing the model option. This keeps existing deployment customization intact while letting chat commands change the active default model.

If no model is configured, Wukong preserves current behavior.

## Surface Behavior

### CLI/REPL

REPL slash command classification should recognize the new commands. Responses are printed as normal text events.

CLI one-shot prompt behavior should continue to treat ordinary non-slash prompts as Wukong turns. Slash command support should be consistent with the existing session command path where applicable.

### Web

Web should keep the current rule that leading-slash input is a command, not a Wukong turn.

For each command:

- Insert the user message into the selected scope's chat history.
- Execute the command.
- Insert the assistant response into the same chat history.
- Stream the rendered response through SSE as the final answer.

### Telegram

Telegram should keep allowlist checks before command execution.

For each command:

- Record the incoming slash command in `user:tg-<chat_id>` history when history is configured.
- Execute the command.
- Record and send the assistant response.

Command failures should be converted to user-facing error text and should not stop the long-poll loop.

### Scheduler

Scheduler does not parse slash commands. It only benefits from the persisted default model when it executes future scheduled turn jobs.

## Error Handling

- `/compact` with no session: return a no-op message, no backend call.
- opencode utility command failure: return `⚠️ 失敗：<error>` with useful stderr/detail.
- `/set_models` with empty model: return usage text.
- settings write failure: return `⚠️ 失敗：<error>`.
- unknown slash command: keep returning unsupported-command text.

## Security and Safety

The implementation must not passthrough arbitrary slash commands. Only explicit commands in the Wukong parser are allowed.

`/providers` and `/models` execute fixed opencode subcommands, not user-supplied shell strings. `/set_models` stores the user-provided model name as data; it must be passed to opencode as a single argument, not interpolated into a shell command.

## Testing

Add or update tests for:

- Parser recognition for `/compact`, `/providers`, `/models`, and `/set_models <model>`.
- Unknown slash commands remain unsupported.
- `/compact` sends the exact allowlisted passthrough string to the stored session and does not call the planner.
- `/providers` calls the utility runner with `providers list`.
- `/models` calls the utility runner with `models`.
- `/set_models` persists the model and can be read back.
- Backend command construction adds or replaces the model option for future turns.
- Web command path writes user and assistant messages to chat history.
- Telegram command path writes user and assistant messages to chat history.
- Scheduler turn execution observes the persisted model through shared config/backend construction.

## Success Criteria

- A Telegram user can send `/compact` and compact the current chat scope's opencode session.
- A Web user can send `/providers` and see opencode provider output in the chat UI.
- A Web or Telegram user can send `/models` and see opencode model output.
- A Web or Telegram user can send `/set_models opencode/deepseek-v4-flash-free`, receive confirmation, restart services, and future Wukong turns still use that model.
- Unknown commands remain blocked instead of becoming generic passthrough.
