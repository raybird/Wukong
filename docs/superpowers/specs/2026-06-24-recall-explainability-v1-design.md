# Recall Explainability v1 Design

**Date:** 2026-06-24  
**Status:** Draft approved for planning  
**Scope:** Add read-only score explanations to memory recall results and show them in the Web Memory panel.

## Context

The Web Memory panel now supports read-only recall queries through `/api/memory/recall-preview`. The current response shows hit scope, kind, text, final score, confidence, and latency. It does not explain why a hit ranked highly.

The recall pipeline already computes the useful inputs inside `wukong-memory`:

- lexical normalized score from FTS5 BM25
- semantic normalized score from vector similarity
- time decay
- importance
- recall-count hotness bonus
- candidate source paths: keyword, recent, vector

These values are currently local to ranking and are discarded before building `RecallHit`.

## Goals

- Make recall results explainable without changing recall behavior.
- Add structured score breakdown data to each `RecallHit`.
- Surface the breakdown in Web recall preview hit cards.
- Keep the feature read-only and diagnostic-only.
- Avoid adding a new endpoint or repeated recall execution.

## Non-Goals

- No scoring weight controls.
- No per-scope or per-user recall tuning.
- No changes to the ranking formula.
- No memory edits, deletes, prune, consolidate, or export operations.
- No separate explain endpoint in v1.

## Data Model

Add a serializable `RecallExplanation` to `wukong-memory`:

```rust
pub struct RecallExplanation {
    pub lexical: f64,
    pub semantic: f64,
    pub decay: f64,
    pub importance: f64,
    pub recall_bonus: f64,
    pub age_seconds: i64,
    pub recall_count: i64,
    pub source_signals: Vec<String>,
}
```

Add it to `RecallHit` as an additive field:

```rust
pub struct RecallHit {
    pub id: i64,
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    pub score: f64,
    pub explanation: RecallExplanation,
}
```

The field is additive JSON output. Existing consumers that ignore unknown fields continue to work.

## Ranking Behavior

`rank` continues to compute the same final score. It also retains the score inputs in `Scored` so they can be copied into `RecallHit`.

Definitions:

- `lexical`: normalized lexical score in `[0, 1]` derived from BM25. `0` when there was no keyword signal.
- `semantic`: normalized semantic score in `[0, 1]` derived from vector similarity. `0` when there was no vector signal.
- `decay`: time decay score from the existing 90-day half-life formula.
- `importance`: stored memory importance.
- `recall_bonus`: the existing hotness bonus `0.02 * ln(1 + recall_count)`.
- `age_seconds`: non-negative age used for decay.
- `recall_count`: recall count before this recall call increments touched rows.
- `source_signals`: human-readable signal labels, initially `keyword`, `recent`, and/or `vector`.

No formula changes are allowed in this phase. Tests should prove existing ordering behavior remains intact while explanation values are present.

## API Behavior

`POST /api/memory/recall-preview` keeps the same route and request body.

Each hit now includes `explanation`:

```json
{
  "id": 1,
  "scope": "project:Wukong",
  "kind": "note",
  "text": "...",
  "score": 0.82,
  "explanation": {
    "lexical": 1.0,
    "semantic": 0.0,
    "decay": 0.98,
    "importance": 0.8,
    "recall_bonus": 0.0,
    "age_seconds": 120,
    "recall_count": 0,
    "source_signals": ["keyword", "recent"]
  }
}
```

No response field is removed.

## Web UI

Update `wukong-memory` recall result cards to show one compact explanation line below each hit text.

Example:

```text
lexical 1.000 · semantic 0.000 · decay 0.984 · importance 0.800 · hotness +0.000
signals keyword, recent · age 120s · recalled 0
```

If an older response lacks `explanation`, the UI should still render the hit with only the final score. This keeps the component robust during development and browser cache reloads.

## Safety

- Explanation is read-only derived metadata.
- No persistence schema change is required.
- No mutation behavior is added.
- Existing recall behavior may still touch recall bookkeeping as it already does today; explainability itself does not introduce new writes.

## Testing Strategy

Backend tests:

- `rank` returns explanations with lexical, decay, importance, recall bonus, age, recall count, and source signals.
- Existing ranking-order tests still pass.
- `Memory::recall` returns hits containing explanation data.
- `/api/memory/recall-preview` response includes `explanation`.

Frontend checks:

- `node --check crates/wukong-web/static/components/wukong-memory.js` passes.
- Manual smoke test: recall query hit cards show explanation lines.

Regression checks:

- `cargo test -p wukong-memory`
- `cargo test -p wukong-web`
- `cargo test`
- `cargo clippy --all-targets -- -D warnings`

## Acceptance Criteria

- Recall hits include structured `explanation` data.
- Final recall ordering and score formula are unchanged.
- Web recall cards show compact score breakdown when available.
- The existing recall-preview endpoint remains the only Web recall query endpoint.
- No tuning, memory mutation, maintenance, or destructive operation is introduced.
