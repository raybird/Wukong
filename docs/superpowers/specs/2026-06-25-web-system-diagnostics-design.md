# Web System Diagnostics Design

**Date:** 2026-06-25  
**Status:** Draft approved for planning  
**Scope:** Expand the Web Console System tab into a read-only runtime diagnostics dashboard.

## Context

The Web Console Control Center now has working Chat, Memory, Skills, Schedules, Settings, and System tabs. Memory observability and recall explainability are far enough along to pause that track. The next Control Center gap is the System tab: it currently exposes only a compact summary through `GET /api/system`, including scope, Web token status, memory DB configuration, schedule counts, and next scheduled run.

The System tab should become a read-only diagnostics surface for runtime health. It should help the user answer whether Wukong can reach opencode, list providers and models, use GitHub CLI, identify its workspace/data context, and inspect scheduler readiness without leaving the browser.

## Goals

- Keep System read-only: diagnostics and suggested fixes only, no persisted settings writes.
- Preserve the existing `/api/system` top-level summary fields for compatibility.
- Add grouped diagnostic status cards to `/api/system`.
- Use the existing backend command path for `/providers` and `/models` so Web reflects CLI behavior.
- Isolate failures per diagnostic item so one broken check does not break the whole System page.
- Keep the frontend zero-build and consistent with existing plain Web Components.

## Non-Goals

- No new settings writes or provider credential management.
- No provider/model schema normalization beyond displaying command output summaries.
- No Docker daemon calls or container management.
- No background persistence of diagnostics results.
- No replacement for existing slash commands such as `/providers`, `/models`, or `/set_models`.
- No multi-endpoint diagnostics fan-out in this first version.

## API Design

`GET /api/system` remains the single System endpoint and keeps existing Web token protection.

The response preserves existing top-level fields:

```json
{
  "scope": "global",
  "token_enabled": true,
  "memory_db": "configured",
  "schedule_total": 1,
  "schedule_enabled": 1,
  "next_run_at": 1234567890
}
```

It adds `groups`, a list of diagnostic groups:

```json
{
  "groups": [
    {
      "id": "runtime",
      "title": "Runtime",
      "items": [
        {
          "id": "opencode",
          "label": "opencode",
          "status": "ok",
          "summary": "available",
          "detail": "opencode version ...",
          "suggestion": null
        }
      ]
    }
  ]
}
```

Diagnostic statuses:

- `ok`: the check succeeded or the subsystem is configured.
- `warn`: the subsystem is unavailable, unauthenticated, slow, or partially configured, but Wukong can continue running.
- `error`: an unexpected diagnostic failure occurred and should be investigated.

Each item contains:

- `id`: stable machine-readable key within the group.
- `label`: user-facing short name.
- `status`: `ok`, `warn`, or `error`.
- `summary`: one-line result.
- `detail`: optional command output or context.
- `suggestion`: optional next action.

## Diagnostic Groups

Initial groups:

- `runtime`: opencode command availability, current scope, Web token state.
- `providers`: output from the existing backend command path for `/providers`.
- `models`: output from the existing backend command path for `/models`.
- `tools`: GitHub CLI auth status through `gh auth status`.
- `environment`: workspace path, data or memory DB configuration, and container hint.
- `schedules`: total schedules, enabled schedules, and nearest next run.

Groups may contain one or more items. Empty groups should be omitted unless the frontend needs an explicit empty state.

## Backend Strategy

`system_api.rs` owns response types and diagnostic construction helpers:

- `SystemResponse`
- `DiagnosticGroup`
- `DiagnosticItem`
- `DiagnosticStatus`

The route handler keeps these responsibilities:

- Authorize using the existing Web token flow.
- Open the scheduler store and read jobs.
- Call a diagnostics builder with the app state context and jobs.
- Return HTTP `200` when diagnostics can be assembled, even if individual checks fail.

Providers and models:

- Use the same `AiBackend` command execution path as chat slash commands for `/providers` and `/models`.
- Treat a successful command result as `ok`.
- Treat backend command failure as `warn` or `error` on that diagnostic item, not as a route failure.
- Include short output in `summary` and full or truncated output in `detail`.

Timeouts:

- Command diagnostics should use a short timeout, such as 5 seconds, to avoid freezing the System tab.
- Timeout maps to `warn` with a summary like `command timed out` and a suggestion to retry or check the backend.

GitHub CLI:

- Run `gh auth status` as a read-only process check.
- Missing `gh` or unauthenticated CLI maps to `warn` with a setup suggestion.
- Unexpected process errors map to `error`.

Environment:

- Report workspace path using the current process working directory when available.
- Report memory DB as `configured` or `unavailable`, matching existing behavior.
- Detect a likely container environment through read-only environment or filesystem hints only.
- Do not call Docker daemon APIs.

## Frontend Design

`wukong-system.js` changes from a simple definition list to a dashboard:

- Header with title, short help text, and a `重新整理` button.
- Top summary card showing scope, Web token, memory DB, schedule totals, enabled schedules, and nearest next run.
- Diagnostic group sections below the summary.
- Each diagnostic item renders as a card with:
  - status badge (`ok`, `warn`, `error`),
  - label,
  - summary,
  - optional detail,
  - optional suggestion.

If `/api/system` returns `401`, show the existing unauthorized message. If the route returns another non-OK status, show an HTTP error message. If `groups` is missing, the UI should still render the top-level summary.

## Error Handling

- Token failures remain HTTP `401`.
- Scheduler store open/list failures may still return HTTP `500`, because the current top-level summary depends on jobs.
- Individual diagnostics should not produce route-level failure.
- Command timeout returns a `warn` diagnostic item.
- Command failure returns a `warn` or `error` diagnostic item with readable details.
- Frontend rendering should tolerate missing optional fields.

## Testing Strategy

Backend tests:

- `/api/system` still returns existing top-level summary fields.
- `/api/system` includes diagnostic groups.
- A successful providers command produces an `ok` providers diagnostic item.
- A failing models command produces a non-OK models diagnostic item while `/api/system` still returns HTTP `200`.
- Web token protection still applies to `/api/system`.

Frontend checks:

- `node --check crates/wukong-web/static/components/wukong-system.js`.
- Manual smoke check: System tab loads summary and diagnostic cards.

Regression checks:

- Existing Web system summary test continues to pass.
- Existing chat slash commands `/providers` and `/models` remain available.
- Existing schedule and settings APIs are unaffected.

## Acceptance Criteria

- System tab shows a read-only dashboard with summary and grouped diagnostics.
- `/api/system` preserves current summary fields and adds `groups`.
- Providers and models diagnostics are populated through the existing backend command path.
- One failed diagnostic item does not prevent the rest of the System page from rendering.
- GitHub CLI and environment checks provide actionable `warn` states when unavailable.
- New and existing System API behavior remains protected by Web token authorization.
