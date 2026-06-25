use wukong_memory::RecallHit;

/// Compose the final prompt: when there are recall hits, prepend a memory
/// context block; otherwise return the user input unchanged.
pub fn compose_prompt(hits: &[RecallHit], input: &str) -> String {
    if hits.is_empty() {
        return input.to_string();
    }
    let mut s = String::from("[相關記憶]\n");
    for h in hits {
        s.push_str(&format!("- ({}) {}\n", h.scope, h.text));
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
}
