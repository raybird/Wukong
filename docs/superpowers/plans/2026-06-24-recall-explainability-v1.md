# Recall Explainability v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add read-only score breakdowns to memory recall hits and display those explanations in the Web Memory recall query UI.

**Architecture:** Extend `wukong-memory` recall ranking to retain scoring components in `Scored` and serialize them through `RecallHit.explanation`. The existing Web recall-preview API will automatically return the additive field, and the Memory Web Component will render a compact explanation line when present.

**Tech Stack:** Rust, serde, `wukong-memory`, `wukong-web`, plain ES modules/Web Components, Cargo tests, `node --check`, GitNexus impact checks.

---

## File Structure

- Modify `crates/wukong-memory/src/model.rs`: add `RecallExplanation` and `RecallHit.explanation`.
- Modify `crates/wukong-memory/src/store/mod.rs`: add transient `Candidate.source_signals` and mark keyword/recent/vector candidate sources.
- Modify `crates/wukong-memory/src/recall/mod.rs`: add explanation fields to `Scored`, compute breakdown in `rank`, and preserve existing ordering behavior.
- Modify `crates/wukong-memory/src/lib.rs`: map `Scored.explanation` into `RecallHit`.
- Modify `crates/wukong-web/src/lib.rs`: add API regression assertion that recall-preview includes explanation.
- Modify `crates/wukong-web/static/components/wukong-memory.js`: render explanation lines in recall hit cards.
- Do not modify scoring weights, tuning settings, memory maintenance, prune, consolidate, export, delete, or edit code.

## Task 1: Add Recall Explanation To Memory Ranking

**Files:**
- Modify: `crates/wukong-memory/src/model.rs`
- Modify: `crates/wukong-memory/src/store/mod.rs`
- Modify: `crates/wukong-memory/src/recall/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Impact analysis before editing**

Run:

```text
gitnexus_impact({ target: "RecallHit", direction: "upstream", file_path: "crates/wukong-memory/src/model.rs", kind: "Struct", repo: "Wukong" })
gitnexus_impact({ target: "rank", direction: "upstream", file_path: "crates/wukong-memory/src/recall/mod.rs", kind: "Function", repo: "Wukong" })
```

Expected: `rank` may be HIGH because recall drives runtime, memoryd, tests, and Web. Report any HIGH/CRITICAL blast radius before editing; proceed only because the change is additive output plus tests and does not alter the score formula.

- [ ] **Step 2: Write failing ranking explanation tests**

In `crates/wukong-memory/src/recall/mod.rs`, add this test after `rank_orders_by_score_and_truncates`:

```rust
    #[test]
    fn rank_explains_score_components_and_signals() {
        let now = 200;
        let mut c = cand(1, "global", 100, Some(-5.0));
        c.vector_sim = Some(0.9);
        c.importance = 0.8;
        c.recall_count = 3;
        c.source_signals = vec![
            "keyword".to_string(),
            "recent".to_string(),
            "vector".to_string(),
        ];

        let ranked = rank(vec![c], now, 1, &Weights::default());

        assert_eq!(ranked.len(), 1);
        let explanation = &ranked[0].explanation;
        assert_eq!(explanation.lexical, 1.0);
        assert_eq!(explanation.semantic, 1.0);
        assert!(explanation.decay > 0.99);
        assert_eq!(explanation.importance, 0.8);
        assert!(explanation.recall_bonus > 0.0);
        assert_eq!(explanation.age_seconds, 100);
        assert_eq!(explanation.recall_count, 3);
        assert_eq!(
            explanation.source_signals,
            vec!["keyword".to_string(), "recent".to_string(), "vector".to_string()]
        );
    }

    #[test]
    fn merge_candidates_preserves_recent_signal_on_duplicate() {
        let mut keyword = cand(1, "global", 100, Some(-2.0));
        keyword.source_signals = vec!["keyword".to_string()];
        let mut recent = cand(1, "global", 100, None);
        recent.source_signals = vec!["recent".to_string()];

        let merged = merge_candidates(vec![keyword], vec![recent]);

        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].source_signals,
            vec!["keyword".to_string(), "recent".to_string()]
        );
    }
```

- [ ] **Step 3: Run tests to verify RED**

Run:

```bash
cargo test -p wukong-memory rank_explains_score_components_and_signals -- --nocapture
cargo test -p wukong-memory merge_candidates_preserves_recent_signal_on_duplicate -- --nocapture
```

Expected: FAIL because `Scored` has no `explanation` field and `Candidate` has no `source_signals` field.

- [ ] **Step 4: Add serializable explanation model**

In `crates/wukong-memory/src/model.rs`, add this struct above `RecallHit`:

```rust
/// Score breakdown for a ranked recall result.
#[derive(Debug, Clone, Serialize)]
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

Then add this field to `RecallHit`:

```rust
    pub explanation: RecallExplanation,
```

- [ ] **Step 5: Track candidate source signals**

In `crates/wukong-memory/src/store/mod.rs`, add this field to `Candidate`:

```rust
    /// Transient recall sources that produced this candidate.
    pub source_signals: Vec<String>,
```

Update `row_to_candidate` to initialize the field:

```rust
        source_signals: Vec::new(),
```

Update `keyword_candidates` to mark rows as keyword candidates:

```rust
        Ok(rows
            .into_iter()
            .map(|r| {
                let mut candidate = row_to_candidate(r);
                candidate.source_signals.push("keyword".to_string());
                candidate
            })
            .collect())
```

Update `recent_candidates` to mark rows as recent candidates:

```rust
        Ok(rows
            .into_iter()
            .map(|r| {
                let mut candidate = row_to_candidate(r);
                candidate.source_signals.push("recent".to_string());
                candidate
            })
            .collect())
```

Update `embedded_candidates` so the `cand` in `(cand, blob_to_embedding(&blob))` is marked as vector:

```rust
                let mut cand = row_to_candidate(r);
                cand.source_signals.push("vector".to_string());
                (cand, blob_to_embedding(&blob))
```

In `crates/wukong-memory/src/recall/mod.rs`, update test helper `cand` to include:

```rust
            source_signals: if bm25.is_some() {
                vec!["keyword".to_string()]
            } else {
                Vec::new()
            },
```

- [ ] **Step 6: Preserve source signals during recall merging**

In `crates/wukong-memory/src/recall/mod.rs`, add this helper near `merge_candidates`:

```rust
fn append_missing_signals(target: &mut Vec<String>, source: Vec<String>) {
    for signal in source {
        if !target.iter().any(|existing| existing == &signal) {
            target.push(signal);
        }
    }
}
```

Replace `merge_candidates` with:

```rust
pub fn merge_candidates(keyword: Vec<Candidate>, recent: Vec<Candidate>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = keyword;
    for c in recent {
        if let Some(existing) = out.iter_mut().find(|k| k.id == c.id) {
            append_missing_signals(&mut existing.source_signals, c.source_signals);
        } else {
            out.push(c);
        }
    }
    out
}
```

Update `apply_vector_sims` so vector-only source signals are preserved when a vector candidate matches an existing keyword/recent candidate:

```rust
        if let Some(existing) = base.iter_mut().find(|c| c.id == v.id) {
            existing.vector_sim = v.vector_sim;
            append_missing_signals(&mut existing.source_signals, v.source_signals);
        } else {
            base.push(v);
        }
```

- [ ] **Step 7: Compute explanations without changing score formula**

In `crates/wukong-memory/src/recall/mod.rs`, update imports:

```rust
use crate::model::{RecallExplanation, RecallMode};
use crate::scoring::{combined_score, time_decay, Weights};
```

Add this field to `Scored`:

```rust
    pub explanation: RecallExplanation,
```

Inside `rank`, replace the local scoring block:

```rust
            let age = (now - c.created_at).max(0);
            let score = combined_score(
                lexical_norm,
                semantic_norm,
                age,
                c.importance,
                c.recall_count,
                weights,
            );
            Scored {
                id: c.id,
                scope: c.scope,
                kind: c.kind,
                text: c.text,
                score,
            }
```

with:

```rust
            let age = (now - c.created_at).max(0);
            let decay = time_decay(age, 90.0);
            let recall_bonus = 0.02 * (1.0 + c.recall_count.max(0) as f64).ln();
            let score = combined_score(
                lexical_norm,
                semantic_norm,
                age,
                c.importance,
                c.recall_count,
                weights,
            );
            let explanation = RecallExplanation {
                lexical: lexical_norm,
                semantic: semantic_norm,
                decay,
                importance: c.importance,
                recall_bonus,
                age_seconds: age,
                recall_count: c.recall_count,
                source_signals: c.source_signals.clone(),
            };
            Scored {
                id: c.id,
                scope: c.scope,
                kind: c.kind,
                text: c.text,
                score,
                explanation,
            }
```

- [ ] **Step 8: Map explanation into RecallHit**

In `crates/wukong-memory/src/lib.rs`, update the `RecallHit` mapping inside `Memory::recall`:

```rust
        let hits: Vec<RecallHit> = scored
            .into_iter()
            .map(|s| RecallHit {
                id: s.id,
                scope: s.scope,
                kind: s.kind,
                text: s.text,
                score: s.score,
                explanation: s.explanation,
            })
            .collect();
```

- [ ] **Step 9: Run tests to verify GREEN**

Run:

```bash
cargo test -p wukong-memory rank_explains_score_components_and_signals -- --nocapture
cargo test -p wukong-memory merge_candidates_preserves_recent_signal_on_duplicate -- --nocapture
cargo test -p wukong-memory recall::tests::rank_orders_by_score_and_truncates -- --nocapture
```

Expected: PASS. The existing ordering test proves ranking behavior still works.

- [ ] **Step 10: Run full memory tests**

Run:

```bash
cargo test -p wukong-memory
```

Expected: PASS.

- [ ] **Step 11: Commit Task 1**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Review changed symbols and affected processes. Then run:

```bash
git status --short
git diff -- crates/wukong-memory/src/model.rs crates/wukong-memory/src/store/mod.rs crates/wukong-memory/src/recall/mod.rs crates/wukong-memory/src/lib.rs
git add crates/wukong-memory/src/model.rs crates/wukong-memory/src/store/mod.rs crates/wukong-memory/src/recall/mod.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): explain recall scores"
```

## Task 2: Prove Web Recall Preview Exposes Explanations

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Impact analysis before editing**

Run:

```text
gitnexus_impact({ target: "post_memory_recall_preview", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", kind: "Function", repo: "Wukong" })
```

Expected: LOW or UNKNOWN if GitNexus has not indexed the new handler yet.

- [ ] **Step 2: Write failing/passing API regression assertion**

In `crates/wukong-web/src/lib.rs`, update `memory_recall_preview_returns_hits` by adding these assertions after the existing `mode` assertion:

```rust
        assert!(body.contains("\"explanation\""), "body: {body}");
        assert!(body.contains("\"lexical\""), "body: {body}");
        assert!(body.contains("\"source_signals\""), "body: {body}");
```

This may already pass after Task 1 because the API serializes `RecallHit` directly. It is still required as a regression assertion for the Web boundary.

- [ ] **Step 3: Run targeted Web test**

Run:

```bash
cargo test -p wukong-web memory_recall_preview_returns_hits -- --nocapture
```

Expected: PASS. If it fails, inspect the response body and fix serialization in Task 1/handler without changing endpoint shape.

- [ ] **Step 4: Run full Web tests**

Run:

```bash
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 5: Commit Task 2**

Run:

```bash
git status --short
git diff -- crates/wukong-web/src/lib.rs
git add crates/wukong-web/src/lib.rs
git commit -m "test(web): assert recall explanations in preview"
```

## Task 3: Render Explanation Lines In Memory UI

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-memory.js`

- [ ] **Step 1: Impact analysis before editing**

Run:

```text
gitnexus_impact({ target: "WukongMemory", direction: "upstream", file_path: "crates/wukong-web/static/components/wukong-memory.js", kind: "Class", repo: "Wukong" })
```

Expected: LOW or UNKNOWN. If UNKNOWN because static JS is not indexed, proceed with single-file UI edit and syntax check.

- [ ] **Step 2: Add helper methods**

At the top of `crates/wukong-web/static/components/wukong-memory.js`, change the import to:

```javascript
import { html, unsafe } from '/lib/html.js';
```

In `crates/wukong-web/static/components/wukong-memory.js`, add these methods before `renderRecallResults(data)`:

```javascript
  formatScore(value) {
    const number = Number(value);
    return Number.isFinite(number) ? number.toFixed(3) : '0.000';
  }

  recallExplanationHtml(explanation) {
    if (!explanation) return '';
    const signals = (explanation.source_signals || []).join(', ') || 'none';
    return html`
      <small>
        lexical ${this.formatScore(explanation.lexical)} · semantic ${this.formatScore(explanation.semantic)} · decay ${this.formatScore(explanation.decay)} · importance ${this.formatScore(explanation.importance)} · hotness +${this.formatScore(explanation.recall_bonus)}<br>
        signals ${signals} · age ${explanation.age_seconds || 0}s · recalled ${explanation.recall_count || 0}
      </small>
    `.toString();
  }
```

- [ ] **Step 3: Render helper output in hit cards**

Replace `renderRecallResults(data)` with:

```javascript
  renderRecallResults(data) {
    const hits = data.hits || [];
    this.recallStatus.textContent = '命中 ' + hits.length + ' 筆 · confidence ' + data.confidence + ' · ' + data.latency_ms + 'ms';
    this.recallResults.innerHTML = hits.map((hit) => html`
      <article class="record-card">
        <div><span class="tag">${hit.scope}</span> <span class="tag">${hit.kind}</span> <span class="tag">score ${this.formatScore(hit.score)}</span></div>
        <p>${hit.text}</p>
        ${unsafe(this.recallExplanationHtml(hit.explanation))}
      </article>
    `.toString()).join('') || '<p class="empty-state">沒有符合的記憶。</p>';
  }
```

- [ ] **Step 4: Check JavaScript syntax**

Run:

```bash
node --check crates/wukong-web/static/components/wukong-memory.js
```

Expected: no output and exit 0.

- [ ] **Step 5: Run Web tests**

Run:

```bash
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 6: Commit Task 3**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Then run:

```bash
git status --short
git diff -- crates/wukong-web/static/components/wukong-memory.js
git add crates/wukong-web/static/components/wukong-memory.js
git commit -m "feat(web): show recall explanations"
```

## Task 4: Final Verification

**Files:**
- No production edits expected unless verification reveals an issue.

- [ ] **Step 1: Run formatting**

Run:

```bash
cargo fmt
```

Expected: no errors. If formatting changes files, inspect the diff and commit pure rustfmt output separately.

- [ ] **Step 2: Run targeted tests**

Run:

```bash
cargo test -p wukong-memory rank_explains_score_components_and_signals -- --nocapture
cargo test -p wukong-web memory_recall_preview_returns_hits -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run full verification**

Run:

```bash
node --check crates/wukong-web/static/components/wukong-memory.js
cargo test -p wukong-memory
cargo test -p wukong-web
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all commands pass with 0 failures and no clippy warnings.

- [ ] **Step 4: Final GitNexus and git checks**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Then run:

```bash
git status --short
git log --oneline -10
```

Expected: no uncommitted changes after all commits.
