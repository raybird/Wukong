use wukong_memory::RecallHit;

/// 2026-10-06：每筆注入的記憶上限（字元數）。回合記憶存的是全文，
/// 不設上限時幾筆長回覆就能讓每一棒的 prompt 暴增。
const MAX_MEMORY_CHARS: usize = 800;

/// Compose the final prompt: when there are recall hits, prepend a memory
/// context block; otherwise return the user input unchanged.
pub fn compose_prompt(hits: &[RecallHit], input: &str) -> String {
    if hits.is_empty() {
        return input.to_string();
    }
    let mut s = String::from("[相關記憶]\n");
    for h in hits {
        match h.text.char_indices().nth(MAX_MEMORY_CHARS) {
            Some((cut, _)) => {
                s.push_str(&format!("- ({}) {}…（已截斷）\n", h.scope, &h.text[..cut]))
            }
            None => s.push_str(&format!("- ({}) {}\n", h.scope, h.text)),
        }
    }
    s.push_str("\n[使用者輸入]\n");
    s.push_str(input);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use wukong_memory::{MemoryKind, RecallExplanation};

    fn hit(scope: &str, text: &str) -> RecallHit {
        RecallHit {
            id: 1,
            scope: scope.to_string(),
            kind: MemoryKind::Note,
            text: text.to_string(),
            score: 1.0,
            explanation: RecallExplanation {
                lexical: 1.0,
                semantic: 0.0,
                relevance: 1.0,
                decay: 1.0,
                importance: 1.0,
                recall_bonus: 0.0,
                age_seconds: 0,
                recall_count: 0,
                source_signals: vec!["keyword".to_string()],
            },
        }
    }

    #[test]
    fn no_hits_returns_input_unchanged() {
        assert_eq!(compose_prompt(&[], "just this"), "just this");
    }

    #[test]
    fn hits_are_prepended_as_context() {
        let hits = vec![hit("project:Wukong", "decided to use Rust")];
        let out = compose_prompt(&hits, "what did we decide?");
        assert!(out.contains("[相關記憶]"));
        assert!(out.contains("(project:Wukong) decided to use Rust"));
        assert!(out.contains("[使用者輸入]"));
        assert!(out.contains("what did we decide?"));
    }

    // SCN-005: 800 chars (not bytes) per memory; longer ones are cut and marked.
    #[test]
    fn long_memory_is_truncated_and_marked() {
        let exact = "記".repeat(800);
        let long = "記".repeat(801);
        let out = compose_prompt(&[hit("project:X", &exact), hit("project:Y", &long)], "q");
        let expected = format!(
            "[相關記憶]\n- (project:X) {exact}\n- (project:Y) {exact}…（已截斷）\n\n[使用者輸入]\nq"
        );
        assert_eq!(out, expected);
    }
}
