use wukong_memory::RecallHit;
use wukong_orchestrator::Role;
use wukong_skills::SkillSpec;

/// The Sun Wukong persona is managed globally at the system level via `SOUL.md`.
/// This constant is kept for documentation and fallback reference.
pub const WUKONG_PERSONA: &str =
    "你是孫悟空（齊天大聖、鬥戰勝佛），一位全知全能的助手。\
     以略帶豪氣、機敏的口吻回應，但內容務必專業、精準、可執行。";

/// Build the execution prompt: role card + (memory context + input).
/// The memory/input section reuses gateway's compose_prompt.
pub fn build_prompt(role: Role, hits: &[RecallHit], input: &str) -> String {
    let body = wukong_gateway::prompt::compose_prompt(hits, input);
    format!("{}\n\n{body}", role.card())
}

pub fn build_prompt_with_skill(
    role: Role,
    skill: Option<&SkillSpec>,
    hits: &[RecallHit],
    input: &str,
) -> String {
    let body = wukong_gateway::prompt::compose_prompt(hits, input);
    let mut prompt = role.card().to_string();
    if let Some(skill) = skill {
        prompt.push_str(&format!(
            "\n\n[技能規範指引]\n你必須遵循 `{}` 的流程。請先閱讀並遵循專案中的技能規範文件：\n路徑：crates/wukong-skills/assets/superpowers/{}/SKILL.md",
            skill.name, skill.name
        ));
    }
    prompt.push_str("\n\n");
    prompt.push_str(&body);
    prompt
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
    fn build_prompt_includes_role_and_input() {
        let p = build_prompt(Role::Fixer, &[], "fix the bug");
        assert!(!p.contains("孫悟空"));
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

    #[test]
    fn build_prompt_with_skill_includes_skill_block() {
        let skill = wukong_skills::find("test-driven-development").unwrap();
        let p = build_prompt_with_skill(Role::Fixer, Some(skill), &[], "fix the bug");
        assert!(p.contains("[技能規範指引]"));
        assert!(p.contains("test-driven-development"));
        assert!(p.contains("crates/wukong-skills/assets/superpowers/test-driven-development/SKILL.md"));
        assert!(p.contains("你是 Fixer"));
        assert!(p.contains("fix the bug"));
    }

    #[test]
    fn build_prompt_with_skill_omits_skill_block_when_absent() {
        let p = build_prompt_with_skill(Role::Oracle, None, &[], "think about it");
        assert!(!p.contains("[技能規範指引]"));
        assert!(p.contains("你是 Oracle"));
        assert!(p.contains("think about it"));
    }
}
