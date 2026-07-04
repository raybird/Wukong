# Web Chat Unread Marker Design

Date: 2026-07-04

## Summary

The Web Console chat currently loads the latest messages and then scrolls to the bottom after a short render wait. On first load this can feel unstable: long rendered HTML, images, code blocks, and layout timing can leave the viewport above the newest message or visibly jump after content settles.

This design changes the first-load behavior from "always scroll to bottom" to "restore the user's last seen point." The Web Console will store a per-scope local read marker in `localStorage`, insert an unread divider when newer messages exist, anchor the viewport to that divider, and clear/update the marker after the user interacts with the conversation.

## Goals

- Remember the latest rendered message id per chat scope on the current browser.
- Treat both `user` and `assistant` messages as unread/new records when their id is newer than the stored marker.
- Insert a visible divider before the first message newer than the stored marker.
- On initial load, anchor the viewport to the unread divider when it exists.
- If there is no unread divider, keep the existing latest-message behavior and anchor to the bottom.
- Clear the divider after the first real user interaction and record the latest loaded message id.
- Avoid backend schema changes and cross-device read-state semantics.

## Non-Goals

- Do not persist read state in SQLite or synchronize it across browsers/devices.
- Do not add per-user identity semantics beyond the existing Web token and scope selection.
- Do not change `/api/chat/messages` response shape.
- Do not redesign the whole chat UI.
- Do not change older-message pagination semantics.

## Storage Model

Use `localStorage` with one key per selected chat scope:

```text
wukong.chat.lastSeenMessageId:<scope>
```

The value is the largest message id that the user has acknowledged in that scope. Message ids are monotonic integers from the chat history store, so numeric comparison is sufficient.

Invalid, missing, non-numeric, or non-positive values are treated as absent.

## Initial Load Behavior

`loadLatest()` continues to request the latest page:

```text
GET /api/chat/messages?limit=10&scope=<scope>
```

After the messages are rendered:

1. Read the stored marker for the selected scope.
2. If no marker exists, do not show an unread divider.
3. If a marker exists, find the first loaded message with `message.id > marker`.
4. If found, insert an unread divider immediately before that message.
5. If all loaded messages are newer than the marker, insert the divider before the first loaded message. This means the whole latest page is new.
6. If no loaded message is newer than the marker, do not show a divider.

Viewport anchoring:

- When an unread divider exists, scroll it into view with `behavior: "auto"` and `block: "center"` or a similar stable position.
- When no unread divider exists, scroll to the bottom after layout has settled.
- Programmatic initial anchoring must not count as user interaction.

The first-time case intentionally anchors to the bottom and records the latest loaded id. This avoids showing every old message as unread for new Web Console users.

## Divider UX

The divider should be visually quieter than message bubbles but unmistakable. Suggested label:

```text
以下是上次離開後的新紀錄
```

The divider belongs to the loaded DOM, not to message history. It is not sent to the backend and is not counted as a message.

The divider should have a dedicated class, for example:

```text
unread-divider
```

It should not carry `data-message-id`, so `oldestId` and pagination logic continue to derive from real messages only.

## Clearing And Updating The Marker

The unread divider is cleared after the first real user interaction with the chat view. Valid interactions:

- `wheel`
- `touchstart`
- `pointerdown`
- `keydown`
- submitting a new chat message
- changing the selected scope after the current scope has been viewed

When clearing the divider, write the largest currently loaded message id to that scope's `localStorage` key.

The component should track an internal flag such as `initialAnchoring` so that the programmatic initial scroll does not clear the divider immediately.

If the user opens a scope, sees the divider, and then interacts with the view, the divider disappears and the latest loaded id becomes the new read marker.

## Scope Switching

Each scope has an independent marker.

Before switching away from the current scope, update the current scope's marker only if the user has interacted with the rendered chat or if there is no active unread divider. This preserves unread state when the user briefly switches away without reading.

After switching to the new scope:

1. Reset message DOM and unread state.
2. Load the latest messages for the new scope.
3. Apply the new scope's stored marker.
4. Anchor to the divider or bottom using the same initial-load rules.

## Live Stream Behavior

This change focuses on first load and scope switching. Live events should preserve the existing sticky-bottom behavior:

- If the user is near the bottom, new live messages may continue to follow the bottom.
- If the user has scrolled upward, new live content should not force the viewport down.

When a live message is appended while the user is near the bottom, the latest loaded id can be recorded after the message is persisted and appears in history. If the live message does not yet have a stable history id, marker updates can wait until the next `/api/chat/messages` fetch.

## Error Handling

- If `localStorage` read or write throws, continue without unread markers.
- If the stored marker is newer than all loaded messages, show no divider and do not correct the value until a successful marker update.
- If the latest-message request fails, keep the existing error message behavior.
- If there are no messages, keep the existing empty state and do not write a marker.

## Testing

Frontend behavior should be covered with focused unit-style DOM tests if the project has a suitable JS test harness. If not, add small pure helpers and test them through Rust-served static behavior only where practical.

Minimum test coverage:

- No stored marker: no divider, anchor target is bottom, latest id is recorded.
- Stored marker before some loaded messages: divider is inserted before the first newer message.
- Stored marker older than all loaded messages: divider is inserted before the first loaded message.
- Stored marker equal to newest loaded message: no divider.
- User interaction clears the divider and records the largest loaded message id.
- `loadOlder()` prepends older messages without moving the unread divider or changing the marker.
- Scope markers are independent.

Manual verification:

- Load chat with no local marker and confirm it opens at the bottom.
- Add newer history, reload, and confirm the divider appears and the viewport anchors to it.
- Scroll or click in the chat and confirm the divider disappears.
- Reload again and confirm the same messages are no longer marked unread.
