# Empty Output Retry and Docker Agent Defaults Design

## Context

On 2026-06-22 20:12 +0800, a Telegram turn for `你目前是什麼模型` persisted the sentinel `(本回合未產生文字輸出)`. The underlying opencode session showed reasoning followed by a `bash` tool call rejected by permission handling, then `step-finish` with `reason: tool-calls` and no `text` part. Wukong correctly saw an empty `AgentResponse.text`, but because every planned step was empty it fell through to the sentinel.

Two changes reduce this failure mode:

1. New Docker installs should not override the compose default with a less capable `.env` value.
2. A final all-empty turn should get one text-only repair attempt before Wukong stores the sentinel.

## Goals

- Make `.env.example` match the Docker Compose default: `opencode run --dangerously-skip-permissions`.
- Preserve existing fallback behavior when any step produced non-empty text.
- When all planned steps produced no text, retry the final answer once with an explicit no-tools/direct-text instruction.
- Persist the repair answer into `TurnOutput`, memory, and chat history when it succeeds.
- Keep `SOUL.md` ownership at the opencode/system layer; Wukong should not read or inject it manually.

## Non-Goals

- Do not introduce a generic retry system for every backend failure.
- Do not retry when the final step is empty but an earlier helper step has usable text.
- Do not change opencode permission rules or seed `opencode.json` behavior.
- Do not remove the sentinel entirely; it remains the last-resort fallback when the repair attempt is also empty.

## Approach

### Docker Default

Update `.env.example`:

```env
WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions
```

This prevents the first-install workflow (`cp .env.example .env`) from overriding `docker-compose.yml`'s safer default with plain `opencode run`.

### Runtime Repair Attempt

In `run_turn_observed`, keep the current execution flow:

1. Recall memory.
2. Plan role chain.
3. Execute each planned step.
4. Save the captured final session.
5. Select the final answer.

Change only answer selection for the all-empty case:

- If the final step has text, return it.
- Else if any prior step has text, return the most recent non-empty step, as today.
- Else run one final-step repair call.
- If repair has text, return that as the answer.
- Else return `(本回合未產生文字輸出)`.

The repair call should use the same final role context as the original final step:

- Same final role and skill block.
- Same recall hits.
- Same user input and chain context.
- Same final answer directive and scheduling capability hint.
- Same default model.
- Same final opencode session when available.

The repair prompt appends a short directive:

```text
[修復回覆]
上一輪沒有產生任何可回覆文字，可能是工具不可用、權限被拒，或只完成了工具呼叫。
這次不要呼叫工具，也不要嘗試讀取環境；請直接根據使用者原問題與已知上下文，用繁體中文給出可交付的文字回覆。
```

The repair call should still stream events through the existing `on_event` callback so Web/Telegram/CLI activity rendering remains consistent.

## SOUL.md Handling

Wukong should not manually read `workspace/SOUL.md` or append it to the retry prompt.

Reasons:

- `persona.rs` already documents that Sun Wukong persona is managed globally at the system layer via `SOUL.md`.
- `AgentCliBackend` runs opencode with `current_dir` set to `WUKONG_WORKSPACE`, so any workspace-level context that opencode loads remains available to both the original final step and the repair step.
- Manual injection would duplicate persona text, grow prompts, and couple runtime code to a workspace file convention.

## Data Flow

```text
opencode NDJSON text events
  -> AgentCliBackend::run_streaming accumulates AgentResponse.text
  -> run_turn_observed stores each planned step output
  -> answer selection checks final/non-empty/all-empty
  -> all-empty repair call may produce text
  -> memory.remember stores User + selected Assistant answer
  -> caller persists/sends TurnOutput.text
```

## Error Handling

- If the repair backend call returns an error, propagate the error like any normal backend failure.
- If the repair backend call succeeds but returns empty text, store and return the sentinel.
- If the first pass captured a final session id, save it before repair selection as today; the repair should use the best available final session id so opencode continuity is preserved.

## Tests

- `.env.example` contains `WUKONG_AGENT_CMD=opencode run --dangerously-skip-permissions`.
- `run_turn_all_empty_repairs_with_text`: scripted backend returns an empty final output, then a repair output; `TurnOutput.text` equals the repair output and memory stores it.
- `run_turn_all_empty_repair_empty_returns_sentinel`: scripted backend returns empty final output and empty repair output; sentinel behavior remains.
- Existing `run_turn_falls_back_when_final_output_empty` remains unchanged: if a previous step has text, no repair call is needed and the previous non-empty output wins.

## Rollout Notes

Existing deployments with `.env` already set to `WUKONG_AGENT_CMD=opencode run` will not change automatically. Operators should update their `.env` manually or regenerate it from `.env.example`.
