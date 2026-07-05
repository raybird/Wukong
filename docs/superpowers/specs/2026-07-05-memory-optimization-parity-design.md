# Memory Optimization Parity Design

**Date:** 2026-07-05  
**Status:** Draft  
**Scope:** Bring Wukong's memory mechanism in line with the highest-value Memoria recall, correctness, and observability optimizations.

## Context

Wukong already has a mature memory core in `wukong-memory`: SQLite + FTS5 keyword recall, tree/hybrid recall, optional embeddings, time decay, recall-count hotness, scope isolation, snapshots, and per-hit recall explanations.

The comparison with Memoria's recent optimization releases shows Wukong does not need a full feature port. The remaining gaps are concentrated in three areas:

- Recall quality for Chinese/CJK short queries.
- Data correctness around duplicate writes and database upgrades.
- Recall telemetry that can quantify whether routing and scoring changes improve real usage.

This design covers the prioritized gap list only. It intentionally avoids host-specific adapter work from Memoria because Wukong's memory writes are primarily driven by Wukong runtime/gateway flows, not short-lived external hook processes.

## Goals

- Prevent meaningful short CJK queries from being skipped by the adaptive recall gate.
- Verify Chinese keyword recall behavior and add a fallback if FTS5 does not reliably match short Chinese queries.
- Make repeated writes idempotent enough to avoid polluting memory with duplicated turns.
- Add regression coverage for upgrading populated older databases.
- Persist minimal recall telemetry for later tuning and health reporting.
- Make response-level `confidence` represent match quality rather than final ranking score polluted by time decay, importance, or hotness.

## Non-Goals

- No Antigravity, Codex, Claude Code, or other host hook adapter changes.
- No recall weight tuning UI.
- No telemetry dashboard in this phase.
- No destructive memory maintenance automation.
- No replacement of the existing FTS5/BM25 recall path.
- No external model requirement; embedding remains optional.

## Approach

Use an incremental, low-risk approach:

1. Improve query handling and tests without changing the ranking formula.
2. Add correctness guards around writes and migrations.
3. Add append-only telemetry and clarify confidence semantics.

This keeps existing recall behavior stable while making failure modes measurable and easier to tune later.

## Phase 1: Recall Quality

### CJK-Aware Adaptive Gate

Current `is_trivial()` only checks character count and English stopwords. It should distinguish between low-information short ASCII text and information-dense CJK queries.

Proposed behavior:

- Continue skipping blank input, English stopword-only input, and very short ASCII fragments.
- Treat CJK ideographs, kana, and hangul as higher-information characters.
- Do not skip short meaningful CJK queries such as `連線池設定`.
- Still skip common low-information confirmations such as `好`, `嗯`, `可以`, and `謝謝`.

Implementation shape:

- Add small helper functions in `crates/wukong-memory/src/recall/mod.rs`:
  - `contains_cjk(query: &str) -> bool`
  - `cjk_weighted_len(query: &str) -> usize`
  - `is_low_information_cjk(query: &str) -> bool`
- Keep these helpers private unless tests need direct access.
- Update `is_trivial()` to use weighted length and low-information phrase checks.

### Chinese Keyword Recall Verification

Wukong already uses FTS5 + `bm25()`. The missing piece is proof that Chinese terms behave correctly with the current tokenizer and query builder.

Required tests:

- Store a memory containing a Chinese phrase such as `連線池設定使用 SQLite WAL`.
- Query `連線池設定` and verify the result is not skipped and can be recalled.
- Query a short low-information Chinese phrase and verify it is skipped.

Fallback behavior if FTS5 misses short Chinese queries:

- Add a bounded fallback for CJK queries only.
- Use a parameterized `LIKE` scan over `memories.text` with a small limit.
- Mark fallback candidates with a source signal such as `cjk_fallback`.
- Keep fallback behind the same scope filtering and ranking path as other candidates.

The fallback should only run when the query contains CJK and keyword recall returns no candidates. It should not replace FTS5 for normal keyword recall.

## Phase 2: Data Correctness

### Write Deduplication

Current writes insert every memory item directly. This can duplicate user/assistant turns if a turn is retried, re-submitted, or persisted through multiple paths.

Proposed behavior:

- Add idempotency at the memory item level using a deterministic content key.
- The key should include at least `scope`, `session_id`, `kind`, normalized `text`, and a coarse turn/write context when available.
- Duplicate inserts should return the existing row id rather than creating a new row.
- Distinct turns with similar text must still be allowed.

Data model:

- Add nullable `dedupe_key TEXT` to `memories`.
- Add a unique index on `dedupe_key` where it is not null.
- Add optional `dedupe_key` to memory write input so callers that can identify a turn or source event can make writes idempotent.
- Runtime and gateway turn persistence should supply stable keys for generated `User:` and `Assistant:` event memories.
- Manual notes, decisions, and imported memories may omit `dedupe_key`; omitted keys always insert normally.
- Add `insert_memory_deduped()` in `Store` that uses `ON CONFLICT(dedupe_key) DO UPDATE` or a select-after-conflict pattern to return the existing id.
- Do not infer broad duplicate keys solely from text content. Exact repeated text can be a legitimate later turn.

### Migration Regression Coverage

Current migrations are idempotent `ALTER TABLE` checks. That is acceptable for now, but tests must cover populated old databases.

Required test fixture:

- Create a database with the older `sessions`, `memories`, `memories_fts`, and insert-trigger schema but without newer columns.
- Insert existing memories before opening through `Memory::open()`.
- Open with the current code.
- Verify new columns exist.
- Verify existing rows remain.
- Verify existing rows are recallable through keyword recall.
- Verify opening the database twice is idempotent.

If the old fixture reveals FTS backfill gaps, add a migration step that rebuilds or repopulates `memories_fts` for existing rows.

### HTTP Boundary Validation

Axum `Json<T>` already rejects malformed JSON and Serde type mismatches. The remaining gap is explicit contract coverage and clearer domain validation.

Required tests for `wukong-memoryd`:

- Wrong-typed `items` field returns `400`.
- Unknown `kind` returns `400` rather than silently becoming `note`.
- Empty `items` returns `400`.
- Blank recall `query` returns `400`.
- Unsupported recall `mode` returns `400`.

Model change:

- Keep `MemoryKind::from_db_str()` permissive for reading old database rows.
- Make request deserialization strict for external API input so unknown `kind` values do not silently downgrade to `Note`.

## Phase 3: Observability

### Recall Telemetry

Add minimal persistent telemetry for each recall call. This should be append-only and privacy-preserving.

Proposed table:

```sql
CREATE TABLE IF NOT EXISTS recall_telemetry (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL,
    scope TEXT,
    mode TEXT NOT NULL,
    query_hash TEXT NOT NULL,
    token_count INTEGER NOT NULL,
    skipped INTEGER NOT NULL DEFAULT 0,
    skipped_reason TEXT,
    hit_count INTEGER NOT NULL,
    top_score REAL,
    top_relevance REAL,
    latency_ms INTEGER NOT NULL
);
```

Telemetry fields:

- `query_hash`: stable hash of the query text, not raw query text.
- `token_count`: token count after recall tokenization.
- `skipped`: whether adaptive gate skipped recall.
- `skipped_reason`: examples: `blank`, `short_ascii`, `stopwords`, `low_information_cjk`.
- `hit_count`: final result count.
- `top_score`: current final ranking score.
- `top_relevance`: decay-free match quality.
- `latency_ms`: total recall latency.

Stats additions:

- Add recall telemetry summary to snapshot or a new stats method.
- Include at least zero-hit rate and average top relevance for non-skipped queries.

### Confidence Semantics

Current `WukongResult.confidence` uses the top hit's final score. Final score includes lexical, semantic, decay, importance, and recall-count bonus, so it is useful for ranking but not a clean confidence signal.

Proposed behavior:

- Keep `RecallHit.score` as the final ranking score.
- Add a decay-free relevance calculation from lexical and semantic signals.
- Set response `confidence` to top relevance, clamped to `[0, 1]`.
- Keep per-hit explanation fields unchanged except for adding `relevance` if useful.

Suggested relevance formula:

```text
relevance = max(lexical, semantic)
```

This is intentionally simple and avoids re-tuning the ranking formula. A later phase can replace it with a weighted relevance formula if telemetry shows it is too coarse.

## Data Flow

Recall flow after these changes:

1. Receive `RecallQuery`.
2. Normalize and classify the query.
3. If trivial, return no hits and write skipped telemetry.
4. Fetch keyword candidates through FTS5.
5. If CJK query and FTS5 returns no keyword candidates, fetch bounded CJK fallback candidates.
6. Fetch recent and vector candidates according to recall mode.
7. Merge, scope-filter, rank, and explain candidates.
8. Touch recalled rows.
9. Compute response confidence from top relevance.
10. Write recall telemetry.

Remember flow after these changes:

1. Validate scope and item list.
2. Preserve caller-provided dedupe keys for idempotent generated writes.
3. Insert new row or return existing duplicate row id when a dedupe key conflicts.
4. Embed and mirror markdown only for newly inserted rows.
5. Return row ids in the same response shape.

## Error Handling

- Invalid external request payloads return `400` with a descriptive message.
- Database migration failures remain startup/open failures.
- Telemetry write failures should not fail recall; log and continue.
- Markdown mirror and embedding failures remain best-effort as today.
- Deduplication conflicts should be treated as successful idempotent writes.

## Testing Strategy

Unit tests:

- CJK gate classification.
- Low-information CJK phrase skipping.
- Relevance/confidence calculation.
- Dedupe key normalization.

Integration tests:

- Chinese recall can find a Chinese memory.
- Duplicate remember calls return stable ids and do not increase memory count.
- Populated old database upgrades and remains recallable.
- Recall telemetry records skipped, zero-hit, and hit cases.

HTTP tests:

- Malformed payloads return `400`.
- Unknown memory kind is rejected.
- Empty item list is rejected.
- Unsupported recall mode is rejected.

Regression commands:

```bash
cargo test -p wukong-memory
cargo test -p wukong-memoryd
cargo test -p wukong-web
cargo test
```

## Acceptance Criteria

- `連線池設定`-style short CJK queries are not skipped by the adaptive gate.
- Chinese keyword recall is covered by tests; if FTS5 misses it, fallback returns bounded candidates.
- Duplicate writes of the same generated turn do not create duplicate memory rows.
- Populated pre-migration databases upgrade without data loss and keep old rows recallable.
- HTTP boundary tests cover malformed and semantically invalid payloads.
- Recall telemetry stores privacy-preserving routing and result quality data.
- Response `confidence` is decay-free match relevance, while `RecallHit.score` remains the ranking score.

## Deferred Work

- Full telemetry dashboard.
- User-configurable recall weights.
- Host-specific hook adapters and cross-process hook state.
- More advanced multilingual tokenization beyond the minimal CJK fallback.
- Replacing ad-hoc migrations with a full migration framework, unless future schema churn justifies it.
