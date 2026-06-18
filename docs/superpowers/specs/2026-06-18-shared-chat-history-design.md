# Shared Chat History Across Web, Telegram, and Scheduler

## Context

Wukong currently has two different conversation experiences:

- Web chat persists messages into `chat_threads` and `chat_messages` through `wukong-web::chat_history::ChatHistoryStore`.
- Telegram receives messages, runs turns with scope `user:tg-<chat_id>`, and sends replies back to Telegram, but does not persist those messages into `chat_messages`.

This makes the Web history page incomplete: it only shows Web-originated conversations even though Telegram and Web share the same memory database and scope model. The product expectation is that Web can inspect the same conversation timeline used by Telegram.

## Goals

- Persist Telegram incoming messages and assistant replies into the same chat history tables used by Web.
- Persist scheduler Telegram notifications into the same Telegram-scoped timeline.
- Let Web switch between conversation sources/scopes, including Telegram scopes such as `user:tg-915354960`.
- Preserve the current lazy-loading behavior: latest messages first, older messages loaded upward, date jump support.
- Keep the first implementation simple and avoid a separate inbox model.

## Non-Goals

- No cross-user authorization model beyond the existing Web token and Telegram allowlist.
- No multi-account Telegram identity management UI.
- No migration from old Telegram logs, because Telegram messages were not previously persisted.
- No full-text search or global inbox in this change.

## Architecture

Create a shared chat history crate, `wukong-chat-history`, and move the current Web-only store there.

The crate owns:

- `ChatHistoryStore`
- `ChatMessage`
- `ChatThread`
- table creation for `chat_threads` and `chat_messages`
- helpers for default thread creation, message insertion, scope listing, latest/older/date-window reads

Consumers:

- `wukong-web` uses the shared store for Web chat persistence and read APIs.
- `wukong-telegram` uses the shared store inside message dispatch.
- `wukong-schedulerd` uses the shared store when a scheduled Telegram-originated job sends a result back to Telegram.

The memory database remains the single SQLite file configured by `WUKONG_MEMORY_DB`; chat history tables continue to live in that database.

## Scope Model

Each conversation timeline is keyed by Wukong scope:

- Web default scope: existing `AppState.scope`, usually `global` or `project:<name>`.
- Telegram chat scope: `user:tg-<chat_id>` from `wukong_tg_client::parse::scope_for_chat`.
- Scheduler Telegram job scope: recovered from the job's `JobKind::Turn { scope, .. }`.

The default thread id remains deterministic: `scope:<scope>`. This preserves the current one-thread-per-scope behavior.

## Data Flow

### Web Turn

1. Web calls `/chat?q=...&scope=<optional-scope>`.
2. Handler validates Web token.
3. Handler resolves scope: query parameter if present, otherwise `AppState.scope`.
4. Store inserts user message into the scope thread.
5. `run_turn` executes with the same scope.
6. Store inserts assistant response or error into the same scope thread.
7. SSE streams the response to the browser as it does today.

### Telegram Turn

1. Telegram receives an allowed message from chat id `N`.
2. Dispatch resolves scope `user:tg-N`.
3. Store inserts the incoming user message into that scope thread.
4. Dispatch sends the existing progress bubble to Telegram and runs the turn.
5. On success, dispatch sends rendered Telegram HTML chunks and inserts the assistant response into the same scope thread.
6. On failure, dispatch edits/sends the failure message and inserts an assistant error message into the same scope thread.

Telegram command messages such as session commands also write the incoming command and the command reply to the same scope thread. Unsupported commands write the incoming message and unsupported-command reply.

### Scheduler Telegram Notification

1. Schedulerd executes a due job.
2. If the job is a `Turn` with scope `user:tg-N`, notification sends the result back to Telegram as today.
3. The daemon inserts an assistant message into the same `user:tg-N` scope thread.
4. If Telegram delivery fails, job execution remains recorded as today; chat history records only messages that Wukong attempted to produce, with status `error` for execution failures.

## Web API

Add scope-aware chat APIs:

- `GET /api/chat/scopes`
  - Returns scopes with at least one thread/message, plus the current default Web scope even if empty.
  - Each item includes `scope`, `label`, `message_count`, and `updated_at`.
  - Labels are derived simply:
    - `user:tg-915354960` -> `Telegram 915354960`
    - `project:Wukong` -> `Project Wukong`
    - `global` -> `Global`
    - anything else -> raw scope

- `GET /api/chat/messages?scope=<scope>&limit=10&before=<id>&date=<YYYY-MM-DD>`
  - Reads the selected scope thread.
  - If `scope` is omitted, uses the current Web default scope.
  - Keeps existing pagination and date behavior.

- `GET /chat?q=<message>&scope=<scope>`
  - Runs a Web-originated turn against the selected scope.
  - If `scope` is omitted, uses the current Web default scope.

All endpoints keep the existing Web token authorization behavior.

## Web UI

The chat page gains a small source selector above the message list.

Behavior:

- On load, fetch `/api/chat/scopes`.
- Default selected scope is the current Web default scope unless the URL has a selected scope parameter.
- Selecting `Telegram 915354960` reloads messages from `user:tg-915354960`.
- Sending a message from Web while a Telegram scope is selected writes and runs the turn in that Telegram scope, but does not send the Web-originated response to Telegram. Only Telegram-originated turns and scheduler notifications push to Telegram.
- If only one scope exists, the selector remains visible but simple; it should not block the current chat workflow.

The UI stays plain vanilla web and keeps the existing lazy-loading/date-jump interaction.

## Error Handling

- Chat history insertion failures in Web remain HTTP 500 for user-message insertion before a turn starts.
- Assistant history insertion failures after a turn starts are logged/ignored so a successful response is still delivered.
- Telegram history insertion failures are logged and must not prevent Telegram replies.
- Scheduler history insertion failures are logged and must not change job success/failure status.
- Scope labels are derived server-side without trusting client-provided display names.

## Testing

- Move existing `ChatHistoryStore` tests to the new shared crate and keep coverage for latest/older/date-window reads.
- Add tests for `list_scopes` including label derivation and the empty default scope.
- Add `wukong-web` tests for scope-aware `/api/chat/messages`, `/api/chat/scopes`, and `/chat?q=&scope=` persistence.
- Add `wukong-telegram` dispatch tests proving allowed turns insert both user and assistant messages into `user:tg-<chat_id>`.
- Add scheduler notification tests proving Telegram-scoped scheduled results insert an assistant message into the same scope thread.
- Run package-level tests for `wukong-chat-history`, `wukong-web`, `wukong-telegram`, and `wukong-schedulerd`.

## Release Notes

The release should describe the behavior as shared conversation history: Web can inspect Telegram conversations by switching source, and Telegram/scheduler turns now persist to the same history timeline.
