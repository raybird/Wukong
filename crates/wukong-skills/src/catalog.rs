use wukong_orchestrator::router::SkillRouteOption;
use wukong_orchestrator::Role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillId {
    Brainstorming,
    WritingPlans,
    ExecutingPlans,
    TestDrivenDevelopment,
    SystematicDebugging,
    VerificationBeforeCompletion,
    RequestingCodeReview,
    ReceivingCodeReview,
}

#[derive(Debug, Clone, Copy)]
pub struct SkillSpec {
    pub id: SkillId,
    pub name: &'static str,
    pub description: &'static str,
    pub primary_role: Role,
    pub collaborator_role: Option<Role>,
    pub content: &'static str,
}

impl SkillId {
    pub fn as_str(&self) -> &'static str {
        match self {
            SkillId::Brainstorming => "brainstorming",
            SkillId::WritingPlans => "writing-plans",
            SkillId::ExecutingPlans => "executing-plans",
            SkillId::TestDrivenDevelopment => "test-driven-development",
            SkillId::SystematicDebugging => "systematic-debugging",
            SkillId::VerificationBeforeCompletion => "verification-before-completion",
            SkillId::RequestingCodeReview => "requesting-code-review",
            SkillId::ReceivingCodeReview => "receiving-code-review",
        }
    }

    pub fn from_name(name: &str) -> Option<SkillId> {
        let normalized = name.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "brainstorming" => Some(SkillId::Brainstorming),
            "writing-plans" => Some(SkillId::WritingPlans),
            "executing-plans" => Some(SkillId::ExecutingPlans),
            "test-driven-development" => Some(SkillId::TestDrivenDevelopment),
            "systematic-debugging" => Some(SkillId::SystematicDebugging),
            "verification-before-completion" => Some(SkillId::VerificationBeforeCompletion),
            "requesting-code-review" => Some(SkillId::RequestingCodeReview),
            "receiving-code-review" => Some(SkillId::ReceivingCodeReview),
            _ => None,
        }
    }
}

pub fn all() -> &'static [SkillSpec] {
    const SKILLS: &[SkillSpec] = &[
        SkillSpec {
            id: SkillId::Brainstorming,
            name: "brainstorming",
            description: "創意發想、需求澄清、方案比較",
            primary_role: Role::Oracle,
            collaborator_role: Some(Role::Designer),
            content: include_str!("../assets/superpowers/brainstorming/SKILL.md"),
        },
        SkillSpec {
            id: SkillId::WritingPlans,
            name: "writing-plans",
            description: "多步驟實作計畫撰寫",
            primary_role: Role::Oracle,
            collaborator_role: Some(Role::Librarian),
            content: include_str!("../assets/superpowers/writing-plans/SKILL.md"),
        },
        SkillSpec {
            id: SkillId::ExecutingPlans,
            name: "executing-plans",
            description: "依既有計畫批次執行",
            primary_role: Role::Explorer,
            collaborator_role: Some(Role::Fixer),
            content: include_str!("../assets/superpowers/executing-plans/SKILL.md"),
        },
        SkillSpec {
            id: SkillId::TestDrivenDevelopment,
            name: "test-driven-development",
            description: "新功能或 bugfix 的測試先行實作",
            primary_role: Role::Fixer,
            collaborator_role: Some(Role::Oracle),
            content: include_str!("../assets/superpowers/test-driven-development/SKILL.md"),
        },
        SkillSpec {
            id: SkillId::SystematicDebugging,
            name: "systematic-debugging",
            description: "錯誤追因、根因定位",
            primary_role: Role::Fixer,
            collaborator_role: Some(Role::Explorer),
            content: include_str!("../assets/superpowers/systematic-debugging/SKILL.md"),
        },
        SkillSpec {
            id: SkillId::VerificationBeforeCompletion,
            name: "verification-before-completion",
            description: "宣告完成、commit、PR 前驗證",
            primary_role: Role::Fixer,
            collaborator_role: Some(Role::Oracle),
            content: include_str!("../assets/superpowers/verification-before-completion/SKILL.md"),
        },
        SkillSpec {
            id: SkillId::RequestingCodeReview,
            name: "requesting-code-review",
            description: "發 PR 或完成任務後請求審查",
            primary_role: Role::Librarian,
            collaborator_role: Some(Role::Oracle),
            content: include_str!("../assets/superpowers/requesting-code-review/SKILL.md"),
        },
        SkillSpec {
            id: SkillId::ReceivingCodeReview,
            name: "receiving-code-review",
            description: "接收審查意見並分級處理",
            primary_role: Role::Fixer,
            collaborator_role: Some(Role::Librarian),
            content: include_str!("../assets/superpowers/receiving-code-review/SKILL.md"),
        },
    ];
    SKILLS
}

pub fn find(name: &str) -> Option<&'static SkillSpec> {
    let id = SkillId::from_name(name)?;
    all().iter().find(|spec| spec.id == id)
}

pub fn route_options() -> Vec<SkillRouteOption> {
    all()
        .iter()
        .map(|spec| SkillRouteOption {
            skill_name: spec.name,
            description: spec.description,
            primary_role: spec.primary_role,
            collaborator_role: spec.collaborator_role,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_id_round_trips_names() {
        for id in [
            SkillId::Brainstorming,
            SkillId::WritingPlans,
            SkillId::ExecutingPlans,
            SkillId::TestDrivenDevelopment,
            SkillId::SystematicDebugging,
            SkillId::VerificationBeforeCompletion,
            SkillId::RequestingCodeReview,
            SkillId::ReceivingCodeReview,
        ] {
            assert_eq!(SkillId::from_name(id.as_str()), Some(id));
        }
    }

    #[test]
    fn skill_id_parse_is_case_insensitive_and_trimmed() {
        assert_eq!(SkillId::from_name("  FIXER  "), None);
        assert_eq!(
            SkillId::from_name("  TEST-DRIVEN-DEVELOPMENT  "),
            Some(SkillId::TestDrivenDevelopment)
        );
    }

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
    fn route_options_match_catalog() {
        let options = route_options();
        assert_eq!(options.len(), all().len());
        assert!(options.iter().any(|option| {
            option.skill_name == "systematic-debugging" && option.primary_role == Role::Fixer
        }));
    }

    #[test]
    fn find_returns_skill_specs_by_name() {
        let skill = find("test-driven-development").unwrap();
        assert_eq!(skill.id, SkillId::TestDrivenDevelopment);
        assert_eq!(skill.primary_role, Role::Fixer);
        assert!(find("unknown-skill").is_none());
    }
}