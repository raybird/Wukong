//! 內嵌的 Superpowers 技能。
//!
//! 2026-11-26（#45）：只留寫出技能所需的名稱與內容。原本的角色對應、`route_options`、`find`
//! 給 planner 與 Web 的技能偏好用，兩者移除後一併刪掉；技能說明由各 SKILL.md 的 frontmatter
//! 提供，V2 依它決定要不要載入。

#[derive(Debug, Clone, Copy)]
pub struct SkillSpec {
    pub name: &'static str,
    pub content: &'static str,
}

pub fn all() -> &'static [SkillSpec] {
    const SKILLS: &[SkillSpec] = &[
        SkillSpec {
            name: "brainstorming",
            content: include_str!("../assets/superpowers/brainstorming/SKILL.md"),
        },
        SkillSpec {
            name: "writing-plans",
            content: include_str!("../assets/superpowers/writing-plans/SKILL.md"),
        },
        SkillSpec {
            name: "executing-plans",
            content: include_str!("../assets/superpowers/executing-plans/SKILL.md"),
        },
        SkillSpec {
            name: "test-driven-development",
            content: include_str!("../assets/superpowers/test-driven-development/SKILL.md"),
        },
        SkillSpec {
            name: "systematic-debugging",
            content: include_str!("../assets/superpowers/systematic-debugging/SKILL.md"),
        },
        SkillSpec {
            name: "verification-before-completion",
            content: include_str!("../assets/superpowers/verification-before-completion/SKILL.md"),
        },
        SkillSpec {
            name: "requesting-code-review",
            content: include_str!("../assets/superpowers/requesting-code-review/SKILL.md"),
        },
        SkillSpec {
            name: "receiving-code-review",
            content: include_str!("../assets/superpowers/receiving-code-review/SKILL.md"),
        },
    ];
    SKILLS
}

pub fn source_content() -> &'static str {
    include_str!("../assets/superpowers/SOURCE.md")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_contains_eight_runtime_skills() {
        assert_eq!(all().len(), 8);
    }

    #[test]
    fn catalog_skills_have_non_empty_embedded_content() {
        for skill in all() {
            assert!(
                !skill.content.trim().is_empty(),
                "{} has empty content",
                skill.name
            );
            assert!(skill.content.contains("#") || skill.content.contains("Skill"));
        }
    }

    #[test]
    fn source_content_is_embedded() {
        let source = source_content();
        assert!(source.contains("Source Attribution"));
        assert!(source.contains("superpowers"));
    }
}
