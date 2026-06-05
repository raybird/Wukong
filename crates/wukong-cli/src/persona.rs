use wukong_memory::RecallHit;
use wukong_orchestrator::Role;

/// The light-touch Sun Wukong persona, prepended to every execution prompt.
pub const WUKONG_PERSONA: &str =
    "你是孫悟空（齊天大聖、鬥戰勝佛），一位全知全能的助手。\
     以略帶豪氣、機敏的口吻回應，但內容務必專業、精準、可執行。";

/// Build the execution prompt: persona + role card + (memory context + input).
/// The memory/input section reuses gateway's compose_prompt.
pub fn build_prompt(role: Role, hits: &[RecallHit], input: &str) -> String {
    let body = wukong_gateway::prompt::compose_prompt(hits, input);
    format!("{WUKONG_PERSONA}\n\n{}\n\n{body}", role.card())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wukong_memory::MemoryKind;

    fn hit(scope: &str, text: &str) -> RecallHit {
        RecallHit {
            id: 1,
            scope: scope.to_string(),
            kind: MemoryKind::Note,
            text: text.to_string(),
            score: 1.0,
        }
    }

    #[test]
    fn build_prompt_includes_persona_role_and_input() {
        let p = build_prompt(Role::Fixer, &[], "fix the bug");
        assert!(p.contains("孫悟空"));
        assert!(p.contains("你是 Fixer"));
        assert!(p.contains("fix the bug"));
    }

    #[test]
    fn build_prompt_includes_memory_block_when_hits_present() {
        let hits = vec![hit("project:Wukong", "earlier decision")];
        let p = build_prompt(Role::Oracle, &hits, "what now?");
        assert!(p.contains("[相關記憶]"));
        assert!(p.contains("earlier decision"));
        assert!(p.contains("你是 Oracle"));
    }
}
