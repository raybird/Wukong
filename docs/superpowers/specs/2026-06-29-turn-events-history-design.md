# Turn Events History Design

Date: 2026-06-29

## Goal

Preserve full turn-level reasoning and tool activity for both Web console and Telegram-originated conversations, while keeping Telegram's live UX lightweight.

## Current State

- `wukong-gateway` already parses stream events into `StreamEvent::Reasoning`, `StreamEvent::ToolUse`, `StreamEvent::StepStart`, and `StreamEvent::StepFinish`.
- Telegram currently shows reasoning in a single edited status bubble, then deletes that bubble after a successful final answer.
- Web console currently streams reasoning into a live collapsible block, but does not persist it for historical display.
- `wukong-chat-history` stores final user/assistant messages in `chat_messages` and helper-baton outputs in `turn_steps`.
- Web console scopes already support Telegram conversations through `user:tg-<chat_id>`, displayed as `Telegram <id>`.

## Requirements

- Store full `thinking`/`reasoning` text in the database.
- Store tool/function-calling status in the database.
- Keep Telegram live behavior: show reasoning text and tool status in the status bubble, then delete it after success.
- Do not send extra Telegram messages containing the reasoning history.
- Show complete reasoning and tool history in Web console for both Web-originated and Telegram-originated turns.
- In Web console history, render reasoning as one merged block, not as separate chunks.
- Keep tool/status events ordered as a timeline.
- Preserve the existing meaning of `turn_steps` as helper-baton output.

## Data Model

Add a new `turn_events` table linked to the final assistant message:

```sql
CREATE TABLE IF NOT EXISTS turn_events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id INTEGER NOT NULL,
    seq INTEGER NOT NULL,
    kind TEXT NOT NULL,
    label TEXT,
    content TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(message_id) REFERENCES chat_messages(id) ON DELETE CASCADE
);
```

Add an index on `message_id` and read events ordered by `seq ASC, id ASC`.

Event kinds:

- `reasoning`: raw reasoning chunk content.
- `tool_use`: tool/function name in `label`; optional human-readable content.
- `step_start`: optional status marker.
- `step_finish`: optional status marker.

Add `event_count` to the chat message projection, similar to the existing `step_count`, so the Web console can lazily show the history expander only when needed.

## Runtime Flow

### Web Console Turns

During `/chat` handling:

1. Insert the user message as today.
2. Run the turn and continue streaming live SSE updates.
3. Accumulate stream events in memory as `(seq, kind, label, content, created_at)`.
4. Insert the final assistant message.
5. Insert accumulated `turn_events` for that assistant message.
6. Insert helper-baton `turn_steps` exactly as today.

If the turn fails, insert an error assistant message and attach the accumulated events to that error message.

### Telegram Turns

During `handle_message` turn handling:

1. Insert the Telegram user message in the `user:tg-<chat_id>` scope as today.
2. Show a single status bubble.
3. Append reasoning text to the status bubble.
4. Include tool-use status in the same status bubble, such as `▸ 使用工具 read`.
5. Accumulate the same event records in memory.
6. Insert the final assistant message or error message.
7. Insert accumulated `turn_events` linked to that assistant message.
8. On success, delete the Telegram status bubble and only send the final answer chunks.

## Web Console API

Add:

`GET /api/chat/messages/:id/events`

Response: ordered turn events for one assistant message.

Existing message list APIs should include `event_count` so the UI can avoid unnecessary event fetches.

## Web Console UI

For assistant messages with events:

- Show a collapsible block above the final answer labeled `思考與工具紀錄`.
- Lazy-load events on first expand.
- Merge all `reasoning` event content into one `<pre>` block labeled `思考過程`.
- Render `tool_use` events as an ordered timeline, for example `使用工具 read`.
- Keep existing helper-baton `turn_steps` display separate from stream events.

When the selected scope is a Telegram scope, the same UI should display Telegram conversation history and its event records. No separate Telegram-only page is needed.

## Error Handling

- Empty or whitespace-only reasoning chunks are not stored.
- Event persistence is best-effort after the final assistant/error message is inserted.
- A failure to insert events should not block the final answer.
- API failures when loading events should show a retryable inline error in the collapsible block.

## Testing

- `wukong-chat-history`: insert/list turn events in order; message projections include `event_count`; event records cascade by message.
- `wukong-web`: `/chat` persists reasoning/tool events; `/api/chat/messages/:id/events` returns ordered events; history includes `event_count`.
- `wukong-web` static behavior: event expander merges reasoning chunks and lists tool events.
- `wukong-telegram`: reasoning and tool status appear in the live status bubble; successful turns still delete the status bubble; Telegram-scope histories are readable from Web console.

## Non-Goals

- Do not add Telegram messages that preserve full reasoning after the turn completes.
- Do not repurpose `turn_steps` for stream event history.
- Do not implement retention, redaction, or summarization policies in this change.
