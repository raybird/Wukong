# Web Memory Read And Query Design

**Date:** 2026-06-24  
**Status:** Draft approved for planning  
**Scope:** Read-only Memory panel query improvements for Wukong Web Console Phase 3.

## Context

The Control Center design originally described Phase 3 as memory maintenance and Phase 4 as recall sandbox. After completing the Control Center shell, memory observability, global model settings, and skill preferences, the next practical step is narrower: help the user inspect memory state and query remembered content from Web without adding write or destructive operations.

The existing Web Memory panel already shows aggregate memory summary, scope/kind filters, and recent records via `/api/memory/summary` and `/api/memory/records`. Runtime memory operations for snapshot, consolidation, prune, and export exist in CLI/Scheduler paths, but they are intentionally out of scope for this first Web memory query phase.

## Goals

- Let users query Wukong memory from the Web Console using the same recall path used by turns.
- Keep the feature read-only: no consolidate, prune, export, delete, update, or confirmation flow.
- Make recall behavior explainable enough for daily use by showing hits, score, confidence, and latency.
- Reuse existing `wukong-memory` APIs and Web token protection.
- Preserve the zero-build Web Component frontend.

## Non-Goals

- No memory maintenance execution from Web.
- No destructive memory operation or preview for destructive operations.
- No scoring weight controls.
- No per-scope recall tuning settings.
- No editing memory records.
- No long-running job progress UI.

## User Experience

Add a read-only `Recall 查詢` card to the existing Memory panel.

Fields:

- Query textarea for the search prompt.
- Scope selector reusing the scopes already loaded from summary data.
- Top-K selector or numeric input, clamped by the backend.
- Mode display set to `hybrid` for the first version.

Results:

- Summary line showing hit count, confidence, and latency.
- Hit cards showing scope, kind, score, and text.
- Empty state: `沒有符合的記憶。`
- Error state with readable HTTP or validation failure text.

The panel continues to show current summary and recent records. Recall query results do not replace the records browser; they are a separate diagnostic view.

## API Design

Add:

| Endpoint | Method | Purpose |
| --- | --- | --- |
| `/api/memory/recall-preview` | POST | Run read-only recall against the selected query and scope. |

Request:

```json
{
  "query": "使用者輸入",
  "scope": "project:Wukong",
  "top_k": 8,
  "mode": "hybrid"
}
```

Response:

```json
{
  "query": "使用者輸入",
  "scope": "project:Wukong",
  "mode": "hybrid",
  "hits": [
    {
      "id": 1,
      "scope": "project:Wukong",
      "kind": "note",
      "text": "...",
      "score": 0.82
    }
  ],
  "evidence": [],
  "confidence": 0.7,
  "latency_ms": 12
}
```

Validation:

- Empty or whitespace-only query returns HTTP 400.
- `top_k` defaults to `8` and is clamped to `1..=20`.
- `mode` is accepted for forward compatibility but only `hybrid` is supported in this version.
- Unknown mode returns HTTP 400.
- Missing scope uses the Web app default scope.
- Existing token behavior applies.

## Backend Design

Extend `crates/wukong-web/src/memory_api.rs` with request/response types and small validation helpers.

The handler in `crates/wukong-web/src/lib.rs` will:

1. Authorize the request using the existing token model.
2. Validate query, top-K, scope, and mode.
3. Call `Memory::recall` with `RecallMode::Hybrid`.
4. Return the recall hits and metadata as JSON.

No new memory store mutation is introduced. Recall may update existing recall bookkeeping if the underlying memory recall path already does so; the Web API itself does not directly write, delete, consolidate, prune, or export records.

## Frontend Design

Update `crates/wukong-web/static/components/wukong-memory.js`.

Additions:

- A recall query form below the summary and before the record list.
- `runRecall()` method that POSTs JSON to `/api/memory/recall-preview`.
- Result rendering with hit cards using the existing `record-card` and `tag` styles.
- Client-side blank query guard to avoid avoidable 400s.

Keep the implementation plain ES modules and Web Components. No build tooling or new dependency is added.

## Safety

- The feature is read-only at the product level.
- No buttons for consolidate, prune, export, delete, or edit appear in this phase.
- API names use `recall-preview` to make the diagnostic nature explicit.
- Error messages must not imply data was changed.
- Token protection matches existing Memory endpoints.

## Testing Strategy

Backend tests:

- `POST /api/memory/recall-preview` returns hits from a temporary memory database.
- Empty query returns HTTP 400.
- Unknown mode returns HTTP 400.
- Token protection applies when Web token is configured.
- Existing `/api/memory/summary` and `/api/memory/records` tests remain unchanged.

Frontend checks:

- `node --check crates/wukong-web/static/components/wukong-memory.js` passes.
- Manual smoke test: enter a query, run recall, see hit cards or empty state.

Regression checks:

- `cargo test -p wukong-web`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`

## Acceptance Criteria

- Memory panel includes a read-only recall query card.
- Users can query memory for a selected scope from Web.
- Recall results show hit count, confidence, latency, scope, kind, score, and text.
- Blank queries are blocked or rejected with a clear error.
- New API is protected by existing Web token behavior.
- No Web UI or API in this phase can prune, consolidate, export, delete, or edit memory.
