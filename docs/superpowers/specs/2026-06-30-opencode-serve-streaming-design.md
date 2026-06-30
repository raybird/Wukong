# Opencode Serve Streaming Design

Date: 2026-06-30

## Goal

Make the Docker-first `opencode serve` backend surface progress events like the CLI backend does today, without changing the final answer delivery model.

This version streams reasoning, tool/function calls, and step boundaries from the opencode server event bus. Assistant answer text remains a final whole response to avoid duplicate or partial text handling in Web and Telegram.

## Current Behavior

`AgentCliBackend::run_streaming` runs `opencode run --format json`, parses NDJSON events, and emits existing `StreamEvent` values:

- `Text(String)`
- `Reasoning(String)`
- `ToolUse(String)`
- `StepStart`
- `StepFinish`

Web and Telegram already consume `Reasoning`, `ToolUse`, and step events for progress bubbles, live event history, and chat history persistence.

`OpencodeServerBackend::run_streaming` currently calls `run`, waits for `POST /session/:id/message` to finish, then emits one final `Text` event. It does not expose thinking, tool use, or function-calling progress.

## Proposed Behavior

Keep `OpencodeServerBackend::run` as the synchronous whole-response path.

Change only `OpencodeServerBackend::run_streaming` to use opencode server's async prompt and event stream APIs:

1. Run `GET /global/health`.
2. Reuse `AgentRequest.session_id`, or create a new session with `POST /session`.
3. Open `GET /event` as an SSE stream.
4. Send the prompt with `POST /session/:id/prompt_async`.
5. Read SSE events until the target session becomes idle.
6. Convert relevant events into Wukong `StreamEvent`s.
7. Fetch the final session messages and extract the final assistant text.
8. Return `AgentResponse { text, session_id }`.

If the stored session id no longer exists, create a new session and retry, matching the existing server backend behavior.

## Event Mapping

The opencode server emits global events with a payload shape similar to:

```json
{
  "directory": "/workspace",
  "payload": {
    "type": "message.part.updated",
    "properties": {
      "part": { "sessionID": "ses_123", "type": "reasoning", "text": "..." },
      "delta": "..."
    }
  }
}
```

The backend should only process events whose nested `sessionID` matches the active session.

Mappings:

- `message.part.updated`, `part.type == "reasoning"` → `StreamEvent::Reasoning(delta_or_text)`
- `message.part.updated`, `part.type == "tool"` → `StreamEvent::ToolUse(part.tool)` once per tool call id
- `message.part.updated`, `part.type == "step-start"` → `StreamEvent::StepStart`
- `message.part.updated`, `part.type == "step-finish"` → `StreamEvent::StepFinish`
- `session.idle` for the active session → finish the stream loop
- `session.status` with `status.type == "idle"` for the active session → finish the stream loop

Text parts are intentionally ignored for this version. Final assistant text is extracted after idle from the finished message state.

## SSE Parsing

Implement a small parser for server-sent events rather than adding a new streaming dependency.

The parser should:

- read response bytes line-by-line;
- collect `data:` lines for one event;
- dispatch the joined JSON payload when a blank line ends the event;
- ignore comment lines and unknown fields;
- tolerate malformed event payloads by ignoring them unless the HTTP request itself fails.

This keeps the backend lightweight and focused on the opencode event shape.

## Final Text Extraction

After idle, call `GET /session/:id/message` and choose the latest assistant message for the active session. Extract text from its `parts` array using the existing text extraction behavior.

If no assistant text is found, return an empty `text` string rather than failing. Existing runtime fallback logic can handle empty final output.

## Error Handling

Do not silently fall back to CLI when `WUKONG_AGENT_SERVER_URL` is set.

Return `GatewayError::AgentFailed` for:

- SSE connection failure;
- `prompt_async` HTTP failure;
- invalid non-JSON success responses;
- timeout before the session becomes idle;
- final message fetch failure.

Use the existing `WUKONG_AGENT_TIMEOUT_SECS` timeout for the full streaming operation.

## Testing

Unit tests should cover:

- SSE frame parsing with single-line and multi-line `data:` payloads;
- event filtering by session id;
- reasoning delta extraction;
- fallback from missing `delta` to `part.text`;
- tool-use dedupe by `callID` or part id;
- idle detection from both `session.idle` and `session.status`;
- final assistant text extraction from `GET /session/:id/message` shape.

If the implementation remains small enough, add a mock HTTP integration test that serves `/event`, `/session`, `/session/:id/prompt_async`, and `/session/:id/message` to verify `run_streaming` emits progress and returns final text.

## Non-Goals

- Streaming assistant answer text token-by-token.
- Changing Web or Telegram rendering semantics.
- Replacing the CLI NDJSON parser.
- Implementing permission-response flows for interactive approval prompts.
- Adding helper session pooling.

## Rollout

1. Add parser and event-mapping tests.
2. Implement `OpencodeServerBackend::run_streaming` with `prompt_async` and `/event`.
3. Verify Web and Telegram get reasoning/tool progress without UI changes.
4. Cut an RC after tests pass.
5. Evaluate whether a later release should stream assistant text deltas.
