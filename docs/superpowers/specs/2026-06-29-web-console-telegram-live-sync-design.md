# Web Console Telegram Live Sync Design

## Goal

Improve the web console chat experience so it behaves like a normal messaging app when used with Telegram-backed conversations.

The web console should prefer the most recently active Telegram conversation by default, scroll to the newest message when a conversation is opened or switched, and receive live Telegram progress and replies without requiring a browser refresh.

## Current Context

`<wukong-chat>` already supports chat scopes through `/api/chat/scopes` and loads historical messages through `/api/chat/messages`. Telegram conversations are persisted in chat history under scopes like `user:tg-915354960`. The web console currently has one-off SSE for turns submitted from the web via `/chat?q=...`, but Telegram-originated turns only become visible after reloading or manually re-fetching history.

The recent web UI already has richer live turn rendering: a progress bubble, collapsible thinking output, tool-use updates, helper-baton steps, and final assistant bubbles. Telegram live sync should preserve that display style instead of reducing Telegram updates to plain final messages.

## Selected Approach

Use a shared chat-history live event log plus a web SSE stream, with normal chat history as the durable fallback.

Telegram dispatch and the web server run as separate binaries, so an in-memory event bus in `wukong-web` alone would not receive Telegram events. Instead, Telegram dispatch will append lightweight live events to a shared store in the chat-history database as it handles an incoming Telegram message. The web server will expose an SSE endpoint that tails those live events for a selected scope. The web console will subscribe to that endpoint only when viewing a Telegram scope, render incoming events with the existing thinking/tool/answer UI, and still reload history on initial load, scope switch, and reconnect to avoid missed messages.

## Web Console Behavior

When `/chat` opens, the scope selector should default to the most recently active Telegram scope. If no Telegram scope exists, it should fall back to the current default behavior. The selected scope should be reflected in the existing source dropdown.

When the selected scope changes, the chat component should:

1. Close any existing live stream.
2. Clear the current message state.
3. Load the latest messages for the new scope.
4. Scroll to the bottom after rendering.
5. Open a live stream only if the selected scope is a Telegram scope.

Live Telegram events should render as follows:

1. `user` event: append a user bubble for the Telegram user message.
2. `role` event: update or create the single progress bubble.
3. `reasoning` event: append to the existing collapsible thinking block.
4. `tool` event: append tool-use text using the current web console style.
5. `step` event: append a helper-baton details card using the current web console style.
6. `answer` event: remove the progress bubble, append the assistant bubble, and enhance code blocks.
7. `error` event: remove the progress bubble and append an assistant error bubble.

The chat log should stay pinned to the bottom while live events arrive if the user is already near the bottom. If the user has intentionally scrolled up to read older messages, incoming live events should not force-scroll them away from that position. Initial loads and explicit scope switches should always scroll to the bottom.

## Backend Behavior

Add a lightweight live event log to the shared chat-history layer. This can be a small table or equivalent store that records scoped events with a monotonically increasing id, event kind, optional label, content/payload, and creation timestamp. Events must include the scope so the web stream can filter safely across Telegram conversations.

The live event log is for short-lived delivery, not long-term history. It may be pruned by age or by retaining only a bounded number of recent events per scope. Normal chat messages and persisted turn events remain the durable source of truth.

Add a web endpoint such as `GET /api/chat/stream?scope=...` that:

1. Requires the same token authorization as the existing chat APIs.
2. Requires a non-empty scope.
3. Tails live events matching the requested scope from the shared chat-history live event log.
4. Sends only events newer than the client's cursor when a cursor is supplied.
5. Uses SSE event names compatible with the existing web turn event names where possible: `user`, `role`, `reasoning`, `tool`, `step`, `answer`, `error`, and `done` when useful.

Telegram dispatch should publish live events while preserving its current responsibilities:

1. Record the Telegram user message in chat history.
2. Append live `user` event after the user message is accepted.
3. Publish `role`, `reasoning`, and `tool` events from the turn progress callbacks.
4. Publish `answer` or `error` after the final assistant result is available.
5. Continue recording assistant messages and turn events in chat history.
6. Continue sending/editing messages in Telegram exactly as it does today.

The live event log is a delivery mechanism, not the canonical conversation record. If no web client is connected, events may eventually be pruned because chat history remains the source of truth.

## Data And Reconnect Strategy

Chat history remains the durable source of truth. The live SSE stream only improves responsiveness.

The front end should read `/api/chat/messages` on initial load and scope switch. It should also re-fetch latest history after an SSE error/reconnect path or when a stream is opened after being disconnected. This fills gaps if the browser was offline, the web server restarted, live events were pruned, or the user opened the tab after Telegram processing had already started.

To prevent duplicate visual messages, live `user`, `answer`, and `error` events should include the persisted message id when it is available. The front end should track rendered persisted message ids and live temporary messages. Persisted history entries with ids should win over temporary live entries when both describe the same user or assistant message.

## Scope Selection

The preferred default scope is the Telegram scope with the most recent activity. The simplest API-compatible path is to ensure `/api/chat/scopes` returns enough ordering or timestamp information for the front end to select that scope. If the current scope list is already sorted by recent activity, the front end can choose the first scope whose value starts with `user:tg-`. If it is not guaranteed, add `latest_at` or equivalent metadata to the scope response and sort explicitly.

## Error Handling

If the live stream cannot open, the chat should remain usable through history loading. The UI should not block sending web messages or browsing history because Telegram live sync is unavailable.

If a live event payload is malformed, the front end should ignore that event and keep the stream open. If authorization fails, the stream endpoint should return `401` like the other chat APIs.

## Tests

Backend tests should cover:

1. `/api/chat/stream` requires token authorization when a token is configured.
2. `/api/chat/stream` only emits live events for the requested scope.
3. `/api/chat/stream` respects a cursor and does not replay older events unnecessarily.
4. Telegram dispatch appends user, role, reasoning, tool, and final answer/error live events while preserving chat history writes.
5. Chat scopes provide enough information for the front end to choose the most recently active Telegram scope.

Frontend tests should cover the pure behavior where practical:

1. Default scope selection prefers the most recently active Telegram scope.
2. Switching scope closes the previous `EventSource` before opening a new one.
3. Initial load and scope switch scroll to the bottom.
4. Live reasoning/tool events reuse the existing thinking/tool rendering.
5. Live answer/error events clear the progress bubble and append the final assistant bubble.

If the project does not have browser-based front-end tests, keep the front-end changes small and validate with existing Rust tests plus manual browser verification.

## Out Of Scope

This design does not add durable replay of live SSE events, multi-tab coordination, push notifications, or cross-device read state. It also does not change Telegram Bot API behavior or the format of messages sent back to Telegram users.
